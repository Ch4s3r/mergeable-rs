pub mod and;
pub mod begins_with;
pub mod ends_with;
pub mod jira;
pub mod lib;
pub mod max;
pub mod min;
pub mod must_exclude;
pub mod must_include;
pub mod no_empty;
pub mod none_of;
pub mod one_of;
pub mod or;
pub mod required;

use crate::error::{JsError, JsResult};
use crate::js;
use crate::results;
use futures::future::join_all;
use lib::OptionResult;
use serde_json::{Map, Value};

fn is_option_supported(validator_context: &Value, key: &str) -> JsResult<bool> {
    match js::get(validator_context, "supportedOptions") {
        Some(Value::Array(supported)) => {
            Ok(supported.iter().any(|option| option.as_str() == Some(key)))
        }
        Some(Value::String(supported)) => Ok(supported.contains(key)),
        Some(other) if js::is_truthy(other) => Err(JsError::type_error(
            "validatorContext.supportedOptions.indexOf is not a function",
        )),
        _ => Ok(true),
    }
}

fn synchronous_option(
    key: &str,
    validator_context: &Value,
    input: &Value,
    rule: &Value,
) -> Option<JsResult<OptionResult>> {
    let result = match key {
        "begins_with" => begins_with::process(validator_context, input, rule),
        "ends_with" => ends_with::process(validator_context, input, rule),
        "max" => max::process(validator_context, input, rule),
        "min" => min::process(validator_context, input, rule),
        "must_exclude" => must_exclude::process(validator_context, input, rule),
        "must_include" => must_include::process(validator_context, input, rule),
        "no_empty" => no_empty::process(validator_context, input, rule),
        "none_of" => none_of::process(validator_context, input, rule),
        "one_of" => one_of::process(validator_context, input, rule),
        "required" => required::process(validator_context, input, rule),
        _ => return None,
    };
    Some(result)
}

async fn asynchronous_option(
    key: &str,
    validator_context: &Value,
    input: &Value,
    rule: &Value,
) -> JsResult<OptionResult> {
    match key {
        "and" => and::process(validator_context, input, rule).await,
        "or" => or::process(validator_context, input, rule).await,
        _ => jira::process(validator_context, input, rule).await,
    }
}

fn output_for(
    validator_context: &Value,
    input: &Value,
    setting: &Value,
    outcome: JsResult<OptionResult>,
) -> Value {
    match outcome {
        Ok(result) => results::construct_output(
            validator_context,
            input,
            setting,
            result.status,
            result.description,
            None,
        ),
        Err(error) => {
            results::construct_error_output(validator_context, input, setting, &error.message, None)
        }
    }
}

struct DeferredOption {
    key: String,
    rule: Value,
    setting: Value,
}

