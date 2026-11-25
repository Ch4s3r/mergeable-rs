use crate::app_config::JiraSettings;
use crate::context::Context;
use crate::error::{Error, ErrorKind, Result};
use crate::json_value;
use crate::registry::Registry;
use crate::results;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::sync::Arc;

pub mod logical_connective;
pub mod options;
pub mod options_processor;

pub mod age;
pub mod and_operator;
pub mod approvals;
pub mod assignee;
pub mod author;
pub mod base_ref;
pub mod changeset;
pub mod commit;
pub mod contents;
pub mod dependent;
pub mod description;
pub mod head_ref;
pub mod label;
pub mod last_comment;
pub mod milestone;
pub mod not_operator;
pub mod or_operator;
pub mod project;
pub mod size;
pub mod stale;
pub mod title;

pub const DEFAULT_SUPPORTED_OPTIONS: [&str; 14] = [
    "and",
    "or",
    "begins_with",
    "ends_with",
    "max",
    "min",
    "must_exclude",
    "must_include",
    "no_empty",
    "required",
    "jira",
    "team",
    "one_of",
    "none_of",
];

#[async_trait]
pub trait Validator: Send + Sync {
    fn name(&self) -> &'static str;
    fn supported_events(&self) -> &'static [&'static str];
    fn supported_settings(&self) -> Value;
    fn checks_settings(&self) -> bool {
        true
    }
    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        registry: &Registry,
    ) -> Result<Value>;
}

#[must_use]
pub fn create(key: &str) -> Option<Arc<dyn Validator>> {
    let validator: Arc<dyn Validator> = match key {
        "age" => Arc::new(age::Age),
        "and" => Arc::new(and_operator::And),
        "approvals" => Arc::new(approvals::Approvals),
        "assignee" => Arc::new(assignee::Assignee),
        "author" => Arc::new(author::Author),
        "baseRef" => Arc::new(base_ref::BaseRef),
        "changeset" => Arc::new(changeset::Changeset),
        "commit" => Arc::new(commit::Commit),
        "contents" => Arc::new(contents::Contents),
        "dependent" => Arc::new(dependent::Dependent),
        "description" => Arc::new(description::Description),
        "headRef" => Arc::new(head_ref::HeadRef),
        "label" => Arc::new(label::Label),
        "lastComment" => Arc::new(last_comment::LastComment),
        "milestone" => Arc::new(milestone::Milestone),
        "not" => Arc::new(not_operator::Not),
        "or" => Arc::new(or_operator::Or),
        "project" => Arc::new(project::Project),
        "size" => Arc::new(size::Size),
        "stale" => Arc::new(stale::Stale),
        "title" => Arc::new(title::Title),
        _ => return None,
    };
    Some(validator)
}

pub fn is_event_supported(validator: &dyn Validator, event_name: &str) -> bool {
    crate::event_aware::is_event_supported(validator.supported_events(), event_name)
}

#[tracing::instrument(name = "validator", skip_all, fields(validator.name = validator.name()))]
pub async fn process_validate(
    validator: &dyn Validator,
    context: &Context,
    settings: &Value,
    registry: &Registry,
) -> Result<Value> {
    let supported_settings = validator.supported_settings();
    tracing::info!(
        validator = validator.name(),
        settings = %settings,
        "validator_process"
    );
    if validator.checks_settings() {
        match validate_settings(
            &format!("validator/{}", validator.name()),
            &supported_settings,
            settings,
            &[],
        ) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::UnsupportedSetting => {
                let validator_context = json!({"name": validator.name()});
                let output = vec![results::construct_error_output(
                    &validator_context,
                    Value::String(json_value::stringify(&supported_settings)),
                    settings,
                    error.kind().name(),
                    Some(Value::String(error.to_string())),
                )];
                return Ok(results::consolidate_result(output, &validator_context));
            }
            Err(error) => return Err(error),
        }
    }
    validator.validate(context, settings, registry).await
}

#[must_use]
pub fn option_type(option: &Value) -> &'static str {
    match option {
        Value::Array(_) => "array",
        other => json_value::type_of(Some(other)),
    }
}

pub fn validate_settings(
    owner: &str,
    supported_settings: &Value,
    setting_to_check: &Value,
    nestings: &[String],
) -> Result<()> {
    let supported_keys = json_value::object_keys(Some(supported_settings))?;
    let prefix = if nestings.is_empty() {
        String::new()
    } else {
        format!("{}.", nestings.join("."))
    };
    for key in json_value::object_keys(Some(setting_to_check))? {
        if key == "do" || key == "and" || key == "or" {
            continue;
        }
        if !supported_keys.contains(&key) {
            return Err(Error::unsupported_setting(format!(
                "{owner}: {prefix}{key} option is not supported"
            )));
        }
        let value_to_check = json_value::get(setting_to_check, &key).unwrap_or(&Value::Null);
        let supported_value = supported_setting_at(supported_settings, &key);
        let value_type = option_type(value_to_check);
        if value_type == "object" {
            let mut deeper = nestings.to_vec();
            deeper.push(key);
            validate_settings(owner, &supported_value, value_to_check, &deeper)?;
        } else {
            let allowed = match supported_value.as_ref() {
                Value::String(text) => text.contains(value_type),
                Value::Array(items) => items.iter().any(|item| item.as_str() == Some(value_type)),
                _ => {
                    return Err(Error::type_mismatch(
                        "supportedSettings[key].includes is not a function",
                    ));
                }
            };
            if !allowed {
                return Err(Error::unsupported_setting(format!(
                    "{owner}: {prefix}{key} is expected to be of type: {}",
                    json_value::to_display_string(Some(supported_value.as_ref()))
                )));
            }
        }
    }
    Ok(())
}

