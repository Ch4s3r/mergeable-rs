use crate::context::Context;
use crate::error::{Error, Result};
use crate::event_aware;
use crate::github::api;
use crate::json_value;
use crate::registry::Registry;
use crate::results;
use crate::validators::Validator;
use crate::validators::options::lib::OptionStatus;
use crate::validators::options::must_include;
use crate::value_lists;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckedCommitField {
    Message,
    AuthorEmail,
    CommitterEmail,
}

impl CheckedCommitField {
    fn from_message_type(message_type: Option<&str>) -> Self {
        match message_type {
            Some("author_email") => CheckedCommitField::AuthorEmail,
            Some("committer_email") => CheckedCommitField::CommitterEmail,
            _ => CheckedCommitField::Message,
        }
    }

    fn path(self) -> &'static [&'static str] {
        match self {
            CheckedCommitField::Message => &["message"],
            CheckedCommitField::AuthorEmail => &["author", "email"],
            CheckedCommitField::CommitterEmail => &["committer", "email"],
        }
    }

    fn fail_message(self) -> &'static str {
        match self {
            CheckedCommitField::Message => DEFAULT_FAIL_MESSAGE,
            CheckedCommitField::AuthorEmail => DEFAULT_FAIL_AUTHOR_EMAIL_MESSAGE,
            CheckedCommitField::CommitterEmail => DEFAULT_FAIL_COMMITTER_EMAIL_MESSAGE,
        }
    }
}

pub enum CommitPlan {
    SkippedForMultipleCommits(Vec<Value>),
    Check(CommitSelection),
}

fn validator_context() -> Value {
    json!({"name": "commit"})
}

fn is_option_enabled(settings: &Value, key: &str, default: bool) -> bool {
    settings.get(key).map_or(default, json_value::is_truthy)
}

fn includes_merge_branch(message: Option<&Value>) -> Result<bool> {
    match message {
        Some(Value::String(text)) => Ok(text.contains("Merge branch")),
        Some(Value::Array(items)) => Ok(items
            .iter()
            .any(|item| item.as_str() == Some("Merge branch"))),
        None => Err(Error::missing_property("includes")),
        Some(Value::Null) => Err(Error::type_mismatch(
            "Cannot read properties of null (reading 'includes')",
        )),
        Some(_) => Err(Error::type_mismatch(
            "commit.message.includes is not a function",
        )),
    }
}

fn take_each(commits: &mut [Option<Value>], path: &[&str]) -> Result<Vec<Value>> {
    commits
        .iter_mut()
        .map(|commit| {
            let mut current = commit.as_ref();
            for key in path {
                current = json_value::read(current, key)?;
            }
            let mut target = commit.as_mut();
            for key in path {
                target = target.and_then(|present| json_value::get_mut(present, key));
            }
            Ok(target.map_or(Value::Null, Value::take))
        })
        .collect()
}

pub fn select_commits(commits: Vec<Value>, message_settings: &Value) -> Result<CommitPlan> {
    let oldest_only = is_option_enabled(message_settings, "oldest_only", false);
    let newest_only = is_option_enabled(message_settings, "newest_only", false);
    let skip_merge = is_option_enabled(message_settings, "skip_merge", true);
    let single_commit_only = is_option_enabled(message_settings, "single_commit_only", false);
    let checked_field = CheckedCommitField::from_message_type(json_value::as_str(
        message_settings.get("message_type"),
    ));

    let mut ordered =
        value_lists::order_by_path(commits, &["date"], value_lists::SortOrder::Ascending);

    if single_commit_only && ordered.len() != 1 {
        let messages = ordered
            .into_iter()
            .map(|mut commit| {
                Ok(json_value::take(Some(&mut commit), "message")?.unwrap_or(Value::Null))
            })
            .collect::<Result<Vec<Value>>>()?;
        return Ok(CommitPlan::SkippedForMultipleCommits(messages));
    }

    if skip_merge {
        let mut kept = Vec::new();
        for commit in ordered {
            if !includes_merge_branch(json_value::read(Some(&commit), "message")?)? {
                kept.push(commit);
            }
        }
        ordered = kept;
    }

    let mut selected: Vec<Option<Value>> = ordered.into_iter().map(Some).collect();
    if oldest_only {
        selected = vec![selected.into_iter().next().flatten()];
    }
    if newest_only {
        selected = vec![selected.into_iter().next_back().flatten()];
    }

    let messages = take_each(&mut selected, CheckedCommitField::Message.path())?;
    let inputs = match checked_field {
        CheckedCommitField::Message => messages,
        email_field => take_each(&mut selected, email_field.path())?,
    };
    Ok(CommitPlan::Check(CommitSelection {
        inputs,
        fail_message: checked_field.fail_message(),
    }))
}

