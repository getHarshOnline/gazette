//! Complete replaceable publication artifacts, independent from release history.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::{
    publication::Key,
    render::MediaType,
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest},
    store::{
        BuildCacheDependency, BuildCacheDependencyKind, BuildCacheDependencySet, BuildCacheHead,
        BuildCacheInput, BuildCacheInspection, BuildCacheSite, CachedBuildSite, Error, ReadOptions,
    },
};
use gazette_ports::store::BuildCacheReader;
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::{Sqlite, journal, native_error, with_source};

#[derive(Clone, Serialize)]
struct SiteRow {
    site: String,
    dependency_sha256: Sha256Digest,
    output_sha256: Sha256Digest,
    entries: u64,
    bytes: u64,
}

#[derive(Clone, Serialize)]
struct DependencyRow {
    site: String,
    kind: BuildCacheDependencyKind,
    identity: String,
    sha256: Sha256Digest,
}

#[derive(Clone, Serialize)]
struct ArtifactRow {
    site: String,
    path: EntryPath,
    directory: bool,
    sha256: Option<Sha256Digest>,
    media_type: Option<MediaType>,
    #[serde(skip)]
    payload: Option<Vec<u8>>,
}

#[derive(Serialize)]
struct RowsIdentity<'a> {
    format: &'static str,
    sites: &'a [SiteRow],
    dependencies: &'a [DependencyRow],
    artifacts: Vec<ArtifactIdentity<'a>>,
}

#[derive(Serialize)]
struct ArtifactIdentity<'a> {
    site: &'a str,
    path: &'a EntryPath,
    directory: bool,
    sha256: Option<Sha256Digest>,
    media_type: Option<MediaType>,
}

pub(super) struct Desired {
    sites: Vec<SiteRow>,
    dependencies: Vec<DependencyRow>,
    artifacts: Vec<ArtifactRow>,
    pub head: BuildCacheHead,
}

impl Desired {
    pub(super) fn prepare(input: BuildCacheInput<'_>) -> Result<Self, Error> {
        if input.sites.is_empty() {
            return Err(Error::BuildCacheVerification);
        }
        let mut seen = BTreeSet::new();
        let mut sites = Vec::new();
        let mut dependencies = Vec::new();
        let mut artifacts = Vec::new();
        for input in input.sites {
            let site = input.site.to_string();
            if !seen.insert(site.clone()) {
                return Err(Error::BuildCacheVerification);
            }
            let dependency_sha256 = dependency_identity(input.dependencies)?;
            for dependency in input.dependencies.iter() {
                dependencies.push(DependencyRow {
                    site: site.clone(),
                    kind: dependency.kind,
                    identity: dependency.identity.clone(),
                    sha256: dependency.sha256,
                });
            }
            for entry in input.output.inventory().entries() {
                let (directory, sha256, payload) = match entry.content {
                    Content::Directory => (true, None, None),
                    Content::File { sha256, .. } => {
                        let payload = input
                            .output
                            .file(&entry.path)
                            .ok_or(Error::BuildCacheVerification)?;
                        (false, Some(sha256), Some(payload.to_vec()))
                    }
                };
                let media_type = input.media_types.get(&entry.path).copied();
                if directory && media_type.is_some() {
                    return Err(Error::BuildCacheVerification);
                }
                artifacts.push(ArtifactRow {
                    site: site.clone(),
                    path: entry.path.clone(),
                    directory,
                    sha256,
                    media_type,
                    payload,
                });
            }
            if input
                .media_types
                .keys()
                .any(|path| input.output.file(path).is_none())
            {
                return Err(Error::BuildCacheVerification);
            }
            sites.push(SiteRow {
                site,
                dependency_sha256,
                output_sha256: input.output.inventory().identity().sha256(),
                entries: u64::try_from(input.output.inventory().entries().len())
                    .map_err(|_| Error::BuildCacheVerification)?,
                bytes: input.output.inventory().total_bytes(),
            });
        }
        sites.sort_by(|left, right| left.site.cmp(&right.site));
        dependencies.sort_by(|left, right| {
            (&left.site, left.kind.as_str(), &left.identity).cmp(&(
                &right.site,
                right.kind.as_str(),
                &right.identity,
            ))
        });
        artifacts.sort_by(|left, right| (&left.site, &left.path).cmp(&(&right.site, &right.path)));
        let head = identity(&sites, &dependencies, &artifacts)?;
        Ok(Self {
            sites,
            dependencies,
            artifacts,
            head,
        })
    }

