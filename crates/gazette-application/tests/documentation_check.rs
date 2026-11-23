//! Documentation inspection preserves configured manual/reference intent without native effects.

use std::cell::Cell;

use gazette_application::build::check_documentation;
use gazette_compiler::{toml, workspace::DocumentationNavigation};
use gazette_core::{
    documentation::{Error, Format, Plan, PlannedProjectItem, Request},
    publication::Key,
    source::{CapturedSource, Limits},
};
use gazette_ports::documentation::DocumentationTarget;

const WORKSPACE: &str = r##"
[[principals]]
id = "owner:publisher"
kind = "person"

[[sites]]
id = "site:docs"
publisher = "owner:publisher"
deployable = false

[[projects]]
id = "project:gazette"
lifecycle = "active"
visibility = "public"
availability = "available"

[[sources]]
id = "source:gazette"
location = { kind = "repository", root = "/not-opened/gazette", commit = "0000000000000000000000000000000000000000", subdirectory = "retained" }

[[brands]]
id = "brand:gazette"
name = "Gazette"

[[brands.publications]]
site = "site:docs"
documentation_repository = { name = "gazette_doc", path = "/not-opened/gazette_doc", build = { target = { kind = "mintlify", theme = "mint", primary_color = "#123456" }, modules = [
  { project = "project:gazette", slug = "gazette", title = "Gazette", manual = { source = "source:gazette", assets = [
    { input = { kind = "file", path = "docs/logo.svg" }, path = "assets/logo.svg" }
  ] }, navigation = [
    { kind = "manual", title = "Manual", pages = [
      { title = "Start", route = "gazette/start", input = { kind = "file", path = "docs/start.md" } }
    ] },
    { kind = "reference", slug = "api", title = "API", input = { kind = "artifact", source = "source:gazette", selection = { kind = "file", path = "generated/api.md" }, format = "markdown" } }
  ] }
] } }
"##;

#[derive(Default)]
struct InspectTarget {
    preflight_called: Cell<bool>,
}

impl DocumentationTarget for InspectTarget {
    fn preflight(&self, plan: Plan<'_>) -> Result<(), Error> {
        self.preflight_called.set(true);
        assert_eq!(plan.name, "Gazette");
        assert_eq!(plan.projects.len(), 1);
        let project = &plan.projects[0];
        assert_eq!(project.slug, "gazette");
        assert_eq!(project.manual_assets[0].as_str(), "assets/logo.svg");
        assert!(matches!(
            project.navigation[0],
            PlannedProjectItem::Manual(_)
        ));
        let PlannedProjectItem::Reference(reference) = &project.navigation[1] else {
            panic!("second item remains a generated reference")
        };
        assert_eq!(reference.slug, "api");
        assert_eq!(reference.format, Format::Markdown);
        Ok(())
    }

    fn assemble(&self, _: Request<'_>, _: Limits) -> Result<CapturedSource, Error> {
        panic!("read-only documentation inspection must not assemble output")
    }
}

#[test]
fn check_preserves_complete_resolved_navigation_without_acquisition_or_assembly() {
    let target = InspectTarget::default();
    let site: Key = "site:docs".parse().expect("site key");
    let checked = check_documentation(WORKSPACE, &site, toml::Limits::default(), &target)
        .expect("effect-free documentation inspection");

    assert!(target.preflight_called.get());
    assert_eq!(checked.site, site);
    assert_eq!(checked.brand.as_str(), "brand:gazette");
    assert_eq!(checked.repository_name, "gazette_doc");
    assert_eq!(checked.repository_path, "/not-opened/gazette_doc");
    let module = &checked.build.modules[0];
    assert_eq!(module.project.as_str(), "project:gazette");
    assert_eq!(
        module.manual.as_ref().expect("manual").source.as_str(),
        "source:gazette"
    );
    assert!(matches!(
        module.navigation[0],
        DocumentationNavigation::Manual { .. }
    ));
    assert!(matches!(
        module.navigation[1],
        DocumentationNavigation::Reference { .. }
    ));
}

struct NavigationTarget;

impl DocumentationTarget for NavigationTarget {
    fn preflight(&self, plan: Plan<'_>) -> Result<(), Error> {
        let PlannedProjectItem::Reference(reference) = &plan.projects[0].navigation[1] else {
            panic!("reference retains its configured position")
        };
        let pages = reference
            .pages
            .expect("explicit navigation reaches preflight");
        assert_eq!(
            serde_json::to_value(pages).unwrap(),
            serde_json::json!([
                {"kind":"page","path":"index"},
                {"kind":"group","title":"Members","pages":[{"kind":"page","path":"types/Value"}]}
            ])
        );
        plan.validate()
    }

    fn assemble(&self, _: Request<'_>, _: Limits) -> Result<CapturedSource, Error> {
        panic!("checking reference intent must not assemble or acquire artifacts")
    }
}

#[test]
fn explicit_reference_navigation_reaches_effect_free_preflight_and_checked_projection() {
    let source = WORKSPACE
        .replace("slug = \"api\"", "slug = \"reference\"")
        .replace(
            "title = \"API\", input =",
            "title = \"API\", pages = [{ kind = 'page', path = 'index' }, { kind = 'group', title = 'Members', pages = [{ kind = 'page', path = 'types/Value' }] }], input =",
        )
        .replace(
            "{ kind = \"file\", path = \"generated/api.md\" }",
            "{ kind = \"directory\", path = \"generated\" }",
        );
    let site = "site:docs".parse().unwrap();
    let checked = check_documentation(&source, &site, Default::default(), &NavigationTarget)
        .expect("shape can be checked without opening declared repository paths");
    let DocumentationNavigation::Reference { pages, .. } = &checked.build.modules[0].navigation[1]
    else {
        panic!("reference retained")
    };
    assert_eq!(pages.as_ref().unwrap().len(), 2);
    for invalid in [
        source.replace("format = \"markdown\"", "format = \"typedoc-json\""),
        source.replace(
            "{ kind = \"directory\", path = \"generated\" }",
            "{ kind = \"file\", path = \"generated/api.md\" }",
        ),
    ] {
        assert!(
            check_documentation(&invalid, &site, Default::default(), &NavigationTarget).is_err()
        );
    }
}
