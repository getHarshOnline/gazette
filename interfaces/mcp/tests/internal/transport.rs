//! Bounds are applied before the protocol decoder and reset only at newline.

use tokio::io::AsyncReadExt;

use super::*;

#[test]
fn exact_per_line_bounds_survive_one_byte_reads_and_multiple_frames() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let (failed, _failure) = Failure::new();
        let mut reader = BoundedLines::new(
            std::io::Cursor::new(b"abcd\nabcd\n".to_vec()),
            4,
            Arc::clone(&failed),
        );
        let mut actual = Vec::new();
        let mut byte = [0];
        while reader.read(&mut byte).await.expect("admitted byte") != 0 {
            actual.push(byte[0]);
        }
        assert_eq!(actual, b"abcd\nabcd\n");
        assert!(!failed.observed());
        let mut reader = BoundedLines::new(
            std::io::Cursor::new(b"abcde\nx\n".to_vec()),
            4,
            Arc::clone(&failed),
        );
        let mut actual = Vec::new();
        assert!(reader.read_to_end(&mut actual).await.is_err());
        assert_eq!(actual, b"abcd");
        assert!(failed.observed());
        assert!(
            reader.read(&mut byte).await.is_err(),
            "no resynchronization after rejected bytes"
        );
    });
}