    pub(super) fn replace(
        &self,
        connection: &Connection,
        options: ReadOptions,
    ) -> Result<(), Error> {
        connection
            .execute("DELETE FROM gazette_cache_head", [])
            .map_err(native_error)?;
        connection
            .execute("DELETE FROM gazette_cache_artifacts", [])
            .map_err(native_error)?;
        connection
            .execute("DELETE FROM gazette_cache_dependencies", [])
            .map_err(native_error)?;
        connection
            .execute("DELETE FROM gazette_cache_sites", [])
            .map_err(native_error)?;
        for row in &self.sites {
            connection.execute(
                "INSERT INTO gazette_cache_sites(site,dependency_sha256,output_sha256,entries,bytes) VALUES(?1,?2,?3,?4,?5)",
                params![row.site, row.dependency_sha256.as_bytes(), row.output_sha256.as_bytes(), i64::try_from(row.entries).map_err(|_| Error::BuildCacheVerification)?, i64::try_from(row.bytes).map_err(|_| Error::BuildCacheVerification)?],
            ).map_err(native_error)?;
        }
        for row in &self.dependencies {
            connection.execute(
                "INSERT INTO gazette_cache_dependencies(site,kind,identity,sha256) VALUES(?1,?2,?3,?4)",
                params![row.site, row.kind.as_str(), row.identity, row.sha256.as_bytes()],
            ).map_err(native_error)?;
        }
        for row in &self.artifacts {
            connection.execute(
                "INSERT INTO gazette_cache_artifacts(site,path,kind,sha256,media_type,payload) VALUES(?1,?2,?3,?4,?5,?6)",
                params![row.site, row.path.as_str(), if row.directory { "directory" } else { "file" }, row.sha256.map(|value| value.as_bytes().to_vec()), row.media_type.map(media_type), row.payload],
            ).map_err(native_error)?;
        }
        connection.execute(
            "INSERT INTO gazette_cache_head(singleton,rows_sha256,sites,dependencies,artifacts) VALUES(1,?1,?2,?3,?4)",
            params![self.head.rows_sha256.as_bytes(), i64::try_from(self.head.sites).map_err(|_| Error::BuildCacheVerification)?, i64::try_from(self.head.dependencies).map_err(|_| Error::BuildCacheVerification)?, i64::try_from(self.head.artifacts).map_err(|_| Error::BuildCacheVerification)?],
        ).map_err(native_error)?;
        journal::foreign_keys(connection)?;
        if read(connection, options)?.0 != Some(self.head) {
            return Err(Error::BuildCacheVerification);
        }
        Ok(())
    }
}

impl BuildCacheReader for Sqlite {
    fn load_build_cache_site(
        &self,
        path: &std::path::Path,
        site: &Key,
        dependencies: &BuildCacheDependencySet,
        options: ReadOptions,
    ) -> Result<Option<CachedBuildSite>, Error> {
        with_source(path, options, |connection| {
            let (_, stored) = read(connection, options)?;
            let Some((_, stored_dependencies, artifacts)) = stored.get(site.as_str()) else {
                return Ok(None);
            };
            if stored_dependencies != dependencies {
                return Ok(None);
            }
            Ok(Some(cached_site(
                site.clone(),
                stored_dependencies.clone(),
                artifacts,
                options,
            )?))
        })
    }

    fn inspect_build_cache(
        &self,
        path: &std::path::Path,
        options: ReadOptions,
    ) -> Result<BuildCacheInspection, Error> {
        with_source(path, options, |connection| inspect(connection, options))
    }
}

pub(super) fn inspect(
    connection: &Connection,
    options: ReadOptions,
) -> Result<BuildCacheInspection, Error> {
    let journal = journal::inspect(connection)?;
    let (cache, _) = read(connection, options)?;
    Ok(BuildCacheInspection { journal, cache })
}

type StoredSite = (BuildCacheSite, BuildCacheDependencySet, Vec<ArtifactRow>);

