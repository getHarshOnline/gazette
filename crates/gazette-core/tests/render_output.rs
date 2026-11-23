//! Observable output identities and byte-writer boundaries, independent of rendering providers.

use std::io::{self, Write};

use gazette_core::render::{LimitedWriter, MediaType};

#[test]
fn execution_depth_has_one_native_default_and_schema_admission_range() {
    let defaults: gazette_core::render::Limits =
        serde_json::from_str("{}").expect("native omitted-field defaults");
    assert_eq!(defaults.execution_depth, 64);
    assert_eq!(defaults, gazette_core::render::Limits::default());
    let schema = serde_json::to_value(schemars::schema_for!(gazette_core::render::Limits))
        .expect("native render limits schema");
    let execution = &schema["properties"]["execution_depth"];
    assert_eq!(execution["minimum"], 1);
    assert_eq!(execution["maximum"], 64);
    assert_eq!(execution["default"], 64);
}

#[test]
fn declared_media_types_serialize_as_provider_independent_representation_names() {
    let representations = [
        MediaType::Html,
        MediaType::Json,
        MediaType::JsonLd,
        MediaType::Xml,
        MediaType::Rss,
        MediaType::Atom,
        MediaType::Text,
        MediaType::Markdown,
        MediaType::JavaScript,
        MediaType::Css,
        MediaType::Png,
        MediaType::Jpeg,
        MediaType::Svg,
        MediaType::Woff2,
    ];
    assert_eq!(
        serde_json::to_value(representations).expect("native media declarations"),
        serde_json::json!([
            "html",
            "json",
            "json-ld",
            "xml",
            "rss",
            "atom",
            "text",
            "markdown",
            "javascript",
            "css",
            "png",
            "jpeg",
            "svg",
            "woff2"
        ])
    );
}

struct Recording {
    bytes: Vec<u8>,
    attempts: usize,
    flushes: usize,
    chunk: usize,
    fail_next_write: bool,
    fail_flush: bool,
}

impl Default for Recording {
    fn default() -> Self {
        Self {
            bytes: Vec::new(),
            attempts: 0,
            flushes: 0,
            chunk: usize::MAX,
            fail_next_write: false,
            fail_flush: false,
        }
    }
}

impl Write for Recording {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.attempts += 1;
        if self.fail_next_write {
            self.fail_next_write = false;
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "native write rejection",
            ));
        }
        let written = bytes.len().min(self.chunk);
        self.bytes.extend_from_slice(&bytes[..written]);
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        if self.fail_flush {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "native flush rejection",
            ));
        }
        Ok(())
    }
}

#[test]
fn zero_and_exact_allowances_do_not_truncate_or_forward_overflow() {
    let mut zero = LimitedWriter::new(Recording::default(), 0);
    assert_eq!(zero.write(b"").expect("empty request"), 0);
    assert!(!zero.exceeded());
    assert_eq!(
        zero.write(b"x").expect_err("zero ceiling").kind(),
        io::ErrorKind::Other
    );
    assert!(zero.exceeded());
    let inner = zero.into_inner();
    assert_eq!(inner.attempts, 1, "only empty request reached the writer");
    assert!(inner.bytes.is_empty());

    let mut exact = LimitedWriter::new(Recording::default(), 4);
    exact.write_all(b"ab").expect("prefix");
    exact.write_all(b"cd").expect("exact bound");
    assert!(!exact.exceeded());
    assert!(exact.write(b"e").is_err());
    assert!(exact.exceeded());
    let inner = exact.into_inner();
    assert_eq!(inner.bytes, b"abcd");
    assert_eq!(inner.attempts, 2, "overflow never reached native writer");
}

#[test]
fn overflow_is_sticky_even_when_a_later_smaller_request_and_flush_succeed() {
    let mut writer = LimitedWriter::new(Recording::default(), 3);
    assert!(writer.write(b"four").is_err());
    assert!(writer.exceeded());
    writer
        .write_all(b"ok")
        .expect("remaining allowance is unchanged");
    writer.flush().expect("flush still reaches native writer");
    assert!(
        writer.exceeded(),
        "later success must not conceal earlier failure"
    );
    let inner = writer.into_inner();
    assert_eq!(inner.attempts, 1);
    assert_eq!(inner.bytes, b"ok");
    assert_eq!(inner.flushes, 1);
}

#[test]
fn partial_writes_charge_only_accepted_bytes_but_oversized_requests_are_not_split() {
    let mut writer = LimitedWriter::new(
        Recording {
            chunk: 2,
            ..Recording::default()
        },
        4,
    );
    assert_eq!(writer.write(b"abcd").expect("native short write"), 2);
    assert_eq!(writer.write(b"cd").expect("remaining allowance"), 2);
    assert!(!writer.exceeded());
    let inner = writer.into_inner();
    assert_eq!(inner.bytes, b"abcd");
    assert_eq!(inner.attempts, 2);

    let mut writer = LimitedWriter::new(
        Recording {
            chunk: 1,
            ..Recording::default()
        },
        3,
    );
    assert!(
        writer.write(b"four").is_err(),
        "do not ask inner writer to accept a prefix"
    );
    assert!(writer.exceeded());
    assert_eq!(writer.into_inner().attempts, 0);
}

#[test]
fn native_write_failure_preserves_error_and_does_not_charge_or_mark_overflow() {
    let mut writer = LimitedWriter::new(
        Recording {
            fail_next_write: true,
            ..Recording::default()
        },
        3,
    );
    let error = writer.write(b"abc").expect_err("original error");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "native write rejection");
    assert!(!writer.exceeded());
    writer
        .write_all(b"abc")
        .expect("failed write consumed no allowance");
    assert!(!writer.exceeded());
    let inner = writer.into_inner();
    assert_eq!(inner.bytes, b"abc");
    assert_eq!(inner.attempts, 2);
}

#[test]
fn native_zero_write_and_flush_failure_retain_standard_write_semantics() {
    let mut writer = LimitedWriter::new(
        Recording {
            chunk: 0,
            fail_flush: true,
            ..Recording::default()
        },
        3,
    );
    assert_eq!(
        writer
            .write_all(b"abc")
            .expect_err("native zero progress")
            .kind(),
        io::ErrorKind::WriteZero
    );
    assert!(!writer.exceeded());
    let error = writer.flush().expect_err("original flush error");
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(error.to_string(), "native flush rejection");
    assert!(!writer.exceeded());
    let inner = writer.into_inner();
    assert_eq!(inner.flushes, 1);
    assert_eq!(inner.attempts, 1);
    assert!(inner.bytes.is_empty());
}
