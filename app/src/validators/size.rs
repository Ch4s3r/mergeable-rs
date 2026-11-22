use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::event_aware;
use crate::github::api;
use crate::js;
use crate::lodash;
use crate::minimatch;
use crate::registry::Registry;
use crate::results;
use crate::validators::Validator;
use async_trait::async_trait;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

const ERROR_MESSAGE: &str = "Failed to validate because the 'lines' or 'max / total', 'additions' or 'deletions' option is missing. Please check the documentation.";
const ERROR_MAX_TOTAL: &str = "Options max and total cannot be used together. Please choose one";
const VALIDATOR_NAME: &str = "Size";
const EXTENSIONS_WITH_IGNORABLE_COMMENTS: [&str; 2] = [".js", ".py"];

static ADD_OR_DELETE_LINE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^(\+|-)").ok());
static FILE_MODIFIER_LINE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(\+\+\+|---)").ok());
static FILE_EXTENSION: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?i)\.[0-9a-z]+$").ok());

pub struct Size;

pub struct ChangedFile {
    pub filename: Value,
    pub additions: Option<Value>,
    pub deletions: Option<Value>,
    pub changes: Option<Value>,
}

fn regex_test(pattern: &LazyLock<Option<Regex>>, text: &str) -> bool {
    pattern
        .as_ref()
        .is_some_and(|compiled| compiled.is_match(text))
}

fn is_single_line_comment(extension: &str, line_content: &str) -> bool {
    match extension {
        ".js" => line_content.starts_with("//"),
        ".py" => line_content.starts_with('#'),
        _ => true,
    }
}

fn has_block_comments(extension: &str) -> bool {
    extension == ".js"
}

fn utf16_units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn utf16_index_of(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    let start = from.min(haystack.len());
    if needle.is_empty() {
        return Some(start);
    }
    haystack
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| offset + start)
}

fn javascript_substring(units: &[u16], start: i64, end: i64) -> String {
    let length = units.len() as i64;
    let clamped_start = start.clamp(0, length) as usize;
    let clamped_end = end.clamp(0, length) as usize;
    let (from, to) = if clamped_start > clamped_end {
        (clamped_end, clamped_start)
    } else {
        (clamped_start, clamped_end)
    };
    String::from_utf16_lossy(&units[from..to])
}

pub fn extract_file_name(file_diff: &str) -> String {
    let units = utf16_units(file_diff);
    let start_index =
        utf16_index_of(&units, &utf16_units("+++ b/"), 0).map_or(-1, |index| index as i64);
    let end_index = utf16_index_of(&units, &[u16::from(b'\n')], start_index.max(0) as usize)
        .map_or(-1, |index| index as i64);
    javascript_substring(&units, start_index + 6, end_index)
}

pub fn extract_file_extension(filename: &str) -> Option<String> {
    FILE_EXTENSION
        .as_ref()
        .and_then(|compiled| compiled.find(filename))
        .map(|found| found.as_str().to_string())
}

pub fn changes_without_comments(diffs: &str) -> Vec<ChangedFile> {
    let mut processed_files = Vec::new();
    for file_diff in diffs.split("diff --git").skip(1) {
        let filename = extract_file_name(file_diff);
        let extension = extract_file_extension(&filename);
        if file_diff.contains("deleted") {
            continue;
        }
        let extension_text = extension.as_deref().unwrap_or_default();
        let ignores_comments = EXTENSIONS_WITH_IGNORABLE_COMMENTS.contains(&extension_text);
        let mut additions = 0u64;
        let mut deletions = 0u64;
        let mut is_block_comment_active = false;
        for line in file_diff.split('\n') {
            if !regex_test(&ADD_OR_DELETE_LINE, line) || regex_test(&FILE_MODIFIER_LINE, line) {
                continue;
            }
            if ignores_comments {
                let line_content = lodash::js_trim(line.get(1..).unwrap_or_default());
                if is_single_line_comment(extension_text, line_content) {
                    continue;
                }
                if has_block_comments(extension_text) {
                    let starts_block = line_content.contains("/*");
                    let ends_block = line_content.contains("*/");
                    if is_block_comment_active {
                        if ends_block {
                            is_block_comment_active = false;
                        }
                        continue;
                    } else if starts_block {
                        if !ends_block {
                            is_block_comment_active = true;
                        }
                        continue;
                    }
                }
            }
            if line.starts_with('+') {
                additions += 1;
            } else {
                deletions += 1;
            }
        }
        processed_files.push(ChangedFile {
            filename: Value::String(filename),
            additions: Some(json!(additions)),
            deletions: Some(json!(deletions)),
            changes: Some(json!(additions + deletions)),
        });
    }
    processed_files
}

