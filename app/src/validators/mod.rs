use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::js;
use crate::registry::Registry;
use crate::results;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

pub mod lib;
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
    fn supported_settings(&self) -> Option<Value>;
    fn checks_settings(&self) -> bool {
        true
    }
    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        registry: &Registry,
    ) -> JsResult<Value>;
}

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

pub async fn process_validate(
    validator: &dyn Validator,
    context: &Context,
    settings: &Value,
    registry: &Registry,
) -> JsResult<Value> {
    let supported_settings = validator.supported_settings().ok_or_else(|| {
        JsError::error("Class extending validators must provide supported Settings")
    })?;
    tracing::info!(
        event_id = %context.event_id,
        repo = %context.repository_full_name(),
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
            Err(error) if error.is_named("UnSupportedSettingError") => {
                let validator_context = json!({"name": validator.name()});
                let output = vec![results::construct_error_output(
                    &validator_context,
                    &Value::String(js::stringify(&supported_settings)),
                    settings,
                    &error.name,
                    Some(Value::String(error.to_string())),
                )];
                return Ok(results::consolidate_result(output, &validator_context));
            }
            Err(error) => return Err(error),
        }
    }
    validator.validate(context, settings, registry).await
}

pub fn option_type(option: &Value) -> &'static str {
    match option {
        Value::Array(_) => "array",
        other => js::type_of(Some(other)),
    }
}

pub fn validate_settings(
    owner: &str,
    supported_settings: &Value,
    setting_to_check: &Value,
    nestings: &[String],
) -> JsResult<()> {
    let supported_keys = js::object_keys(Some(supported_settings))?;
    let prefix = if nestings.is_empty() {
        String::new()
    } else {
        format!("{}.", nestings.join("."))
    };
    for key in js::object_keys(Some(setting_to_check))? {
        if key == "do" || key == "and" || key == "or" {
            continue;
        }
        if !supported_keys.contains(&key) {
            return Err(JsError::unsupported_setting(format!(
                "{owner}: {prefix}{key} option is not supported"
            )));
        }
        let value_to_check = js::get(setting_to_check, &key).unwrap_or(&Value::Null);
        let supported_value = supported_setting_at(supported_settings, &key);
        let value_type = option_type(value_to_check);
        if value_type == "object" {
            let mut deeper = nestings.to_vec();
            deeper.push(key.clone());
            validate_settings(owner, &supported_value, value_to_check, &deeper)?;
        } else {
            let allowed = match &supported_value {
                Value::String(text) => text.contains(value_type),
                Value::Array(items) => items.iter().any(|item| item.as_str() == Some(value_type)),
                _ => {
                    return Err(JsError::type_error(
                        "supportedSettings[key].includes is not a function",
                    ));
                }
            };
            if !allowed {
                return Err(JsError::unsupported_setting(format!(
                    "{owner}: {prefix}{key} is expected to be of type: {}",
                    js::to_js_string(Some(&supported_value))
                )));
            }
        }
    }
    Ok(())
}

fn supported_setting_at(supported_settings: &Value, key: &str) -> Value {
    match supported_settings {
        Value::String(text) => key
            .parse::<usize>()
            .ok()
            .and_then(|index| text.encode_utf16().nth(index))
            .map_or(Value::Null, |unit| {
                Value::String(String::from_utf16_lossy(&[unit]))
            }),
        other => js::get(other, key).cloned().unwrap_or(Value::Null),
    }
}

pub async fn process_options(
    settings: &Value,
    input: &Value,
    supported_options: Option<&[&str]>,
) -> JsResult<Value> {
    let supported: Vec<Value> = supported_options
        .unwrap_or(&DEFAULT_SUPPORTED_OPTIONS)
        .iter()
        .map(|option| Value::String((*option).to_string()))
        .collect();
    let validator_context = json!({
        "name": settings.get("do").cloned().unwrap_or(Value::Null),
        "supportedOptions": supported,
    });
    options::process(&validator_context, input, settings, false).await
}

#[cfg(test)]
mod tests {
    use super::*;

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
            error.message,
            "validator/title: must_include.regex is expected to be of type: string,array"
        );
    }
}
