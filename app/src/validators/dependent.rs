use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::github::api;
use crate::glob;
use crate::json_value;
use crate::registry::Registry;
use crate::results;
use crate::validators::Validator;
use crate::value_lists;
use async_trait::async_trait;
use serde_json::{Value, json};

const FILE_NOT_FOUND_ERROR: &str = "Failed to validate because the 'file' sub option for 'changed' option is missing. Please check the documentation";
const FILES_NOT_FOUND_ERROR: &str = "Failed to validate because the 'files' or 'changed' option is missing. Please check the documentation.";
const DEFAULT_SUCCESS_MESSAGE: &str = "All the Dependents files are present!";
const VALIDATOR_NAME: &str = "Dependent";

pub struct Dependent;

fn modified_file_names(files: Vec<Value>) -> Result<Vec<Value>> {
    let mut modified = Vec::new();
    for mut file in files {
        let status = json_value::as_str(json_value::read(Some(&file), "status")?);
        if status == Some("modified") || status == Some("added") {
            modified.push(json_value::take(Some(&mut file), "filename")?.unwrap_or(Value::Null));
        }
    }
    Ok(modified)
}

fn error_result(modified_files: Vec<Value>, settings: &Value, message: &str) -> Value {
    let output = results::construct_error_output(
        &Value::String(VALIDATOR_NAME.to_string()),
        Value::Array(modified_files),
        settings,
        message,
        None,
    );
    results::consolidate_result(vec![output], &json!({"name": VALIDATOR_NAME}))
}

