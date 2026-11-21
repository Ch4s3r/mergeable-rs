use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::event_aware;
use crate::github::api;
use crate::js;
use crate::lodash;
use crate::registry::Registry;
use crate::results;
use crate::validators::Validator;
use crate::validators::options::must_include;
use async_trait::async_trait;
use serde_json::{Map, Value, json};

const MESSAGE_NOT_FOUND_ERROR: &str = "Failed to run the 'commit' validator because 'message' option is not found. Please check README for more information about configuration";
const REGEX_NOT_FOUND_ERROR: &str = "Failed to run the test because 'regex' is not provided for 'message' option. Please check README for more information about configuration";
const DEFAULT_FAIL_MESSAGE: &str = "Some or all of your commit messages doesn't meet the criteria";
const DEFAULT_FAIL_AUTHOR_EMAIL_MESSAGE: &str =
    "Some or all of your commit author emails doesn't meet the criteria";
const DEFAULT_FAIL_COMMITTER_EMAIL_MESSAGE: &str =
    "Some or all of your commit committer emails doesn't meet the criteria";
const DEFAULT_SUCCESS_MESSAGE: &str = "Your commit messages met the specified criteria";

pub struct Commit;

pub struct CommitSelection {
    pub inputs: Vec<Value>,
    pub fail_message: &'static str,
}

pub enum CommitPlan {
    SkippedForMultipleCommits(Vec<Value>),
    Check(CommitSelection),
}

fn validator_context() -> Value {
    json!({"name": "commit"})
}

fn option_or(settings: &Value, key: &str, default: Value) -> Value {
    settings.get(key).cloned().unwrap_or(default)
}

fn includes_merge_branch(message: Option<&Value>) -> JsResult<bool> {
    match message {
        Some(Value::String(text)) => Ok(text.contains("Merge branch")),
        Some(Value::Array(items)) => Ok(items
            .iter()
            .any(|item| item.as_str() == Some("Merge branch"))),
        None => Err(JsError::reading_property_of_undefined("includes")),
        Some(Value::Null) => Err(JsError::type_error(
            "Cannot read properties of null (reading 'includes')",
        )),
        Some(_) => Err(JsError::type_error(
            "commit.message.includes is not a function",
        )),
    }
}

fn read_each(commits: &[Option<Value>], path: &[&str]) -> JsResult<Vec<Value>> {
    commits
        .iter()
        .map(|commit| {
            let mut current = commit.as_ref();
            for key in path {
                current = js::read(current, key)?;
            }
            Ok(current.cloned().unwrap_or(Value::Null))
        })
        .collect()
}

pub fn select_commits(commits: Vec<Value>, message_settings: &Value) -> JsResult<CommitPlan> {
    let oldest_only = option_or(message_settings, "oldest_only", Value::Bool(false));
    let newest_only = option_or(message_settings, "newest_only", Value::Bool(false));
    let skip_merge = option_or(message_settings, "skip_merge", Value::Bool(true));
    let single_commit_only = option_or(message_settings, "single_commit_only", Value::Bool(false));
    let message_type = option_or(
        message_settings,
        "message_type",
        Value::String(String::new()),
    );

    let mut ordered = lodash::order_by_path(commits, &["date"], false);

    if js::is_truthy(&single_commit_only) && ordered.len() != 1 {
        let messages = ordered
            .iter()
            .map(|commit| {
                Ok(js::read(Some(commit), "message")?
                    .cloned()
                    .unwrap_or(Value::Null))
            })
            .collect::<JsResult<Vec<Value>>>()?;
        return Ok(CommitPlan::SkippedForMultipleCommits(messages));
    }

    if js::is_truthy(&skip_merge) {
        let mut kept = Vec::new();
        for commit in ordered {
            if !includes_merge_branch(js::read(Some(&commit), "message")?)? {
                kept.push(commit);
            }
        }
        ordered = kept;
    }

    let mut selected: Vec<Option<Value>> = ordered.into_iter().map(Some).collect();
    if js::is_truthy(&oldest_only) {
        selected = vec![selected.first().cloned().flatten()];
    }
    if js::is_truthy(&newest_only) {
        selected = vec![selected.last().cloned().flatten()];
    }

    let mut inputs = read_each(&selected, &["message"])?;
    let mut fail_message = DEFAULT_FAIL_MESSAGE;
    if message_type.as_str() == Some("author_email") {
        inputs = read_each(&selected, &["author", "email"])?;
        fail_message = DEFAULT_FAIL_AUTHOR_EMAIL_MESSAGE;
    } else if message_type.as_str() == Some("committer_email") {
        inputs = read_each(&selected, &["committer", "email"])?;
        fail_message = DEFAULT_FAIL_COMMITTER_EMAIL_MESSAGE;
    }
    Ok(CommitPlan::Check(CommitSelection {
        inputs,
        fail_message,
    }))
}

