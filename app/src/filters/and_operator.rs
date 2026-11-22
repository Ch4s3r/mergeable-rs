use crate::context::Context;
use crate::error::Result;
use crate::filters::Filter;
use crate::filters::lib::logical_connective::{self, Connective};
use crate::registry::Registry;
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct And;

#[async_trait]
impl Filter for And {
    fn name(&self) -> &'static str {
        "and"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["*"]
    }

    fn supported_settings(&self) -> Value {
        json!({})
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
    ) -> Result<Value> {
        logical_connective::process(context, settings.get("filter"), registry, Connective::And)
            .await
    }
}
