//! Social capture settings inherit as one recipe with the declaring template namespace.

use super::{EffectiveTheme, Error, Graph, id, parse};

fn source(
    package: &str,
    namespace: &str,
    recipe: Option<(&str, u32)>,
    dependencies: &[&str],
) -> String {
    let recipe = recipe.map(|(template, width)| format!(",social_card={{template='{template}',width={width},height=630,media_selector='img.hero',media_kind='image'}}")).unwrap_or_default();
    let dependencies = dependencies
        .iter()
        .map(|package| format!("[[dependencies]]\npackage='{package}'\n"))
        .collect::<String>();
    format!(
        "package='{package}'\nnamespace='{namespace}'\ntemplates='templates'\nrender={{page='page.html'{recipe}}}\n{dependencies}"
    )
}

#[test]
fn social_card_inherits_through_diamonds_and_nearest_override_is_atomic() {
    let base_source = source("theme:base", "base", Some(("card.html", 1200)), &[]);
    let base = parse(&base_source);
    let branch_source = source("theme:branch", "branch", None, &["theme:base"]);
    let branch = parse(&branch_source);
    for (middle_recipe, root_recipe, owner, width) in [
        (None, None, "base", 1200),
        (Some(("middle.html", 900)), None, "middle", 900),
        (
            Some(("middle.html", 900)),
            Some(("root.html", 600)),
            "root",
            600,
        ),
    ] {
        let middle_source = source("theme:middle", "middle", middle_recipe, &["theme:base"]);
        let middle = parse(&middle_source);
        // A shared inherited provider is deduplicated across both branches. An
        // independent override would require an explicit root recipe instead.
        let dependencies: &[&str] = if middle_recipe.is_none() {
            &["theme:middle", "theme:branch"]
        } else {
            &["theme:middle"]
        };
        let root_source = source("theme:root", "root", root_recipe, dependencies);
        let root = parse(&root_source);
        let graph = Graph::resolve(
            &id("theme:root"),
            [
                branch.manifest(),
                root.manifest(),
                base.manifest(),
                middle.manifest(),
            ],
        )
        .unwrap();
        let effective = EffectiveTheme::resolve(&graph).unwrap();
        let recipe = effective.render().social_card.as_ref().unwrap();
        assert_eq!(recipe.width, width);
        let template = match owner {
            "base" => "card.html",
            "middle" => "middle.html",
            _ => "root.html",
        };
        assert_eq!(recipe.template.as_str(), format!("{owner}/{template}"));
        assert!(
            effective
                .render()
                .entries()
                .any(|entry| entry == &recipe.template)
        );
    }
}

#[test]
fn social_card_independent_providers_conflict_even_when_template_spelling_matches() {
    let first_source = source("theme:first", "first", Some(("card.html", 1200)), &[]);
    let second_source = source("theme:second", "second", Some(("card.html", 800)), &[]);
    let first = parse(&first_source);
    let second = parse(&second_source);
    for own in [false, true] {
        let root_source = source(
            "theme:root",
            "root",
            own.then_some(("selected.html", 1000)),
            &["theme:second", "theme:first"],
        );
        let root = parse(&root_source);
        let graph = Graph::resolve(
            &id("theme:root"),
            [root.manifest(), first.manifest(), second.manifest()],
        )
        .unwrap();
        let resolved = EffectiveTheme::resolve(&graph);
        if own {
            let effective = resolved.unwrap();
            let recipe = effective.render().social_card.as_ref().unwrap();
            assert_eq!(recipe.template.as_str(), "root/selected.html");
            assert_eq!(recipe.width, 1000);
        } else {
            assert!(
                matches!(resolved, Err(Error::RenderCollision {entry, packages}) if entry == "social_card" && packages == [id("theme:first"),id("theme:second")])
            );
        }
    }
}

#[test]
fn card_only_requirements_follow_atomic_recipe_inheritance() {
    let base_source = source("theme:base", "base", Some(("card.html", 1200)), &[]).replace(
        "media_kind='image'",
        "media_kind='image',resource_requirements=['stylesheet:card','module:media']",
    );
    let base = parse(&base_source);
    for own in [false, true] {
        let root_source = source(
            "theme:root",
            "root",
            own.then_some(("own.html", 900)),
            &["theme:base"],
        );
        let root = parse(&root_source);
        let graph = Graph::resolve(&id("theme:root"), [base.manifest(), root.manifest()]).unwrap();
        let effective = EffectiveTheme::resolve(&graph).unwrap();
        let recipe = effective.render().social_card.as_ref().unwrap();
        assert_eq!(recipe.resource_requirements.len(), if own { 0 } else { 2 });
        assert!(
            effective.resource_requirements().is_empty(),
            "card requirements never become ordinary page requirements"
        );
    }
}
