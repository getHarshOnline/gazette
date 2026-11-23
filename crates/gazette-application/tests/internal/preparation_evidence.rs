//! Documentation admission accepts its producer's complete configuration evidence.

use super::documentation_inputs;
use crate::{build::CompositionInspection, configuration};
use gazette_compiler::workspace;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn produced_inputs() -> Value {
    let compilation = workspace::compile(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n",
        Default::default(),
    )
    .expect("actual workspace compilation");
    let configuration = configuration::Evidence::Supplied {
        source_sha256: compilation.source_sha256,
    };
    serde_json::to_value(CompositionInspection {
        configuration: &configuration,
        compilation: &compilation,
        site: &"site:one".parse().unwrap(),
        sources: BTreeMap::new(),
        math: None,
        diagrams: None,
    })
    .expect("actual producer serialization")
}

#[test]
fn documentation_inputs_accept_the_actual_composition_inspection() {
    let produced = produced_inputs();
    let accepted = documentation_inputs(&produced).expect("complete producer envelope");
    assert_eq!(accepted["configuration"], produced["configuration"]);
    assert_eq!(accepted["configuration"]["kind"], "supplied");
}

#[test]
fn documentation_inputs_refuse_missing_extra_and_non_object_provenance() {
    let produced = produced_inputs();
    for key in produced.as_object().unwrap().keys() {
        let mut missing = produced.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(documentation_inputs(&missing).is_err(), "missing {key}");
    }
    let mut extra = produced.clone();
    extra["unrecognized"] = json!(true);
    assert!(documentation_inputs(&extra).is_err());
    for key in ["configuration", "compilation", "sources"] {
        for replacement in [Value::Null, json!(true), json!([]), json!("opaque text")] {
            let mut malformed = produced.clone();
            malformed[key] = replacement;
            assert!(documentation_inputs(&malformed).is_err(), "malformed {key}");
        }
    }
}
