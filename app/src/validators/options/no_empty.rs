use crate::error::{JsError, JsResult};
use crate::js;
use crate::validators::options::lib::{self, OptionResult};
use serde_json::Value;

const ENABLED_NOT_FOUND_ERROR: &str = "Failed to run the test because 'enabled' is not provided for 'no_empty' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected string or Array as input";

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> JsResult<OptionResult> {
    let filter = js::get(rule, "no_empty");
    let enabled = js::read(filter, "enabled")?;
    let message = js::read(filter, "message")?;
    let explicitly_disabled = enabled == Some(&Value::Bool(false));
    if !js::truthy(enabled) && !explicitly_disabled {
        return Err(JsError::error(ENABLED_NOT_FOUND_ERROR));
    }
    if explicitly_disabled {
        return Ok(OptionResult {
            status: "pass",
            description: Value::String(
                "No_empty option is not enabled, as such this validator did not run".to_string(),
            ),
        });
    }
    let validator_name = lib::validator_name(validator_context);
    let success_description = format!("The {validator_name} is not empty");
    let failure_description =
        lib::message_or_default(message, format!("The {validator_name} can't be empty"));
    let is_mergeable = match input {
        Value::String(text) => !lib::js_trim(text).is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Null => false,
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

    fn assert_status(enabled: bool, input: &Value, list_input: &Value, expected: &str) {
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
        assert_status(true, &json!("NOT EMPTY"), &json!([""]), "pass");
        assert_status(false, &json!("NOT EMPTY"), &json!([""]), "pass");
        assert_status(false, &json!(""), &json!([""]), "pass");
    }

    #[test]
    fn empty_or_missing_input_fails() {
        assert_status(true, &json!(""), &json!([]), "fail");
        assert_status(true, &Value::Null, &json!([]), "fail");
        assert_status(true, &json!("  \n "), &json!([]), "fail");
    }

    #[test]
    fn number_input_is_an_input_type_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!(1),
            &json!({"no_empty": {"enabled": true}}),
        )
        .unwrap_err();
        assert_eq!(error.message, UNKNOWN_INPUT_TYPE_ERROR);
    }

    #[test]
    fn missing_enabled_is_an_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!("the test"),
            &json!({"no_empty": {"regex": true}}),
        )
        .unwrap_err();
        assert_eq!(error.message, ENABLED_NOT_FOUND_ERROR);
    }
}
