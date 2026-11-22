use crate::context::Context;
use crate::error::{Error, Result};
use crate::json_value;
use crate::results::Status;
use serde_json::{Value, json};

const MATCH_NOT_FOUND_ERROR: &str = "Failed to run the test because 'match' is not provided for 'boolean' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected strings \"true\" or \"false\", or boolean literal `true` or `false` as input";

pub fn process(
    _context: &Context,
    filter_context: &Value,
    input: &Value,
    rule: &Value,
) -> Result<Value> {
    match_boolean(filter_context, input, rule)
}

pub fn match_boolean(filter_context: &Value, input: &Value, rule: &Value) -> Result<Value> {
    let boolean_option = json_value::read(Some(rule), "boolean")?;
    let expected = match json_value::read(boolean_option, "match")? {
        None | Some(Value::Null) => return Err(Error::general(MATCH_NOT_FOUND_ERROR)),
        Some(present) => present,
    };
    let is_boolean_input = matches!(input, Value::Bool(_))
        || input.as_str() == Some("true")
        || input.as_str() == Some("false");
    if !is_boolean_input {
        return Err(Error::general(UNKNOWN_INPUT_TYPE_ERROR));
    }
    let filter_name = json_value::to_display_string(filter_context.get("name"));
    let expected_text = json_value::to_display_string(Some(expected));
    let failure_description = match boolean_option.and_then(|option| option.get("message")) {
        Some(message) if json_value::is_truthy(message) => message.clone(),
        _ => Value::String(format!("The {filter_name} must be {expected_text}")),
    };
    let success_description = Value::String(format!("The {filter_name} is {expected_text}"));
    let is_mergeable = json_value::to_display_string(Some(input)) == expected_text;
    Ok(json!({
        "status": Status::from_check(is_mergeable).as_str(),
        "description": if is_mergeable { success_description } else { failure_description },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload_filter() -> Value {
        json!({"name": "payload", "supportedOptions": ["boolean", "must_exclude", "must_include"]})
    }

    fn status_for(expected: &Value, input: &Value) -> String {
        let result = match_boolean(
            &payload_filter(),
            input,
            &json!({"boolean": {"match": expected}}),
        )
        .unwrap();
        json_value::to_display_string(result.get("status"))
    }

    #[test]
    fn string_input_matching_the_expected_boolean_passes() {
        assert_eq!(status_for(&json!(true), &json!("true")), "pass");
        assert_eq!(status_for(&json!(false), &json!("false")), "pass");
    }

    #[test]
    fn boolean_input_matching_the_expected_boolean_passes() {
        assert_eq!(status_for(&json!(true), &json!(true)), "pass");
        assert_eq!(status_for(&json!(false), &json!(false)), "pass");
    }

    #[test]
    fn input_not_matching_the_expected_boolean_fails() {
        assert_eq!(status_for(&json!(true), &json!(false)), "fail");
        assert_eq!(status_for(&json!(false), &json!("true")), "fail");
    }

    #[test]
    fn failing_match_describes_the_expected_value() {
        let result = match_boolean(
            &payload_filter(),
            &json!(false),
            &json!({"boolean": {"match": true}}),
        )
        .unwrap();
        assert_eq!(
            result,
            json!({"status": "fail", "description": "The payload must be true"})
        );
    }

    #[test]
    fn input_that_is_not_a_boolean_is_rejected() {
        let error = match_boolean(
            &payload_filter(),
            &json!(["true"]),
            &json!({"boolean": {"match": true}}),
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Input type invalid, expected strings \"true\" or \"false\", or boolean literal `true` or `false` as input"
        );
    }

    #[test]
    fn missing_match_is_rejected() {
        let error =
            match_boolean(&payload_filter(), &json!(true), &json!({"boolean": {}})).unwrap_err();
        assert_eq!(error.message(), MATCH_NOT_FOUND_ERROR);
    }
}