#[must_use]
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
        .filter(|message| json_value::is_truthy(message))
    {
        Some(custom) => custom.clone(),
        None => Value::String(fail_message.to_string()),
    };
    rule.insert("message".to_string(), message);
    json!({"must_include": Value::Object(rule)})
}

fn output(input: Value, settings: &Value, status: Value, description: Value) -> Value {
    let mut details = Map::new();
    details.insert("input".to_string(), input);
    details.insert("settings".to_string(), settings.clone());
    let mut result = Map::new();
    result.insert("validator".to_string(), validator_context());
    result.insert("status".to_string(), status);
    result.insert("description".to_string(), description);
    result.insert("details".to_string(), Value::Object(details));
    results::consolidate_result(vec![Value::Object(result)], &validator_context())
}

pub fn check_settings(settings: &Value) -> Result<&Value> {
    let message_settings = settings
        .get("message")
        .ok_or_else(|| Error::general(MESSAGE_NOT_FOUND_ERROR))?;
    if json_value::read(Some(message_settings), "regex")?.is_none() {
        return Err(Error::general(REGEX_NOT_FOUND_ERROR));
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

    fn supported_settings(&self) -> Value {
        json!({
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
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let message_settings = check_settings(settings)?;
        let pull_number =
            json_value::read(event_aware::get_payload(context), "number")?.unwrap_or(&Value::Null);
        let commits = api::list_commits(context, pull_number).await?;
        evaluate(commits, settings, message_settings)
    }
}

pub fn evaluate(commits: Vec<Value>, settings: &Value, message_settings: &Value) -> Result<Value> {
    let selection = match select_commits(commits, message_settings)? {
        CommitPlan::SkippedForMultipleCommits(messages) => {
            return Ok(output(
                Value::Array(messages),
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
    let inputs = Value::Array(selection.inputs);
    let result = must_include::process(&validator_context(), &inputs, &rule)?;
    let description = if result.status == OptionStatus::Pass {
        Value::String(DEFAULT_SUCCESS_MESSAGE.to_string())
    } else {
        result.description
    };
    Ok(output(
        inputs,
        settings,
        Value::String(result.status.as_str().to_string()),
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
    fn oldest_only_on_an_empty_list_fails_with_a_missing_property_error() {
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
                .message(),
            MESSAGE_NOT_FOUND_ERROR
        );
        assert_eq!(
            check_settings(&json!({"message": {}}))
                .unwrap_err()
                .message(),
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

    fn run(commits: Vec<Value>, settings: &Value) -> Value {
        let message_settings = check_settings(settings).unwrap().clone();
        evaluate(commits, settings, &message_settings).unwrap()
    }

    #[test]
    fn every_commit_message_must_match() {
        let settings = json!({"do": "commit", "message": {"regex": "feat:"}});
        let mixed = vec![
            commit("2024-01-01T00:00:00Z", "fix: this", "a@x", "c@x"),
            commit("2024-01-02T00:00:00Z", "feat: that", "a@x", "c@x"),
        ];
        assert_eq!(run(mixed, &settings)["status"], "fail");
        let all_features = vec![
            commit("2024-01-01T00:00:00Z", "feat: this", "a@x", "c@x"),
            commit("2024-01-02T00:00:00Z", "feat: that", "a@x", "c@x"),
        ];
        let result = run(all_features, &settings);
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
        let skipped = run(two, &settings);
        assert_eq!(
            skipped["validations"][0]["description"],
            "Since there are more than one commits, Skipping validation"
        );
        let one = vec![commit("2024-01-01T00:00:00Z", "fix: this", "a@x", "c@x")];
        let failed = run(one, &settings);
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
        assert_eq!(run(matching, &settings)["status"], "pass");
        let other_domain = vec![commit(
            "2024-01-01T00:00:00Z",
            "fix",
            "support1@github1.com",
            "valdis@github.com",
        )];
        let result = run(other_domain, &settings);
        assert_eq!(result["status"], "fail");
        assert_eq!(
            result["validations"][0]["description"],
            DEFAULT_FAIL_AUTHOR_EMAIL_MESSAGE
        );
    }
}
