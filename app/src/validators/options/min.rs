use crate::error::{JsError, JsResult};
use crate::js;
use crate::validators::options::lib::{self, OptionResult};
use serde_json::Value;

const COUNT_NOT_FOUND_ERROR: &str = "Failed to run the test because 'count' is not provided for 'min' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected Array as input";

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> JsResult<OptionResult> {
    let filter = js::get(rule, "min");
    let count_setting = js::read(filter, "count")?;
    let count = if js::truthy(count_setting) {
        count_setting
    } else {
        filter
    };
    let message = js::read(filter, "message")?;
    let Some(Value::Number(count_number)) = count else {
        return Err(JsError::error(COUNT_NOT_FOUND_ERROR));
    };
    let count_limit = count_number.as_f64().unwrap_or(f64::NAN);
    let count_text = js::number_to_string(count_number);
    let validator_name = lib::validator_name(validator_context);
    let success_description = format!("{validator_name} does have a minimum of '{count_text}'");
    let failure_description = lib::message_or_default(
        message,
        format!("{validator_name} count is less than \"{count_text}\""),
    );
    let is_mergeable = match input {
        Value::Array(items) => js::number_from_length(items.len()) >= count_limit,
        _ => return Err(JsError::error(UNKNOWN_INPUT_TYPE_ERROR)),
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

    #[test]
    fn list_at_minimum_passes_and_below_fails() {
        let rule = json!({"min": {"count": 3}});
        assert_eq!(
            process(&json!({"name": "label"}), &json!(["A", "B", "C"]), &rule)
                .unwrap()
                .status,
            "pass"
        );
        assert_eq!(
            process(&json!({"name": "label"}), &json!(["A", "B"]), &rule)
                .unwrap()
                .status,
            "fail"
        );
    }

    #[test]
    fn missing_count_is_an_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!(["the test"]),
            &json!({"min": {"regex": 3}}),
        )
        .unwrap_err();
        assert_eq!(error.message, COUNT_NOT_FOUND_ERROR);
    }

    #[test]
    fn integer_input_is_an_input_type_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!(3),
            &json!({"min": {"count": 3}}),
        )
        .unwrap_err();
        assert_eq!(error.message, UNKNOWN_INPUT_TYPE_ERROR);
    }
}
