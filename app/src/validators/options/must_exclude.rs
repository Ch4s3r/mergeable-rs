use crate::error::{JsError, JsResult};
use crate::js;
use crate::validators::options::lib::js_regex::JsRegex;
use crate::validators::options::lib::{self, OptionResult};
use serde_json::Value;

const REGEX_NOT_FOUND_ERROR: &str = "Failed to run the test because 'regex' is not provided for 'must_exclude' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str =
    "Input type invalid, expected either string or array of string as input";
const KEY_NOT_FOUND_ERROR: &str =
    "Input type is an object and requires providing a 'key' for the 'must_exclude' option.";

fn label_matches(regex: &JsRegex, label: &Value, key: Option<&Value>) -> JsResult<bool> {
    match label {
        Value::String(text) => Ok(regex.test(text)),
        _ if js::truthy(key) => {
            let property = js::read(Some(label), &js::to_js_string(key))?;
            Ok(regex.test(&js::to_js_string(property)))
        }
        _ => Err(JsError::error(KEY_NOT_FOUND_ERROR)),
    }
}

fn every_label(
    regex: &JsRegex,
    labels: &[Value],
    key: Option<&Value>,
    expected: bool,
) -> JsResult<bool> {
    for label in labels {
        if label_matches(regex, label, key)? != expected {
            return Ok(false);
        }
    }
    Ok(true)
}

fn some_label(regex: &JsRegex, labels: &[Value], key: Option<&Value>) -> JsResult<bool> {
    for label in labels {
        if label_matches(regex, label, key)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn excludes_pattern(
    regex: &JsRegex,
    input: &Value,
    key: Option<&Value>,
    all: bool,
) -> JsResult<bool> {
    match input {
        Value::String(text) => Ok(!regex.test(text)),
        Value::Array(labels) if all => every_label(regex, labels, key, false),
        Value::Array(labels) => Ok(!some_label(regex, labels, key)?),
        _ => Err(JsError::error(UNKNOWN_INPUT_TYPE_ERROR)),
    }
}

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> JsResult<OptionResult> {
    let filter = js::get(rule, "must_exclude");
    let regex = js::read(filter, "regex")?;
    let key = js::read(filter, "key")?;
    let message = js::read(filter, "message")?;
    let Some(regex) = regex.filter(|present| js::is_truthy(present)) else {
        return Err(JsError::error(REGEX_NOT_FOUND_ERROR));
    };
    let filter = filter.unwrap_or(&Value::Null);
    let regex_list = lib::concat_into_list(regex);
    let all = js::truthy(js::get(filter, "all"));
    let all_text = if all { "all" } else { "" };
    let joined_patterns = lib::join_list(&regex_list, ", ");
    let validator_name = lib::validator_name(validator_context);
    let success_description =
        format!("{validator_name} {all_text}must exclude '{joined_patterns}'");
    let failure_description = lib::message_or_default(
        message,
        format!("{validator_name} {all_text}does not exclude \"{joined_patterns}\""),
    );
    let flags = lib::regex_flags(filter);

    let mut is_mergeable = true;
    for pattern in &regex_list {
        let compiled = lib::compile_user_regex(pattern, &flags)?;
        if !excludes_pattern(&compiled, input, key, all)? {
            is_mergeable = false;
            break;
        }
    }
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

    fn status_for(rule: Value, input: Value) -> &'static str {
        process(
            &json!({"name": "label"}),
            &input,
            &json!({"must_exclude": rule}),
        )
        .unwrap()
        .status
    }

    #[test]
    fn passes_when_no_label_matches_and_fails_otherwise() {
        assert_eq!(
            status_for(json!({"regex": "test"}), json!(["A", "B", "C"])),
            "pass"
        );
        assert_eq!(
            status_for(json!({"regex": "test"}), json!(["A", "B", "the test"])),
            "fail"
        );
    }

    #[test]
    fn missing_regex_is_an_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!(["the test"]),
            &json!({"must_exclude": {"count": "test"}}),
        )
        .unwrap_err();
        assert_eq!(error.message, REGEX_NOT_FOUND_ERROR);
    }

    #[test]
    fn all_sub_option_checks_every_label() {
        assert_eq!(
            status_for(
                json!({"regex": "test", "all": true}),
                json!(["A", "B", "the test"])
            ),
            "fail"
        );
        assert_eq!(
            status_for(
                json!({"regex": "test", "all": true}),
                json!(["A", "B", "the"])
            ),
            "pass"
        );
    }

    #[test]
    fn regex_flag_none_makes_matching_case_sensitive() {
        assert_eq!(
            status_for(
                json!({"regex": "test", "regex_flag": "none"}),
                json!(["A", "B", "Test"])
            ),
            "pass"
        );
    }

    #[test]
    fn every_regex_of_a_list_must_be_absent() {
        assert_eq!(
            status_for(json!({"regex": ["test", "D"]}), json!(["A", "B", "C"])),
            "pass"
        );
        assert_eq!(
            status_for(
                json!({"regex": ["^the", "F", "G"]}),
                json!(["A", "B", "the test"])
            ),
            "fail"
        );
    }

    #[test]
    fn title_with_odd_but_valid_regex_passes() {
        assert_eq!(
            status_for(json!({"regex": "@#$@#$@#$"}), json!("WIP Title")),
            "pass"
        );
    }
}
