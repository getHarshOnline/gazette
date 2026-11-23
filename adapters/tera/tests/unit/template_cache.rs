//! Collisions cannot merge distinct native namespaces or consume unbounded work.

use super::{Budget, CacheIndex, Loaded, Projection};
use gazette_core::render::{Error, Limits};
use std::collections::BTreeMap;

fn loaded(empty: &'static str, target: &'static str) -> Loaded<'static> {
    BTreeMap::from([
        ("owner", BTreeMap::from([("helper", target)])),
        (empty, BTreeMap::new()),
    ])
}

#[test]
fn collisions_keep_complete_loader_states_and_first_equivalent_pure_state() {
    let caches = vec![loaded("a", "one"), loaded("b", "one"), loaded("b", "two")];
    let mut budget = Budget::new(Limits::default()).unwrap();
    let mut index = CacheIndex::default();
    // Force all states into the same bucket independently of the hash function.
    for id in 0..caches.len() {
        index.insert(0, id, &mut budget).unwrap();
    }
    for (id, candidate) in caches.iter().enumerate() {
        assert_eq!(
            index
                .find(0, &caches, candidate, Projection::Complete, &mut budget)
                .unwrap(),
            Some(id)
        );
    }
    assert_eq!(
        index
            .find(0, &caches, &caches[1], Projection::Nonempty, &mut budget)
            .unwrap(),
        Some(0)
    );
    assert_eq!(
        index
            .find(0, &caches, &caches[2], Projection::Nonempty, &mut budget)
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        index
            .find(
                0,
                &caches,
                &loaded("b", "absent"),
                Projection::Nonempty,
                &mut budget
            )
            .unwrap(),
        None
    );
    assert_eq!(
        Projection::Nonempty
            .fingerprint(&caches[0], &mut budget)
            .unwrap(),
        Projection::Nonempty
            .fingerprint(&caches[1], &mut budget)
            .unwrap()
    );
}

#[test]
fn distinct_namespace_lookup_does_not_scan_unrelated_states() {
    let names = (0..512)
        .map(|id| format!("target-{id}"))
        .collect::<Vec<_>>();
    let caches = names
        .iter()
        .map(|name| BTreeMap::from([("owner", BTreeMap::from([("helper", name.as_str())]))]))
        .collect::<Vec<_>>();
    let mut budget = Budget::new(Limits::default()).unwrap();
    let mut index = CacheIndex::default();
    for (id, loaded) in caches.iter().enumerate() {
        let fingerprint = Projection::Complete
            .fingerprint(loaded, &mut budget)
            .unwrap();
        index.insert(fingerprint, id, &mut budget).unwrap();
    }
    let fingerprint = Projection::Complete
        .fingerprint(&caches[511], &mut budget)
        .unwrap();
    budget.work = 1;
    assert_eq!(
        index
            .find(
                fingerprint,
                &caches,
                &caches[511],
                Projection::Complete,
                &mut budget
            )
            .unwrap(),
        Some(511)
    );
}

#[test]
fn indexes_and_collision_comparisons_keep_the_storage_and_work_ceilings() {
    let caches = vec![loaded("a", "one"), loaded("b", "two")];
    let mut budget = Budget {
        remaining: 127,
        work: 10,
    };
    let mut index = CacheIndex::default();
    assert!(matches!(
        index.insert(0, 0, &mut budget),
        Err(Error::Limit("template safety analysis storage"))
    ));
    let mut budget = Budget::new(Limits::default()).unwrap();
    index.insert(0, 0, &mut budget).unwrap();
    index.insert(0, 1, &mut budget).unwrap();
    budget.work = 1;
    assert!(matches!(
        index.find(0, &caches, &caches[1], Projection::Complete, &mut budget),
        Err(Error::Limit("template safety analysis work"))
    ));
    budget.work = 1;
    assert!(matches!(
        Projection::Complete.fingerprint(&caches[0], &mut budget),
        Err(Error::Limit("template safety analysis work"))
    ));
}