fn read(
    connection: &Connection,
    options: ReadOptions,
) -> Result<(Option<BuildCacheHead>, BTreeMap<String, StoredSite>), Error> {
    let maximum = usize::try_from(options.vm_steps.min(1_000_000))
        .map_err(|_| Error::BuildCacheVerification)?;
    let sites = read_sites(connection, maximum)?;
    let dependencies = read_dependencies(connection, maximum)?;
    let artifacts = read_artifacts(connection, maximum, options)?;
    let head = read_head(connection)?;
    if sites.is_empty() && dependencies.is_empty() && artifacts.is_empty() {
        return if head.is_none() {
            Ok((None, BTreeMap::new()))
        } else {
            Err(Error::BuildCacheVerification)
        };
    }
    let expected = identity(&sites, &dependencies, &artifacts)?;
    if head != Some(expected) {
        return Err(Error::BuildCacheVerification);
    }
    let mut stored = BTreeMap::new();
    for site in &sites {
        let dependency_values = dependencies
            .iter()
            .filter(|row| row.site == site.site)
            .map(|row| BuildCacheDependency::new(row.kind, row.identity.clone(), row.sha256))
            .collect::<Result<Vec<_>, _>>()?;
        let dependency_set = BuildCacheDependencySet::new(dependency_values)?;
        if dependency_identity(&dependency_set)? != site.dependency_sha256 {
            return Err(Error::BuildCacheVerification);
        }
        let site_artifacts: Vec<_> = artifacts
            .iter()
            .filter(|row| row.site == site.site)
            .cloned()
            .collect();
        stored.insert(
            site.site.clone(),
            (
                BuildCacheSite {
                    dependency_sha256: site.dependency_sha256,
                    output_sha256: site.output_sha256,
                    entries: site.entries,
                    bytes: site.bytes,
                },
                dependency_set,
                site_artifacts,
            ),
        );
    }
    if dependencies
        .iter()
        .any(|row| !stored.contains_key(&row.site))
        || artifacts.iter().any(|row| !stored.contains_key(&row.site))
    {
        return Err(Error::BuildCacheVerification);
    }
    for (site, (metadata, dependencies, artifacts)) in &stored {
        let cached = cached_site(
            site.parse().map_err(|_| Error::BuildCacheVerification)?,
            dependencies.clone(),
            artifacts,
            options,
        )?;
        if cached.output.inventory().identity().sha256() != metadata.output_sha256
            || u64::try_from(cached.output.inventory().entries().len()).ok()
                != Some(metadata.entries)
            || cached.output.inventory().total_bytes() != metadata.bytes
        {
            return Err(Error::BuildCacheVerification);
        }
    }
    Ok((Some(expected), stored))
}

fn cached_site(
    site: Key,
    dependencies: BuildCacheDependencySet,
    artifacts: &[ArtifactRow],
    options: ReadOptions,
) -> Result<CachedBuildSite, Error> {
    let mut builder = InventoryBuilder::new(Limits {
        entries: artifacts.len(),
        depth: 64,
        file_bytes: options.storage_bytes,
        total_bytes: options.storage_bytes,
    });
    let mut files = BTreeMap::new();
    let mut media_types = BTreeMap::new();
    for row in artifacts {
        let content = if row.directory {
            Content::Directory
        } else {
            let payload = row.payload.as_ref().ok_or(Error::BuildCacheVerification)?;
            let sha256 = Sha256Digest::of(payload);
            if row.sha256 != Some(sha256) {
                return Err(Error::BuildCacheVerification);
            }
            files.insert(row.path.clone(), payload.clone());
            Content::File {
                bytes: u64::try_from(payload.len()).map_err(|_| Error::StorageLimit)?,
                sha256,
            }
        };
        builder
            .insert(row.path.clone(), content)
            .map_err(|_| Error::BuildCacheVerification)?;
        if let Some(media_type) = row.media_type {
            media_types.insert(row.path.clone(), media_type);
        }
    }
    let output = CapturedSource::from_parts(builder.finish(), files)
        .map_err(|_| Error::BuildCacheVerification)?;
    Ok(CachedBuildSite {
        site,
        dependencies,
        output,
        media_types,
    })
}

