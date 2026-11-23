//! Compile-time repository source observation, not executable or toolchain attestation.

use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

pub(crate) const PROFILE: &str = "gazette-repository-source-provenance/1";

pub(crate) fn validate_runtime_coordinate(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'/' | b'\\'))
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) struct Snapshot {
    pub(crate) digest: [u8; 32],
    pub(crate) files: usize,
    pub(crate) bytes: u64,
    pub(crate) watched: BTreeSet<PathBuf>,
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "workspace source closure is missing, unsupported or exceeds bounds",
    )
}

fn collect(
    root: &Path,
    relative: &Path,
    files: &mut BTreeSet<PathBuf>,
    watched: &mut BTreeSet<PathBuf>,
) -> io::Result<()> {
    if relative.components().count() > 64 || watched.len() >= 16384 {
        return Err(invalid());
    }
    let path = root.join(relative);
    let kind = fs::symlink_metadata(&path)?.file_type();
    watched.insert(path.clone());
    if kind.is_file() {
        if files.len() >= 8192 {
            return Err(invalid());
        }
        files.insert(relative.to_owned());
    } else if kind.is_dir() {
        for entry in fs::read_dir(path)? {
            collect(root, &relative.join(entry?.file_name()), files, watched)?;
        }
    } else {
        return Err(invalid());
    }
    Ok(())
}

fn manifest(path: &Path) -> io::Result<(toml_edit::DocumentMut, [u8; 32])> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(invalid());
    }
    let mut contents = String::new();
    fs::File::open(path)?
        .take((1 << 20) + 1)
        .read_to_string(&mut contents)?;
    if contents.len() > 1 << 20 {
        return Err(invalid());
    }
    let document = contents
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| invalid())?;
    Ok((document, Sha256::digest(contents.as_bytes()).into()))
}

fn relative(name: &str) -> io::Result<PathBuf> {
    if name.len() > 4096
        || name.split('/').count() > 64
        || name.split('/').any(|part| matches!(part, "" | "." | ".."))
        || name
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\\' | '*' | '?' | '[' | ']'))
    {
        return Err(invalid());
    }
    Ok(PathBuf::from(name))
}

fn confined(root: &Path, path: &Path) -> io::Result<fs::FileType> {
    let mut ancestor = root.to_path_buf();
    let mut kind = fs::symlink_metadata(root)?.file_type();
    for part in path {
        if !kind.is_dir() {
            return Err(invalid());
        }
        ancestor.push(part);
        kind = fs::symlink_metadata(&ancestor)?.file_type();
    }
    if !kind.is_file() && !kind.is_dir() {
        return Err(invalid());
    }
    Ok(kind)
}

// Cargo owns membership. This profile deliberately requires explicit, confined
// member paths instead of guessing glob expansion or following outside checkouts.
fn members(document: &toml_edit::DocumentMut) -> io::Result<BTreeSet<PathBuf>> {
    let workspace = document
        .get("workspace")
        .and_then(toml_edit::Item::as_table)
        .ok_or_else(invalid)?;
    let declared = workspace
        .get("members")
        .and_then(toml_edit::Item::as_array)
        .ok_or_else(invalid)?;
    if declared.is_empty() || declared.len() > 256 {
        return Err(invalid());
    }
    let mut members = BTreeSet::new();
    for member in declared {
        if !members.insert(relative(member.as_str().ok_or_else(invalid)?)?) {
            return Err(invalid());
        }
    }
    Ok(members)
}

// Components declare embedded resources outside their normal source/build trees.
fn resources(document: &toml_edit::DocumentMut) -> io::Result<BTreeSet<PathBuf>> {
    let mut item = document.as_item();
    for key in ["package", "metadata", "gazette", "source-inputs"] {
        let Some(next) = item.get(key) else {
            return Ok(BTreeSet::new());
        };
        item = next;
    }
    let declared = item.as_array().ok_or_else(invalid)?;
    if declared.len() > 256 {
        return Err(invalid());
    }
    let mut inputs = BTreeSet::new();
    for input in declared {
        if !inputs.insert(relative(input.as_str().ok_or_else(invalid)?)?) {
            return Err(invalid());
        }
    }
    Ok(inputs)
}

