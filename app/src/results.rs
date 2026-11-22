use serde_json::{Map, Value};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Pass,
    Fail,
    Error,
}

impl Status {
    pub const ALL: [Status; 3] = [Status::Pass, Status::Fail, Status::Error];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Fail => "fail",
            Status::Error => "error",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Status::ALL
            .into_iter()
            .find(|status| status.as_str() == text)
    }

    #[must_use]
    pub fn of(output: &Value) -> Option<Self> {
        output
            .get("status")
            .and_then(Value::as_str)
            .and_then(Self::parse)
    }

    #[must_use]
    pub fn from_check(passed: bool) -> Self {
        if passed { Status::Pass } else { Status::Fail }
    }

    #[must_use]
    pub fn negated(self) -> Self {
        match self {
            Status::Pass => Status::Fail,
            Status::Fail => Status::Pass,
            Status::Error => Status::Error,
        }
    }

    fn most_severe(statuses: impl IntoIterator<Item = Option<Status>>) -> Status {
        statuses.into_iter().flatten().max().unwrap_or(Status::Pass)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<Status> for Value {
    fn from(status: Status) -> Self {
        Value::String(status.as_str().to_string())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationSummary {
    pub status: Status,
    pub report: Value,
}

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

fn consolidate(outputs: Vec<Value>, context: &Value, outputs_key: &str) -> Value {
    let status = Status::most_severe(outputs.iter().map(Status::of));
    let mut result = Map::new();
    result.insert("status".to_string(), status.into());
    if let Some(name) = crate::json_value::get(context, "name") {
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

fn construct_filter_output(
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
pub fn summarize_validations(results: Vec<Value>) -> ValidationSummary {
    let statuses: Vec<Option<Status>> = results.iter().map(Status::of).collect();
    let count_of = |wanted: Status| {
        statuses
            .iter()
            .filter(|status| **status == Some(wanted))
            .count()
    };
    let (pass_count, fail_count, error_count) = (
        count_of(Status::Pass),
        count_of(Status::Fail),
        count_of(Status::Error),
    );
    let status = Status::most_severe(statuses.iter().copied());
    let failures: Vec<Value> = results
        .iter()
        .zip(&statuses)
        .filter(|(_, status)| **status == Some(Status::Fail))
        .map(|(result, _)| result.clone())
        .collect();
    let mut report = Map::new();
    report.insert("validationStatus".to_string(), status.into());
    report.insert("validationCount".to_string(), Value::from(results.len()));
    report.insert("passCount".to_string(), Value::from(pass_count));
    report.insert("failCount".to_string(), Value::from(fail_count));
    report.insert("errorCount".to_string(), Value::from(error_count));
    report.insert("validationSuites".to_string(), Value::Array(results));
    report.insert("failures".to_string(), Value::Array(failures));
    ValidationSummary {
        status,
        report: Value::Object(report),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn negating_swaps_pass_and_fail_but_keeps_error() {
        assert_eq!(Status::Pass.negated(), Status::Fail);
        assert_eq!(Status::Fail.negated(), Status::Pass);
        assert_eq!(Status::Error.negated(), Status::Error);
    }

    #[test]
    fn unknown_status_text_is_ignored_when_consolidating() {
        let outputs = vec![json!({"status": "skipped"}), json!({"status": "fail"})];
        assert_eq!(consolidate_result(outputs, &json!({}))["status"], "fail");
    }

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
    fn validation_summary_counts_each_status() {
        let summary = summarize_validations(vec![
            json!({"status": "pass"}),
            json!({"status": "fail", "name": "x"}),
        ]);
        assert_eq!(summary.status, Status::Fail);
        assert_eq!(summary.report["validationStatus"], "fail");
        assert_eq!(summary.report["passCount"], 1);
        assert_eq!(
            summary.report["failures"],
            json!([{"status": "fail", "name": "x"}])
        );
    }

    #[test]
    fn validation_report_keeps_its_documented_key_order() {
        let summary = summarize_validations(vec![json!({"status": "error"})]);
        assert_eq!(summary.status, Status::Error);
        let keys: Vec<&String> = summary
            .report
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
