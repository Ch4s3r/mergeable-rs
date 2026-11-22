use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::js;
use crate::registry::Registry;
use crate::results;
use async_trait::async_trait;
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub mod lib;
pub mod options;
pub mod options_processor;

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
    fn supported_settings(&self) -> Option<Value>;
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
    ) -> JsResult<Value>;
}

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

pub async fn process_filter(
    filter: &dyn Filter,
    context: &Context,
    settings: &Value,
    registry: &Registry,
) -> JsResult<Value> {
    let supported_settings = filter
        .supported_settings()
        .ok_or_else(|| JsError::error("Class extending filters must provide supported Settings"))?;
    tracing::info!(
        event_id = %context.event_id,
        repo = %context.repository_full_name(),
        filter = filter.name(),
        settings = %settings,
        "filter_process"
    );
    if filter.checks_settings() {
        match crate::validators::validate_settings(
            &format!("filter/{}", filter.name()),
            &supported_settings,
            settings,
            &[],
        ) {
            Ok(()) => {}
            Err(error) if error.is_named("UnSupportedSettingError") => {
                let filter_context = json!({"name": filter.name()});
                let output = vec![results::construct_filter_error(
                    &filter_context,
                    &Value::String(js::stringify(&supported_settings)),
                    settings,
                    &error.name,
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
) -> JsResult<Value> {
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
