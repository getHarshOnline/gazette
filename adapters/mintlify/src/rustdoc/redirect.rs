//! Resolve exact native Rustdoc forwarding pages without executing JavaScript.

use crate::{Error, invalid};
use gazette_core::source::EntryPath;

pub(super) fn target(bytes: &[u8], path: &EntryPath) -> Result<Option<EntryPath>, Error> {
    let file = path.as_str().rsplit('/').next().unwrap_or_default();
    let macro_bang = file.starts_with("macro.") && file.ends_with("!.html");
    if macro_bang {
        let name = &file["macro.".len()..file.len() - "!.html".len()];
        if name.is_empty()
            || !name
                .chars()
                .all(|character| character.is_alphanumeric() || character == '_')
        {
            return Err(invalid(
                path.to_string(),
                "unsupported Rustdoc macro redirect name",
            ));
        }
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| invalid(path.to_string(), "non-UTF-8 Rustdoc page"))?;
    let marker = "<meta http-equiv=\"refresh\" content=\"0;URL=";
    let Some(destination) = text
        .split_once(marker)
        .and_then(|(_, value)| value.split_once('"').map(|(target, _)| target))
    else {
        if macro_bang {
            return Err(invalid(
                path.to_string(),
                "unqualified Rustdoc macro redirect envelope",
            ));
        }
        return Ok(None);
    };
    let reject = || {
        invalid(
            path.to_string(),
            if macro_bang {
                "unqualified Rustdoc macro redirect envelope"
            } else {
                "unqualified Rustdoc redirect envelope"
            },
        )
    };
    if destination.is_empty()
        || !destination
            .chars()
            .all(|ch| ch.is_alphanumeric() || "_./-!".contains(ch))
    {
        return Err(reject());
    }
    if macro_bang && destination != file.replace("!.html", ".html") {
        return Err(reject());
    }
    // Qualify the entire observed native wrapper, including agreement between
    // its link, refresh and script. Extra content cannot be silently discarded.
    let expected = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta http-equiv="refresh" content="0;URL={destination}">
    <title>Redirection</title>
</head>
<body>
    <p>Redirecting to <a href="{destination}">{destination}</a>...</p>
    <script>location.replace("{destination}" + location.search + location.hash);</script>
</body>
</html>"#
    );
    if bytes != expected.as_bytes() {
        return Err(reject());
    }
    let super::links::Destination::Local {
        path: target,
        fragment: None,
    } = super::links::resolve(path.as_str(), destination)?
    else {
        return Err(reject());
    };
    if !target.as_str().ends_with(".html") || target == *path {
        return Err(reject());
    }
    Ok(Some(target))
}
