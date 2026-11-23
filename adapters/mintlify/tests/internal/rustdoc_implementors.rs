use super::validate;

// Exact installed Rustdoc 1.94.0 trait.impl/core/marker/trait.ConstParamTy_.js.
const NATIVE: &str = concat!(
    "(function() {\n    const implementors = Object.fromEntries([[\"core\",[]],[\"std\",[]]]);\n",
    "    if (window.register_implementors) {\n",
    "        window.register_implementors(implementors);\n",
    "    } else {\n",
    "        window.pending_implementors = implementors;\n",
    "    }\n})()\n//{\"start\":59,\"fragment_lengths\":[11,11]}"
);

fn envelope(rows: &[&str]) -> String {
    let lengths: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| row.len() + usize::from(index != 0))
        .collect();
    NATIVE
        .replace(
            "[[\"core\",[]],[\"std\",[]]]",
            &format!("[{}]", rows.join(",")),
        )
        .replace(
            "[11,11]}",
            &format!("{}}}", serde_json::to_string(&lengths).unwrap()),
        )
}

#[test]
fn native_noncontributing_literal_entries_preserve_static_api() {
    validate(NATIVE.as_bytes(), "core", "native").unwrap();
    let script = envelope(&[
        r#"["sample",[["impl Describe for Record",0]]]"#,
        r#"["other",[]]"#,
    ]);
    validate(script.as_bytes(), "sample", "current crate").unwrap();
    let synthetic = envelope(&[
        r#"["alloc",[["impl !Send for ExtractIf",1,1,["alloc::collections::linked_list::ExtractIf"]]]]"#,
    ]);
    validate(
        synthetic.as_bytes(),
        "alloc",
        "current-crate native numeric synthetic flag",
    )
    .unwrap();
    validate(envelope(&[]).as_bytes(), "sample", "empty data").unwrap();
}

#[test]
fn extra_statements_malformed_literals_and_fragment_metadata_are_refused() {
    for script in [
        format!("{NATIVE}\nwindow.extra = true;"),
        NATIVE.replace("    if (window", "    window.extra = true;\n    if (window"),
        NATIVE.replace("[[\"core\",[]]", "[[\"core\",broken]"),
        NATIVE.replace("\"start\":59", "\"start\":60"),
        NATIVE.replace("[11,11]}", "[10,12]}"),
        NATIVE.replace("[11,11]}", "[11]}"),
        NATIVE.replace("\"start\":59", "\"start\":59,\"start\":59"),
        NATIVE.replace("\"start\":59", "\"start\":59,\"extra\":true"),
    ] {
        assert!(
            validate(script.as_bytes(), "core", "altered native data").is_err(),
            "{script}"
        );
    }
}

#[test]
fn duplicate_crate_keys_bad_rows_and_contributing_foreign_data_are_refused() {
    for rows in [
        vec![r#"["sample",[]]"#, r#"["sample",[]]"#],
        vec![r#"["sample",[["impl Describe for Record",2]]]"#],
        vec![r#"["sample",[["impl Describe for Record",0,"script"]]]"#],
        vec![r#"["sample",[["impl !Send for Record",1,true,["sample::Record"]]]]"#],
        vec![r#"["sample",[["impl !Send for Record",1,2,["sample::Record"]]]]"#],
        vec![r#"["foreign",[["impl Describe for Foreign",0]]]"#],
    ] {
        assert!(validate(envelope(&rows).as_bytes(), "sample", "refused data").is_err());
    }
}
