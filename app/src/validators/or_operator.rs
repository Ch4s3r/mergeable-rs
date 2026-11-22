use crate::context::Context;
use crate::error::Result;
use crate::registry::Registry;
use crate::validators::Validator;
use crate::validators::lib::logical_connective::{self, LogicalOperator};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Or;

#[async_trait]
impl Validator for Or {
    fn name(&self) -> &'static str {
        "or"
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
        logical_connective::process(
            context,
            settings.get("validate"),
            registry,
            LogicalOperator::Or,
        )
        .await
    }
}
