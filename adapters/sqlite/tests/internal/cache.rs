//! Persistent whole-site cache hit, invalidation, complete replacement and byte parity.

use super::*;
use gazette_core::{
    source::{Content, InventoryBuilder},
    store::{
        BuildCacheDependencyKind as Kind, BuildCacheSiteInput, BuildCacheWriteOptions,
        InitializeOptions, SessionOptions, WriterOptions,
    },
};
use gazette_ports::store::{
    BuildCacheReader, BuildCacheWriter, JournalWriter, StoreInitializer, WriterConnector,
};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt};

fn options() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        vm_steps: 1_000_000,
        ..Default::default()
    }
}

fn fixture() -> (tempfile::TempDir, crate::Writer) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private store");
    let session = SessionOptions {
        allow_local_reservation: true,
        read: options(),
    };
    let initialized = Sqlite
        .initialize(
            root.path(),
            InitializeOptions {
                allow_create: true,
                session,
                ..Default::default()
            },
        )
        .expect("fresh store");
    let writer = Sqlite
        .acquire_writer(
            root.path(),
            initialized.header,
            WriterOptions {
                allow_generation_advance: true,
                session,
            },
        )
        .expect("writer");
    (root, writer)
}

fn dependencies(seed: &str) -> BuildCacheDependencySet {
    BuildCacheDependencySet::new([
        make_dependency(Kind::Source, "source:content", seed),
        make_dependency(Kind::Configuration, "workspace", seed),
        make_dependency(Kind::ThemeTemplate, "theme:site", seed),
        make_dependency(Kind::BuildMechanism, "tera", seed),
        make_dependency(Kind::SelectedSemanticInput, "site:model", seed),
    ])
    .expect("complete typed dependencies")
}

fn make_dependency(kind: Kind, identity: &str, value: &str) -> BuildCacheDependency {
    BuildCacheDependency::new(kind, identity, Sha256Digest::of(value.as_bytes()))
        .expect("dependency")
}

fn output(path: &str, bytes: &[u8]) -> CapturedSource {
    let path: EntryPath = path.parse().expect("path");
    let mut builder = InventoryBuilder::new(Limits::default());
    let mut parent = String::new();
    let components = path.as_str().split('/').collect::<Vec<_>>();
    for component in &components[..components.len().saturating_sub(1)] {
        if !parent.is_empty() {
            parent.push('/');
        }
        parent.push_str(component);
        builder
            .insert(parent.parse().expect("parent path"), Content::Directory)
            .expect("parent inventory");
    }
    builder
        .insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )
        .expect("inventory");
    CapturedSource::from_parts(builder.finish(), BTreeMap::from([(path, bytes.to_vec())]))
        .expect("capture")
}

#[test]
fn clean_and_warm_outputs_are_identical_and_every_dependency_kind_invalidates() {
    let (root, mut writer) = fixture();
    let site: Key = "site:one".parse().expect("site");
    let deps = dependencies("v1");
    let clean = output("index.html", b"<html>exact</html>\n");
    let media = BTreeMap::from([("index.html".parse().expect("path"), MediaType::Html)]);
    let receipt = writer
        .replace_build_cache(
            BuildCacheInput {
                sites: vec![BuildCacheSiteInput {
                    site: &site,
                    dependencies: &deps,
                    output: &clean,
                    media_types: &media,
                }],
            },
            BuildCacheWriteOptions {
                allow_replace: true,
            },
        )
        .expect("replace");
    writer.close().expect("close");
    let database = Sqlite::journal_database(root.path());
    let warm = Sqlite
        .load_build_cache_site(&database, &site, &deps, options())
        .expect("read")
        .expect("warm hit");
    assert_eq!(warm.output.inventory(), clean.inventory());
    assert_eq!(
        warm.output.file(&"index.html".parse().unwrap()),
        Some(b"<html>exact</html>\n".as_slice())
    );
    assert_eq!(warm.media_types, media);
    for kind in [
        Kind::Source,
        Kind::Configuration,
        Kind::ThemeTemplate,
        Kind::BuildMechanism,
        Kind::SelectedSemanticInput,
    ] {
        let changed = BuildCacheDependencySet::new(deps.iter().cloned().map(|dependency| {
            if dependency.kind == kind {
                make_dependency(kind, &dependency.identity, "changed")
            } else {
                dependency
            }
        }))
        .expect("changed closure");
        assert!(
            Sqlite
                .load_build_cache_site(&database, &site, &changed, options())
                .expect("miss")
                .is_none(),
            "{kind:?}"
        );
    }
    let inspection = Sqlite
        .inspect_build_cache(&database, options())
        .expect("inspect");
    assert_eq!(inspection.journal.header, receipt.journal);
    assert_eq!(inspection.cache, Some(receipt.cache));
}

