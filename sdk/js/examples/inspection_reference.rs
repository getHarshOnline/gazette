//! Independent direct Rust input/inspection reference for the real Node tree fixture.

use gazette_sdk::{document, markdown, source};
use serde_json::json;
use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::args_os().nth(1).expect("exact fixture root"));
    assert!(root.is_absolute());
    let identity = source::inspect(&root, None, source::Limits::default())
        .expect("independent complete-tree inventory")
        .identity();
    let mut result = json!({});
    for (name, path) in [("markdown", "page.md"), ("document", "framed.md")] {
        let entry: source::EntryPath = path.parse().expect("literal fixture entry");
        let selection = json!({"root":root,"identity":identity,"document":entry});
        let loaded = markdown::input::Tree {
            root: root.clone(),
            identity,
            document: entry,
            limits: source::Limits::default(),
        }
        .load(true)
        .expect("complete retained acquisition");
        let text = loaded.text().expect("retained UTF-8");
        let reports = [false, true].map(|html| {
            if name == "markdown" {
                serde_json::to_value(markdown::input::Report {
                    input: loaded.evidence(),
                    projection: markdown::inspect(text, markdown::Limits::default(), html)
                        .expect("native Markdown"),
                })
                .expect("native report")
            } else {
                serde_json::to_value(markdown::input::Report {
                    input: loaded.evidence(),
                    projection: document::inspect(text, document::Limits::default(), html)
                        .expect("native framed document"),
                })
                .expect("native report")
            }
        });
        result[name] = json!({"selection":selection,"plain":reports[0],"html":reports[1]});
    }
    println!("{result}");
}
