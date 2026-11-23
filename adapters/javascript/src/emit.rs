//! One bounded emitter for the final merged module; no maps or native coordinates.

use gazette_core::render::LimitedWriter;
use gazette_core::resource::Error;
use std::io::Write;
use swc_core::common::comments::{Comment, CommentKind};
use swc_core::common::{SourceMap, sync::Lrc};
use swc_core::ecma::ast::Module;
use swc_core::ecma::codegen::{Config, Emitter, text_writer::JsWriter};

pub(super) fn javascript<'a>(
    module: &Module,
    cm: Lrc<SourceMap>,
    notices: impl Iterator<Item = &'a Comment>,
    limit: usize,
) -> Result<Vec<u8>, Error> {
    let mut output = LimitedWriter::new(Vec::new(), limit);
    let result = {
        let writer = JsWriter::new(cm.clone(), "\n", &mut output, None);
        let mut emitter = Emitter {
            cfg: Config::default().with_ascii_only(false).with_minify(true),
            cm,
            comments: None,
            wr: writer,
        };
        emitter.emit_module(module)
    };
    // A footer keeps any native hashbang first. Legal text is retained verbatim,
    // with its parsed comment kind, inside the same complete-output byte ceiling.
    let result = result.and_then(|()| {
        for notice in notices {
            match notice.kind {
                CommentKind::Line => writeln!(output, "\n//{}", notice.text)?,
                CommentKind::Block => writeln!(output, "\n/*{}*/", notice.text)?,
            }
        }
        Ok(())
    });
    if output.exceeded() {
        return Err(Error::Limit("resource output bytes"));
    }
    result.map_err(|error| Error::Compile(format!("browser emission: {error}")))?;
    let bytes = output.into_inner();
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| Error::Compile(format!("browser emission UTF-8: {error}")))?;
    crate::validation::module(text, false)
        .map_err(|error| Error::Compile(format!("emitted browser module: {error}")))?;
    Ok(bytes)
}