#[test]
fn producer_declared_markdown_media_type_survives_cache_round_trip() {
    let (root, mut writer) = fixture();
    let site: Key = "site:markdown".parse().expect("site");
    let dependencies = dependencies("markdown-v1");
    let path: EntryPath = "alternate.html".parse().expect("producer-selected path");
    let bytes = "# Retained Ω\n\n[Destination](https://public.test/guide/)\n".as_bytes();
    let clean = output(path.as_str(), bytes);
    let media = BTreeMap::from([(path.clone(), MediaType::Markdown)]);
    writer
        .replace_build_cache(
            BuildCacheInput {
                sites: vec![BuildCacheSiteInput {
                    site: &site,
                    dependencies: &dependencies,
                    output: &clean,
                    media_types: &media,
                }],
            },
            BuildCacheWriteOptions {
                allow_replace: true,
            },
        )
        .expect("replace Markdown output");
    writer.close().expect("close");
    let warm = Sqlite
        .load_build_cache_site(
            &Sqlite::journal_database(root.path()),
            &site,
            &dependencies,
            options(),
        )
        .expect("read")
        .expect("warm hit");
    assert_eq!(warm.output.inventory(), clean.inventory());
    assert_eq!(warm.output.file(&path), Some(bytes));
    assert_eq!(warm.media_types, media);
}

#[test]
fn producer_declared_woff2_media_type_survives_cache_round_trip() {
    let (root, mut writer) = fixture();
    let site: Key = "site:font".parse().expect("site");
    let dependencies = dependencies("font-v1");
    let path: EntryPath = "fonts/fixture.woff2".parse().expect("font path");
    let clean = output(path.as_str(), b"retained font bytes");
    let media = BTreeMap::from([(path.clone(), MediaType::Woff2)]);
    writer
        .replace_build_cache(
            BuildCacheInput {
                sites: vec![BuildCacheSiteInput {
                    site: &site,
                    dependencies: &dependencies,
                    output: &clean,
                    media_types: &media,
                }],
            },
            BuildCacheWriteOptions {
                allow_replace: true,
            },
        )
        .expect("replace font output");
    writer.close().expect("close");
    let database = Sqlite::journal_database(root.path());
    let warm = Sqlite
        .load_build_cache_site(&database, &site, &dependencies, options())
        .expect("read")
        .expect("warm hit");
    assert_eq!(
        warm.output.file(&path),
        Some(b"retained font bytes".as_slice())
    );
    assert_eq!(warm.media_types, media);
}

#[test]
fn complete_replacement_removes_stale_sites_without_changing_journal_identity() {
    let (root, mut writer) = fixture();
    let one: Key = "site:one".parse().unwrap();
    let two: Key = "site:two".parse().unwrap();
    let deps = dependencies("same");
    let first = output("index.html", b"one");
    let second = output("index.html", b"two");
    let media = BTreeMap::from([("index.html".parse().unwrap(), MediaType::Html)]);
    let first_receipt = writer
        .replace_build_cache(
            BuildCacheInput {
                sites: vec![
                    BuildCacheSiteInput {
                        site: &one,
                        dependencies: &deps,
                        output: &first,
                        media_types: &media,
                    },
                    BuildCacheSiteInput {
                        site: &two,
                        dependencies: &deps,
                        output: &second,
                        media_types: &media,
                    },
                ],
            },
            BuildCacheWriteOptions {
                allow_replace: true,
            },
        )
        .expect("first replace");
    writer.close().expect("close first");
    let session = SessionOptions {
        allow_local_reservation: true,
        read: options(),
    };
    let mut writer = Sqlite
        .acquire_writer(
            root.path(),
            first_receipt.journal,
            WriterOptions {
                allow_generation_advance: true,
                session,
            },
        )
        .expect("second writer");
    let second_receipt = writer
        .replace_build_cache(
            BuildCacheInput {
                sites: vec![BuildCacheSiteInput {
                    site: &one,
                    dependencies: &deps,
                    output: &first,
                    media_types: &media,
                }],
            },
            BuildCacheWriteOptions {
                allow_replace: true,
            },
        )
        .expect("second replace");
    writer.close().expect("close second");
    let database = Sqlite::journal_database(root.path());
    assert!(
        Sqlite
            .load_build_cache_site(&database, &two, &deps, options())
            .expect("read stale")
            .is_none()
    );
    assert!(
        Sqlite
            .load_build_cache_site(&database, &one, &deps, options())
            .expect("read retained")
            .is_some()
    );
    assert_eq!(
        second_receipt.journal.journal_id,
        first_receipt.journal.journal_id
    );
    assert_eq!(
        second_receipt.journal.journal_epoch,
        first_receipt.journal.journal_epoch
    );
    assert!(second_receipt.journal.writer_fence > first_receipt.journal.writer_fence);
}

#[test]
fn changed_cached_payload_is_refused_instead_of_returned_as_a_warm_hit() {
    let (root, mut writer) = fixture();
    let site: Key = "site:one".parse().unwrap();
    let deps = dependencies("same");
    let clean = output("index.html", b"trusted");
    let media = BTreeMap::from([("index.html".parse().unwrap(), MediaType::Html)]);
    writer
        .replace_build_cache(
            BuildCacheInput {
                sites: vec![BuildCacheSiteInput {
                    site: &site,
                    dependencies: &deps,
                    output: &clean,
                    media_types: &media,
                }],
            },
            BuildCacheWriteOptions {
                allow_replace: true,
            },
        )
        .unwrap();
    writer.close().unwrap();
    let database = Sqlite::journal_database(root.path());
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute(
            "UPDATE gazette_cache_artifacts SET payload=?1 WHERE site=?2 AND path=?3",
            rusqlite::params![b"changed".as_slice(), site.as_str(), "index.html"],
        )
        .unwrap();
    connection.close().unwrap();
    assert!(matches!(
        Sqlite.load_build_cache_site(&database, &site, &deps, options()),
        Err(Error::BuildCacheVerification)
    ));
}
