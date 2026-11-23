use super::*;

#[test]
fn only_selected_url_bases_project_and_suffixes_survive() {
    let map = PreviewLinkBases::try_from(BTreeMap::from([
        (
            "https://docs.example.test/".into(),
            "http://127.0.0.1:4000/".parse().unwrap(),
        ),
        (
            "https://example.test/help/".into(),
            "http://[::1]:4001/help/".parse().unwrap(),
        ),
    ]))
    .unwrap();
    assert_eq!(
        map.project("https://docs.example.test/tool/install?q=a%20b#build")
            .unwrap()
            .unwrap(),
        "http://127.0.0.1:4000/tool/install?q=a%20b#build"
    );
    assert_eq!(
        map.project("https://example.test/help/start/#edit")
            .unwrap()
            .unwrap(),
        "http://[::1]:4001/help/start/#edit"
    );
    assert_eq!(
        map.project("https://docs.example.test.evil/guide").unwrap(),
        None
    );
    assert_eq!(map.project("https://example.test/other/").unwrap(), None);
    for (public, local) in [
        ("https://docs.example.test", "http://127.0.0.1:4000/"),
        (
            "https://docs.example.test?q=1#top",
            "http://127.0.0.1:4000/?q=1#top",
        ),
    ] {
        assert_eq!(map.project(public).unwrap().as_deref(), Some(local));
    }
    for address in [
        "https://docs.example.test:443",
        "https://docs.example.test.evil",
        "https://example.test/help",
    ] {
        assert_eq!(map.project(address).unwrap(), None);
    }
    for path in ["../secret", "%2e%2e/secret", "a%2Fb", "%GG"] {
        assert!(
            map.project(&format!("https://docs.example.test/{path}"))
                .is_err()
        );
    }
}

#[test]
fn targets_require_literal_loopback_and_the_original_mount() {
    for target in [
        "https://127.0.0.1:4000/help/",
        "http://localhost:4000/help/",
        "http://192.0.2.1:4000/help/",
        "http://127.0.0.1:0/help/",
        "http://127.0.0.1:4000/other/",
    ] {
        let map = BTreeMap::from([("https://example.test/help/".into(), target.parse().unwrap())]);
        assert!(PreviewLinkBases::try_from(map).is_err(), "{target}");
    }
}

#[test]
fn nested_mount_uses_the_most_specific_provider() {
    let map = PreviewLinkBases::try_from(BTreeMap::from([
        (
            "https://example.test/".into(),
            "http://127.0.0.1:4000/".parse().unwrap(),
        ),
        (
            "https://example.test/help/".into(),
            "http://127.0.0.1:4001/help/".parse().unwrap(),
        ),
    ]))
    .unwrap();
    assert_eq!(
        map.project("https://example.test/help/start?q=1#next")
            .unwrap()
            .as_deref(),
        Some("http://127.0.0.1:4001/help/start?q=1#next")
    );
    assert_eq!(
        map.project("https://example.test/helpful/")
            .unwrap()
            .as_deref(),
        Some("http://127.0.0.1:4000/helpful/")
    );
}
