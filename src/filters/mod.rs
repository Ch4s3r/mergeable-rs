use crate::context::Context;
use crate::error::{ErrorKind, Result};
use crate::json_value;
use crate::registry::Registry;
use crate::results;
use async_trait::async_trait;
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub mod logical_connective;
pub mod options;
pub mod options_processor;
#[cfg(test)]
pub mod test_context;

pub mod and_operator;
pub mod author;
pub mod base_ref;
pub mod not_operator;
pub mod or_operator;
pub mod payload;
pub mod repository;

#[async_trait]
pub trait Filter: Send + Sync {
    fn name(&self) -> &'static str;
    fn supported_events(&self) -> &'static [&'static str];
    fn supported_settings(&self) -> Value;
    fn supported_options(&self) -> Option<&'static [&'static str]> {
        None
    }
    fn checks_settings(&self) -> bool {
        true
    }
    async fn filter(
        &self,
        context: &Context,
        settings: &Value,
        registry: &Registry,
    ) -> Result<Value>;
}

#[must_use]
pub fn create(key: &str) -> Option<Arc<dyn Filter>> {
    let filter: Arc<dyn Filter> = match key {
        "and" => Arc::new(and_operator::And),
        "author" => Arc::new(author::Author),
        "baseRef" => Arc::new(base_ref::BaseRef),
        "not" => Arc::new(not_operator::Not),
        "or" => Arc::new(or_operator::Or),
        "payload" => Arc::new(payload::Payload),
        "repository" => Arc::new(repository::Repository),
        _ => return None,
    };
    Some(filter)
}

pub fn is_event_supported(filter: &dyn Filter, event_name: &str) -> bool {
    crate::event_aware::is_event_supported(filter.supported_events(), event_name)
}

#[tracing::instrument(name = "filter", skip_all, fields(filter.name = filter.name()))]
pub async fn process_filter(
    filter: &dyn Filter,
    context: &Context,
    settings: &Value,
    registry: &Registry,
) -> Result<Value> {
    let supported_settings = filter.supported_settings();
    tracing::info!(
        filter = filter.name(),
        settings = %settings,
        "filter.process.started"
    );
    if filter.checks_settings() {
        match crate::validators::validate_settings(
            &format!("filter/{}", filter.name()),
            &supported_settings,
            settings,
            &[],
        ) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::UnsupportedSetting => {
                let filter_context = json!({"name": filter.name()});
                let output = vec![results::construct_filter_error(
                    &filter_context,
                    Value::String(json_value::stringify(&supported_settings)),
                    settings,
                    error.kind().name(),
                    Some(Value::String(error.to_string())),
                )];
                return Ok(results::consolidate_filter_result(output, &filter_context));
            }
            Err(error) => return Err(error),
        }
    }
    filter.filter(context, settings, registry).await
}

pub async fn process_options(
    filter: &dyn Filter,
    context: &Context,
    input: &Value,
    settings: &Value,
) -> Result<Value> {
    let mut filter_context = Map::new();
    filter_context.insert(
        "name".to_string(),
        settings.get("do").cloned().unwrap_or(Value::Null),
    );
    if let Some(supported) = filter.supported_options() {
        filter_context.insert(
            "supportedOptions".to_string(),
            Value::Array(
                supported
                    .iter()
                    .map(|option| Value::String((*option).to_string()))
                    .collect(),
            ),
        );
    }
    options::process(context, &Value::Object(filter_context), input, settings).await
}