pub(crate) fn snapshot(root: &Path) -> io::Result<Snapshot> {
    let mut files = BTreeSet::new();
    let mut watched = BTreeSet::new();
    let mut inputs = [
        ("Cargo.toml", false),
        ("Cargo.lock", false),
        ("rust-toolchain.toml", false),
    ]
    .map(|(path, directory)| (PathBuf::from(path), directory))
    .to_vec();
    let (document, digest) = manifest(&root.join("Cargo.toml"))?;
    let mut manifests = BTreeMap::from([(PathBuf::from("Cargo.toml"), digest)]);
    for member in members(&document)? {
        if !confined(root, &member)?.is_dir() {
            return Err(invalid());
        }
        let member_manifest = member.join("Cargo.toml");
        let (document, digest) = manifest(&root.join(&member_manifest))?;
        manifests.insert(member_manifest, digest);
        for resource in resources(&document)? {
            let input = member.join(resource);
            let kind = confined(root, &input)?;
            inputs.push((input, kind.is_dir()));
        }
        // Watch discovery too: a newly added build script/support directory must
        // not leave the old source observation cached. Runtime state is not hashed.
        watched.insert(root.join(&member));
        for (input, directory) in [("Cargo.toml", false), ("src", true)] {
            inputs.push((member.join(input), directory));
        }
        for (input, directory) in [("build.rs", false), ("build_support", true)] {
            let relative = member.join(input);
            match fs::symlink_metadata(root.join(&relative)) {
                Ok(_) => inputs.push((relative, directory)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    for (input, directory) in inputs {
        let kind = fs::symlink_metadata(root.join(&input))?.file_type();
        if (directory && !kind.is_dir()) || (!directory && !kind.is_file()) {
            return Err(invalid());
        }
        collect(root, &input, &mut files, &mut watched)?;
    }
    let mut hash = Sha256::new();
    hash.update(PROFILE.as_bytes());
    let mut bytes = 0_u64;
    for relative in &files {
        let name = relative
            .iter()
            .map(|part| part.to_str().ok_or_else(invalid))
            .collect::<io::Result<Vec<_>>>()?
            .join("/");
        let path = root.join(relative);
        let before = fs::symlink_metadata(&path)?;
        if !before.is_file() || before.len() > 64 << 20 {
            return Err(invalid());
        }
        let file = fs::File::open(&path)?;
        if !file.metadata()?.is_file() {
            return Err(invalid());
        }
        let ceiling = (64_u64 << 20).min((256_u64 << 20) - bytes);
        let mut contents = Vec::new();
        file.take(ceiling + 1).read_to_end(&mut contents)?;
        if contents.len() as u64 > ceiling {
            return Err(invalid());
        }
        if manifests
            .get(relative)
            .is_some_and(|digest| *digest != <[u8; 32]>::from(Sha256::digest(&contents)))
        {
            return Err(invalid());
        }
        let after = fs::symlink_metadata(&path)?;
        if !after.is_file()
            || before.len() != contents.len() as u64
            || after.len() != before.len()
            || after.modified()? != before.modified()?
        {
            return Err(invalid());
        }
        bytes = bytes
            .checked_add(contents.len() as u64)
            .ok_or_else(invalid)?;
        if bytes > 256 << 20 {
            return Err(invalid());
        }
        hash.update((name.len() as u64).to_be_bytes());
        hash.update(name.as_bytes());
        hash.update((contents.len() as u64).to_be_bytes());
        hash.update(&contents);
    }
    Ok(Snapshot {
        digest: hash.finalize().into(),
        files: files.len(),
        bytes,
        watched,
    })
}