fn dependency_identity(dependencies: &BuildCacheDependencySet) -> Result<Sha256Digest, Error> {
    let bytes = serde_json::to_vec(dependencies).map_err(|_| Error::BuildCacheVerification)?;
    Ok(Sha256Digest::of(&bytes))
}

fn identity(
    sites: &[SiteRow],
    dependencies: &[DependencyRow],
    artifacts: &[ArtifactRow],
) -> Result<BuildCacheHead, Error> {
    let artifacts_identity = artifacts
        .iter()
        .map(|row| ArtifactIdentity {
            site: &row.site,
            path: &row.path,
            directory: row.directory,
            sha256: row.sha256,
            media_type: row.media_type,
        })
        .collect();
    let encoded = serde_json::to_vec(&RowsIdentity {
        format: "gazette.build-cache.rows.v1",
        sites,
        dependencies,
        artifacts: artifacts_identity,
    })
    .map_err(|_| Error::BuildCacheVerification)?;
    Ok(BuildCacheHead {
        rows_sha256: Sha256Digest::of(&encoded),
        sites: u64::try_from(sites.len()).map_err(|_| Error::BuildCacheVerification)?,
        dependencies: u64::try_from(dependencies.len())
            .map_err(|_| Error::BuildCacheVerification)?,
        artifacts: u64::try_from(artifacts.len()).map_err(|_| Error::BuildCacheVerification)?,
    })
}

fn read_head(connection: &Connection) -> Result<Option<BuildCacheHead>, Error> {
    let mut statement = connection.prepare("SELECT singleton,rows_sha256,sites,dependencies,artifacts FROM gazette_cache_head LIMIT 2").map_err(native_error)?;
    let mut rows = statement.query([]).map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return Ok(None);
    };
    let head = BuildCacheHead {
        rows_sha256: Sha256Digest::from(
            row.get::<_, [u8; 32]>(1)
                .map_err(|_| Error::BuildCacheVerification)?,
        ),
        sites: u64::try_from(
            row.get::<_, i64>(2)
                .map_err(|_| Error::BuildCacheVerification)?,
        )
        .map_err(|_| Error::BuildCacheVerification)?,
        dependencies: u64::try_from(
            row.get::<_, i64>(3)
                .map_err(|_| Error::BuildCacheVerification)?,
        )
        .map_err(|_| Error::BuildCacheVerification)?,
        artifacts: u64::try_from(
            row.get::<_, i64>(4)
                .map_err(|_| Error::BuildCacheVerification)?,
        )
        .map_err(|_| Error::BuildCacheVerification)?,
    };
    if row
        .get::<_, i64>(0)
        .map_err(|_| Error::BuildCacheVerification)?
        != 1
        || rows.next().map_err(native_error)?.is_some()
    {
        return Err(Error::BuildCacheVerification);
    }
    Ok(Some(head))
}

