pub(super) fn text(html: &str) -> String {
    let mut text = String::new();
    gazette_compiler::html::inspect(html.as_bytes(), Default::default(), |kind, value| {
        if kind == gazette_compiler::html::TextKind::Text {
            text.push_str(value);
        }
        Ok::<_, std::convert::Infallible>(())
    })
    .expect("decode rendered HTML text");
    text
}
