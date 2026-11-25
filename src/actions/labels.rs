use crate::actions::{Action, actionables, spawn_detached};
use crate::context::Context;
use crate::error::{Error, Result};
use crate::github::api;
use crate::json_value;
use async_trait::async_trait;
use futures::future::join_all;
use serde_json::Value;
use std::sync::Arc;

pub const SUPPORTED_EVENTS: [&str; 4] = [
    "pull_request.*",
    "issues.*",
    "issue_comment.*",
    "schedule.repository",
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelMode {
    Add,
    Delete,
    Replace,
}

impl LabelMode {
    const ALL: [LabelMode; 3] = [LabelMode::Add, LabelMode::Delete, LabelMode::Replace];

    fn as_str(self) -> &'static str {
        match self {
            LabelMode::Add => "add",
            LabelMode::Delete => "delete",
            LabelMode::Replace => "replace",
        }
    }

    fn from_setting(mode: Option<&Value>) -> Result<Self> {
        if !json_value::truthy(mode) {
            return Ok(LabelMode::Add);
        }
        mode.and_then(Value::as_str)
            .and_then(|name| Self::ALL.into_iter().find(|mode| mode.as_str() == name))
            .ok_or_else(|| {
                let supported: Vec<&str> = Self::ALL.iter().map(|mode| mode.as_str()).collect();
                Error::unsupported_setting(format!(
                    "Unknown mode, supported modes are {}",
                    supported.join(", ")
                ))
            })
    }
}

fn pattern_list(patterns: &Value) -> Result<&[Value]> {
    match patterns {
        Value::Array(items) => Ok(items),
        Value::Null => Err(Error::type_mismatch(
            "Cannot read properties of null (reading 'some')",
        )),
        _ => Err(Error::type_mismatch("patterns.some is not a function")),
    }
}

