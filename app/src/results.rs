use serde_json::{Map, Value};

fn output_details(input: Value, settings: &Value, error: Option<Value>) -> Value {
    let mut details = Map::new();
    details.insert("input".to_string(), input);
    details.insert("settings".to_string(), settings.clone());
    if let Some(error_value) = error {
        details.insert("error".to_string(), error_value);
    }
    Value::Object(details)
}

#[must_use]
pub fn construct_output(
    validator_context: &Value,
    input: Value,
    settings: &Value,
    status: &str,
    description: Value,
    error: Option<Value>,
) -> Value {
    construct_output_with_owned_validator(
        validator_context.clone(),
        input,
        settings,
        status,
        description,
        error,
    )
}

#[must_use]
pub fn construct_output_with_owned_validator(
    validator_context: Value,
    input: Value,
    settings: &Value,
    status: &str,
    description: Value,
    error: Option<Value>,
) -> Value {
    let mut output = Map::new();
    output.insert("validator".to_string(), validator_context);
    output.insert("status".to_string(), Value::String(status.to_string()));
    output.insert("description".to_string(), description);
    output.insert(
        "details".to_string(),
        output_details(input, settings, error),
    );
    Value::Object(output)
}

#[must_use]
pub fn construct_error_output(
    validator_context: &Value,
    input: Value,
    settings: &Value,
    description: &str,
    error: Option<Value>,
) -> Value {
    construct_output(
        validator_context,
        input,
        settings,
        "error",
        Value::String(description.to_string()),
        error,
    )
}

pub fn consolidate_status<'item>(
    statuses: impl Iterator<Item = Option<&'item str>>,
) -> &'static str {
    let mut status = "pass";
    for item_status in statuses {
        if item_status == Some("fail") && status != "error" {
            status = "fail";
        }
        if item_status == Some("error") {
            status = "error";
        }
    }
    status
}

fn status_of(output: &Value) -> Option<&str> {
    output.get("status").and_then(Value::as_str)
}

fn consolidate(outputs: Vec<Value>, context: &Value, outputs_key: &str) -> Value {
    let status = consolidate_status(outputs.iter().map(status_of));
    let mut result = Map::new();
    result.insert("status".to_string(), Value::String(status.to_string()));
    if let Some(name) = crate::js::get(context, "name") {
        result.insert("name".to_string(), name.clone());
    }
    result.insert(outputs_key.to_string(), Value::Array(outputs));
    Value::Object(result)
}

#[must_use]
pub fn consolidate_result(outputs: Vec<Value>, validator_context: &Value) -> Value {
    consolidate(outputs, validator_context, "validations")
}

#[must_use]
pub fn consolidate_filter_result(outputs: Vec<Value>, filter_context: &Value) -> Value {
    consolidate(outputs, filter_context, "filters")
}

#[must_use]
pub fn construct_filter_output(
    filter_context: &Value,
    input: Value,
    settings: &Value,
    status: &str,
    description: Value,
    error: Option<Value>,
) -> Value {
    let mut output = Map::new();
    output.insert("status".to_string(), Value::String(status.to_string()));
    output.insert("description".to_string(), description);
    output.insert(
        "details".to_string(),
        output_details(input, settings, error),
    );
    output.insert("filter".to_string(), filter_context.clone());
    Value::Object(output)
}

#[must_use]
pub fn construct_filter_error(
    filter_context: &Value,
    input: Value,
    settings: &Value,
    description: &str,
    error: Option<Value>,
) -> Value {
    construct_filter_output(
        filter_context,
        input,
        settings,
        "error",
        Value::String(description.to_string()),
        error,
    )
}

#[must_use]
pub fn extract_validation_stats(results: Vec<Value>) -> Value {
    let (mut pass_count, mut fail_count, mut error_count) = (0_usize, 0_usize, 0_usize);
    for result in &results {
        match status_of(result) {
            Some("pass") => pass_count += 1,
            Some("fail") => fail_count += 1,
            Some("error") => error_count += 1,
            _ => {}
        }
    }
    let validation_status = if error_count > 0 {
        "error"
    } else if fail_count > 0 {
        "fail"
    } else {
        "pass"
    };
    let failures: Vec<Value> = results
        .iter()
        .filter(|result| status_of(result) == Some("fail"))
        .cloned()
        .collect();
    let mut stats = Map::new();
    stats.insert(
        "validationStatus".to_string(),
        Value::String(validation_status.to_string()),
    );
    stats.insert("validationCount".to_string(), Value::from(results.len()));
    stats.insert("passCount".to_string(), Value::from(pass_count));
    stats.insert("failCount".to_string(), Value::from(fail_count));
    stats.insert("errorCount".to_string(), Value::from(error_count));
    stats.insert("validationSuites".to_string(), Value::Array(results));
    stats.insert("failures".to_string(), Value::Array(failures));
    Value::Object(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn error_status_wins_over_fail_when_consolidating() {
        let outputs = vec![
            json!({"status": "fail"}),
            json!({"status": "error"}),
            json!({"status": "fail"}),
        ];
        let result = consolidate_result(outputs, &json!({"name": "title"}));
        assert_eq!(result["status"], "error");
        assert_eq!(result["name"], "title");
    }

    #[test]
    fn validation_stats_count_each_status() {
        let stats = extract_validation_stats(vec![
            json!({"status": "pass"}),
            json!({"status": "fail", "name": "x"}),
        ]);
        assert_eq!(stats["validationStatus"], "fail");
        assert_eq!(stats["passCount"], 1);
        assert_eq!(stats["failures"], json!([{"status": "fail", "name": "x"}]));
    }

    #[test]
    fn validation_stats_keep_the_javascript_key_order() {
        let stats = extract_validation_stats(vec![json!({"status": "error"})]);
        let keys: Vec<&String> = stats
            .as_object()
            .map(|fields| fields.keys().collect())
            .unwrap_or_default();
        assert_eq!(
            keys,
            [
                "validationStatus",
                "validationCount",
                "passCount",
                "failCount",
                "errorCount",
                "validationSuites",
                "failures"
            ]
        );
    }
}