fn matches_patterns(file_name: &Value, patterns: &Value) -> JsResult<bool> {
    let Value::Array(patterns) = patterns else {
        return Err(JsError::type_error("patterns.some is not a function"));
    };
    for pattern in patterns {
        if minimatch::minimatch(file_name, pattern, true)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn select_listed_files(
    files: &[Value],
    patterns_to_include: &Value,
    patterns_to_ignore: &Value,
) -> JsResult<Vec<ChangedFile>> {
    let mut selected = Vec::new();
    for file in files {
        let file_name = js::read(Some(file), "filename")?
            .cloned()
            .unwrap_or(Value::Null);
        if !matches_patterns(&file_name, patterns_to_include)?
            || matches_patterns(&file_name, patterns_to_ignore)?
        {
            continue;
        }
        let status = js::as_str(file.get("status"));
        if status != Some("modified") && status != Some("added") {
            continue;
        }
        selected.push(ChangedFile {
            filename: file_name,
            additions: file.get("additions").cloned(),
            deletions: file.get("deletions").cloned(),
            changes: file.get("changes").cloned(),
        });
    }
    Ok(selected)
}

fn javascript_add(total: f64, addition: Option<&Value>) -> f64 {
    total + lodash::to_number(addition)
}

pub struct SizeChanges {
    pub additions: f64,
    pub deletions: f64,
    pub total: f64,
}

pub fn sum_changes(files: &[ChangedFile]) -> SizeChanges {
    let mut changes = SizeChanges {
        additions: 0.0,
        deletions: 0.0,
        total: 0.0,
    };
    for file in files {
        changes.additions = javascript_add(changes.additions, file.additions.as_ref());
        changes.deletions = javascript_add(changes.deletions, file.deletions.as_ref());
        changes.total = javascript_add(changes.total, file.changes.as_ref());
    }
    changes
}

pub fn input_message(changes: &SizeChanges, files: &[ChangedFile]) -> String {
    let file_names: Vec<String> = files
        .iter()
        .map(|file| match &file.filename {
            Value::Null => String::new(),
            other => js::to_js_string(Some(other)),
        })
        .collect();
    format!(
        "{} total, {} additions, {} deletions\n    Files: {}",
        js::float_to_string(changes.total),
        js::float_to_string(changes.additions),
        js::float_to_string(changes.deletions),
        file_names.join(",")
    )
}

fn error_result(input: &Value, settings: &Value, message: &str) -> Value {
    let output = results::construct_error_output(
        &Value::String(VALIDATOR_NAME.to_string()),
        input,
        settings,
        message,
        None,
    );
    results::consolidate_result(vec![output], &json!({"name": VALIDATOR_NAME}))
}

fn change_count(changes: &SizeChanges, key: &str) -> Option<f64> {
    match key {
        "additions" => Some(changes.additions),
        "deletions" => Some(changes.deletions),
        "total" => Some(changes.total),
        _ => None,
    }
}

pub fn evaluate(changes: &SizeChanges, files: &[ChangedFile], settings: &Value) -> JsResult<Value> {
    let input = Value::String(input_message(changes, files));
    let Some(lines) = settings.get("lines").filter(|value| js::is_truthy(value)) else {
        return Ok(error_result(&input, settings, ERROR_MESSAGE));
    };

    let mut size_limits = Map::new();
    for key in ["additions", "deletions", "total", "max"] {
        if let Some(limit) = js::get(lines, key) {
            size_limits.insert(key.to_string(), limit.clone());
        }
    }
    if size_limits.contains_key("max") && size_limits.contains_key("total") {
        return Ok(error_result(&input, settings, ERROR_MAX_TOTAL));
    }
    if let Some(max) = size_limits
        .get("max")
        .filter(|max| js::is_truthy(max))
        .cloned()
    {
        size_limits.insert("total".to_string(), max);
        size_limits.shift_remove("max");
    }

    let validator_context = json!({"name": VALIDATOR_NAME});
    let mut output = Vec::new();
    for (key, limit) in &size_limits {
        let count = js::read(Some(limit), "count")?;
        let current = change_count(changes, key).unwrap_or(f64::NAN);
        let is_mergeable = current <= lodash::to_number(count);
        let label = if key == "total" {
            "total additions + deletions"
        } else {
            key.as_str()
        };
        let description = if is_mergeable {
            Value::String(format!("PR size for {label} is OK!"))
        } else {
            match js::read(Some(limit), "message")?.filter(|message| js::is_truthy(message)) {
                Some(message) => message.clone(),
                None => Value::String(format!(
                    "PR is too large. Should be under {} {label}",
                    js::to_js_string(count)
                )),
            }
        };
        output.push(results::construct_output(
            &validator_context,
            &input,
            settings,
            if is_mergeable { "pass" } else { "fail" },
            description,
            None,
        ));
    }

    if output.is_empty() {
        return Ok(error_result(&input, settings, ERROR_MESSAGE));
    }
    Ok(results::consolidate_result(output, &validator_context))
}

async fn fetch_diff(url: &Value) -> JsResult<String> {
    let response = reqwest::get(js::to_js_string(Some(url)))
        .await
        .map_err(|error| JsError::named("FetchError", error.to_string()))?;
    response
        .text()
        .await
        .map_err(|error| JsError::named("FetchError", error.to_string()))
}

#[async_trait]
impl Validator for Size {
    fn name(&self) -> &'static str {
        "size"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "match": "array",
            "ignore": "array",
            "lines": {
                "max": {"count": "number", "message": "string"},
                "total": {"count": "number", "message": "string"},
                "additions": {"count": "number", "message": "string"},
                "deletions": {"count": "number", "message": "string"},
                "ignore_comments": "boolean",
            },
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let patterns_to_include = settings
            .get("match")
            .filter(|value| js::is_truthy(value))
            .cloned()
            .unwrap_or_else(|| json!(["**"]));
        let patterns_to_ignore = settings
            .get("ignore")
            .filter(|value| js::is_truthy(value))
            .cloned()
            .unwrap_or_else(|| json!([]));
        let payload = event_aware::get_payload(context);
        let pull_number = js::read(payload, "number")?.cloned().unwrap_or(Value::Null);
        let listed_files = api::list_files(
            context,
            context.repo_params(json!({"pull_number": pull_number}))?,
        )
        .await?;

        let ignore_comments = settings
            .get("lines")
            .filter(|value| js::is_truthy(value))
            .is_some_and(|lines| js::truthy(js::get(lines, "ignore_comments")));
        let modified_files = if ignore_comments {
            let diff_url = js::read(event_aware::get_payload(context), "diff_url")?
                .cloned()
                .unwrap_or(Value::Null);
            changes_without_comments(&fetch_diff(&diff_url).await?)
        } else {
            select_listed_files(&listed_files, &patterns_to_include, &patterns_to_ignore)?
        };

        evaluate(&sum_changes(&modified_files), &modified_files, settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull_request_files() -> Vec<Value> {
        vec![
            json!({"filename": "thing.js", "status": "modified", "additions": 10, "deletions": 5, "changes": 15}),
            json!({"filename": "another.js", "status": "added", "additions": 3, "deletions": 2, "changes": 5}),
            json!({"filename": "removed_file_should_be_ignored.js", "status": "removed", "additions": 0, "deletions": 500, "changes": 500}),
        ]
    }

    fn validate_listed(files: &[Value], settings: Value) -> Value {
        let patterns_to_include = settings
            .get("match")
            .cloned()
            .unwrap_or_else(|| json!(["**"]));
        let patterns_to_ignore = settings.get("ignore").cloned().unwrap_or_else(|| json!([]));
        let selected =
            select_listed_files(files, &patterns_to_include, &patterns_to_ignore).unwrap();
        evaluate(&sum_changes(&selected), &selected, &settings).unwrap()
    }

    fn validate_diff(diff: &str, settings: Value) -> Value {
        let files = changes_without_comments(diff);
        evaluate(&sum_changes(&files), &files, &settings).unwrap()
    }

    fn limits(additions: u64, deletions: u64, total: u64) -> Value {
        json!({
            "additions": {"count": additions, "message": "Too big!"},
            "deletions": {"count": deletions, "message": "Too big!"},
            "total": {"count": total, "message": "Too big!"},
        })
    }

    fn statuses_and_descriptions(result: &Value) -> Vec<(String, String)> {
        result["validations"]
            .as_array()
            .map(|validations| {
                validations
                    .iter()
                    .map(|validation| {
                        (
                            js::to_js_string(validation.get("status")),
                            js::to_js_string(validation.get("description")),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn max_and_total_together_is_an_error() {
        let settings = json!({"do": "size", "lines": {"max": {"count": 10, "message": "Too big!"}, "total": {"count": 10, "message": "Too big!"}}});
        let result = validate_listed(&pull_request_files(), settings);
        assert_eq!(result["status"], "error");
        assert_eq!(result["validations"][0]["description"], ERROR_MAX_TOTAL);
    }

    #[test]
    fn max_is_a_backwards_compatible_alias_for_total() {
        let settings =
            json!({"do": "size", "lines": {"max": {"count": 10, "message": "Too big!"}}});
        let result = validate_listed(&pull_request_files(), settings);
        assert_eq!(result["status"], "fail");
        assert_eq!(result["validations"][0]["description"], "Too big!");
    }

    #[test]
    fn missing_lines_option_is_an_error() {
        let result = validate_listed(&pull_request_files(), json!({"do": "size"}));
        assert_eq!(result["status"], "error");
        assert_eq!(result["validations"][0]["description"], ERROR_MESSAGE);
        assert_eq!(result["validations"][0]["validator"], "Size");
    }

    #[test]
    fn changes_above_every_count_fail_each_limit() {
        let result = validate_listed(
            &pull_request_files(),
            json!({"do": "size", "lines": limits(10, 5, 10)}),
        );
        assert_eq!(result["status"], "fail");
        assert!(
            statuses_and_descriptions(&result)
                .iter()
                .all(|(status, description)| status == "fail" && description == "Too big!")
        );
    }

    #[test]
    fn changes_equal_to_the_counts_pass_with_ok_descriptions() {
        let result = validate_listed(
            &pull_request_files(),
            json!({"do": "size", "lines": limits(13, 7, 20)}),
        );
        assert_eq!(result["status"], "pass");
        assert_eq!(
            statuses_and_descriptions(&result),
            vec![
                (
                    "pass".to_string(),
                    "PR size for additions is OK!".to_string()
                ),
                (
                    "pass".to_string(),
                    "PR size for deletions is OK!".to_string()
                ),
                (
                    "pass".to_string(),
                    "PR size for total additions + deletions is OK!".to_string()
                ),
            ]
        );
    }

    #[test]
    fn one_limit_exceeded_fails_while_others_pass() {
        let result = validate_listed(
            &pull_request_files(),
            json!({"do": "size", "lines": limits(13, 5, 20)}),
        );
        assert_eq!(result["status"], "fail");
        assert_eq!(result["validations"][1]["description"], "Too big!");
        assert_eq!(result["validations"][0]["status"], "pass");
    }

    #[test]
    fn ignored_files_are_not_counted() {
        let settings = json!({"do": "size", "lines": limits(10, 5, 15), "ignore": ["another.js"]});
        assert_eq!(
            validate_listed(&pull_request_files(), settings)["status"],
            "pass"
        );
    }

    #[test]
    fn hidden_files_are_matched_by_the_default_pattern() {
        let files = vec![
            json!({"filename": ".test/thing.js", "status": "modified", "additions": 20, "deletions": 5, "changes": 15}),
        ];
        let settings =
            json!({"do": "size", "lines": {"additions": {"count": 10, "message": "Too big!"}}});
        assert_eq!(validate_listed(&files, settings)["status"], "fail");
    }

    #[test]
    fn ignore_and_match_accept_glob_patterns() {
        let files = vec![
            json!({"filename": "not_too_big.js", "status": "modified", "additions": 10, "deletions": 0, "changes": 10}),
            json!({"filename": "nested/one/big_file_1.js", "status": "added", "additions": 30, "deletions": 0, "changes": 30}),
            json!({"filename": "nested/one/big_file_2.js", "status": "added", "additions": 30, "deletions": 0, "changes": 30}),
            json!({"filename": "nested/two/three/big_file_3.js", "status": "modified", "additions": 30, "deletions": 0, "changes": 30}),
        ];
        let by_directory = json!({"do": "size", "lines": limits(10, 5, 15), "ignore": ["nested/one/*", "nested/two/**"]});
        assert_eq!(validate_listed(&files, by_directory)["status"], "pass");
        let by_name =
            json!({"do": "size", "lines": limits(10, 5, 15), "ignore": ["**/big_file_*.js"]});
        assert_eq!(validate_listed(&files, by_name)["status"], "pass");
        let by_match =
            json!({"do": "size", "lines": limits(10, 5, 15), "match": ["not_*.js"], "ignore": []});
        assert_eq!(validate_listed(&files, by_match)["status"], "pass");
    }

    #[test]
    fn input_message_lists_totals_and_file_names() {
        let selected =
            select_listed_files(&pull_request_files(), &json!(["**"]), &json!([])).unwrap();
        assert_eq!(
            input_message(&sum_changes(&selected), &selected),
            "20 total, 13 additions, 7 deletions\n    Files: thing.js,another.js"
        );
    }

    #[test]
    fn single_line_and_block_comments_are_not_counted() {
        let settings = json!({"do": "size", "lines": {"total": {"count": 1, "message": "Too big!"}, "ignore_comments": true}});
        let with_code = "diff --git a/commentTestFiles.js b/commentTestFiles.js\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.js\n@@ -0,0 +1,5 @@\n+// this is a comment\n+ // second comment\n+ non comment\n+\n+/*\n+    Comment block\n+ */\n";
        assert_eq!(validate_diff(with_code, settings.clone())["status"], "fail");
        let comments_only = "diff --git a/commentTestFiles.js b/commentTestFiles.js\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.js\n@@ -0,0 +1,5 @@\n+// this is a comment\n+/*\n+    Comment block\n+ */\n";
        let result = validate_diff(comments_only, settings);
        assert_eq!(result["status"], "pass");
        assert_eq!(
            result["validations"][0]["description"],
            "PR size for total additions + deletions is OK!"
        );
    }

    #[test]
    fn comments_in_unsupported_file_types_are_counted() {
        let settings = json!({"do": "size", "lines": {"total": {"count": 1, "message": "Too big!"}, "ignore_comments": true}});
        let diff = "diff --git a/commentTestFiles.notsupportedExtension b/commentTestFiles.notsupportedExtension\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.notsupportedExtension\n@@ -0,0 +1,5 @@\n+// this is a comment\n+/*\n+    Comment block\n+ */\n";
        assert_eq!(validate_diff(diff, settings)["status"], "fail");
    }

    #[test]
    fn python_hash_comments_are_not_counted() {
        let settings = json!({"do": "size", "lines": {"total": {"count": 0, "message": "Too big!"}, "ignore_comments": true}});
        let javascript_named = "diff --git a/commentTestFiles.py b/commentTestFiles.py\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.js\n@@ -0,0 +1,5 @@\n+# this is a comment\n+ # second comment\n+ non comment\n";
        assert_eq!(
            validate_diff(javascript_named, settings.clone())["status"],
            "fail"
        );
        let python = "diff --git a/commentTestFiles.py b/commentTestFiles.py\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.py\n@@ -0,0 +1,5 @@\n+# this is a comment\n+ # second comment\n";
        assert_eq!(validate_diff(python, settings)["status"], "pass");
    }

    #[test]
    fn file_name_is_read_from_the_new_file_header() {
        assert_eq!(
            extract_file_name(" a/x.js b/x.js\n--- a/x.js\n+++ b/src/x.js\n@@"),
            "src/x.js"
        );
        assert_eq!(extract_file_extension("src/x.JS"), Some(".JS".to_string()));
        assert_eq!(extract_file_extension("Makefile"), None);
    }
}
