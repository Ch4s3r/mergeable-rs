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

use crate::error::{Error, Result};
use crate::json_value;
use crate::results;
use futures::future::join_all;
use lib::OptionResult;
use serde_json::{Map, Value};

fn is_option_supported(validator_context: &Value, key: &str) -> Result<bool> {
    match json_value::get(validator_context, "supportedOptions") {
        Some(Value::Array(supported)) => {
            Ok(supported.iter().any(|option| option.as_str() == Some(key)))
        }
        Some(Value::String(supported)) => Ok(supported.contains(key)),
        Some(other) if json_value::is_truthy(other) => Err(Error::type_mismatch(
            "validatorContext.supportedOptions.indexOf is not a function",
        )),
        _ => Ok(true),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImmediateOption {
    BeginsWith,
    EndsWith,
    Max,
    Min,
    MustExclude,
    MustInclude,
    NoEmpty,
    NoneOf,
    OneOf,
    Required,
}

impl ImmediateOption {
    fn evaluate(
        self,
        validator_context: &Value,
        input: &Value,
        rule: &Value,
    ) -> Result<OptionResult> {
        let process = match self {
            ImmediateOption::BeginsWith => begins_with::process,
            ImmediateOption::EndsWith => ends_with::process,
            ImmediateOption::Max => max::process,
            ImmediateOption::Min => min::process,
            ImmediateOption::MustExclude => must_exclude::process,
            ImmediateOption::MustInclude => must_include::process,
            ImmediateOption::NoEmpty => no_empty::process,
            ImmediateOption::NoneOf => none_of::process,
            ImmediateOption::OneOf => one_of::process,
            ImmediateOption::Required => required::process,
        };
        process(validator_context, input, rule)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeferredOption {
    And,
    Or,
    Jira,
}

impl DeferredOption {
    async fn evaluate(
        self,
        validator_context: &Value,
        input: &Value,
        rule: &Value,
    ) -> Result<OptionResult> {
        match self {
            DeferredOption::And => and::process(validator_context, input, rule).await,
            DeferredOption::Or => or::process(validator_context, input, rule).await,
            DeferredOption::Jira => jira::process(validator_context, input, rule).await,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionKind {
    Immediate(ImmediateOption),
    Deferred(DeferredOption),
}

impl OptionKind {
    fn from_key(key: &str) -> Option<Self> {
        let kind = match key {
            "begins_with" => OptionKind::Immediate(ImmediateOption::BeginsWith),
            "ends_with" => OptionKind::Immediate(ImmediateOption::EndsWith),
            "max" => OptionKind::Immediate(ImmediateOption::Max),
            "min" => OptionKind::Immediate(ImmediateOption::Min),
            "must_exclude" => OptionKind::Immediate(ImmediateOption::MustExclude),
            "must_include" => OptionKind::Immediate(ImmediateOption::MustInclude),
            "no_empty" => OptionKind::Immediate(ImmediateOption::NoEmpty),
            "none_of" => OptionKind::Immediate(ImmediateOption::NoneOf),
            "one_of" => OptionKind::Immediate(ImmediateOption::OneOf),
            "required" => OptionKind::Immediate(ImmediateOption::Required),
            "and" => OptionKind::Deferred(DeferredOption::And),
            "or" => OptionKind::Deferred(DeferredOption::Or),
            "jira" => OptionKind::Deferred(DeferredOption::Jira),
            _ => return None,
        };
        Some(kind)
    }
}

fn output_for(
    validator_context: &Value,
    input: &Value,
    setting: &Value,
    outcome: Result<OptionResult>,
) -> Value {
    match outcome {
        Ok(result) => results::construct_output(
            validator_context,
            input.clone(),
            setting,
            result.status.as_str(),
            result.description,
            None,
        ),
        Err(error) => results::construct_error_output(
            validator_context,
            input.clone(),
            setting,
            error.message(),
            None,
        ),
    }
}

struct PendingOption<'rule> {
    option: DeferredOption,
    rule: &'rule Value,
    setting: Value,
}

pub async fn process(
    validator_context: &Value,
    input: &Value,
    rules: &Value,
    return_raw_output: bool,
) -> Result<Value> {
    let rule_list: &[Value] = match rules {
        Value::Array(items) => items,
        single_rule => std::slice::from_ref(single_rule),
    };
    let mut error_outputs = Vec::new();
    let mut immediate_outputs = Vec::new();
    let mut pending_options = Vec::new();

    for rule in rule_list {
        for key in json_value::object_keys(Some(rule))? {
            if key == "do" {
                continue;
            }
            let mut setting_map = Map::new();
            setting_map.insert(
                key.clone(),
                json_value::get(rule, &key).cloned().unwrap_or(Value::Null),
            );
            let setting = Value::Object(setting_map);
            let is_supported = match is_option_supported(validator_context, &key) {
                Ok(is_supported) => is_supported,
                Err(error) => {
                    error_outputs.push(output_for(validator_context, input, &setting, Err(error)));
                    continue;
                }
            };
            if !is_supported {
                let description = format!(
                    "The '{key}' option is not supported for '{}' validator, please see README for all available options",
                    lib::validator_name(validator_context)
                );
                error_outputs.push(results::construct_error_output(
                    validator_context,
                    input.clone(),
                    &setting,
                    &description,
                    None,
                ));
                continue;
            }
            match OptionKind::from_key(&key) {
                Some(OptionKind::Deferred(option)) => {
                    pending_options.push(PendingOption {
                        option,
                        rule,
                        setting,
                    });
                }
                Some(OptionKind::Immediate(option)) => {
                    let outcome = option.evaluate(validator_context, input, rule);
                    let is_error = outcome.is_err();
                    let output = output_for(validator_context, input, &setting, outcome);
                    if is_error {
                        error_outputs.push(output);
                    } else {
                        immediate_outputs.push(output);
                    }
                }
                None => {
                    let missing_module =
                        Error::general(format!("Cannot find module './options/{key}'"));
                    error_outputs.push(output_for(
                        validator_context,
                        input,
                        &setting,
                        Err(missing_module),
                    ));
                }
            }
        }
    }

    // ponytail: and/or/jira outputs are appended in declaration order rather than in completion order
    let deferred_outputs = join_all(pending_options.iter().map(|pending| async move {
        let outcome = pending
            .option
            .evaluate(validator_context, input, pending.rule)
            .await;
        output_for(validator_context, input, &pending.setting, outcome)
    }))
    .await;

    let mut output = error_outputs;
    output.extend(immediate_outputs);
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
    async fn outputs_list_simple_options_before_and_or_results() {
        let rule = json!({"do": "title", "and": [{"must_include": {"regex": "x"}}], "begins_with": {"match": "x"}, "foo": 1, "must_include": {}});
        let result = process(&label_context(), &json!("x"), &rule, true)
            .await
            .unwrap();
        let described: Vec<String> = result
            .as_array()
            .unwrap()
            .iter()
            .map(|output| json_value::to_display_string(output.get("description")))
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
