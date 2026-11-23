use super::*;

#[test]
fn native_companions_have_exact_literal_ignore_paths_and_shared_bounds() {
    let mut output = Output::new(Limits::default());
    output.provider_companion("rust/type.[T]*?.md").unwrap();
    output.provider_companion("rust/space name.md").unwrap();
    let ignores = output.provider_ignores(b"/artifacts/\n").unwrap();
    assert_eq!(
        ignores,
        b"/artifacts/\n/rust/space\\ name.md\n/rust/type.\\[T\\]\\*\\?.md\n"
    );
    let limits = Limits {
        file_bytes: 10,
        ..Default::default()
    };
    let mut output = Output::new(limits);
    output.provider_companion("rust/reference.md").unwrap();
    assert!(output.provider_ignores(b"").is_err());
    output.add_owned("page.md", b"native".to_vec()).unwrap();
    assert!(
        output
            .add_owned("page.mdx", b"literal provider".to_vec())
            .is_err()
    );
}
