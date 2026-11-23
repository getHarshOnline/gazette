//! Schema validation cannot acquire external documents during pure compilation.

pub(crate) struct DenyExternal;

impl jsonschema::Retrieve for DenyExternal {
    fn retrieve(
        &self,
        _: &jsonschema::Uri<String>,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("external schema retrieval is forbidden".into())
    }
}
