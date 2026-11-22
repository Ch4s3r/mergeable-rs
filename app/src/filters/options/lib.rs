use crate::results;
use serde_json::{Map, Value};

#[must_use]
pub fn construct_output(
    filter_context: &Value,
    input: Value,
    settings: Value,
    mut result: Value,
    error: Option<Value>,
) -> Value {
    let mut details = Map::new();
    details.insert("input".to_string(), input);
    details.insert("settings".to_string(), settings);
    if let Some(error_details) = error {
        details.insert("error".to_string(), error_details);
    }
    let mut output = Map::new();
    if let Some(status) = result.get_mut("status") {
        output.insert("status".to_string(), status.take());
    }
    if let Some(description) = result.get_mut("description") {
        output.insert("description".to_string(), description.take());
    }
    output.insert("details".to_string(), Value::Object(details));
    output.insert("filter".to_string(), filter_context.clone());
    Value::Object(output)
}

#[must_use]
pub fn construct_error(
    filter_context: &Value,
    input: Value,
    settings: &Value,
    error: &str,
    details: Option<Value>,
) -> Value {
    results::construct_filter_error(filter_context, input, settings, error, details)
}

#[must_use]
pub fn consolidate_results(outputs: Vec<Value>, filter_context: &Value) -> Value {
    results::consolidate_filter_result(outputs, filter_context)
}
