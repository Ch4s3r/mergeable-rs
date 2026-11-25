use crate::context::Context;
use crate::error::Result;
use crate::registry::Registry;
use crate::results::Connective;
use crate::validators::Validator;
use crate::validators::logical_connective;
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Not;

#[async_trait]
impl Validator for Not {
    fn name(&self) -> &'static str {
        "not"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["*"]
    }

    fn supported_settings(&self) -> Value {
        json!({})
    }

    fn checks_settings(&self) -> bool {
        false
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        registry: &Registry,
    ) -> Result<Value> {
        logical_connective::process(context, settings.get("validate"), registry, Connective::Not)
            .await
    }
}
