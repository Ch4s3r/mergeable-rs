use crate::error::{JsError, JsResult};
use crate::js;
use crate::validators::options::lib::js_regex::JsRegex;
use crate::validators::options::lib::{self, OptionResult};
use serde_json::Value;

const REGEX_NOT_FOUND_ERROR: &str = "Failed to run the test because 'regex' is not provided for 'must_include' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str =
    "Input type invalid, expected either string or array of string as input";
const KEY_NOT_FOUND_ERROR: &str =
    "Input type is an object and requires providing a 'key' for the 'must_include' option.";

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

fn includes_pattern(
    regex: &JsRegex,
    input: &Value,
    key: Option<&Value>,
    all: bool,
) -> JsResult<bool> {
    match input {
        Value::String(text) => Ok(regex.test(text)),
        Value::Array(labels) if all => every_label(regex, labels, key, true),
        Value::Array(labels) => some_label(regex, labels, key),
        _ => Err(JsError::error(UNKNOWN_INPUT_TYPE_ERROR)),
    }
}

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> JsResult<OptionResult> {
    let filter = js::get(rule, "must_include");
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
        format!("{validator_name} {all_text}must include '{joined_patterns}'");
    let failure_description = lib::message_or_default(
        message,
        format!("{validator_name} {all_text}does not include \"{joined_patterns}\""),
    );
    let flags = lib::regex_flags(filter);

    let mut is_mergeable = false;
    for pattern in &regex_list {
        let compiled = lib::compile_user_regex(pattern, &flags)?;
        if includes_pattern(&compiled, input, key, all)? {
            is_mergeable = true;
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

    fn run(rule: Value, input: Value) -> OptionResult {
        process(
            &json!({"name": "label"}),
            &input,
            &json!({"must_include": rule}),
        )
        .unwrap()
    }

    #[test]
    fn passes_when_a_label_matches() {
        assert_eq!(
            run(json!({"regex": "test"}), json!(["A", "B", "the test"])).status,
            "pass"
        );
    }

    #[test]
    fn fails_with_custom_message_when_nothing_matches() {
        let result = run(
            json!({"regex": "test", "message": "failed Test"}),
            json!(["A", "B"]),
        );
        assert_eq!(result.status, "fail");
        assert_eq!(result.description, json!("failed Test"));
    }

    #[test]
    fn missing_regex_is_an_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!(["the test"]),
            &json!({"must_include": {"count": "test"}}),
        )
        .unwrap_err();
        assert_eq!(error.message, REGEX_NOT_FOUND_ERROR);
    }

    #[test]
    fn all_requires_every_label_to_match() {
        assert_eq!(
            run(
                json!({"regex": "test", "all": true}),
                json!(["A test", "B", "the test"])
            )
            .status,
            "fail"
        );
        assert_eq!(
            run(
                json!({"regex": "test", "all": true}),
                json!(["A test", "B test", "the test"])
            )
            .status,
            "pass"
        );
    }

    #[test]
    fn regex_flag_none_makes_matching_case_sensitive() {
        assert_eq!(
            run(
                json!({"regex": "test", "regex_flag": "none"}),
                json!(["A", "B", "Test"])
            )
            .status,
            "fail"
        );
    }

    #[test]
    fn any_regex_of_a_list_may_match() {
        assert_eq!(
            run(
                json!({"regex": ["^the\\stest$", "nothing"]}),
                json!(["A", "B", "the test"])
            )
            .status,
            "pass"
        );
        let result = run(
            json!({"regex": ["test", "another"], "message": "failed array Test"}),
            json!(["E", "F"]),
        );
        assert_eq!(result.status, "fail");
        assert_eq!(result.description, json!("failed array Test"));
    }

    #[test]
    fn object_labels_need_a_key() {
        assert_eq!(
            run(
                json!({"regex": "bug", "key": "name"}),
                json!([{"name": "bug"}])
            )
            .status,
            "pass"
        );
        let error = process(
            &json!({"name": "label"}),
            &json!([{"name": "bug"}]),
            &json!({"must_include": {"regex": "bug"}}),
        )
        .unwrap_err();
        assert_eq!(error.message, KEY_NOT_FOUND_ERROR);
    }

    #[test]
    fn invalid_regex_names_the_pattern() {
        let error = process(
            &json!({"name": "title"}),
            &json!("x"),
            &json!({"must_include": {"regex": "("}}),
        )
        .unwrap_err();
        assert_eq!(
            error.message,
            "Failed to create a regex expression with the provided regex: ("
        );
    }

    #[test]
    fn default_descriptions_join_patterns_and_glue_all_to_the_verb() {
        let result = run(json!({"regex": ["a", "b"], "all": true}), json!(["a"]));
        assert_eq!(result.description, json!("label allmust include 'a, b'"));
    }
}
