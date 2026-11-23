//! Real TOML declarations and typed references share exact original source coordinates.

use gazette_compiler::{
    toml::{self, PathSegment},
    workspace::{self, DeclarationKind, FieldQuery, ParsedWorkspace},
};
use gazette_core::source::Precision;

const SOURCE: &str = r#"# 🦀 decoy publisher='entity:same'
policy_check = {program='/not-opened/checker',directory='/not-opened/work'}
policy_source = {source='entity:same',entry='entity:same'}
composition.theme = { package='theme:base', palette='entity:same' }
[composition.plugins]
'entity-plugin' = {state='enabled'}
[composition.capabilities]
'navigation.z/v1' = {state='enabled',provider={kind='plugin',package='entity-plugin',provider='same'},requirement='required'}
'navigation.same/v1' = {state='enabled',provider={kind='plugin',package='entity-plugin',provider='same'},requirement='optional'}
'navigation.off/v1' = {state='disabled'}
[[sites]]
id = 'site:z'
publisher = "entity\u003asame"
deployable = false
[sites.composition]
theme = {package='theme:local'}
[sites.composition.capabilities]
'navigation.local/v1' = {state='enabled',provider={kind='plugin',package='entity-plugin',provider='same'},requirement='required'}
'navigation.workspace/v1' = {state='enabled',provider={kind='plugin',package='entity-plugin',provider='same'},requirement='optional'}
[[sites]]
id = 'site:a'
publisher = 'entity:same'
deployable = false
[[principals]]
id = 'entity:same'
kind = 'person'
name = 'entity:same'
[[sources]]
id = 'entity:same'
location = {kind='repository',root='/not-opened/entity:same',commit='1111111111111111111111111111111111111111',subdirectory='docs'}
[[theme_packages]]
package = 'theme:base'
source = 'entity:same'
manifest = 'theme.toml'
[[theme_packages]]
package = 'theme:local'
source = 'entity:same'
manifest = 'local.toml'
[[plugin_packages]]
package = 'entity-plugin'
source = 'entity:same'
manifest = 'plugin.toml'
[[projects]]
id = 'project:one'
lifecycle = 'active'
visibility = 'public'
availability = 'available'
[[taxonomies]]
id = 'taxonomy:one'
label = 'entity:same'
terms = [{id='entity:same',label='entity:same'}]
[[brands]]
id = 'brand:one'
name = 'entity:same'
[[brands.publications]]
site = 'site:z'
authored = [{source='entity:same',target='authored'}]
documentation_repository = {name='entity:same',path='/generated/one',build={target={kind='mintlify',theme='mint',primary_color='#123456'},modules=[{project='project:one',slug='one',title='entity:same',navigation=[{kind='reference',slug='api',title='entity:same',input={kind='artifact',source='entity:same',selection={kind='file',path='api.json'},format='typedoc-json'}}]}]}}
[[placements]]
role = 'primary'
site = 'site:z'
project = 'project:one'
render = true
discoverable = false
documents = {source='entity:same',target='project-docs'}
"#;

#[test]
fn replacement_slots_are_exact_strings_and_candidates_follow_native_namespaces() {
    let parsed =
        ParsedWorkspace::parse(SOURCE, Default::default()).expect("resolved native source");
    let navigation = parsed.navigation();
    for kind in [
        DeclarationKind::Principal,
        DeclarationKind::Source,
        DeclarationKind::Site,
        DeclarationKind::Project,
    ] {
        let mut seen = false;
        for symbol in navigation
            .symbols()
            .iter()
            .filter_map(|s| s.declaration())
            .filter(|s| s.kind == kind)
        {
            let association = navigation
                .at(symbol.location.span.start)
                .expect("declaration");
            assert!(
                navigation
                    .reference_at(symbol.location.span.start)
                    .is_none(),
                "declarations are not reference edits"
            );
            for occurrence in association.references {
                seen = true;
                let slot = navigation
                    .reference_at(occurrence.span.start)
                    .expect("typed reference");
                assert_eq!(slot.location.precision, Precision::Exact);
                assert_eq!(slot.kind, workspace::WorkspaceSymbolKind::Declaration(kind));
                assert_eq!(
                    slot.symbol
                        .expect("resolved reference")
                        .declaration()
                        .expect("Key declaration")
                        .kind,
                    kind
                );
                assert_eq!(
                    slot.candidates()
                        .map(|s| &s.declaration().expect("Key declaration").identity)
                        .collect::<Vec<_>>(),
                    navigation
                        .symbols()
                        .iter()
                        .filter_map(|s| s.declaration())
                        .filter(|s| s.kind == kind)
                        .map(|s| &s.identity)
                        .collect::<Vec<_>>()
                );
                assert!(
                    slot.location
                        .span
                        .slice(SOURCE)
                        .expect("scalar")
                        .starts_with(['\'', '"'])
                );
            }
        }
        assert!(seen, "actual typed references for {kind:?}");
    }
    assert!(
        navigation
            .reference_at(SOURCE.find("🦀").expect("comment"))
            .is_none()
    );
    let offset = SOURCE.find("entity\\u003asame").expect("escaped reference");
    assert_eq!(
        navigation
            .reference_at(offset)
            .expect("exact escaped scalar")
            .location
            .span
            .slice(SOURCE),
        Some("\"entity\\u003asame\"")
    );
}