pub fn must_include_rule(message_settings: &Value, fail_message: &str) -> Value {
    let mut rule = Map::new();
    rule.insert("all".to_string(), Value::Bool(true));
    if let Some(regex) = message_settings.get("regex") {
        rule.insert("regex".to_string(), regex.clone());
    }
    if let Some(regex_flag) = message_settings.get("regex_flag") {
        rule.insert("regex_flag".to_string(), regex_flag.clone());
    }
    let message = match message_settings
        .get("message")
        .filter(|message| js::is_truthy(message))
    {
        Some(custom) => custom.clone(),
        None => Value::String(fail_message.to_string()),
    };
    rule.insert("message".to_string(), message);
    json!({"must_include": Value::Object(rule)})
}

fn output(input: Vec<Value>, settings: &Value, status: Value, description: Value) -> Value {
    let mut result = Map::new();
    result.insert("validator".to_string(), validator_context());
    result.insert("status".to_string(), status);
    result.insert("description".to_string(), description);
    result.insert(
        "details".to_string(),
        json!({"input": input, "settings": settings}),
    );
    results::consolidate_result(vec![Value::Object(result)], &validator_context())
}

pub fn check_settings(settings: &Value) -> JsResult<&Value> {
    let message_settings = settings
        .get("message")
        .ok_or_else(|| JsError::error(MESSAGE_NOT_FOUND_ERROR))?;
    if js::read(Some(message_settings), "regex")?.is_none() {
        return Err(JsError::error(REGEX_NOT_FOUND_ERROR));
    }
    Ok(message_settings)
}

#[async_trait]
impl Validator for Commit {
    fn name(&self) -> &'static str {
        "commit"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "jira": {"regex": "string", "regex_flag": "string", "message": "string"},
            "message": {
                "regex": "string",
                "regex_flag": "string",
                "message": "string",
                "message_type": "string",
                "skip_merge": "boolean",
                "oldest_only": "boolean",
                "newest_only": "boolean",
                "single_commit_only": "boolean",
            },
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let message_settings = check_settings(settings)?;
        let pull_number = js::read(event_aware::get_payload(context), "number")?
            .cloned()
            .unwrap_or(Value::Null);
        let commits = api::list_commits(context, &pull_number).await?;
        evaluate(commits, settings, message_settings)
    }
}

