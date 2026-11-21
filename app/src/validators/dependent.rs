use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::github::api;
use crate::js;
use crate::lodash;
use crate::minimatch;
use crate::registry::Registry;
use crate::results;
use crate::validators::Validator;
use async_trait::async_trait;
use serde_json::{Value, json};

const FILE_NOT_FOUND_ERROR: &str = "Failed to validate because the 'file' sub option for 'changed' option is missing. Please check the documentation";
const FILES_NOT_FOUND_ERROR: &str = "Failed to validate because the 'files' or 'changed' option is missing. Please check the documentation.";
const DEFAULT_SUCCESS_MESSAGE: &str = "All the Dependents files are present!";
const VALIDATOR_NAME: &str = "Dependent";

pub struct Dependent;

fn modified_file_names(files: &[Value]) -> JsResult<Vec<Value>> {
    let mut modified = Vec::new();
    for file in files {
        let status = js::read(Some(file), "status")?;
        if js::as_str(status) == Some("modified") || js::as_str(status) == Some("added") {
            modified.push(
                js::read(Some(file), "filename")?
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
    }
    Ok(modified)
}

fn error_result(modified_files: Vec<Value>, settings: &Value, message: &str) -> Value {
    let output = results::construct_error_output(
        &Value::String(VALIDATOR_NAME.to_string()),
        &Value::Array(modified_files),
        settings,
        message,
        None,
    );
    results::consolidate_result(vec![output], &json!({"name": VALIDATOR_NAME}))
}

fn any_file_matches(modified_files: &[Value], pattern: &Value) -> JsResult<bool> {
    for file_name in modified_files {
        if minimatch::minimatch(file_name, pattern, false)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn join_values(values: &[Value], separator: &str) -> String {
    values
        .iter()
        .map(|value| match value {
            Value::Null => String::new(),
            other => js::to_js_string(Some(other)),
        })
        .collect::<Vec<_>>()
        .join(separator)
}

pub fn evaluate(files: &[Value], settings: &Value) -> JsResult<Value> {
    let modified_files = modified_file_names(files)?;
    let changed_option = settings.get("changed").filter(|value| js::is_truthy(value));
    if !js::truthy(settings.get("files")) && changed_option.is_none() {
        return Ok(error_result(
            modified_files,
            settings,
            FILES_NOT_FOUND_ERROR,
        ));
    }

    let dependent_files: Vec<Value>;
    let file_difference: Vec<Value>;
    let is_mergeable: bool;
    if let Some(changed) = changed_option {
        let Some(trigger_pattern) = js::read(Some(changed), "file")? else {
            return Ok(error_result(modified_files, settings, FILE_NOT_FOUND_ERROR));
        };
        dependent_files = if any_file_matches(&modified_files, trigger_pattern)? {
            let required =
                js::read(Some(changed), "required")?.filter(|value| js::is_truthy(value));
            lodash::array_like(required.or(changed.get("files")))
        } else {
            Vec::new()
        };
        file_difference = lodash::difference(&dependent_files, &modified_files);
        is_mergeable = file_difference.is_empty();
    } else {
        dependent_files = lodash::array_like(settings.get("files"));
        file_difference = lodash::difference(&dependent_files, &modified_files);
        is_mergeable = file_difference.len() == dependent_files.len() || file_difference.is_empty();
    }

    let description = match settings
        .get("message")
        .filter(|message| js::is_truthy(message))
    {
        Some(message) => message.clone(),
        None => Value::String(format!(
            "One or more files ({}) are missing from your pull request because they are dependent on the following: {}",
            join_values(&file_difference, ", "),
            join_values(&lodash::difference(&dependent_files, &file_difference), ",")
        )),
    };
    let output = results::construct_output(
        &Value::String(VALIDATOR_NAME.to_string()),
        &Value::Array(modified_files),
        settings,
        if is_mergeable { "pass" } else { "fail" },
        if is_mergeable {
            Value::String(DEFAULT_SUCCESS_MESSAGE.to_string())
        } else {
            description
        },
        None,
    );
    Ok(results::consolidate_result(
        vec![output],
        &json!({"name": VALIDATOR_NAME}),
    ))
}

#[async_trait]
impl Validator for Dependent {
    fn name(&self) -> &'static str {
        "dependent"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "files": "array",
            "message": "string",
            "changed": {"file": "string", "required": "array", "files": "array"},
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
        evaluate(&files, settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modified(names: &[&str]) -> Vec<Value> {
        names
            .iter()
            .map(|name| json!({"filename": name, "status": "modified"}))
            .collect()
    }

    fn status_of(files: &[Value], settings: Value) -> Value {
        evaluate(files, &settings).unwrap()["status"].clone()
    }

    #[test]
    fn changed_file_present_with_all_dependent_files_passes() {
        let files = modified(&["package-lock.json", "yarn.lock", "package.json", "a.js"]);
        let settings = json!({"do": "dependent", "changed": {"file": "package.json", "files": ["package-lock.json", "yarn.lock"]}});
        assert_eq!(status_of(&files, settings), "pass");
        let one_dependent = json!({"do": "dependent", "changed": {"file": "package.json", "files": ["package-lock.json"]}});
        assert_eq!(status_of(&files, one_dependent), "pass");
    }

    #[test]
    fn changed_file_present_without_dependent_file_fails() {
        let files = modified(&["package.json", "a.js", "b.js"]);
        let settings = json!({"do": "dependent", "changed": {"file": "package.json", "files": ["package-lock.json"]}});
        assert_eq!(status_of(&files, settings), "fail");
    }

    #[test]
    fn required_sub_option_is_an_alias_for_files() {
        let settings = json!({"do": "dependent", "changed": {"file": "package.json", "required": ["package-lock.json"]}});
        assert_eq!(status_of(&[], settings), "pass");
    }

    #[test]
    fn missing_file_sub_option_is_an_error() {
        let settings = json!({"do": "dependent", "changed": {"required": ["package-lock.json"]}});
        let result = evaluate(&[], &settings).unwrap();
        assert_eq!(result["status"], "error");
        assert_eq!(
            result["validations"][0]["description"],
            FILE_NOT_FOUND_ERROR
        );
        assert_eq!(result["validations"][0]["validator"], "Dependent");
    }

    #[test]
    fn missing_files_and_changed_options_is_an_error() {
        let result = evaluate(&[], &json!({"do": "dependent"})).unwrap();
        assert_eq!(
            result["validations"][0]["description"],
            FILES_NOT_FOUND_ERROR
        );
    }

    #[test]
    fn glob_in_changed_file_matches_nested_files() {
        let settings = json!({"do": "dependent", "changed": {"file": "**/*.js", "files": ["package-lock.json"]}});
        assert_eq!(status_of(&modified(&["a.js"]), settings.clone()), "fail");
        assert_eq!(
            status_of(&modified(&["test/test.js"]), settings.clone()),
            "fail"
        );
        assert_eq!(
            status_of(&modified(&["test/test.js", "package-lock.json"]), settings),
            "pass"
        );
    }

    #[test]
    fn untouched_dependent_files_pass() {
        assert_eq!(
            status_of(&[], json!({"do": "dependent", "files": ["a.js", "b.go"]})),
            "pass"
        );
    }

    #[test]
    fn all_dependent_files_modified_or_added_pass() {
        let files = vec![
            json!({"filename": "package.json", "status": "added"}),
            json!({"filename": "yarn.lock", "status": "modified"}),
        ];
        assert_eq!(
            status_of(
                &files,
                json!({"do": "dependent", "files": ["package.json", "yarn.lock"]})
            ),
            "pass"
        );
    }

    #[test]
    fn only_some_dependent_files_modified_fails_with_default_description() {
        let result = evaluate(
            &modified(&["package.json"]),
            &json!({"do": "dependent", "files": ["package.json", "yarn.lock"]}),
        )
        .unwrap();
        assert_eq!(result["status"], "fail");
        assert_eq!(
            result["validations"][0]["description"],
            "One or more files (yarn.lock) are missing from your pull request because they are dependent on the following: package.json"
        );
    }

    #[test]
    fn custom_message_replaces_the_default_description() {
        let settings = json!({"do": "dependent", "files": ["package.json", "yarn.lock"], "message": "Test Message"});
        let result = evaluate(&modified(&["package.json"]), &settings).unwrap();
        assert_eq!(result["validations"][0]["description"], "Test Message");
    }
}
