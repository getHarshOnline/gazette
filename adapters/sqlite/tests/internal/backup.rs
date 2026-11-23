//! Native snapshot and descriptor boundaries under deterministic source replacement.

use super::{copy, open_unchanged, read_metadata, with_source};
use gazette_core::store::{BackupOptions, Error, ReadOptions};
use rusqlite::Connection;
use std::{
    fs,
    io::Read,
    os::unix::{fs::symlink, net::UnixListener},
    sync::mpsc,
    thread,
    time::Duration,
};
use tempfile::tempdir;

#[test]
fn descriptor_reads_exact_regular_file_and_rejects_changed_bytes_or_same_inode_symlink() {
    let root = tempdir().expect("private fixture");
    let path = root.path().join("original");
    fs::write(&path, b"original bytes").expect("fixture bytes");
    let identity = fs::metadata(&path).expect("observed identity");
    let mut bytes = Vec::new();
    open_unchanged(&path, &identity)
        .expect("same regular file")
        .read_to_end(&mut bytes)
        .expect("native read");
    assert_eq!(bytes, b"original bytes");

    let alias = root.path().join("alias");
    symlink(&path, &alias).expect("same-inode symlink fixture");
    assert!(matches!(
        open_unchanged(&alias, &identity),
        Err(Error::UnsafePath)
    ));

    fs::write(&path, b"changed").expect("same inode, different state");
    assert!(matches!(
        open_unchanged(&path, &identity),
        Err(Error::Changed)
    ));
    let directory = root.path().join("directory");
    fs::create_dir(&directory).expect("directory replacement");
    assert!(matches!(
        open_unchanged(
            &directory,
            &fs::metadata(&directory).expect("directory identity")
        ),
        Err(Error::Changed)
    ));
}

#[test]
fn replacing_an_observed_regular_file_with_a_socket_never_opens_the_special_file() {
    let root = tempdir().expect("private fixture");
    let path = root.path().join("selected");
    fs::write(&path, b"observed regular file").expect("initial file");
    let identity = fs::metadata(&path).expect("captured before replacement");
    fs::rename(&path, root.path().join("preserved")).expect("preserve original fixture");
    let _listener = UnixListener::bind(&path).expect("native disposable Unix socket fixture");
    // The special file is never opened. Bound the assertion so a regression
    // fails instead of hanging the test suite.
    let (send, receive) = mpsc::channel();
    let reader = thread::spawn(move || {
        send.send(open_unchanged(&path, &identity))
            .expect("test receiver");
    });
    let result = receive
        .recv_timeout(Duration::from_secs(5))
        .expect("replacement socket must be rejected without opening the special file");
    reader.join().expect("reader returned");
    assert!(
        result.is_err(),
        "a selected socket cannot become a readable store"
    );
    assert_eq!(
        fs::read(root.path().join("preserved")).expect("original remains"),
        b"observed regular file"
    );
}

#[test]
fn held_source_transaction_binds_metadata_and_copy_across_another_writers_commit() {
    let root = tempdir().expect("disposable source");
    let output = tempdir().expect("disposable output");
    // Production resolves the trusted parent before entering this copy seam.
    let output_path = output.path().canonicalize().expect("native output path");
    let path = root.path().join("source.db");
    let writer = Connection::open(&path).expect("fixture database");
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE fixture(value); INSERT INTO fixture VALUES('old'); PRAGMA user_version=7;").expect("first commit");
    let options = BackupOptions {
        allow_create: true,
        read: ReadOptions {
            allow_coordination_files: true,
            ..ReadOptions::default()
        },
        ..BackupOptions::default()
    };
    let snapshot = with_source(&path, options.read, |source| {
        let metadata = read_metadata(source)?;
        assert_eq!(metadata.user_version, 7);
        writer
            .execute_batch("UPDATE fixture SET value='new'; PRAGMA user_version=8;")
            .expect("independent commit after snapshot");
        copy(source, &output_path, metadata, options)
    })
    .expect("copy of held snapshot");
    assert_eq!(snapshot.source.user_version, 7);
    let copied = Connection::open_with_flags(
        output.path().join("store.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("copied database");
    let value: String = copied
        .query_row("SELECT value FROM fixture", [], |row| row.get(0))
        .expect("copied row");
    assert_eq!(value, "old");
    let current: String = writer
        .query_row("SELECT value FROM fixture", [], |row| row.get(0))
        .expect("current source");
    assert_eq!(
        current, "new",
        "backup neither blocked nor undid the separate WAL commit"
    );
}
