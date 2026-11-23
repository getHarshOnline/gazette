//! Principal-only website changes survive complete normalized replacement and replay.

use super::*;
use gazette_core::publication::PrincipalWebsite;

#[test]
fn principal_websites_and_identity_references_replace_replay_and_remove() {
    let (root, mut writer) = fixture();
    let path = root.path().join(journal::DATABASE);
    let mut data = Data::new(false);
    let mut previous: Option<gazette_core::store::RegistryHead> = None;
    for website in [
        None,
        Some(PrincipalWebsite::Site {
            site: data.site.clone(),
        }),
        Some(PrincipalWebsite::External {
            url: "https://writer.test/".into(),
        }),
        None,
    ] {
        data.principal.website = website;
        data.principal.same_as = if data.principal.website.is_some() {
            vec![
                "https://identity.test/one".into(),
                "https://identity.test/two".into(),
            ]
        } else {
            vec![]
        };
        let model = data.model();
        let documents = data.documents();
        let receipt = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .unwrap();
        let connection = Connection::open(&path).unwrap();
        let actual: Vec<(String, Option<String>, Option<String>)> = connection
            .prepare("SELECT kind,site,url FROM gazette_registry_principal_websites WHERE principal='person:one'").unwrap()
            .query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap()
            .collect::<Result<_,_>>().unwrap();
        let expected = match &data.principal.website {
            None => vec![],
            Some(PrincipalWebsite::Site { site }) => {
                vec![("site".into(), Some(site.to_string()), None)]
            }
            Some(PrincipalWebsite::External { url }) => {
                vec![("external".into(), None, Some(url.clone()))]
            }
        };
        assert_eq!(actual, expected);
        let identities: Vec<String> = connection.prepare("SELECT url FROM gazette_registry_principal_same_as WHERE principal='person:one' ORDER BY ordinal").unwrap()
            .query_map([], |row| row.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
        assert_eq!(identities, data.principal.same_as);
        assert_eq!(
            Sqlite.inspect_registry(&path, options()).unwrap().registry,
            Some(receipt.registry)
        );
        if let Some(prior) = previous {
            assert_ne!(prior.semantic_sha256, receipt.registry.semantic_sha256);
            assert_ne!(prior.rows_sha256, receipt.registry.rows_sha256);
        }
        let replay = writer
            .replace_registry(
                input(&model, &documents),
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .unwrap();
        assert_eq!(replay.registry, receipt.registry);
        previous = Some(receipt.registry);
    }
    writer.close().unwrap();
}
