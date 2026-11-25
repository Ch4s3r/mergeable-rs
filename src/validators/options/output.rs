use crate::json_value;
use crate::results::Status;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, PartialEq)]
pub struct OptionResult {
    pub status: Status,
    pub description: Value,
}

impl OptionResult {
    pub fn decided(
        is_mergeable: bool,
        success_description: impl Into<Value>,
        failure_description: impl Into<Value>,
    ) -> Self {
        let status = Status::from_check(is_mergeable);
        let description = match status {
            Status::Pass => success_description.into(),
            Status::Fail | Status::Error => failure_description.into(),
        };
        OptionResult {
            status,
            description,
        }
    }

    #[must_use]
    pub fn into_value(self) -> Value {
        let mut result = Map::new();
        result.insert("status".to_string(), self.status.into());
        result.insert("description".to_string(), self.description);
        Value::Object(result)
    }
}

#[must_use]
pub fn construct_output(
    validator_context: &Value,
    input: Option<&Value>,
    settings: Option<&Value>,
    status: Status,
    description: &Value,
    error: Option<Value>,
) -> Value {
    let mut details = Map::new();
    if let Some(present_input) = input {
        details.insert("input".to_string(), present_input.clone());
    }
    if let Some(present_settings) = settings {
        details.insert("settings".to_string(), present_settings.clone());
    }
    if let Some(present_error) = error {
        details.insert("error".to_string(), present_error);
    }
    json!({
        "validator": validator_context,
        "status": status.as_str(),
        "description": description,
        "details": Value::Object(details),
    })
}

#[must_use]
pub fn construct_error_output(
    validator_context: &Value,
    input: Option<&Value>,
    settings: Option<&Value>,
    description: &str,
) -> Value {
    construct_output(
        validator_context,
        input,
        settings,
        Status::Error,
        &Value::String(description.to_string()),
        None,
    )
}

#[must_use]
pub fn validator_name(validator_context: &Value) -> String {
    json_value::to_display_string(json_value::get(validator_context, "name"))
}

#[must_use]
pub fn message_or_default(message: Option<&Value>, default_description: String) -> Value {
    match message {
        Some(custom) if json_value::is_truthy(custom) => custom.clone(),
        _ => Value::String(default_description),
    }
}

#[cfg(test)]
mod tests {
    use crate::results::consolidate_result;
    use serde_json::json;

    #[test]
    fn returns_pass_status_if_no_fail_or_error_status_exists() {
        let result = consolidate_result(vec![json!({"status": "pass"})], &json!({"name": "label"}));
        assert_eq!(result["status"], "pass");
        assert_eq!(result["name"], "label");
    }

    #[test]
    fn returns_fail_status_if_fail_status_exists_but_no_error_status_exists() {
        let outputs = vec![json!({"status": "pass"}), json!({"status": "fail"})];
        assert_eq!(
            consolidate_result(outputs, &json!({"name": "label"}))["status"],
            "fail"
        );
    }

    #[test]
    fn returns_error_status_if_error_status_exists() {
        let outputs = vec![
            json!({"status": "pass"}),
            json!({"status": "fail"}),
            json!({"status": "error"}),
        ];
        assert_eq!(
            consolidate_result(outputs, &json!({"name": "label"}))["status"],
            "error"
        );
    }
}
