//! Default intent covers ordinary content and required assets without widening an override.

use super::*;
use gazette_sdk::{authored::Selection, publication::content::Disposition};

pub(in crate::preparation) const DOWNLOAD: &[u8] = b"Exact retained download.\n";

pub(in crate::preparation) fn fixture() -> crate::Fixture {
    let page = format!(
        "{}\n[Read the retained download](required.txt).\n",
        crate::PAGE.replace("lifecycle='draft'", "lifecycle='published'")
    );
    let extra = |state, id| {
        format!(
            "+++\n[content]\nid='content:{id}'\nsite='site:one'\ntitle='{id}'\nlifecycle='{state}'\nroute={{path='/{id}/'}}\nstructure={{kind='page',family={{kind='utility'}}}}\n+++\n{id} body.\n"
        )
    };
    let archived = extra("archived", "archived-extra");
    let draft = extra("draft", "draft-extra");
    super::candidate::captured::policy_fixture(&[
        ("one/page.md", page.as_bytes()),
        ("one/archived.md", archived.as_bytes()),
        ("one/draft.md", draft.as_bytes()),
        ("one/required.txt", DOWNLOAD),
        ("one/unrelated.txt", b"Unreferenced retained asset.\n"),
        ("project/native.md", crate::PROJECT_DOC.as_bytes()),
    ])
}

pub(in crate::preparation) fn configuration(fixture: &crate::Fixture) -> String {
    fixture.project_config(true).replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/downloads/'}}]",
    )
}

pub(in crate::preparation) fn selection(config: &str) -> Selection {
    let inputs = build::acquire_publications(
        config,
        &["site:one".parse().unwrap(), "site:other".parse().unwrap()],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let parsed = inputs.sites()[&"site:one".parse().unwrap()]
        .as_ref()
        .unwrap()
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .unwrap();
    let resolved = parsed.resolve(Default::default()).unwrap();
    let selection = resolved.authored().non_draft_selection().unwrap();
    assert_selection(&selection);
    selection
}

pub(in crate::preparation) fn assert_selection(selection: &Selection) {
    for id in [
        "collection:home",
        "content:one",
        "content:other",
        "doc:native",
        "content:archived-extra",
    ] {
        assert_eq!(
            selection.content[&id.parse().unwrap()],
            Disposition::Include,
            "{id}"
        );
    }
    assert_eq!(
        selection.content[&"content:draft-extra".parse().unwrap()],
        Disposition::Exclude
    );
    assert_eq!(selection.assets["one/required.txt"], Disposition::Include);
    assert_eq!(selection.assets["one/unrelated.txt"], Disposition::Exclude);
}

#[test]
fn unchecked_preparation_derives_non_draft_content_assets_and_respects_explicit_blocks() {
    let fixture = fixture();
    let config = configuration(&fixture);
    for explicit_block in [false, true] {
        let root = private_directory();
        let journal = initialize(root.path());
        let parent = private_directory();
        let mut request = input(&config, root.path(), journal.header, parent.path());
        if explicit_block {
            let mut selected = selection(&config);
            selected.content.insert(
                "content:archived-extra".parse().unwrap(),
                Disposition::Exclude,
            );
            request["selection"] = json!(selected);
        } else {
            request.as_object_mut().unwrap().remove("selection");
        }
        assert!(request["candidate_check"].is_null());
        let receipts = recipe(&request)
            .execute(permitted())
            .expect("unchecked local preparation");
        let receipts = json!(receipts);
        let header: store::JournalHeader =
            serde_json::from_value(receipts[1]["preparation"]["journal"].clone()).unwrap();
        let record = observed(&journal.database, "one", header);
        let output = record.intent.address.path();
        let model: Value =
            serde_json::from_slice(&fs::read(output.join("model.json")).unwrap()).unwrap();
        for id in ["collection:home", "content:one", "doc:native"] {
            assert!(model["pages"][id].is_object(), "{id}");
        }
        assert_eq!(
            model["pages"].get("content:archived-extra").is_some(),
            !explicit_block
        );
        assert!(model["pages"].get("content:draft-extra").is_none());
        assert_eq!(
            fs::read(output.join("downloads/required.txt")).unwrap(),
            DOWNLOAD
        );
        assert!(!output.join("downloads/unrelated.txt").exists());
        assert!(!output.join("draft-extra/index.html").exists());
        assert_eq!(
            output.join("archived-extra/index.html").exists(),
            !explicit_block
        );
    }
}
