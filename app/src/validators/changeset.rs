use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::github::api;
use crate::json_value;
use crate::registry::Registry;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::borrow::Cow;

pub struct Changeset;

pub fn files_with_selected_status(
    files: Vec<Value>,
    file_status_settings: &Value,
) -> Result<Vec<Value>> {
    let selected_statuses: Vec<String> = json_value::object_keys(Some(file_status_settings))?
        .into_iter()
        .filter(|status| json_value::truthy(json_value::get(file_status_settings, status)))
        .collect();
    let mut selected = Vec::new();
    for file in files {
        let status = json_value::as_str(json_value::read(Some(&file), "status")?);
        if status.is_some_and(|status| {
            selected_statuses
                .iter()
                .any(|selected_status| selected_status == status)
        }) {
            selected.push(file);
        }
    }
    Ok(selected)
}

pub fn file_names(files: Vec<Value>) -> Result<Vec<Value>> {
    files
        .into_iter()
        .map(|mut file| Ok(json_value::take(Some(&mut file), "filename")?.unwrap_or(Value::Null)))
        .collect()
}

pub async fn evaluate(mut files: Vec<Value>, settings: &Value) -> Result<Value> {
    let mut remaining_settings = Cow::Borrowed(settings);
    if let Some(file_status_settings) = settings
        .get("files")
        .filter(|value| json_value::is_truthy(value))
    {
        files = files_with_selected_status(files, file_status_settings)?;
        if let Value::Object(map) = remaining_settings.to_mut() {
            map.shift_remove("files");
        }
    }
    let changed_files = Value::Array(file_names(files)?);
    validators::process_options(&remaining_settings, &changed_files, None).await
}

#[async_trait]
impl Validator for Changeset {
    fn name(&self) -> &'static str {
        "changeset"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Value {
        json!({
            "no_empty": {"enabled": "boolean", "message": "string"},
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string", "all": "boolean"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string", "all": "boolean"},
            "begins_with": {"match": ["string", "array"], "message": "string"},
            "ends_with": {"match": ["string", "array"], "message": "string"},
            "min": {"count": "number", "message": "string"},
            "max": {"count": "number", "message": "string"},
            "files": {"added": "boolean", "modified": "boolean", "removed": "boolean"},
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let pull_number =
            json_value::read(event_aware::payload(context), "number")?.unwrap_or(&Value::Null);
        let files = api::list_files(
            context,
            context.repo_params(json!({"pull_number": pull_number}))?,
        )
        .await?;
        evaluate(files, settings).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_files_with_enabled_statuses_are_considered() {
        let files = vec![
            json!({"filename": "added-file.rs", "status": "added"}),
            json!({"filename": "modified-file.py", "status": "modified"}),
            json!({"filename": "deleted-file.go", "status": "deleted"}),
        ];
        let selected =
            files_with_selected_status(files, &json!({"added": true, "modified": false})).unwrap();
        assert_eq!(file_names(selected).unwrap(), vec![json!("added-file.rs")]);
    }

    #[test]
    fn no_enabled_status_selects_nothing() {
        let files = vec![json!({"filename": "a.rs", "status": "removed"})];
        let selected = files_with_selected_status(files, &json!({"removed": false})).unwrap();
        assert_eq!(selected, [] as [serde_json::Value; 0]);
    }

    fn modified(names: &[&str]) -> Vec<Value> {
        names
            .iter()
            .map(|name| json!({"filename": name, "status": "modified"}))
            .collect()
    }

    async fn status_of(names: &[&str], settings: Value) -> Value {
        evaluate(modified(names), &settings).await.unwrap()["status"].clone()
    }

    #[tokio::test]
    async fn must_exclude_fails_when_a_changed_file_matches() {
        let settings = json!({"do": "changeset", "must_exclude": {"regex": "b.rs"}});
        assert_eq!(
            status_of(&["Cargo.toml", "a.rs"], settings.clone()).await,
            "pass"
        );
        assert_eq!(status_of(&["Cargo.toml", "b.rs"], settings).await, "fail");
    }

    #[tokio::test]
    async fn must_include_accepts_a_list_of_patterns() {
        let settings =
            json!({"do": "changeset", "must_include": {"regex": ["a.rs", "b.rs", "c.md"]}});
        assert_eq!(
            status_of(&["rust-toolchain.toml", "a.rs"], settings.clone()).await,
            "pass"
        );
        assert_eq!(
            status_of(&["rust-toolchain.toml", "b.rs"], settings.clone()).await,
            "pass"
        );
        assert_eq!(
            status_of(&["rust-toolchain.toml", "Cargo.toml"], settings).await,
            "fail"
        );
    }

    #[tokio::test]
    async fn ends_with_matches_lock_files() {
        let settings = json!({"do": "changeset", "ends_with": {"match": "lock"}});
        assert_eq!(
            status_of(&["Cargo.lock", "rust-toolchain.toml"], settings).await,
            "pass"
        );
    }

    #[tokio::test]
    async fn must_include_with_all_requires_every_file_to_match() {
        let settings =
            json!({"do": "changeset", "must_include": {"regex": "^\\w+\\.md$", "all": true}});
        assert_eq!(
            status_of(&["test.md", "another.md"], settings.clone()).await,
            "pass"
        );
        assert_eq!(status_of(&["test.md", "not_md.rs"], settings).await, "fail");
    }

    #[tokio::test]
    async fn must_exclude_with_all_fails_when_any_file_matches() {
        let settings =
            json!({"do": "changeset", "must_exclude": {"regex": "^\\w+\\.md$", "all": true}});
        assert_eq!(
            status_of(&["test.rs", "another.py"], settings.clone()).await,
            "pass"
        );
        assert_eq!(
            status_of(&["another.py", "test.md"], settings).await,
            "fail"
        );
    }

    #[tokio::test]
    async fn file_status_setting_limits_the_checked_files() {
        let files = vec![
            json!({"filename": "added-file.rs", "status": "added"}),
            json!({"filename": "modified-file.py", "status": "modified"}),
            json!({"filename": "deleted-file.go", "status": "deleted"}),
        ];
        let settings = json!({
            "do": "changeset",
            "files": {"added": true, "modified": false},
            "must_include": {"regex": "added-file.rs"},
            "must_exclude": {"regex": "(modified-file.py)|(deleted-file.go)"},
        });
        assert_eq!(evaluate(files, &settings).await.unwrap()["status"], "pass");
    }
}
