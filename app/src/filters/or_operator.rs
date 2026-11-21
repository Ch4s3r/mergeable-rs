use crate::context::Context;
use crate::error::JsResult;
use crate::filters::Filter;
use crate::filters::lib::logical_connective;
use crate::registry::Registry;
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Or;

#[async_trait]
impl Filter for Or {
    fn name(&self) -> &'static str {
        "or"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["*"]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({}))
    }

    fn supported_options(&self) -> Option<&'static [&'static str]> {
        Some(&["filter"])
    }

    fn checks_settings(&self) -> bool {
        false
    }

    async fn filter(
        &self,
        context: &Context,
        settings: &Value,
        registry: &Registry,
    ) -> JsResult<Value> {
        logical_connective::process(context, settings.get("filter"), registry, "Or").await
    }
}