pub async fn process(
    validator_context: &Value,
    input: &Value,
    rules: &Value,
    return_raw_output: bool,
) -> JsResult<Value> {
    let rule_list = js::as_array_or_wrap(rules);
    let mut pushed_synchronously = Vec::new();
    let mut pushed_after_one_tick = Vec::new();
    let mut deferred_options = Vec::new();

    for rule in &rule_list {
        for key in js::object_keys(Some(rule))? {
            if key == "do" {
                continue;
            }
            let mut setting_map = Map::new();
            setting_map.insert(
                key.clone(),
                js::get(rule, &key).cloned().unwrap_or(Value::Null),
            );
            let setting = Value::Object(setting_map);
            let is_supported = match is_option_supported(validator_context, &key) {
                Ok(is_supported) => is_supported,
                Err(error) => {
                    pushed_synchronously.push(output_for(
                        validator_context,
                        input,
                        &setting,
                        Err(error),
                    ));
                    continue;
                }
            };
            if !is_supported {
                let description = format!(
                    "The '{key}' option is not supported for '{}' validator, please see README for all available options",
                    lib::validator_name(validator_context)
                );
                pushed_synchronously.push(results::construct_error_output(
                    validator_context,
                    input,
                    &setting,
                    &description,
                    None,
                ));
                continue;
            }
            if matches!(key.as_str(), "and" | "or" | "jira") {
                deferred_options.push(DeferredOption {
                    key,
                    rule: rule.clone(),
                    setting,
                });
                continue;
            }
            match synchronous_option(&key, validator_context, input, rule) {
                Some(Ok(result)) => pushed_after_one_tick.push(output_for(
                    validator_context,
                    input,
                    &setting,
                    Ok(result),
                )),
                Some(Err(error)) => pushed_synchronously.push(output_for(
                    validator_context,
                    input,
                    &setting,
                    Err(error),
                )),
                None => {
                    let missing_module =
                        JsError::error(format!("Cannot find module './options/{key}'"));
                    pushed_synchronously.push(output_for(
                        validator_context,
                        input,
                        &setting,
                        Err(missing_module),
                    ));
                }
            }
        }
    }

    // ponytail: Node appends and/or/jira outputs in promise completion order; they are appended here in declaration order
    let deferred_outputs = join_all(deferred_options.iter().map(|deferred| async move {
        let outcome =
            asynchronous_option(&deferred.key, validator_context, input, &deferred.rule).await;
        output_for(validator_context, input, &deferred.setting, outcome)
    }))
    .await;

    let mut output = pushed_synchronously;
    output.extend(pushed_after_one_tick);
    output.extend(deferred_outputs);
    if return_raw_output {
        Ok(Value::Array(output))
    } else {
        Ok(results::consolidate_result(output, validator_context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn label_context() -> Value {
        json!({"name": "label", "supportedOptions": ["and", "or", "begins_with", "ends_with", "max", "min", "must_exclude", "must_include", "no_empty", "required", "jira", "team", "one_of", "none_of"]})
    }

    #[tokio::test]
    async fn valid_rules_pass_for_single_rule_and_rule_list() {
        let single = process(
            &json!("label"),
            &json!(["A", "C"]),
            &json!({"do": "label", "must_include": {"regex": "A"}}),
            false,
        )
        .await
        .unwrap();
        assert_eq!(single["status"], "pass");
        let list = json!([{"do": "label", "must_include": {"regex": "A"}}, {"do": "label", "must_exclude": {"regex": "B"}}]);
        let combined = process(&json!("label"), &json!(["A", "C"]), &list, false)
            .await
            .unwrap();
        assert_eq!(combined["status"], "pass");
    }

    #[tokio::test]
    async fn unknown_option_reports_missing_module() {
        let result = process(
            &json!("label"),
            &json!(["A"]),
            &json!({"do": "label", "must_be_include": {"regex": "A"}}),
            false,
        )
        .await
        .unwrap();
        assert_eq!(result["status"], "error");
        assert_eq!(
            result["validations"][0]["description"],
            "Cannot find module './options/must_be_include'"
        );
    }

    #[tokio::test]
    async fn team_is_supported_by_default_but_has_no_option_module() {
        let result = process(
            &label_context(),
            &json!("alice"),
            &json!({"do": "author", "team": ""}),
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            result["validations"][0]["description"],
            "Cannot find module './options/team'"
        );
    }

    #[tokio::test]
    async fn raw_output_is_the_list_of_option_outputs() {
        let result = process(
            &json!("label"),
            &json!("A"),
            &json!({"do": "label", "must_include": {"regex": "A"}}),
            true,
        )
        .await
        .unwrap();
        assert!(result.is_array());
    }

    #[tokio::test]
    async fn unsupported_option_names_the_validator() {
        let context = json!({"name": "title", "supportedOptions": ["must_include"]});
        let result = process(
            &context,
            &json!("x"),
            &json!({"do": "title", "max": {"count": 1}}),
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            result["validations"][0]["description"],
            "The 'max' option is not supported for 'title' validator, please see README for all available options"
        );
    }

    #[tokio::test]
    async fn outputs_are_ordered_like_node_microtasks() {
        let rule = json!({"do": "title", "and": [{"must_include": {"regex": "x"}}], "begins_with": {"match": "x"}, "foo": 1, "must_include": {}});
        let result = process(&label_context(), &json!("x"), &rule, true)
            .await
            .unwrap();
        let described: Vec<String> = result
            .as_array()
            .unwrap()
            .iter()
            .map(|output| js::to_js_string(output.get("description")))
            .collect();
        assert_eq!(
            described,
            vec![
                "The 'foo' option is not supported for 'label' validator, please see README for all available options".to_string(),
                "Failed to run the test because 'regex' is not provided for 'must_include' option. Please check README for more information about configuration".to_string(),
                "label does begins with 'x'".to_string(),
                "All the requisite validations passed for 'and' option".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn null_rule_is_a_type_error() {
        let error = process(&label_context(), &json!("x"), &Value::Null, false)
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot convert undefined or null to object"
        );
    }
}