fn supported_setting_at<'settings>(
    supported_settings: &'settings Value,
    key: &str,
) -> Cow<'settings, Value> {
    match supported_settings {
        Value::String(text) => Cow::Owned(
            key.parse::<usize>()
                .ok()
                .and_then(|index| text.encode_utf16().nth(index))
                .map_or(Value::Null, |unit| {
                    Value::String(String::from_utf16_lossy(&[unit]))
                }),
        ),
        other => Cow::Borrowed(json_value::get(other, key).unwrap_or(&json_value::NULL)),
    }
}

#[must_use]
pub fn options_context(settings: &Value, supported_options: &[&str]) -> Value {
    json!({
        "name": settings.get("do").unwrap_or(&Value::Null),
        "supportedOptions": supported_options,
    })
}

pub async fn process_options(
    settings: &Value,
    input: &Value,
    supported_options: Option<&[&str]>,
    jira: &JiraSettings,
) -> Result<Value> {
    let validator_context = options_context(
        settings,
        supported_options.unwrap_or(&DEFAULT_SUPPORTED_OPTIONS),
    );
    options::process(&validator_context, input, settings, false, jira).await
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::event_aware::is_event_supported;

    #[test]
    fn event_support_with_one_supported_event() {
        let supported_events = ["issues.opened"];
        assert!(!is_event_supported(
            &supported_events,
            "pull_request.opened"
        ));
        assert!(is_event_supported(&supported_events, "issues.opened"));
    }

    #[test]
    fn event_support_with_several_supported_events() {
        let supported_events = ["issues.opened", "pull_request.opened"];
        assert!(!is_event_supported(
            &supported_events,
            "pull_request.labeled"
        ));
        assert!(is_event_supported(&supported_events, "issues.opened"));
        assert!(is_event_supported(&supported_events, "pull_request.opened"));
    }

    #[test]
    fn event_support_with_a_wildcard_in_the_event_name() {
        let supported_events = ["issues.opened", "pull_request.*"];
        assert!(is_event_supported(
            &supported_events,
            "pull_request.labeled"
        ));
        assert!(!is_event_supported(&supported_events, "issues.milestoned"));
    }

    #[test]
    fn setting_of_the_wrong_type_is_rejected() {
        let supported = json!({"first": "string", "second": "number", "third": "array"});
        let error = validate_settings(
            "validator/test",
            &supported,
            &json!({"first": "Test", "second": "32", "third": [2, 3]}),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "validator/test: second is expected to be of type: number"
        );
        validate_settings(
            "validator/test",
            &supported,
            &json!({"first": "Test", "second": 32}),
            &[],
        )
        .unwrap();
    }

    #[test]
    fn unknown_setting_is_rejected() {
        let setting_to_validate = json!({"first": "Test", "second": 32});
        let error = validate_settings(
            "validator/test",
            &json!({"first": "string"}),
            &setting_to_validate,
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "validator/test: second option is not supported"
        );
        validate_settings(
            "validator/test",
            &json!({"first": "string", "second": "number"}),
            &setting_to_validate,
            &[],
        )
        .unwrap();
    }

    #[test]
    fn nested_settings_are_validated() {
        let supported = json!({"first": "string", "second": {"third": "number"}});
        let error = validate_settings(
            "validator/test",
            &supported,
            &json!({"first": "Test", "second": {"test": 32}}),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "validator/test: second.test option is not supported"
        );
        validate_settings(
            "validator/test",
            &supported,
            &json!({"first": "Test", "second": {"third": 32}}),
            &[],
        )
        .unwrap();
    }

    #[test]
    fn setting_may_allow_several_types() {
        let supported = json!({"first": ["string", "array"]});
        validate_settings("validator/test", &supported, &json!({"first": "Test"}), &[]).unwrap();
        validate_settings("validator/test", &supported, &json!({"first": [32]}), &[]).unwrap();
    }

    #[test]
    fn unsupported_nested_setting_names_its_path() {
        let supported = json!({"must_include": {"regex": "string"}});
        let error = validate_settings(
            "validator/title",
            &supported,
            &json!({"do": "title", "must_include": {"regexp": "x"}}),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "UnSupportedSettingError: validator/title: must_include.regexp option is not supported"
        );
    }

    #[test]
    fn wrong_setting_type_lists_expected_types() {
        let supported = json!({"must_include": {"regex": ["string", "array"]}});
        let error = validate_settings(
            "validator/title",
            &supported,
            &json!({"must_include": {"regex": 5}}),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "validator/title: must_include.regex is expected to be of type: string,array"
        );
    }
}
