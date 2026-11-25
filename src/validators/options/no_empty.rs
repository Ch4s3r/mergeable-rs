use crate::error::{Error, Result};
use crate::json_value;
use crate::results::Status;
use crate::validators::options::output::{self, OptionResult};
use crate::validators::options::text_matching;
use serde_json::Value;

const ENABLED_NOT_FOUND_ERROR: &str = "Failed to run the test because 'enabled' is not provided for 'no_empty' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected string or Array as input";

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> Result<OptionResult> {
    let filter = json_value::get(rule, "no_empty");
    let enabled = json_value::read(filter, "enabled")?;
    let message = json_value::read(filter, "message")?;
    let explicitly_disabled = enabled == Some(&Value::Bool(false));
    if !json_value::truthy(enabled) && !explicitly_disabled {
        return Err(Error::general(ENABLED_NOT_FOUND_ERROR));
    }
    if explicitly_disabled {
        return Ok(OptionResult {
            status: Status::Pass,
            description: Value::String(
                "No_empty option is not enabled, as such this validator did not run".to_string(),
            ),
        });
    }
    let validator_name = output::validator_name(validator_context);
    let success_description = format!("The {validator_name} is not empty");
    let failure_description =
        output::message_or_default(message, format!("The {validator_name} can't be empty"));
    let is_mergeable = match input {
        Value::String(text) => !text_matching::trim_whitespace(text).is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Null => false,
        _ => return Err(Error::general(UNKNOWN_INPUT_TYPE_ERROR)),
    };
    Ok(OptionResult::decided(
        is_mergeable,
        success_description,
        failure_description,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assert_status(enabled: bool, input: &Value, list_input: &Value, expected: Status) {
        let rule = json!({"no_empty": {"enabled": enabled}});
        assert_eq!(
            process(&json!({"name": "label"}), input, &rule)
                .unwrap()
                .status,
            expected
        );
        assert_eq!(
            process(&json!({"name": "label"}), list_input, &rule)
                .unwrap()
                .status,
            expected
        );
    }

    #[test]
    fn non_empty_input_passes_and_disabled_option_always_passes() {
        assert_status(true, &json!("NOT EMPTY"), &json!([""]), Status::Pass);
        assert_status(false, &json!("NOT EMPTY"), &json!([""]), Status::Pass);
        assert_status(false, &json!(""), &json!([""]), Status::Pass);
    }

    #[test]
    fn empty_or_missing_input_fails() {
        assert_status(true, &json!(""), &json!([]), Status::Fail);
        assert_status(true, &Value::Null, &json!([]), Status::Fail);
        assert_status(true, &json!("  \n "), &json!([]), Status::Fail);
    }

    #[test]
    fn number_input_is_an_input_type_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!(1),
            &json!({"no_empty": {"enabled": true}}),
        )
        .unwrap_err();
        assert_eq!(error.message(), UNKNOWN_INPUT_TYPE_ERROR);
    }

    #[test]
    fn missing_enabled_is_an_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!("the test"),
            &json!({"no_empty": {"regex": true}}),
        )
        .unwrap_err();
        assert_eq!(error.message(), ENABLED_NOT_FOUND_ERROR);
    }

    fn label_validator_context() -> Value {
        json!({"name": "label", "supportedOptions": ["and", "or", "begins_with", "ends_with", "max", "min", "must_exclude", "must_include", "no_empty", "required"]})
    }

    fn verify(enabled: bool, input: Option<&Value>, list_input: &Value, expected: Status) {
        let rule = json!({"no_empty": {"enabled": enabled}});
        let absent_or_given = input.unwrap_or(&Value::Null);
        assert_eq!(
            process(&label_validator_context(), absent_or_given, &rule)
                .unwrap()
                .status,
            expected
        );
        assert_eq!(
            process(&label_validator_context(), list_input, &rule)
                .unwrap()
                .status,
            expected
        );
    }

    #[test]
    fn return_pass_if_input_meets_the_criteria() {
        verify(true, Some(&json!("NOT EMPTY")), &json!([""]), Status::Pass);
        verify(false, Some(&json!("NOT EMPTY")), &json!([""]), Status::Pass);
    }

    #[test]
    fn return_fail_if_input_does_not_meet_the_criteria() {
        verify(true, Some(&json!("")), &json!([]), Status::Fail);
        verify(true, Some(&Value::Null), &json!([]), Status::Fail);
        verify(true, None, &json!([]), Status::Fail);
        verify(false, Some(&json!("")), &json!([""]), Status::Pass);
    }

    #[test]
    fn return_error_if_input_does_not_meet_the_criteria() {
        let rule = json!({"no_empty": {"enabled": true}});
        let error = process(&label_validator_context(), &json!(1), &rule).unwrap_err();
        assert_eq!(
            error.message(),
            "Input type invalid, expected string or Array as input"
        );
        let list_of_number = process(&label_validator_context(), &json!([1]), &rule);
        assert!(list_of_number.is_ok());
    }

    #[test]
    fn return_error_if_inputs_are_not_in_expected_format() {
        let rule = json!({"no_empty": {"regex": true}});
        let error = process(&label_validator_context(), &json!("the test"), &rule).unwrap_err();
        assert_eq!(
            error.message(),
            "Failed to run the test because 'enabled' is not provided for 'no_empty' option. Please check README for more information about configuration"
        );
    }
}
