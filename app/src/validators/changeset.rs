use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::github::api;
use crate::js;
use crate::registry::Registry;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Changeset;

pub fn files_with_selected_status(
    files: Vec<Value>,
    file_status_settings: &Value,
) -> JsResult<Vec<Value>> {
    let selected_statuses: Vec<String> = js::object_keys(Some(file_status_settings))?
        .into_iter()
        .filter(|status| js::truthy(js::get(file_status_settings, status)))
        .collect();
    let mut selected = Vec::new();
    for file in files {
        let status = js::as_str(js::read(Some(&file), "status")?);
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

pub fn file_names(files: &[Value]) -> JsResult<Vec<Value>> {
    files
        .iter()
        .map(|file| {
            Ok(js::read(Some(file), "filename")?
                .cloned()
                .unwrap_or(Value::Null))
        })
        .collect()
}

pub async fn evaluate(mut files: Vec<Value>, settings: &Value) -> JsResult<Value> {
    let mut remaining_settings = settings.clone();
    if let Some(file_status_settings) = settings.get("files").filter(|value| js::is_truthy(value)) {
        files = files_with_selected_status(files, file_status_settings)?;
        if let Value::Object(map) = &mut remaining_settings {
            map.shift_remove("files");
        }
    }
    let changed_files = Value::Array(file_names(&files)?);
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

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "no_empty": {"enabled": "boolean", "message": "string"},
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string", "all": "boolean"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string", "all": "boolean"},
            "begins_with": {"match": ["string", "array"], "message": "string"},
            "ends_with": {"match": ["string", "array"], "message": "string"},
            "min": {"count": "number", "message": "string"},
            "max": {"count": "number", "message": "string"},
            "files": {"added": "boolean", "modified": "boolean", "removed": "boolean"},
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let pull_number = js::read(event_aware::get_payload(context), "number")?
            .cloned()
            .unwrap_or(Value::Null);
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
            json!({"filename": "added-file.js", "status": "added"}),
            json!({"filename": "modified-file.py", "status": "modified"}),
            json!({"filename": "deleted-file.ts", "status": "deleted"}),
        ];
        let selected =
            files_with_selected_status(files, &json!({"added": true, "modified": false})).unwrap();
        assert_eq!(file_names(&selected).unwrap(), vec![json!("added-file.js")]);
    }

    #[test]
    fn no_enabled_status_selects_nothing() {
        let files = vec![json!({"filename": "a.js", "status": "removed"})];
        let selected = files_with_selected_status(files, &json!({"removed": false})).unwrap();
        assert!(selected.is_empty());
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
        let settings = json!({"do": "changeset", "must_exclude": {"regex": "b.js"}});
        assert_eq!(
            status_of(&["package.json", "a.js"], settings.clone()).await,
            "pass"
        );
        assert_eq!(status_of(&["package.json", "b.js"], settings).await, "fail");
    }

    #[tokio::test]
    async fn must_include_accepts_a_list_of_patterns() {
        let settings =
            json!({"do": "changeset", "must_include": {"regex": ["a.js", "b.js", "c.md"]}});
        assert_eq!(
            status_of(&["yarn.lock", "a.js"], settings.clone()).await,
            "pass"
        );
        assert_eq!(
            status_of(&["yarn.lock", "b.js"], settings.clone()).await,
            "pass"
        );
        assert_eq!(
            status_of(&["yarn.lock", "package.json"], settings).await,
            "fail"
        );
    }

    #[tokio::test]
    async fn ends_with_matches_lock_files() {
        let settings = json!({"do": "changeset", "ends_with": {"match": "lock"}});
        assert_eq!(
            status_of(&["package-lock.json", "yarn.lock"], settings).await,
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
        assert_eq!(status_of(&["test.md", "not_md.js"], settings).await, "fail");
    }

    #[tokio::test]
    async fn must_exclude_with_all_fails_when_any_file_matches() {
        let settings =
            json!({"do": "changeset", "must_exclude": {"regex": "^\\w+\\.md$", "all": true}});
        assert_eq!(
            status_of(&["test.js", "another.py"], settings.clone()).await,
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
            json!({"filename": "added-file.js", "status": "added"}),
            json!({"filename": "modified-file.py", "status": "modified"}),
            json!({"filename": "deleted-file.ts", "status": "deleted"}),
        ];
        let settings = json!({
            "do": "changeset",
            "files": {"added": true, "modified": false},
            "must_include": {"regex": "added-file.js"},
            "must_exclude": {"regex": "(modified-file.py)|(deleted-file.ts)"},
        });
        assert_eq!(evaluate(files, &settings).await.unwrap()["status"], "pass");
    }
}
