//! Shared lexical source coordinates; callers retain selection and confinement policy.

use std::borrow::Cow;

use super::{Error, Span, references::decode_reference};

pub(crate) fn is_external(path: &str) -> bool {
    path.starts_with('/')
        || path
            .find(':')
            .is_some_and(|colon| !path[..colon].contains('/'))
}

pub(crate) fn decode(path: &str, span: Span) -> Result<Vec<Cow<'_, str>>, Error> {
    path.split('/')
        .map(|component| {
            let part = decode_reference(component, span)?;
            if part.contains(['/', '\\'])
                || (matches!(part.as_ref(), "." | "..") && part != component)
            {
                return Err(Error::LinkResolution {
                    span,
                    reason: "encoded separator, traversal or control in source reference",
                });
            }
            Ok(part)
        })
        .collect()
}

pub(crate) fn reduce(entry: &str, decoded: Vec<Cow<'_, str>>, span: Span) -> Result<String, Error> {
    let failure = |reason| Error::LinkResolution { span, reason };
    let mut components: Vec<_> = entry
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    for component in decoded {
        match component.as_ref() {
            "." => (),
            ".." => {
                components
                    .pop()
                    .ok_or_else(|| failure("reference escapes its configured source"))?;
            }
            "" => return Err(failure("empty component in source reference")),
            _ => components.push(component.into_owned()),
        }
    }
    Ok(components.join("/"))
}
