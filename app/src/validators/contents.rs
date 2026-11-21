use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::event_aware;
use crate::github::api;
use crate::js;
use crate::minimatch;
use crate::registry::Registry;
use crate::results;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};

const DEFAULT_SUCCESS_MESSAGE: &str = "Your Contents passes validations";

pub struct Contents;

fn validator_context() -> Value {
    json!({"name": "contents"})
}

fn matches_ignored_patterns(file_name: &Value, patterns_to_ignore: &Value) -> JsResult<bool> {
    let patterns = match patterns_to_ignore {
        Value::Array(patterns) => patterns.clone(),
        _ => {
            return Err(JsError::type_error(
                "patternsToIgnore.some is not a function",
            ));
        }
    };
    for pattern in &patterns {
        if minimatch::minimatch(file_name, pattern, false)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn changed_file_names(files: &[Value], patterns_to_ignore: &Value) -> JsResult<Vec<Value>> {
    let mut changed = Vec::new();
    for file in files {
        let file_name = js::read(Some(file), "filename")?
            .cloned()
            .unwrap_or(Value::Null);
        if matches_ignored_patterns(&file_name, patterns_to_ignore)? {
            continue;
        }
        let status = js::as_str(file.get("status"));
        if status == Some("modified") || status == Some("added") {
            changed.push(file_name);
        }
    }
    Ok(changed)
}

pub fn summarize(changed_files: Vec<Value>, failed_files: &[String], settings: &Value) -> Value {
    let is_mergeable = failed_files.is_empty();
    let description = if is_mergeable {
        DEFAULT_SUCCESS_MESSAGE.to_string()
    } else {
        format!("Failed files : '{}'", failed_files.join(","))
    };
    let output = results::construct_output(
        &validator_context(),
        &Value::Array(changed_files),
        settings,
        if is_mergeable { "pass" } else { "fail" },
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

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "files": {"pr_diff": "boolean", "ignore": "array"},
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "begins_with": {"match": ["string", "array"], "message": "string"},
            "ends_with": {"match": ["string", "array"], "message": "string"},
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let mut patterns_to_ignore = json!([".github/mergeable.yml"]);
        let mut parse_option = settings.clone();
        if let Some(file_options) = settings.get("files").filter(|value| js::is_truthy(value)) {
            if let Some(ignore) =
                js::read(Some(file_options), "ignore")?.filter(|value| js::is_truthy(value))
            {
                patterns_to_ignore = ignore.clone();
            }
            if let Value::Object(map) = &mut parse_option {
                map.shift_remove("files");
            }
        }

        let pull_number = js::read(event_aware::get_payload(context), "number")?
            .cloned()
            .unwrap_or(Value::Null);
        let files = api::list_files(
            context,
            context.repo_params(json!({"pull_number": pull_number}))?,
        )
        .await?;
        let changed_files = changed_file_names(&files, &patterns_to_ignore)?;

        let mut failed_files = Vec::new();
        for file in &changed_files {
            let head_sha = js::read_path(&context.payload, &["pull_request", "head", "sha"])?
                .cloned()
                .unwrap_or(Value::Null);
            let params = context.repo_params(json!({"path": file, "ref": head_sha}))?;
            let file_name = js::to_js_string(Some(file));
            let Some(content) = api::get_content(context, params).await? else {
                failed_files.push(format!("{file_name} (Not Found)"));
                continue;
            };
            let processed =
                validators::process_options(&parse_option, &Value::String(content), None).await?;
            match processed.get("status").and_then(Value::as_str) {
                Some("error") => return Ok(processed),
                Some("fail") => failed_files.push(file_name),
                _ => {}
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
        let files = pull_request_files(&[".github/mergeable.yml", "package.json"]);
        let changed = changed_file_names(&files, &json!([".github/mergeable.yml"])).unwrap();
        assert_eq!(changed, vec![json!("package.json")]);
    }

    #[test]
    fn ignore_option_replaces_the_default_patterns() {
        let files = pull_request_files(&["package.json", "src/a.js"]);
        let changed = changed_file_names(&files, &json!(["package.json"])).unwrap();
        assert_eq!(changed, vec![json!("src/a.js")]);
    }

    #[test]
    fn removed_files_are_not_checked() {
        let files = vec![
            json!({"filename": "gone.js", "status": "removed"}),
            json!({"filename": "new.js", "status": "added"}),
        ];
        assert_eq!(
            changed_file_names(&files, &json!([])).unwrap(),
            vec![json!("new.js")]
        );
    }

    #[test]
    fn failed_and_missing_files_are_listed_in_the_description() {
        let result = summarize(
            vec![json!("package.json"), json!("a.js")],
            &["package.json (Not Found)".to_string(), "a.js".to_string()],
            &json!({"do": "contents"}),
        );
        assert_eq!(result["status"], "fail");
        assert_eq!(
            result["validations"][0]["description"],
            "Failed files : 'package.json (Not Found),a.js'"
        );
    }

    #[test]
    fn no_failed_files_passes() {
        let result = summarize(vec![json!("package.json")], &[], &json!({"do": "contents"}));
        assert_eq!(
            result["validations"][0]["description"],
            DEFAULT_SUCCESS_MESSAGE
        );
    }
}
