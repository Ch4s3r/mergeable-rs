use crate::error::{Error, Result};
use crate::json_value;
use crate::validators::options::output::{self, OptionResult};
use serde_json::Value;

const COUNT_NOT_FOUND_ERROR: &str = "Failed to run the test because 'count' is not provided for 'max' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected Array or Integer as input";

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> Result<OptionResult> {
    let filter = json_value::get(rule, "max");
    let count_setting = json_value::read(filter, "count")?;
    let count = if json_value::truthy(count_setting) {
        count_setting
    } else {
        filter
    };
    let message = json_value::read(filter, "message")?;
    let Some(Value::Number(count_number)) = count else {
        return Err(Error::general(COUNT_NOT_FOUND_ERROR));
    };
    let count_limit = count_number.as_f64().unwrap_or(f64::NAN);
    let count_text = json_value::number_to_string(count_number);
    let validator_name = output::validator_name(validator_context);
    let success_description = format!("{validator_name} does have a maximum of '{count_text}'");
    let failure_description = output::message_or_default(
        message,
        format!("{validator_name} count is more than \"{count_text}\""),
    );
    let is_mergeable = match input {
        Value::Array(items) => json_value::number_from_length(items.len()) <= count_limit,
        Value::Number(number)
            if number
                .as_f64()
                .is_some_and(|value| value.is_finite() && value.fract() == 0.0) =>
        {
            number.as_f64().unwrap_or(f64::NAN) <= count_limit
        }
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
    use crate::results::Status;
    use serde_json::json;

    fn status_for(count: &Value, input: &Value) -> Status {
        process(
            &json!({"name": "label"}),
            input,
            &json!({"max": {"count": count}}),
        )
        .unwrap()
        .status
    }

    #[test]
    fn list_within_or_at_maximum_passes_and_above_fails() {
        assert_eq!(status_for(&json!(3), &json!(["A", "B"])), Status::Pass);
        assert_eq!(status_for(&json!(3), &json!(["A", "B", "C"])), Status::Pass);
        assert_eq!(
            status_for(&json!(3), &json!(["A", "B", "C", "D"])),
            Status::Fail
        );
    }

    #[test]
    fn integer_within_or_at_maximum_passes_and_above_fails() {
        assert_eq!(status_for(&json!(20), &json!(10)), Status::Pass);
        assert_eq!(status_for(&json!(10), &json!(10)), Status::Pass);
        assert_eq!(status_for(&json!(10), &json!(20)), Status::Fail);
    }

    #[test]
    fn missing_count_is_an_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!(["the test"]),
            &json!({"max": {"regex": 3}}),
        )
        .unwrap_err();
        assert_eq!(error.message(), COUNT_NOT_FOUND_ERROR);
    }

    #[test]
    fn string_input_is_an_input_type_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!("hello"),
            &json!({"max": {"count": 3}}),
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Input type invalid, expected Array or Integer as input"
        );
    }

    #[test]
    fn bare_number_setting_is_the_count() {
        let result = process(
            &json!({"name": "label"}),
            &json!(["A", "B"]),
            &json!({"max": 1}),
        )
        .unwrap();
        assert_eq!(result.description, json!("label count is more than \"1\""));
    }

    fn label_validator_context() -> Value {
        json!({"name": "label", "supportedOptions": ["and", "or", "begins_with", "ends_with", "max", "min", "must_exclude", "must_include", "no_empty", "required"]})
    }

    fn max_status(rule: &Value, input: &Value) -> Status {
        process(&label_validator_context(), input, rule)
            .unwrap()
            .status
    }

    #[test]
    fn max_option_applied_to_arrays_return_pass_if_input_meets_the_criteria() {
        assert_eq!(
            max_status(&json!({"max": {"count": 3}}), &json!(["A", "B"])),
            Status::Pass
        );
    }

    #[test]
    fn max_option_applied_to_arrays_return_pass_if_input_exactly_meets_the_criteria() {
        assert_eq!(
            max_status(&json!({"max": {"count": 3}}), &json!(["A", "B", "C"])),
            Status::Pass
        );
    }

    #[test]
    fn max_option_applied_to_arrays_return_fail_if_input_does_not_meet_the_criteria() {
        assert_eq!(
            max_status(&json!({"max": {"count": 3}}), &json!(["A", "B", "C", "D"])),
            Status::Fail
        );
    }

    #[test]
    fn max_option_applied_to_integers_return_pass_if_input_meets_the_criteria() {
        assert_eq!(
            max_status(&json!({"max": {"count": 20}}), &json!(10)),
            Status::Pass
        );
    }

    #[test]
    fn max_option_applied_to_integers_return_pass_if_input_exactly_meets_criteria() {
        assert_eq!(
            max_status(&json!({"max": {"count": 10}}), &json!(10)),
            Status::Pass
        );
    }

    #[test]
    fn max_option_applied_to_integers_return_fail_if_input_does_not_meet_the_criteria() {
        assert_eq!(
            max_status(&json!({"max": {"count": 10}}), &json!(20)),
            Status::Fail
        );
    }

    #[test]
    fn max_option_configured_incorrectly_return_error_if_rule_config_is_not_in_expected_format() {
        let error = process(
            &label_validator_context(),
            &json!(["the test"]),
            &json!({"max": {"regex": 3}}),
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Failed to run the test because 'count' is not provided for 'max' option. Please check README for more information about configuration"
        );
    }

    #[test]
    fn max_option_configured_incorrectly_return_error_if_input_is_not_supported_type() {
        let error = process(
            &label_validator_context(),
            &json!("hello"),
            &json!({"max": {"count": 3}}),
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Input type invalid, expected Array or Integer as input"
        );
    }
}
