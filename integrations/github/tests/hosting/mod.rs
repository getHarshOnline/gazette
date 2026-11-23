use gazette_core::{hosting::HostingProvider, source::Limits};
use gazette_ports::hosting::HostFiles;

#[test]
fn custom_domain_files_are_complete_and_bounded_without_delivery() {
    let provider = gazette_integration_github::HostFiles;
    let base = "https://publication.example/subpath/".parse().unwrap();
    let tree = provider
        .generate(HostingProvider::GithubPages, &base, Limits::default())
        .unwrap();
    assert_eq!(
        tree.file(&"CNAME".parse().unwrap()).unwrap(),
        b"publication.example\n"
    );
    assert_eq!(tree.file(&".nojekyll".parse().unwrap()).unwrap(), b"");
    let limits = Limits {
        file_bytes: 1,
        ..Limits::default()
    };
    assert!(
        provider
            .generate(HostingProvider::GithubPages, &base, limits)
            .is_err()
    );
    let limits = Limits {
        entries: 1,
        ..Limits::default()
    };
    assert!(
        provider
            .generate(HostingProvider::GithubPages, &base, limits)
            .is_err()
    );
    let default = "https://publisher.github.io/project/".parse().unwrap();
    let tree = provider
        .generate(HostingProvider::GithubPages, &default, Limits::default())
        .unwrap();
    assert!(tree.file(&"CNAME".parse().unwrap()).is_none());
    assert!(tree.file(&".nojekyll".parse().unwrap()).is_some());
    for address in ["http://127.0.0.1:8080/", "http://localhost:8080/"] {
        assert!(
            provider
                .generate(
                    HostingProvider::GithubPages,
                    &address.parse().unwrap(),
                    Limits::default()
                )
                .is_err()
        );
    }
}
