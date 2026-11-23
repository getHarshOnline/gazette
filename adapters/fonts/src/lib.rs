//! Qualified validation and exact-byte delivery of retained WOFF2 webfonts.

use std::collections::BTreeSet;

use gazette_core::{
    font::{self, Decoded, Limits as FontLimits},
    resource::{Artifact, Error, Limits, Recipe},
    source::{Sha256Digest, TreeInput},
};
use gazette_ports::{font::FontInspector, resource::ResourceCompiler};

/// Complete selected WOFF2 decoder and SFNT metadata-validation profile.
pub const PROFILE: &str =
    "gazette.woff2/1;wuff/0.2.8;brotli;read-fonts/0.39.2;head+maxp+name;exact-retained-bytes";

/// Stateless retained-byte WOFF2 inspector.
#[derive(Clone, Copy, Debug, Default)]
pub struct Inspector;

impl FontInspector for Inspector {
    fn inspect(&self, bytes: &[u8], limits: FontLimits) -> Result<Decoded, font::Error> {
        inspect(bytes, limits)
    }
}

/// Deterministic exact-byte WOFF2 resource compiler.
#[derive(Clone, Copy, Debug, Default)]
pub struct Compiler;

impl ResourceCompiler for Compiler {
    fn profile(&self) -> &'static str {
        PROFILE
    }

    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &Recipe,
        links: &gazette_core::resource::ModuleLinks,
        limits: Limits,
    ) -> Result<Artifact, Error> {
        let Recipe::Woff2Font { entry } = recipe else {
            return Err(Error::Input {
                path: recipe.entry().clone(),
                reason: "WOFF2 compiler requires a WOFF2 font recipe",
            });
        };
        if !links.is_empty() {
            return Err(Error::Input {
                path: entry.clone(),
                reason: "WOFF2 recipes do not accept module links",
            });
        }
        limits.validate_input(source.inventory())?;
        if source.inventory().entries().len() != 1 {
            return Err(Error::Input {
                path: entry.clone(),
                reason: "WOFF2 recipe requires exactly one retained file",
            });
        }
        let bytes = source.file(entry).ok_or_else(|| Error::Input {
            path: entry.clone(),
            reason: "WOFF2 entry must be the sole retained regular file",
        })?;
        if bytes.len() > limits.output_bytes {
            return Err(Error::Limit("WOFF2 output bytes"));
        }
        inspect(bytes, limits.font).map_err(font_error)?;
        Ok(Artifact {
            input: source.inventory().identity(),
            bytes: bytes.to_vec(),
            dependencies: BTreeSet::from([entry.clone()]),
            references: BTreeSet::new(),
            deferred_references: Default::default(),
            svg: None,
            image: None,
            tool: PROFILE,
        })
    }
}

fn inspect(bytes: &[u8], limits: FontLimits) -> Result<Decoded, font::Error> {
    if bytes.len() > limits.encoded_bytes {
        return Err(font::Error::Limit("encoded bytes"));
    }
    let header = header(bytes, limits)?;
    let decoded = wuff::decompress_woff2(bytes).map_err(|_| font::Error::Decode)?;
    if decoded.len() > limits.decoded_bytes {
        return Err(font::Error::Limit("decoded bytes"));
    }
    let font = read_fonts::FontRef::new(&decoded).map_err(|_| font::Error::Decode)?;
    use read_fonts::TableProvider as _;
    font.head().map_err(|_| font::Error::Decode)?;
    font.name().map_err(|_| font::Error::Decode)?;
    let glyphs = font.maxp().map_err(|_| font::Error::Decode)?.num_glyphs();
    let inspection = font::Inspection {
        encoded_bytes: bytes.len(),
        decoded_bytes: decoded.len(),
        tables: header.tables,
        glyphs,
        sha256: Sha256Digest::of(bytes),
        decoded_sha256: Sha256Digest::of(&decoded),
        tool: PROFILE,
    };
    inspection.validate(bytes, &decoded, limits)?;
    Ok(Decoded {
        inspection,
        bytes: decoded,
    })
}

#[derive(Clone, Copy)]
struct Header {
    tables: u16,
}

fn header(bytes: &[u8], limits: FontLimits) -> Result<Header, font::Error> {
    let required = bytes.get(..48).ok_or(font::Error::Decode)?;
    if &required[..4] != b"wOF2" {
        return Err(font::Error::Decode);
    }
    if !matches!(&required[4..8], [0, 1, 0, 0] | b"OTTO") {
        return Err(font::Error::Unsupported(
            "only single TrueType- or CFF-flavored WOFF2 is admitted",
        ));
    }
    let declared = u32::from_be_bytes(required[8..12].try_into().expect("fixed header field"));
    if usize::try_from(declared).ok() != Some(bytes.len()) || required[14..16] != [0, 0] {
        return Err(font::Error::Decode);
    }
    let tables = u16::from_be_bytes(required[12..14].try_into().expect("fixed header field"));
    let decoded = u32::from_be_bytes(required[16..20].try_into().expect("fixed header field"));
    if tables == 0 || tables > limits.tables {
        return Err(font::Error::Limit("table count"));
    }
    if decoded == 0 || u64::from(decoded) > limits.decoded_bytes as u64 {
        return Err(font::Error::Limit("decoded bytes"));
    }
    Ok(Header { tables })
}

fn font_error(error: font::Error) -> Error {
    match error {
        font::Error::Limit(resource) => Error::Limit(resource),
        font::Error::Unsupported(reason) => Error::Compile(reason.into()),
        font::Error::Decode => {
            Error::Compile("WOFF2 decoding or metadata validation failed".into())
        }
        font::Error::Binding => Error::Compile("WOFF2 evidence binding failed".into()),
    }
}