fn symbol<'a>(
    navigation: &'a workspace::WorkspaceNavigation,
    kind: DeclarationKind,
    id: &str,
) -> &'a workspace::DeclarationSymbol {
    navigation
        .symbols()
        .iter()
        .filter_map(|s| s.declaration())
        .find(|symbol| symbol.kind == kind && symbol.identity.as_str() == id)
        .expect("exact declaration namespace")
}

#[test]
fn all_typed_reference_families_use_native_source_spans_and_one_symbol_owner() {
    let source = SOURCE.replace('\n', "\r\n");
    let parsed =
        ParsedWorkspace::parse(&source, Default::default()).expect("complete typed workspace");
    let navigation = parsed.navigation();
    assert_eq!(navigation.symbols().len(), 7);
    assert_eq!(
        navigation.occurrence_count(),
        20,
        "7 definitions plus all 13 explicit source, identity and package-capture reference fields"
    );
    assert_eq!(
        navigation.key_bytes(),
        navigation
            .symbols()
            .iter()
            .map(|symbol| symbol.name().len())
            .sum::<usize>()
    );
    assert!(
        navigation
            .symbols()
            .windows(2)
            .all(|pair| pair[0].location().span.start < pair[1].location().span.start)
    );
    assert_eq!(
        navigation.symbols()[0].name(),
        "site:z",
        "not resolver key order"
    );
    assert_eq!(
        parsed
            .compilation()
            .workspace
            .sites()
            .next()
            .expect("sorted semantic owner")
            .id
            .as_str(),
        "site:a"
    );
    for (kind, id, count) in [
        (DeclarationKind::Principal, "entity:same", 2),
        (DeclarationKind::Source, "entity:same", 7),
        (DeclarationKind::Site, "site:z", 2),
        (DeclarationKind::Site, "site:a", 0),
        (DeclarationKind::Project, "project:one", 2),
        (DeclarationKind::Taxonomy, "taxonomy:one", 0),
        (DeclarationKind::Brand, "brand:one", 0),
    ] {
        let target = symbol(navigation, kind, id);
        let association = navigation
            .at(target.location.span.start + 1)
            .expect("declaration association");
        assert!(std::ptr::eq(
            association.symbol.declaration().expect("Key declaration"),
            target
        ));
        assert_eq!(association.definition, target.location);
        assert_eq!(association.references.len(), count, "{kind:?} references");
        assert_eq!(target.location.precision, Precision::Construct);
        assert!(
            association
                .references
                .windows(2)
                .all(|pair| pair[0].span.start < pair[1].span.start)
        );
        for reference in association.references {
            assert_eq!(reference.precision, Precision::Construct);
            let found = navigation
                .at(reference.span.start + 1)
                .expect("exact reference occurrence");
            assert_eq!(found.definition, target.location);
            assert_eq!(found.occurrence, reference);
            let value = reference
                .span
                .slice(&source)
                .expect("valid UTF-8 construct");
            assert!(value == format!("'{id}'") || value == "\"entity\\u003asame\"");
        }
        let origin = parsed
            .origin(&FieldQuery {
                kind,
                identity: id.parse().expect("key"),
                field: vec![PathSegment::Key("id".into())],
            })
            .expect("same declaration origin");
        assert_eq!(origin.span, target.location.span);
    }
    let (compilation, retained) = parsed.into_parts();
    assert_eq!(
        compilation,
        workspace::compile(&source, Default::default()).expect("ordinary compiler")
    );
    assert_eq!(retained.occurrence_count(), 20);
}