pub fn evaluate(
    commits: Vec<Value>,
    settings: &Value,
    message_settings: &Value,
) -> JsResult<Value> {
    let selection = match select_commits(commits, message_settings)? {
        CommitPlan::SkippedForMultipleCommits(messages) => {
            return Ok(output(
                messages,
                settings,
                Value::String("pass".to_string()),
                Value::String(
                    "Since there are more than one commits, Skipping validation".to_string(),
                ),
            ));
        }
        CommitPlan::Check(selection) => selection,
    };
    let rule = must_include_rule(message_settings, selection.fail_message);
    let result = must_include::process(
        &validator_context(),
        &Value::Array(selection.inputs.clone()),
        &rule,
    )?;
    let description = if result.status == "pass" {
        Value::String(DEFAULT_SUCCESS_MESSAGE.to_string())
    } else {
        result.description
    };
    Ok(output(
        selection.inputs,
        settings,
        Value::String(result.status.to_string()),
        description,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(date: &str, message: &str, author_email: &str, committer_email: &str) -> Value {
        json!({
            "message": message,
            "date": date,
            "author": {"date": date, "email": author_email},
            "committer": {"date": date, "email": committer_email},
        })
    }

    fn checked_inputs(plan: CommitPlan) -> Vec<Value> {
        match plan {
            CommitPlan::Check(selection) => selection.inputs,
            CommitPlan::SkippedForMultipleCommits(_) => {
                panic!("expected the commits to be checked")
            }
        }
    }

    #[test]
    fn commits_are_checked_oldest_first() {
        let commits = vec![
            commit("2024-01-02T00:00:00Z", "feat: newer", "a@x", "c@x"),
            commit("2024-01-01T00:00:00Z", "fix: older", "a@x", "c@x"),
        ];
        let inputs = checked_inputs(select_commits(commits, &json!({"regex": "feat:"})).unwrap());
        assert_eq!(inputs, vec![json!("fix: older"), json!("feat: newer")]);
    }

    #[test]
    fn oldest_only_keeps_only_the_first_commit() {
        let commits = vec![
            commit("2024-01-01T00:00:00Z", "fix: this", "a@x", "c@x"),
            commit("2024-01-02T00:00:00Z", "fix: that", "a@x", "c@x"),
        ];
        let inputs = checked_inputs(
            select_commits(commits, &json!({"regex": "feat:", "oldest_only": true})).unwrap(),
        );
        assert_eq!(inputs, vec![json!("fix: this")]);
    }

    #[test]
    fn newest_only_keeps_only_the_last_commit() {
        let commits = vec![
            commit("2024-01-01T00:00:00Z", "fix: that", "a@x", "c@x"),
            commit("2024-01-02T00:00:00Z", "feat: this", "a@x", "c@x"),
        ];
        let inputs = checked_inputs(
            select_commits(commits, &json!({"regex": "feat:", "newest_only": true})).unwrap(),
        );
        assert_eq!(inputs, vec![json!("feat: this")]);
    }

    #[test]
    fn merge_commits_are_skipped_by_default_but_kept_when_disabled() {
        let commits = vec![
            commit("2024-01-01T00:00:00Z", "feat: this", "a@x", "c@x"),
            commit("2024-01-02T00:00:00Z", "Merge branch", "a@x", "c@x"),
        ];
        let default_inputs =
            checked_inputs(select_commits(commits.clone(), &json!({"regex": "feat:"})).unwrap());
        assert_eq!(default_inputs, vec![json!("feat: this")]);
        let kept_inputs = checked_inputs(
            select_commits(commits, &json!({"regex": "feat:", "skip_merge": false})).unwrap(),
        );
        assert_eq!(kept_inputs.len(), 2);
    }

    #[test]
    fn single_commit_only_skips_pull_requests_with_several_commits() {
        let commits = vec![
            commit("2024-01-01T00:00:00Z", "fix: this", "a@x", "c@x"),
            commit("2024-01-02T00:00:00Z", "test", "a@x", "c@x"),
        ];
        let plan = select_commits(
            commits,
            &json!({"regex": "feat:", "single_commit_only": true}),
        )
        .unwrap();
        assert!(
            matches!(plan, CommitPlan::SkippedForMultipleCommits(messages) if messages.len() == 2)
        );
    }

    #[test]
    fn author_and_committer_email_message_types_check_emails() {
        let commits = vec![commit(
            "2024-01-01T00:00:00Z",
            "feat: that",
            "support1@github.com",
            "valdis1@github1.com",
        )];
        let author = select_commits(
            commits.clone(),
            &json!({"regex": "x", "message_type": "author_email"}),
        )
        .unwrap();
        let CommitPlan::Check(author_selection) = author else {
            panic!("expected check")
        };
        assert_eq!(author_selection.inputs, vec![json!("support1@github.com")]);
        assert_eq!(
            author_selection.fail_message,
            DEFAULT_FAIL_AUTHOR_EMAIL_MESSAGE
        );
        let committer = select_commits(
            commits,
            &json!({"regex": "x", "message_type": "committer_email"}),
        )
        .unwrap();
        let CommitPlan::Check(committer_selection) = committer else {
            panic!("expected check")
        };
        assert_eq!(
            committer_selection.inputs,
            vec![json!("valdis1@github1.com")]
        );
    }

    #[test]
    fn oldest_only_on_an_empty_list_throws_like_node() {
        let error = select_commits(Vec::new(), &json!({"regex": "x", "oldest_only": true}))
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of undefined (reading 'message')"
        );
    }

    #[test]
    fn missing_message_or_regex_options_throw_documented_errors() {
        assert_eq!(
            check_settings(&json!({"do": "commit"}))
                .unwrap_err()
                .message,
            MESSAGE_NOT_FOUND_ERROR
        );
        assert_eq!(
            check_settings(&json!({"message": {}})).unwrap_err().message,
            REGEX_NOT_FOUND_ERROR
        );
    }

    #[test]
    fn must_include_rule_requires_all_and_falls_back_to_default_message() {
        let rule = must_include_rule(&json!({"regex": "feat:"}), DEFAULT_FAIL_MESSAGE);
        assert_eq!(
            rule,
            json!({"must_include": {"all": true, "regex": "feat:", "message": DEFAULT_FAIL_MESSAGE}})
        );
    }

    fn run(commits: Vec<Value>, settings: Value) -> Value {
        let message_settings = check_settings(&settings).unwrap().clone();
        evaluate(commits, &settings, &message_settings).unwrap()
    }

    #[test]
    fn every_commit_message_must_match() {
        let settings = json!({"do": "commit", "message": {"regex": "feat:"}});
        let mixed = vec![
            commit("2024-01-01T00:00:00Z", "fix: this", "a@x", "c@x"),
            commit("2024-01-02T00:00:00Z", "feat: that", "a@x", "c@x"),
        ];
        assert_eq!(run(mixed, settings.clone())["status"], "fail");
        let all_features = vec![
            commit("2024-01-01T00:00:00Z", "feat: this", "a@x", "c@x"),
            commit("2024-01-02T00:00:00Z", "feat: that", "a@x", "c@x"),
        ];
        let result = run(all_features, settings);
        assert_eq!(result["status"], "pass");
        assert_eq!(
            result["validations"][0]["description"],
            DEFAULT_SUCCESS_MESSAGE
        );
    }

    #[test]
    fn single_commit_only_reports_the_skip_or_the_failure() {
        let settings =
            json!({"do": "commit", "message": {"regex": "feat:", "single_commit_only": true}});
        let two = vec![
            commit("2024-01-01T00:00:00Z", "fix: this", "a@x", "c@x"),
            commit("2024-01-02T00:00:00Z", "test", "a@x", "c@x"),
        ];
        let skipped = run(two, settings.clone());
        assert_eq!(
            skipped["validations"][0]["description"],
            "Since there are more than one commits, Skipping validation"
        );
        let one = vec![commit("2024-01-01T00:00:00Z", "fix: this", "a@x", "c@x")];
        let failed = run(one, settings);
        assert_eq!(
            failed["validations"][0]["description"],
            DEFAULT_FAIL_MESSAGE
        );
    }

    #[test]
    fn author_email_regex_is_checked_against_author_emails() {
        let settings = json!({"do": "commit", "message": {"regex": "^[A-Za-z0-9._%+-]+@github.com$", "message_type": "author_email"}});
        let matching = vec![commit(
            "2024-01-01T00:00:00Z",
            "fix",
            "support@github.com",
            "valdis@github1.com",
        )];
        assert_eq!(run(matching, settings.clone())["status"], "pass");
        let other_domain = vec![commit(
            "2024-01-01T00:00:00Z",
            "fix",
            "support1@github1.com",
            "valdis@github.com",
        )];
        let result = run(other_domain, settings);
        assert_eq!(result["status"], "fail");
        assert_eq!(
            result["validations"][0]["description"],
            DEFAULT_FAIL_AUTHOR_EMAIL_MESSAGE
        );
    }
}
