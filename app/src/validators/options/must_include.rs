use crate::error::{Error, Result};
use crate::json_value;
use crate::validators::options::output::{self, OptionResult};
use crate::validators::options::text_matching;
use crate::validators::options::user_pattern::UserPattern;
use serde_json::Value;

const REGEX_NOT_FOUND_ERROR: &str = "Failed to run the test because 'regex' is not provided for 'must_include' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str =
    "Input type invalid, expected either string or array of string as input";
const KEY_NOT_FOUND_ERROR: &str =
    "Input type is an object and requires providing a 'key' for the 'must_include' option.";

fn label_matches(regex: &UserPattern, label: &Value, key: Option<&Value>) -> Result<bool> {
    match label {
        Value::String(text) => Ok(regex.is_match(text)),
        _ if json_value::truthy(key) => {
            let property = json_value::read(Some(label), &json_value::to_display_string(key))?;
            Ok(regex.is_match(&json_value::to_display_string(property)))
        }
        _ => Err(Error::general(KEY_NOT_FOUND_ERROR)),
    }
}

fn every_label(
    regex: &UserPattern,
    labels: &[Value],
    key: Option<&Value>,
    expected: bool,
) -> Result<bool> {
    for label in labels {
        if label_matches(regex, label, key)? != expected {
            return Ok(false);
        }
    }
    Ok(true)
}

fn some_label(regex: &UserPattern, labels: &[Value], key: Option<&Value>) -> Result<bool> {
    for label in labels {
        if label_matches(regex, label, key)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn includes_pattern(
    regex: &UserPattern,
    input: &Value,
    key: Option<&Value>,
    all: bool,
) -> Result<bool> {
    match input {
        Value::String(text) => Ok(regex.is_match(text)),
        Value::Array(labels) if all => every_label(regex, labels, key, true),
        Value::Array(labels) => some_label(regex, labels, key),
        _ => Err(Error::general(UNKNOWN_INPUT_TYPE_ERROR)),
    }
}

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> Result<OptionResult> {
    let filter = json_value::get(rule, "must_include");
    let regex = json_value::read(filter, "regex")?;
    let key = json_value::read(filter, "key")?;
    let message = json_value::read(filter, "message")?;
    let Some(regex) = regex.filter(|present| json_value::is_truthy(present)) else {
        return Err(Error::general(REGEX_NOT_FOUND_ERROR));
    };
    let filter = filter.unwrap_or(&Value::Null);
    let regex_list = text_matching::concat_into_list(regex);
    let all = json_value::truthy(json_value::get(filter, "all"));
    let all_text = if all { "all" } else { "" };
    let joined_patterns = text_matching::join_list(regex_list, ", ");
    let validator_name = output::validator_name(validator_context);
    let success_description =
        format!("{validator_name} {all_text}must include '{joined_patterns}'");
    let failure_description = output::message_or_default(
        message,
        format!("{validator_name} {all_text}does not include \"{joined_patterns}\""),
    );
    let flags = text_matching::regex_flags(filter);

    let mut is_mergeable = false;
    for pattern in regex_list {
        let compiled = text_matching::compile_user_regex(pattern, &flags)?;
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
    use crate::results::Status;
    use serde_json::json;

    fn run(rule: &Value, input: &Value) -> OptionResult {
        process(
            &json!({"name": "label"}),
            input,
            &json!({"must_include": rule}),
        )
        .unwrap()
    }

    #[test]
    fn passes_when_a_label_matches() {
        assert_eq!(
            run(&json!({"regex": "test"}), &json!(["A", "B", "the test"])).status,
            Status::Pass
        );
    }

    #[test]
    fn fails_with_custom_message_when_nothing_matches() {
        let result = run(
            &json!({"regex": "test", "message": "failed Test"}),
            &json!(["A", "B"]),
        );
        assert_eq!(result.status, Status::Fail);
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
        assert_eq!(error.message(), REGEX_NOT_FOUND_ERROR);
    }

    #[test]
    fn all_requires_every_label_to_match() {
        assert_eq!(
            run(
                &json!({"regex": "test", "all": true}),
                &json!(["A test", "B", "the test"])
            )
            .status,
            Status::Fail
        );
        assert_eq!(
            run(
                &json!({"regex": "test", "all": true}),
                &json!(["A test", "B test", "the test"])
            )
            .status,
            Status::Pass
        );
    }

    #[test]
    fn regex_flag_none_makes_matching_case_sensitive() {
        assert_eq!(
            run(
                &json!({"regex": "test", "regex_flag": "none"}),
                &json!(["A", "B", "Test"])
            )
            .status,
            Status::Fail
        );
    }

    #[test]
    fn any_regex_of_a_list_may_match() {
        assert_eq!(
            run(
                &json!({"regex": ["^the\\stest$", "nothing"]}),
                &json!(["A", "B", "the test"])
            )
            .status,
            Status::Pass
        );
        let result = run(
            &json!({"regex": ["test", "another"], "message": "failed array Test"}),
            &json!(["E", "F"]),
        );
        assert_eq!(result.status, Status::Fail);
        assert_eq!(result.description, json!("failed array Test"));
    }

    #[test]
    fn object_labels_need_a_key() {
        assert_eq!(
            run(
                &json!({"regex": "bug", "key": "name"}),
                &json!([{"name": "bug"}])
            )
            .status,
            Status::Pass
        );
        let error = process(
            &json!({"name": "label"}),
            &json!([{"name": "bug"}]),
            &json!({"must_include": {"regex": "bug"}}),
        )
        .unwrap_err();
        assert_eq!(error.message(), KEY_NOT_FOUND_ERROR);
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
            error.message(),
            "Failed to create a regex expression with the provided regex: ("
        );
    }

    #[test]
    fn default_descriptions_join_patterns_and_glue_all_to_the_verb() {
        let result = run(&json!({"regex": ["a", "b"], "all": true}), &json!(["a"]));
        assert_eq!(result.description, json!("label allmust include 'a, b'"));
    }
}
