use crate::context::Context;
use crate::error::{Error, Result};
use crate::event_aware;
use crate::github::api;
use crate::glob;
use crate::json_value;
use crate::registry::Registry;
use crate::results::{self, Status};
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::borrow::Cow;

const DEFAULT_SUCCESS_MESSAGE: &str = "Your Contents passes validations";

pub struct Contents;

fn validator_context() -> Value {
    json!({"name": "contents"})
}

fn matches_ignored_patterns(file_name: &Value, patterns_to_ignore: &Value) -> Result<bool> {
    let Value::Array(patterns) = patterns_to_ignore else {
        return Err(Error::type_mismatch(
            "patternsToIgnore.some is not a function",
        ));
    };
    for pattern in patterns {
        if glob::matches_value(file_name, pattern, glob::DotEntries::Excluded)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn changed_file_names(files: Vec<Value>, patterns_to_ignore: &Value) -> Result<Vec<Value>> {
    let mut changed = Vec::new();
    for mut file in files {
        let file_name = json_value::take(Some(&mut file), "filename")?.unwrap_or(Value::Null);
        if matches_ignored_patterns(&file_name, patterns_to_ignore)? {
            continue;
        }
        let status = json_value::as_str(file.get("status"));
        if status == Some("modified") || status == Some("added") {
            changed.push(file_name);
        }
    }
    Ok(changed)
}

#[must_use]
pub fn summarize(changed_files: Vec<Value>, failed_files: &[String], settings: &Value) -> Value {
    let is_mergeable = failed_files.is_empty();
    let description = if is_mergeable {
        DEFAULT_SUCCESS_MESSAGE.to_string()
    } else {
        format!("Failed files : '{}'", failed_files.join(","))
    };
    let output = results::construct_output(
        &validator_context(),
        Value::Array(changed_files),
        settings,
        Status::from_check(is_mergeable),
        Value::String(description),
        None,
    );
    results::consolidate_result(vec![output], &validator_context())
}

#[async_trait]
impl Validator for Contents {
    fn name(&self) -> &'static str {
        "contents"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Value {
        json!({
            "files": {"pr_diff": "boolean", "ignore": "array"},
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "begins_with": {"match": ["string", "array"], "message": "string"},
            "ends_with": {"match": ["string", "array"], "message": "string"},
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let default_patterns_to_ignore = json!([".github/mergeable.yml"]);
        let mut patterns_to_ignore = &default_patterns_to_ignore;
        let mut parse_option = Cow::Borrowed(settings);
        if let Some(file_options) = settings
            .get("files")
            .filter(|value| json_value::is_truthy(value))
        {
            if let Some(ignore) = json_value::read(Some(file_options), "ignore")?
                .filter(|value| json_value::is_truthy(value))
            {
                patterns_to_ignore = ignore;
            }
            if let Value::Object(map) = parse_option.to_mut() {
                map.shift_remove("files");
            }
        }

        let pull_number =
            json_value::read(event_aware::payload(context), "number")?.unwrap_or(&Value::Null);
        let files = api::list_files(
            context,
            context.repo_params(json!({"pull_number": pull_number}))?,
        )
        .await?;
        let changed_files = changed_file_names(files, patterns_to_ignore)?;

        let mut failed_files = Vec::new();
        for file in &changed_files {
            let head_sha =
                json_value::read_path(&context.payload, &["pull_request", "head", "sha"])?
                    .unwrap_or(&Value::Null);
            let params = context.repo_params(json!({"path": file, "ref": head_sha}))?;
            let file_name = json_value::to_display_string(Some(file));
            let Some(file_content) = api::fetch_content(context, params).await? else {
                failed_files.push(format!("{file_name} (Not Found)"));
                continue;
            };
            let processed =
                validators::process_options(&parse_option, &Value::String(file_content), None)
                    .await?;
            match Status::of(&processed) {
                Some(Status::Error) => return Ok(processed),
                Some(Status::Fail) => failed_files.push(file_name),
                Some(Status::Pass) | None => {}
            }
        }

        Ok(summarize(changed_files, &failed_files, &parse_option))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull_request_files(names: &[&str]) -> Vec<Value> {
        names
            .iter()
            .map(|name| json!({"filename": name, "status": "modified"}))
            .collect()
    }

    #[test]
    fn mergeable_config_is_ignored_by_default() {
        let files = pull_request_files(&[".github/mergeable.yml", "Cargo.toml"]);
        let changed = changed_file_names(files, &json!([".github/mergeable.yml"])).unwrap();
        assert_eq!(changed, vec![json!("Cargo.toml")]);
    }

    #[test]
    fn ignore_option_replaces_the_default_patterns() {
        let files = pull_request_files(&["Cargo.toml", "src/a.rs"]);
        let changed = changed_file_names(files, &json!(["Cargo.toml"])).unwrap();
        assert_eq!(changed, vec![json!("src/a.rs")]);
    }

    #[test]
    fn removed_files_are_not_checked() {
        let files = vec![
            json!({"filename": "gone.rs", "status": "removed"}),
            json!({"filename": "new.rs", "status": "added"}),
        ];
        assert_eq!(
            changed_file_names(files, &json!([])).unwrap(),
            vec![json!("new.rs")]
        );
    }

    #[test]
    fn failed_and_missing_files_are_listed_in_the_description() {
        let result = summarize(
            vec![json!("Cargo.toml"), json!("a.rs")],
            &["Cargo.toml (Not Found)".to_string(), "a.rs".to_string()],
            &json!({"do": "contents"}),
        );
        assert_eq!(result["status"], "fail");
        assert_eq!(
            result["validations"][0]["description"],
            "Failed files : 'Cargo.toml (Not Found),a.rs'"
        );
    }

    #[test]
    fn no_failed_files_passes() {
        let result = summarize(vec![json!("Cargo.toml")], &[], &json!({"do": "contents"}));
        assert_eq!(
            result["validations"][0]["description"],
            DEFAULT_SUCCESS_MESSAGE
        );
    }
}
