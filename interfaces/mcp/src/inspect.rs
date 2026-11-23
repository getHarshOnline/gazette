//! Supplied or explicitly pinned-tree inspection through the same bounded Markdown SDK.

use gazette_cli::sdk::{document, markdown, toml};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use gazette_cli::command::{
    Kernel,
    inspection::{
        FrontMatter as CommandFrontMatter, MarkdownCommand, MarkdownInput, MarkdownLimits,
        MarkdownRequest,
    },
};

/// Inspect exactly one supplied text or complete pinned tree; no publication effects.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Markdown {
    /// Exact UTF-8 Markdown source, including delimiters when `front_matter` is toml.
    #[schemars(with = "String")]
    source: Option<String>,
    /// Exact complete-tree input; never combined with supplied source or inferred from a path.
    #[schemars(with = "markdown::input::Tree")]
    tree: Option<markdown::input::Tree>,
    /// Required true for tree capture; does not override host permissions.
    #[serde(default)]
    allow_native_reads: bool,
    /// Absent means body-only Markdown. TOML requires explicit +++ delimiter lines.
    #[serde(default)]
    front_matter: Option<FrontMatter>,
    /// Also return an HTML fragment. Raw HTML and unsafe URLs fail without a partial report.
    #[serde(default)]
    html: bool,
    /// Independent native parsing/projection ceilings; MCP frame and result ceilings also apply.
    #[serde(default)]
    limits: Limits,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum FrontMatter {
    Toml,
}

/// Limits mirror CLI inspect-markdown; omitted fields use the native SDK defaults.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "MarkdownInspectionLimits")]
struct Limits {
    /// Maximum complete source UTF-8 bytes, including any metadata and delimiters.
    #[schemars(range(max = usize::MAX))]
    source_bytes: usize,
    /// Maximum Markdown syntax nodes, including its document root.
    #[schemars(range(max = usize::MAX))]
    nodes: usize,
    /// Maximum Markdown depth below the document root.
    #[schemars(range(max = usize::MAX))]
    depth: usize,
    /// Maximum retained Markdown analysis string bytes.
    #[schemars(range(max = usize::MAX))]
    fact_bytes: usize,
    /// Maximum inspection string bytes and, independently, optional HTML bytes.
    #[schemars(range(max = usize::MAX))]
    output_bytes: usize,
    /// Maximum TOML metadata bytes; used only with `front_matter=toml`.
    #[schemars(range(max = usize::MAX))]
    metadata_bytes: usize,
    /// Maximum TOML metadata nodes; used only with `front_matter=toml`.
    #[schemars(range(max = usize::MAX))]
    metadata_nodes: usize,
    /// Maximum TOML metadata depth; used only with `front_matter=toml`.
    #[schemars(range(max = usize::MAX))]
    metadata_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        let markdown = markdown::Limits::default();
        let metadata = toml::Limits::default();
        Self {
            source_bytes: markdown.source_bytes,
            nodes: markdown.nodes,
            depth: markdown.depth,
            fact_bytes: markdown.fact_bytes,
            output_bytes: markdown.output_bytes,
            metadata_bytes: metadata.source_bytes,
            metadata_nodes: metadata.nodes,
            metadata_depth: metadata.depth,
        }
    }
}

impl Limits {
    fn markdown(&self) -> markdown::Limits {
        markdown::Limits {
            source_bytes: self.source_bytes,
            nodes: self.nodes,
            depth: self.depth,
            fact_bytes: self.fact_bytes,
            output_bytes: self.output_bytes,
        }
    }
}

pub(super) fn descriptor() -> Tool {
    let mut schema = schemars::schema_for!(Markdown);
    // Presence is the exclusive selection below; Option fields do not admit explicit null.
    schema.ensure_object().remove("required");
    schema.ensure_object().insert(
        "oneOf".into(),
        serde_json::json!([
            {"required":["source"],"not":{"required":["tree"]}},
            {"required":["tree","allow_native_reads"],"not":{"required":["source"]},
             "properties":{"allow_native_reads":{"const":true}}}
        ]),
    );
    Tool::new(
        "inspect_markdown",
        "Use inspect_markdown to see how Gazette parses a Markdown document: supplied source, or one document in an independently pinned complete local tree, through Gazette's shared SDK. One call returns syntax evidence. Tree mode requires allow_native_reads=true and an absolute root; all assets participate in capture and pin verification before parsing retained bytes, with no fallback or second read. Optional explicit TOML front matter and bounded HTML use the same native parser. Results are syntax evidence, not admission, certification or publication approval. Source ranges are native UTF-8 bytes; body ranges remain relative to the reported body span. Whether a page is accepted or published stays with the publisher; use check_workspace to learn whether links and fields resolve.",
        std::mem::take(schema.ensure_object()),
    )
    .with_raw_output_schema(crate::tools::output_schema::<markdown::input::Report<Output>>())
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(true),
    )
}

pub(super) fn decode(arguments: JsonObject) -> Result<Markdown, ErrorData> {
    let value = Value::Object(arguments);
    for pointer in ["/limits", "/tree", "/tree/identity", "/tree/limits"] {
        if value
            .pointer(pointer)
            .is_some_and(|value| !value.is_object())
        {
            return Err(ErrorData::invalid_params(
                format!("{pointer} must be an object"),
                None,
            ));
        }
    }
    if value.get("source").is_some_and(|value| !value.is_string()) {
        return Err(ErrorData::invalid_params("source must be a string", None));
    }
    let input: Markdown = serde_json::from_value(value).map_err(|_| {
        ErrorData::invalid_params("Invalid Markdown arguments; consult inputSchema", None)
    })?;
    if input.source.is_some() == input.tree.is_some() {
        return Err(ErrorData::invalid_params(
            "select exactly one source or tree; no fallback",
            None,
        ));
    }
    if let Some(tree) = &input.tree
        && (!input.allow_native_reads || !tree.root.is_absolute())
    {
        return Err(ErrorData::invalid_params(
            "tree requires allow_native_reads=true and an absolute root; no source opened",
            None,
        ));
    }
    Ok(input)
}

#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum Output {
    Markdown(markdown::Report),
    Document(document::Report),
}

impl Markdown {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let input = match (self.source, self.tree) {
            (Some(source), None) => MarkdownInput::Supplied(source.into_bytes()),
            (None, Some(tree)) => MarkdownInput::Tree {
                tree,
                allow_native_reads: self.allow_native_reads,
            },
            _ => {
                return Err(ErrorData::invalid_params(
                    "select exactly one source or tree",
                    None,
                ));
            }
        };
        let request = MarkdownRequest {
            input,
            front_matter: self.front_matter.map(|_| CommandFrontMatter::Toml),
            html: self.html,
            limits: MarkdownLimits {
                markdown: self.limits.markdown(),
                metadata: toml::Limits {
                    source_bytes: self.limits.metadata_bytes,
                    nodes: self.limits.metadata_nodes,
                    depth: self.limits.metadata_depth,
                },
            },
        };
        match Kernel::new().execute(MarkdownCommand(request)) {
            Ok(outcome) => crate::tools::encode(&outcome, false, maximum),
            Err(error) => crate::tools::failure(
                error.code(),
                "Markdown/document acquisition or inspection failed; no report produced",
                maximum,
            ),
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/inspect.rs"]
mod tests;
