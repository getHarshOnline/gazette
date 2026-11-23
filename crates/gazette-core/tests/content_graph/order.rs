//! Collection order keeps containment, journey completeness and input budgets intact.

use super::*;

fn fixture(order: &[&str]) -> Declaration {
    let mut root = collection("collection:root", None);
    let ContentKind::Collection { member_order, .. } = &mut root.structure else {
        unreachable!()
    };
    *member_order = order.iter().map(|id| key(id)).collect();
    let mut input = declaration(vec![
        root,
        collection("collection:child", Some("collection:root")),
        page("page:a"),
        page("page:b"),
        page("page:c"),
        page("page:step"),
        journey(
            "journey:one",
            Some("collection:root"),
            &[("page:step", true)],
        ),
    ]);
    input.memberships = ["page:a", "page:b", "page:c"]
        .into_iter()
        .map(|id| CollectionMembership {
            content: key(id),
            collection: key("collection:root"),
        })
        .collect();
    input
}

#[test]
fn explicit_prefix_orders_all_direct_child_kinds_and_keeps_unlisted_children() {
    let input = fixture(&["page:c", "journey:one", "collection:child"]);
    let graph = resolved(input.clone());
    let ids: Vec<_> = graph
        .editorial_children(&key("collection:root"))
        .iter()
        .map(|node| node.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "page:c",
            "journey:one",
            "collection:child",
            "page:a",
            "page:b"
        ]
    );
    assert_eq!(
        graph.editorial_children(&key("journey:one"))[0].id,
        key("page:step")
    );
    assert!(graph.primary_journey(&key("page:c")).is_none());
    let mut shuffled = input;
    shuffled.nodes.reverse();
    shuffled.memberships.reverse();
    assert_eq!(graph, resolved(shuffled));
    assert_ne!(graph, resolved(fixture(&["page:a"])));
}

#[test]
fn unknown_repeated_self_and_indirect_children_refuse_instead_of_changing_residence() {
    for (order, code) in [
        (vec!["page:missing"], "collection_order_reference"),
        (vec!["page:a", "page:a"], "collection_order_duplicate"),
        (vec!["collection:root"], "collection_order_not_child"),
        (vec!["page:step"], "collection_order_not_child"),
    ] {
        assert!(
            invalid(fixture(&order))
                .iter()
                .any(|error| error.code == code)
        );
    }
    let mut outside = fixture(&["page:outside"]);
    outside.nodes.push(page("page:outside"));
    assert!(
        invalid(outside)
            .iter()
            .any(|error| error.code == "collection_order_not_child")
    );
}

#[test]
fn ordering_references_are_charged_before_resolution_and_survive_owner_selection() {
    let input = fixture(&["page:c"]);
    let graph = resolved(input.clone());
    let mut decisions = graph
        .nodes()
        .map(|node| {
            (
                node.id.clone(),
                gazette_core::publication::content::Disposition::Include,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    decisions.insert(
        key("page:c"),
        gazette_core::publication::content::Disposition::Exclude,
    );
    assert!(matches!(
        graph.into_selected(&decisions, Limits::default()),
        Err(Error::Invalid(_))
    ));
    let limits = Limits {
        edges: 6,
        ..Limits::default()
    };
    assert!(Graph::resolve(fixture(&[]), limits).is_ok());
    assert!(matches!(
        Graph::resolve(input, limits),
        Err(Error::Limit("edges"))
    ));
}