fn any_file_matches(modified_files: &[Value], pattern: &Value) -> Result<bool> {
    for file_name in modified_files {
        if glob::matches_value(file_name, pattern, glob::DotEntries::Excluded)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn join_values(values: &[&Value], separator: &str) -> String {
    values
        .iter()
        .map(|value| match value {
            Value::Null => String::new(),
            other => json_value::to_display_string(Some(other)),
        })
        .collect::<Vec<_>>()
        .join(separator)
}

pub fn evaluate(files: Vec<Value>, settings: &Value) -> Result<Value> {
    let modified_files = modified_file_names(files)?;
    let changed_option = settings
        .get("changed")
        .filter(|value| json_value::is_truthy(value));
    if !json_value::truthy(settings.get("files")) && changed_option.is_none() {
        return Ok(error_result(
            modified_files,
            settings,
            FILES_NOT_FOUND_ERROR,
        ));
    }

    let dependent_files: &[Value];
    let file_difference: Vec<&Value>;
    let is_mergeable: bool;
    if let Some(changed) = changed_option {
        let Some(trigger_pattern) = json_value::read(Some(changed), "file")? else {
            return Ok(error_result(modified_files, settings, FILE_NOT_FOUND_ERROR));
        };
        dependent_files = if any_file_matches(&modified_files, trigger_pattern)? {
            let required = json_value::read(Some(changed), "required")?
                .filter(|value| json_value::is_truthy(value));
            value_lists::array_items(required.or(changed.get("files")))
        } else {
            &[]
        };
        file_difference = value_lists::difference(dependent_files, &modified_files);
        is_mergeable = file_difference.is_empty();
    } else {
        dependent_files = value_lists::array_items(settings.get("files"));
        file_difference = value_lists::difference(dependent_files, &modified_files);
        is_mergeable = file_difference.len() == dependent_files.len() || file_difference.is_empty();
    }

    let description = match settings
        .get("message")
        .filter(|message| json_value::is_truthy(message))
    {
        Some(message) => message.clone(),
        None => Value::String(format!(
            "One or more files ({}) are missing from your pull request because they are dependent on the following: {}",
            join_values(&file_difference, ", "),
            join_values(
                &value_lists::difference(dependent_files, &file_difference),
                ","
            )
        )),
    };
    let output = results::construct_output(
        &Value::String(VALIDATOR_NAME.to_string()),
        Value::Array(modified_files),
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

    fn supported_settings(&self) -> Value {
        json!({
            "files": "array",
            "message": "string",
            "changed": {"file": "string", "required": "array", "files": "array"},
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let pull_number =
            json_value::read(event_aware::get_payload(context), "number")?.unwrap_or(&Value::Null);
        let files = api::list_files(
            context,
            context.repo_params(json!({"pull_number": pull_number}))?,
        )
        .await?;
        evaluate(files, settings)
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

    fn status_of(files: &[Value], settings: &Value) -> Value {
        evaluate(files.to_vec(), settings).unwrap()["status"].clone()
    }

    #[test]
    fn changed_file_present_with_all_dependent_files_passes() {
        let files = modified(&["Cargo.lock", "rust-toolchain.toml", "Cargo.toml", "a.rs"]);
        let settings = json!({"do": "dependent", "changed": {"file": "Cargo.toml", "files": ["Cargo.lock", "rust-toolchain.toml"]}});
        assert_eq!(status_of(&files, &settings), "pass");
        let one_dependent =
            json!({"do": "dependent", "changed": {"file": "Cargo.toml", "files": ["Cargo.lock"]}});
        assert_eq!(status_of(&files, &one_dependent), "pass");
    }

    #[test]
    fn changed_file_present_without_dependent_file_fails() {
        let files = modified(&["Cargo.toml", "a.rs", "b.rs"]);
        let settings =
            json!({"do": "dependent", "changed": {"file": "Cargo.toml", "files": ["Cargo.lock"]}});
        assert_eq!(status_of(&files, &settings), "fail");
    }

    #[test]
    fn required_sub_option_is_an_alias_for_files() {
        let settings = json!({"do": "dependent", "changed": {"file": "Cargo.toml", "required": ["Cargo.lock"]}});
        assert_eq!(status_of(&[], &settings), "pass");
    }

    #[test]
    fn missing_file_sub_option_is_an_error() {
        let settings = json!({"do": "dependent", "changed": {"required": ["Cargo.lock"]}});
        let result = evaluate(Vec::new(), &settings).unwrap();
        assert_eq!(result["status"], "error");
        assert_eq!(
            result["validations"][0]["description"],
            FILE_NOT_FOUND_ERROR
        );
        assert_eq!(result["validations"][0]["validator"], "Dependent");
    }

    #[test]
    fn missing_files_and_changed_options_is_an_error() {
        let result = evaluate(Vec::new(), &json!({"do": "dependent"})).unwrap();
        assert_eq!(
            result["validations"][0]["description"],
            FILES_NOT_FOUND_ERROR
        );
    }

    #[test]
    fn glob_in_changed_file_matches_nested_files() {
        let settings =
            json!({"do": "dependent", "changed": {"file": "**/*.rs", "files": ["Cargo.lock"]}});
        assert_eq!(status_of(&modified(&["a.rs"]), &settings), "fail");
        assert_eq!(status_of(&modified(&["test/test.rs"]), &settings), "fail");
        assert_eq!(
            status_of(&modified(&["test/test.rs", "Cargo.lock"]), &settings),
            "pass"
        );
    }

    #[test]
    fn untouched_dependent_files_pass() {
        assert_eq!(
            status_of(&[], &json!({"do": "dependent", "files": ["a.rs", "b.go"]})),
            "pass"
        );
    }

    #[test]
    fn all_dependent_files_modified_or_added_pass() {
        let files = vec![
            json!({"filename": "Cargo.toml", "status": "added"}),
            json!({"filename": "rust-toolchain.toml", "status": "modified"}),
        ];
        assert_eq!(
            status_of(
                &files,
                &json!({"do": "dependent", "files": ["Cargo.toml", "rust-toolchain.toml"]})
            ),
            "pass"
        );
    }

    #[test]
    fn only_some_dependent_files_modified_fails_with_default_description() {
        let result = evaluate(
            modified(&["Cargo.toml"]),
            &json!({"do": "dependent", "files": ["Cargo.toml", "rust-toolchain.toml"]}),
        )
        .unwrap();
        assert_eq!(result["status"], "fail");
        assert_eq!(
            result["validations"][0]["description"],
            "One or more files (rust-toolchain.toml) are missing from your pull request because they are dependent on the following: Cargo.toml"
        );
    }

    #[test]
    fn custom_message_replaces_the_default_description() {
        let settings = json!({"do": "dependent", "files": ["Cargo.toml", "rust-toolchain.toml"], "message": "Test Message"});
        let result = evaluate(modified(&["Cargo.toml"]), &settings).unwrap();
        assert_eq!(result["validations"][0]["description"], "Test Message");
    }
}