#[test]
fn policy_source_has_exact_typed_definition_and_replacement_without_linking_its_entry() {
    let source = SOURCE.replace('\n', "\r\n");
    let parsed = ParsedWorkspace::parse(&source, Default::default()).expect("policy workspace");
    let navigation = parsed.navigation();
    let policy_scalar = source
        .find("policy_source = {source=")
        .expect("policy source field")
        + "policy_source = {source=".len();
    let policy = navigation
        .at(policy_scalar + 1)
        .expect("policy source association");
    let target = symbol(navigation, DeclarationKind::Source, "entity:same");
    assert_eq!(policy.definition, target.location);
    assert_eq!(policy.occurrence.span.slice(&source), Some("'entity:same'"));
    assert!(policy.references.contains(&policy.occurrence));
    let replacement = navigation
        .reference_at(policy_scalar + 1)
        .expect("policy replacement scalar");
    assert_eq!(replacement.location.precision, Precision::Exact);
    assert_eq!(
        replacement.location.span.slice(&source),
        Some("'entity:same'")
    );
    assert_eq!(
        replacement.kind,
        workspace::WorkspaceSymbolKind::Declaration(DeclarationKind::Source)
    );
    assert_eq!(
        replacement.symbol.and_then(|s| s.declaration()),
        Some(target)
    );
    assert!(replacement.candidates().all(|candidate| candidate.kind()
        == workspace::WorkspaceSymbolKind::Declaration(DeclarationKind::Source)));
    let entry = source
        .find("entry='entity:same'")
        .expect("literal policy entry")
        + "entry='".len();
    assert!(navigation.at(entry).is_none());
    assert!(navigation.reference_at(entry).is_none());
}

#[test]
fn names_provider_keys_paths_comments_and_eof_are_not_reference_syntax() {
    let parsed = ParsedWorkspace::parse(SOURCE, Default::default()).expect("native workspace");
    for marker in [
        "decoy publisher='entity:same'",
        "palette='entity:same'",
        "provider='same'",
        "name = 'entity:same'",
        "label = 'entity:same'",
        "root='/not-opened/entity:same'",
        "entry='entity:same'",
        "title='entity:same'",
        "terms = [{id='entity:same'",
        "'navigation.same/v1' = {state=",
        "'navigation.off/v1'",
    ] {
        let start = SOURCE.find(marker).expect("literal decoy");
        let offset = start + marker.find("entity:same").unwrap_or(1);
        assert!(
            parsed.navigation().at(offset).is_none(),
            "not a declared workspace reference: {marker}"
        );
    }
    assert!(parsed.navigation().at(SOURCE.len()).is_none());
    assert!(parsed.navigation().at(usize::MAX).is_none());
}

#[test]
fn inline_arrays_and_quoted_dotted_keys_keep_original_declaration_coordinates() {
    let source = "\"principals\" = [{id='principal:p', kind='person'}]\n\"sites\" = [{id='site:z',publisher='principal:p',deployable=false},{id='site:a',publisher='principal:p',deployable=false}]\n";
    let parsed =
        ParsedWorkspace::parse(source, Default::default()).expect("inline declaration arrays");
    let navigation = parsed.navigation();
    assert_eq!(
        navigation
            .symbols()
            .iter()
            .map(|symbol| symbol.name())
            .collect::<Vec<_>>(),
        ["principal:p", "site:z", "site:a"]
    );
    let target = symbol(navigation, DeclarationKind::Principal, "principal:p");
    assert_eq!(
        navigation
            .at(target.location.span.start)
            .expect("definition")
            .references
            .len(),
        2
    );
    assert_eq!(
        parsed
            .origin(&FieldQuery {
                kind: DeclarationKind::Site,
                identity: "site:z".parse().expect("site"),
                field: vec![PathSegment::Key("publisher".into())]
            })
            .expect("original array element")
            .span
            .slice(source),
        Some("'principal:p'")
    );
}

#[test]
fn invalid_current_input_preserves_native_failure_and_never_exposes_partial_navigation() {
    for source in [
        SOURCE.replace(
            "publisher = 'entity:same'",
            "publisher = 'principal:missing'",
        ),
        format!("{SOURCE}\n[[principals]]\nid='entity:same'\nkind='person'\n"),
        SOURCE.replace("deployable = false", "deployable = nope"),
    ] {
        let ordinary =
            workspace::compile(&source, Default::default()).expect_err("native invalid input");
        let parsed = ParsedWorkspace::parse(&source, Default::default())
            .expect_err("no retained partial index");
        assert_eq!(parsed.code(), ordinary.code());
    }
    for limits in [
        toml::Limits {
            source_bytes: SOURCE.len() - 1,
            ..Default::default()
        },
        toml::Limits {
            nodes: 1,
            ..Default::default()
        },
    ] {
        assert_eq!(
            ParsedWorkspace::parse(SOURCE, limits)
                .expect_err("native resource ceiling")
                .code(),
            "toml_limit"
        );
    }
}