fn read_sites(connection: &Connection, maximum: usize) -> Result<Vec<SiteRow>, Error> {
    let mut statement = connection.prepare("SELECT site,dependency_sha256,output_sha256,entries,bytes FROM gazette_cache_sites ORDER BY site LIMIT ?1").map_err(native_error)?;
    let rows = statement
        .query_map(
            [i64::try_from(maximum + 1).map_err(|_| Error::StorageLimit)?],
            |row| {
                Ok(SiteRow {
                    site: row.get(0)?,
                    dependency_sha256: Sha256Digest::from(row.get::<_, [u8; 32]>(1)?),
                    output_sha256: Sha256Digest::from(row.get::<_, [u8; 32]>(2)?),
                    entries: u64::try_from(row.get::<_, i64>(3)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    bytes: u64::try_from(row.get::<_, i64>(4)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                })
            },
        )
        .map_err(native_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(native_error)?;
    if rows.len() > maximum {
        return Err(Error::StorageLimit);
    }
    Ok(rows)
}

fn read_dependencies(connection: &Connection, maximum: usize) -> Result<Vec<DependencyRow>, Error> {
    let mut statement = connection.prepare("SELECT site,kind,identity,sha256 FROM gazette_cache_dependencies ORDER BY site,kind,identity LIMIT ?1").map_err(native_error)?;
    let rows = statement
        .query_map(
            [i64::try_from(maximum + 1).map_err(|_| Error::StorageLimit)?],
            |row| {
                let kind: String = row.get(1)?;
                let kind = dependency_kind(&kind).ok_or(rusqlite::Error::InvalidQuery)?;
                Ok(DependencyRow {
                    site: row.get(0)?,
                    kind,
                    identity: row.get(2)?,
                    sha256: Sha256Digest::from(row.get::<_, [u8; 32]>(3)?),
                })
            },
        )
        .map_err(native_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(native_error)?;
    if rows.len() > maximum {
        return Err(Error::StorageLimit);
    }
    Ok(rows)
}

fn read_artifacts(
    connection: &Connection,
    maximum: usize,
    options: ReadOptions,
) -> Result<Vec<ArtifactRow>, Error> {
    let mut statement = connection.prepare("SELECT site,path,kind,sha256,media_type,payload FROM gazette_cache_artifacts ORDER BY site,path LIMIT ?1").map_err(native_error)?;
    let rows = statement
        .query_map(
            [i64::try_from(maximum + 1).map_err(|_| Error::StorageLimit)?],
            |row| {
                let path: String = row.get(1)?;
                let kind: String = row.get(2)?;
                let media: Option<String> = row.get(4)?;
                Ok(ArtifactRow {
                    site: row.get(0)?,
                    path: path.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    directory: match kind.as_str() {
                        "directory" => true,
                        "file" => false,
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    },
                    sha256: row.get::<_, Option<[u8; 32]>>(3)?.map(Sha256Digest::from),
                    media_type: media
                        .as_deref()
                        .map(parse_media_type)
                        .transpose()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    payload: row.get(5)?,
                })
            },
        )
        .map_err(native_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(native_error)?;
    if rows.len() > maximum
        || rows
            .iter()
            .filter_map(|row| row.payload.as_ref())
            .try_fold(0_u64, |sum, payload| sum.checked_add(payload.len() as u64))
            .is_none_or(|bytes| bytes > options.storage_bytes)
    {
        return Err(Error::StorageLimit);
    }
    Ok(rows)
}

fn dependency_kind(value: &str) -> Option<BuildCacheDependencyKind> {
    Some(match value {
        "source" => BuildCacheDependencyKind::Source,
        "configuration" => BuildCacheDependencyKind::Configuration,
        "theme-template" => BuildCacheDependencyKind::ThemeTemplate,
        "build-mechanism" => BuildCacheDependencyKind::BuildMechanism,
        "selected-semantic-input" => BuildCacheDependencyKind::SelectedSemanticInput,
        _ => return None,
    })
}

fn media_type(value: MediaType) -> &'static str {
    match value {
        MediaType::Html => "html",
        MediaType::Json => "json",
        MediaType::JsonLd => "json-ld",
        MediaType::WebManifest => "web-manifest",
        MediaType::Xml => "xml",
        MediaType::Rss => "rss",
        MediaType::Atom => "atom",
        MediaType::Text => "text",
        MediaType::Markdown => "markdown",
        MediaType::JavaScript => "javascript",
        MediaType::Css => "css",
        MediaType::Png => "png",
        MediaType::Jpeg => "jpeg",
        MediaType::Webp => "webp",
        MediaType::Svg => "svg",
        MediaType::Woff2 => "woff2",
    }
}

fn parse_media_type(value: &str) -> Result<MediaType, ()> {
    Ok(match value {
        "html" => MediaType::Html,
        "json" => MediaType::Json,
        "json-ld" => MediaType::JsonLd,
        "web-manifest" => MediaType::WebManifest,
        "xml" => MediaType::Xml,
        "rss" => MediaType::Rss,
        "atom" => MediaType::Atom,
        "text" => MediaType::Text,
        "markdown" => MediaType::Markdown,
        "javascript" => MediaType::JavaScript,
        "css" => MediaType::Css,
        "png" => MediaType::Png,
        "jpeg" => MediaType::Jpeg,
        "webp" => MediaType::Webp,
        "svg" => MediaType::Svg,
        "woff2" => MediaType::Woff2,
        _ => return Err(()),
    })
}

#[cfg(test)]
#[path = "../tests/internal/cache.rs"]
mod tests;
