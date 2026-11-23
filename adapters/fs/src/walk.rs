//! One confined traversal for content capture and metadata-only discovery.

use super::*;

pub(super) trait Visitor {
    fn check_path(&self, path: &EntryPath) -> Result<(), Error>;
    fn directory(&mut self, path: EntryPath) -> Result<(), Error>;
    fn file(&mut self, path: EntryPath, file: &mut File, metadata: &Metadata) -> Result<(), Error>;
}

pub(super) fn walk(
    directory: &File,
    prefix: &str,
    visitor: &mut impl Visitor,
    profile: ReadProfile,
    mut observed: Option<&mut BTreeMap<EntryPath, Metadata>>,
    selection: Selection<'_>,
) -> Result<(), Error> {
    let path = Path::new(prefix);
    let before = directory
        .metadata()
        .map_err(|e| io_error("inspect directory", path, e))?;
    let entries = Dir::read_from(directory).map_err(|e| io_error("list directory", path, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_error("read directory entry", path, e))?;
        let raw_name = entry.file_name();
        if matches!(raw_name.to_bytes(), b"." | b"..") {
            continue;
        }
        // Only the separately verified root administrative directory is omitted.
        // Source reads still observe every entry; deployment output cannot embed
        // another Git administrative path or a case-variant alias.
        if profile != ReadProfile::Source && prefix.is_empty() && raw_name.to_bytes() == b".git" {
            continue;
        }
        if !selection.admits(prefix, raw_name.to_bytes()) {
            continue;
        }
        let name = entry_name(raw_name.to_bytes(), path)?;
        if profile != ReadProfile::Source && name.eq_ignore_ascii_case(".git") {
            return Err(Error::InvalidName(path.join(name)));
        }
        let relative: EntryPath = (if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        })
        .parse()?;
        if !selection.allows(&relative)? {
            continue;
        }
        let child_path = Path::new(relative.as_str());
        visitor.check_path(&relative)?;
        let declared = fs::statat(directory, raw_name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|e| io_error("inspect entry", child_path, e))?;
        let kind = FileType::from_raw_mode(declared.st_mode);
        let directory_flag = match kind {
            FileType::Directory => OFlags::DIRECTORY,
            FileType::RegularFile if !selection.ancestor_only(relative.as_str()) => OFlags::empty(),
            _ => return Err(Error::Unsupported(child_path.to_owned())),
        };
        // NONBLOCK prevents a replaced FIFO from hanging before type checking;
        // NOFOLLOW rejects symlink replacement during lookup.
        let fd = fs::openat(
            directory,
            raw_name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK | directory_flag,
            Mode::empty(),
        )
        .map_err(|e| io_error("open entry", child_path, e))?;
        let mut file = File::from(fd);
        let opened = file
            .metadata()
            .map_err(|e| io_error("inspect opened entry", child_path, e))?;
        if opened.dev() != declared.st_dev as u64 || opened.ino() != declared.st_ino as u64 {
            return Err(Error::Changed(child_path.to_owned()));
        }
        if let Some(observed) = observed.as_deref_mut() {
            synchronization::validate_entry(
                &opened,
                child_path,
                (opened.dev(), opened.ino()),
                opened.is_dir(),
            )?;
            observed.insert(relative.clone(), opened.clone());
        }
        match kind {
            FileType::Directory if opened.is_dir() => {
                visitor.directory(relative.clone())?;
                if selection.descend(&relative) {
                    walk(
                        &file,
                        relative.as_str(),
                        visitor,
                        profile,
                        observed.as_deref_mut(),
                        selection,
                    )?;
                }
            }
            FileType::RegularFile if opened.is_file() => {
                visitor.file(relative, &mut file, &opened)?;
            }
            _ => return Err(Error::Changed(child_path.to_owned())),
        }
    }
    let after = directory
        .metadata()
        .map_err(|e| io_error("recheck directory", path, e))?;
    if changed(&before, &after) {
        return Err(Error::Changed(path.to_owned()));
    }
    Ok(())
}
