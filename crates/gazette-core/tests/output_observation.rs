//! Native binding facts do not become serialized authority or content-tree identity.

use gazette_core::{
    destination::{OutputAdministrationBinding, OutputBinding, OutputIdentity, OutputObservation},
    source::{Content, InventoryBuilder, Limits, Sha256Digest},
};

#[test]
fn checked_binding_separates_same_content_from_native_root_and_administration_identity() {
    let root = std::env::temp_dir().join("gazette-output-contract-not-created");
    let binding = |inode, administration| {
        OutputBinding::new(root.clone(), OutputIdentity::new(7, inode), administration)
            .expect("absolute native fact coordinate")
    };
    let original = binding(11, None);
    assert_eq!(original.canonical_root(), root);
    assert_ne!(original, binding(12, None));
    let admin = OutputIdentity::new(7, 20);
    assert_ne!(
        original,
        binding(11, Some(OutputAdministrationBinding::Directory(admin)))
    );
    assert_ne!(
        binding(11, Some(OutputAdministrationBinding::Directory(admin))),
        binding(11, Some(OutputAdministrationBinding::File(admin)))
    );
    assert_ne!(
        binding(11, Some(OutputAdministrationBinding::File(admin))),
        binding(
            11,
            Some(OutputAdministrationBinding::File(OutputIdentity::new(
                7, 21
            )))
        )
    );
    let mut inventory = InventoryBuilder::new(Limits::default());
    inventory
        .insert(
            "user.txt".parse().expect("entry"),
            Content::File {
                bytes: 4,
                sha256: Sha256Digest::of(b"user"),
            },
        )
        .expect("structural inventory");
    let observation = OutputObservation::new(inventory.finish(), original.clone());
    let identity = observation.inventory().identity();
    assert_eq!(observation.binding(), &original);
    let (inventory, moved_binding) = observation.into_parts();
    assert_eq!(inventory.identity(), identity);
    assert_eq!(moved_binding, original);
}

#[test]
fn binding_refuses_relative_or_parent_traversing_canonical_coordinates() {
    for path in [
        std::path::PathBuf::from("relative/output"),
        std::env::temp_dir().join("output/../other"),
    ] {
        let error = OutputBinding::new(path, OutputIdentity::new(1, 2), None)
            .expect_err("not a canonical absolute coordinate");
        assert!(matches!(
            error,
            gazette_core::destination::Error::Observation(
                gazette_core::source::Error::InvalidInventory {
                    reason: "output binding requires a canonical absolute root",
                    ..
                }
            )
        ));
    }
}
