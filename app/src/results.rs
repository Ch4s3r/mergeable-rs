use serde_json::{Map, Value, json};

pub fn construct_output(
    validator_context: &Value,
    input: &Value,
    settings: &Value,
    status: &str,
    description: Value,
    error: Option<Value>,
) -> Value {
    let mut details = Map::new();
    details.insert("input".to_string(), input.clone());
    details.insert("settings".to_string(), settings.clone());
    if let Some(error_value) = error {
        details.insert("error".to_string(), error_value);
    }
    json!({
        "validator": validator_context,
        "status": status,
        "description": description,
        "details": Value::Object(details),
    })
}

pub fn construct_error_output(
    validator_context: &Value,
    input: &Value,
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

pub fn consolidate_result(outputs: Vec<Value>, validator_context: &Value) -> Value {
    let status = consolidate_status(
        outputs
            .iter()
            .map(|output| output.get("status").and_then(Value::as_str)),
    );
    let mut result = Map::new();
    result.insert("status".to_string(), Value::String(status.to_string()));
    if let Some(name) = crate::js::get(validator_context, "name") {
        result.insert("name".to_string(), name.clone());
    }
    result.insert("validations".to_string(), Value::Array(outputs));
    Value::Object(result)
}

pub fn consolidate_filter_result(outputs: Vec<Value>, filter_context: &Value) -> Value {
    let status = consolidate_status(
        outputs
            .iter()
            .map(|output| output.get("status").and_then(Value::as_str)),
    );
    let mut result = Map::new();
    result.insert("status".to_string(), Value::String(status.to_string()));
    if let Some(name) = crate::js::get(filter_context, "name") {
        result.insert("name".to_string(), name.clone());
    }
    result.insert("filters".to_string(), Value::Array(outputs));
    Value::Object(result)
}

pub fn construct_filter_output(
    filter_context: &Value,
    input: &Value,
    settings: &Value,
    status: &str,
    description: Value,
    error: Option<Value>,
) -> Value {
    let mut details = Map::new();
    details.insert("input".to_string(), input.clone());
    details.insert("settings".to_string(), settings.clone());
    if let Some(error_value) = error {
        details.insert("error".to_string(), error_value);
    }
    json!({
        "status": status,
        "description": description,
        "details": Value::Object(details),
        "filter": filter_context,
    })
}

pub fn construct_filter_error(
    filter_context: &Value,
    input: &Value,
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

pub fn extract_validation_stats(results: &[Value]) -> Value {
    let statuses: Vec<Option<&str>> = results
        .iter()
        .map(|result| result.get("status").and_then(Value::as_str))
        .collect();
    let count = |wanted: &str| {
        statuses
            .iter()
            .filter(|status| **status == Some(wanted))
            .count()
    };
    let pass_count = count("pass");
    let fail_count = count("fail");
    let error_count = count("error");
    let validation_status = if error_count > 0 {
        "error"
    } else if fail_count > 0 {
        "fail"
    } else {
        "pass"
    };
    let failures: Vec<Value> = results
        .iter()
        .filter(|result| result.get("status").and_then(Value::as_str) == Some("fail"))
        .cloned()
        .collect();
    json!({
        "validationStatus": validation_status,
        "validationCount": statuses.len(),
        "passCount": pass_count,
        "failCount": fail_count,
        "errorCount": error_count,
        "validationSuites": results,
        "failures": failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let stats = extract_validation_stats(&[
            json!({"status": "pass"}),
            json!({"status": "fail", "name": "x"}),
        ]);
        assert_eq!(stats["validationStatus"], "fail");
        assert_eq!(stats["passCount"], 1);
        assert_eq!(stats["failures"], json!([{"status": "fail", "name": "x"}]));
    }
}