fn matches_any_pattern(label: &Value, patterns: &[Value]) -> Result<bool> {
    for pattern in patterns {
        if crate::glob::matches_value(label, pattern, crate::glob::DotEntries::Excluded)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn labels_without_any_pattern(labels: Vec<Value>, patterns: &[Value]) -> Result<Vec<Value>> {
    let mut kept = Vec::new();
    for label in labels {
        if !matches_any_pattern(&label, patterns)? {
            kept.push(label);
        }
    }
    Ok(kept)
}

pub fn labels_without_matches(labels: Vec<Value>, patterns: &Value) -> Result<Vec<Value>> {
    if labels.is_empty() {
        return Ok(labels);
    }
    labels_without_any_pattern(labels, pattern_list(patterns)?)
}

pub fn combine_labels(current: Vec<Value>, settings: &Value) -> Result<Vec<Value>> {
    let mut labels = current;
    if let Some(addition) = settings
        .get("add")
        .filter(|addition| json_value::is_truthy(addition))
    {
        labels.extend_from_slice(json_value::as_array_or_wrap(addition));
    }
    if let Some(deletion) = settings
        .get("delete")
        .filter(|deletion| json_value::is_truthy(deletion))
    {
        labels = labels_without_any_pattern(labels, json_value::as_array_or_wrap(deletion))?;
    }
    Ok(labels)
}

async fn update_labels(context: &Context, issue: &Value, settings: &Value) -> Result<()> {
    let issue_number = json_value::read(Some(issue), "number")?.unwrap_or(&Value::Null);
    let current = match settings
        .get("replace")
        .filter(|replacement| json_value::is_truthy(replacement))
    {
        Some(replacement) => json_value::as_array_or_wrap(replacement).to_vec(),
        None => api::list_labels_on_issue(context, issue_number).await?,
    };
    let labels = combine_labels(current, settings)?;
    api::set_labels(context, issue_number, &Value::Array(labels))
        .await
        .map(|_| ())
}

fn labels_to_keep(current: Vec<Value>, patterns: Option<&Value>) -> Result<Vec<Value>> {
    match patterns {
        _ if current.is_empty() => Ok(Vec::new()),
        None => Err(Error::missing_property("some")),
        Some(pattern_list) => labels_without_matches(current, pattern_list),
    }
}

fn log_detached_failure(error: &Error) {
    tracing::error!(%error, "action.labels.failed");
}

async fn delete_matching_labels(
    context: &Context,
    issue_numbers: Vec<Value>,
    patterns: Option<&Value>,
) {
    let current_labels = join_all(
        issue_numbers
            .iter()
            .map(|issue_number| api::list_labels_on_issue(context, issue_number)),
    )
    .await;
    for (issue_number, listed) in issue_numbers.into_iter().zip(current_labels) {
        match listed.and_then(|current| labels_to_keep(current, patterns)) {
            Ok(kept) => {
                let task_context = context.clone();
                spawn_detached("action/labels", async move {
                    api::set_labels(&task_context, &issue_number, &Value::Array(kept))
                        .await
                        .map(|_| ())
                });
            }
            Err(error) => log_detached_failure(&error),
        }
    }
}

#[derive(Default)]
pub struct Labels;

#[async_trait]
impl Action for Labels {
    fn name(&self) -> &'static str {
        "labels"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &SUPPORTED_EVENTS
    }

    async fn after_validate(
        &self,
        context: &Context,
        settings: &Value,
        _rule_name: Option<&Value>,
        validation_report: &Value,
    ) -> Result<()> {
        let items = actionables(context, validation_report);
        let uses_label_operations = ["replace", "add", "delete"]
            .iter()
            .any(|key| json_value::truthy(settings.get(*key)));
        if uses_label_operations {
            let updates = items
                .iter()
                .map(|issue| update_labels(context, issue, settings));
            for outcome in join_all(updates).await {
                outcome?;
            }
            return Ok(());
        }
        let mode = LabelMode::from_setting(settings.get("mode"))?;
        let configured_labels = settings.get("labels");
        let mut issue_numbers = Vec::new();
        let mut unreadable_issue = None;
        for issue in &items {
            match json_value::read(Some(&**issue), "number") {
                Ok(number) => issue_numbers.push(number.cloned().unwrap_or(Value::Null)),
                Err(error) => {
                    unreadable_issue = Some(error);
                    break;
                }
            }
        }
        if mode == LabelMode::Delete {
            delete_matching_labels(context, issue_numbers, configured_labels).await;
        } else {
            let shared_labels = Arc::new(configured_labels.cloned().unwrap_or(Value::Null));
            for issue_number in issue_numbers {
                let task_context = context.clone();
                let labels = Arc::clone(&shared_labels);
                let replaces = mode == LabelMode::Replace;
                spawn_detached("action/labels", async move {
                    let response = if replaces {
                        api::set_labels(&task_context, &issue_number, &labels).await
                    } else {
                        api::add_labels(&task_context, &issue_number, &labels).await
                    };
                    response.map(|_| ())
                });
            }
        }
        match unreadable_issue {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strings(values: Vec<Value>) -> Vec<String> {
        values
            .into_iter()
            .map(|value| value.as_str().unwrap_or_default().to_string())
            .collect()
    }

    #[test]
    fn add_appends_to_existing_labels() {
        let labels = combine_labels(
            vec![json!("another label"), json!("test label")],
            &json!({"add": ["production", "deploy"]}),
        )
        .unwrap();
        assert_eq!(
            strings(labels),
            vec!["another label", "test label", "production", "deploy"]
        );
    }

    #[test]
    fn delete_removes_matching_existing_labels() {
        let current = vec![
            json!("another label"),
            json!("test label"),
            json!("delete me"),
            json!("delete this too"),
        ];
        let labels = combine_labels(
            current,
            &json!({"delete": ["delete me", "delete this too"]}),
        )
        .unwrap();
        assert_eq!(strings(labels), vec!["another label", "test label"]);
    }

    #[test]
    fn replace_then_add_then_delete_is_the_order_of_operations() {
        let settings = json!({"delete": ["not present", "more adds"], "add": ["addition", "more adds"], "replace": ["test present", "not present"]});
        let labels = combine_labels(
            json_value::as_array_or_wrap(&settings["replace"]).to_vec(),
            &settings,
        )
        .unwrap();
        assert_eq!(strings(labels), vec!["test present", "addition"]);
    }

    #[test]
    fn settings_can_be_single_values() {
        let settings = json!({"delete": "deletion", "add": "addition", "replace": "replace"});
        let labels = combine_labels(
            json_value::as_array_or_wrap(&settings["replace"]).to_vec(),
            &settings,
        )
        .unwrap();
        assert_eq!(strings(labels), vec!["replace", "addition"]);
    }

    #[test]
    fn delete_mode_uses_glob_patterns() {
        let current = [
            "testLabel",
            "2ndTestLabel",
            "test1",
            "test2",
            "label1",
            "Label2",
            "anotherLabel2",
        ]
        .map(|label| json!(label))
        .to_vec();
        let labels = labels_without_matches(current, &json!(["*Label*", "test1"])).unwrap();
        assert_eq!(strings(labels), vec!["test2", "label1"]);
    }

    #[test]
    fn mode_defaults_to_add_and_rejects_unknown_names() {
        assert_eq!(LabelMode::from_setting(None).unwrap(), LabelMode::Add);
        assert_eq!(
            LabelMode::from_setting(Some(&json!("replace"))).unwrap(),
            LabelMode::Replace
        );
        assert_eq!(
            LabelMode::from_setting(Some(&json!("rename")))
                .unwrap_err()
                .to_string(),
            "UnSupportedSettingError: Unknown mode, supported modes are add, delete, replace"
        );
    }

    #[test]
    fn non_string_patterns_are_rejected() {
        assert_eq!(
            crate::glob::matches_value(&json!("a"), &json!(1), crate::glob::DotEntries::Excluded)
                .unwrap_err()
                .to_string(),
            "TypeError: invalid pattern"
        );
    }
}
