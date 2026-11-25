use crate::context::Context;
use crate::error::{Error, Result};
use crate::github::api;
use crate::json_value;
use crate::repository_config;
use crate::results::Status;
use crate::yaml;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;
use tracing::Instrument;

mod v1_config;

pub const FILE_NAME: &str = ".github/mergeable.yml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    BadYml,
    MissingMergeableNode,
    UnknownVersion,
    GithubApiError,
    NoYml,
    MissingRuleSets,
    NonArrayMergeable,
    MissingWhenKeyword,
    MissingValidateKeyword,
    NonArrayValidate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct Configuration {
    pub errors: Vec<ConfigurationError>,
    pub settings: Value,
}

enum FetchError {
    NotFound,
    Yaml(String),
    Other(Error),
}

impl From<Error> for FetchError {
    fn from(error: Error) -> Self {
        FetchError::Other(error)
    }
}

impl Configuration {
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    #[must_use]
    pub fn has_error(&self, code: ErrorCode) -> bool {
        self.errors.iter().any(|error| error.code == code)
    }

    fn set_error(&mut self, code: ErrorCode, message: String) {
        match self.errors.iter_mut().find(|error| error.code == code) {
            Some(existing) => existing.message = message,
            None => self.errors.push(ConfigurationError { code, message }),
        }
    }

    pub fn from_settings(settings: Value) -> Result<Configuration> {
        let mut configuration = Configuration {
            errors: Vec::new(),
            settings,
        };
        configuration.validate()?;
        if configuration.has_errors() {
            return Ok(configuration);
        }
        match config_version(&configuration.settings).as_str() {
            "1" => {
                let transformed = v1_config::transform(&mut configuration.settings)?;
                configuration.settings =
                    transformed.get("mergeable").cloned().unwrap_or(Value::Null);
                Ok(configuration)
            }
            "2" => {
                set_pull_request_defaults(&mut configuration.settings)?;
                configuration.settings = configuration
                    .settings
                    .get_mut("mergeable")
                    .map_or(Value::Null, Value::take);
                Ok(configuration)
            }
            other => Err(Error::general(format!(
                "Cannot find module './transformers/v{other}Config'"
            ))),
        }
    }

    fn validate(&mut self) -> Result<()> {
        for (code, message) in validation_errors(&self.settings)? {
            self.set_error(code, message.to_string());
        }
        Ok(())
    }
}

fn validation_errors(settings: &Value) -> Result<Vec<(ErrorCode, &'static str)>> {
    let mut errors = Vec::new();
    if json_value::truthy(settings.get("version"))
        && !settings.get("version").is_some_and(Value::is_number)
    {
        errors.push((ErrorCode::UnknownVersion, "Invalid `version` found."));
    }
    match settings.get("mergeable") {
        None => {
            errors.push((
                ErrorCode::MissingMergeableNode,
                "The `mergeable` node is missing.",
            ));
            return Ok(errors);
        }
        Some(Value::Null) => {
            errors.push((
                ErrorCode::MissingRuleSets,
                "`mergeable` node does not contain any rule sets",
            ));
            return Ok(errors);
        }
        Some(_) => {}
    }
    if config_version(settings) == "2" {
        let Some(Value::Array(rule_sets)) = settings.get("mergeable") else {
            errors.push((
                ErrorCode::NonArrayMergeable,
                "`mergeable` must be an array for version 2 config",
            ));
            return Ok(errors);
        };
        for rule_set in rule_sets {
            if json_value::read(Some(rule_set), "when")?.is_none() {
                errors.push((
                    ErrorCode::MissingWhenKeyword,
                    "One or more rule set is missing `when` keyword",
                ));
            }
            match json_value::read(Some(rule_set), "validate")? {
                None => {
                    errors.push((
                        ErrorCode::MissingValidateKeyword,
                        "One or more rule set is missing `validate` keyword",
                    ));
                }
                Some(Value::Array(_)) => {}
                Some(_) => {
                    errors.push((
                        ErrorCode::NonArrayValidate,
                        "`validate` must be an array of rules",
                    ));
                }
            }
        }
    }
    Ok(errors)
}

fn config_version(settings: &Value) -> String {
    match settings.get("version") {
        Some(version) if json_value::is_truthy(version) => {
            json_value::to_display_string(Some(version))
        }
        _ => "1".to_string(),
    }
}

static DEFAULT_PASS_ACTIONS: LazyLock<Value> =
    LazyLock::new(|| default_pull_request_actions(Status::Pass));
static DEFAULT_FAIL_ACTIONS: LazyLock<Value> =
    LazyLock::new(|| default_pull_request_actions(Status::Fail));
static DEFAULT_ERROR_ACTIONS: LazyLock<Value> =
    LazyLock::new(|| default_pull_request_actions(Status::Error));

fn default_actions_for(outcome: Status) -> &'static Value {
    match outcome {
        Status::Pass => &DEFAULT_PASS_ACTIONS,
        Status::Fail => &DEFAULT_FAIL_ACTIONS,
        Status::Error => &DEFAULT_ERROR_ACTIONS,
    }
}

fn default_pull_request_actions(outcome: Status) -> Value {
    match outcome {
        Status::Pass => json!([{
            "do": "checks",
            "state": "completed",
            "status": "success",
            "payload": {
                "title": "Mergeable Run has been Completed!",
                "summary": "All the validators have returned 'pass'! \n Here are some stats of the run: \n {{validationCount}} validations were run"
            }
        }]),
        Status::Fail => json!([{
            "do": "checks",
            "state": "completed",
            "status": "failure",
            "payload": {
                "title": "{{failCount}}/{{validationCount}} Fail(s): {{#each failures}} {{toUpperCase name}}{{^@last}}, {{/@last}}{{/each}}",
                "summary": "### Status: {{toUpperCase validationStatus}}\n\n        Here are some stats of the run:\n        {{validationCount}} validations were run.\n        {{passCount}} PASSED\n        {{failCount}} FAILED\n      ",
                "text": "{{#each validationSuites}}\n#### {{{statusIcon status}}} Validator: {{toUpperCase name}}\n{{#each validations }} * {{{statusIcon status}}} ***{{{ description }}}***\n       Input : {{{details.input}}}\n       Settings : {{{displaySettings details.settings}}}\n       {{/each}}\n{{/each}}"
            }
        }]),
        Status::Error => json!([{
            "do": "checks",
            "state": "completed",
            "status": "action_required",
            "payload": {
                "title": "Mergeable found some failed checks!",
                "summary": "### Status: {{toUpperCase validationStatus}}\n      Some or All of the validators have returned 'error' status, please check below for details\n      \n      Here are some stats of the run: \n      {{validationCount}} validations were run. \n      {{passCount}} ***PASSED***\n      {{failCount}} ***FAILED***\n      {{errorCount}} ***ERRORED***",
                "text": "{{#each validationSuites}}\n#### {{{statusIcon status}}} Validator: {{toUpperCase name}}\nStatus {{toUpperCase status}}\n{{#each validations }} * {{{statusIcon status}}} ***{{{ description }}}***\n       Input : {{{details.input}}}\n       Settings : {{{displaySettings details.settings}}}\n       {{#if details.error}}\n       Error : {{{details.error}}}\n       {{/if}}\n       {{/each}}\n{{/each}}"
            }
        }]),
    }
}

fn with_default_checks(outcome_actions: Option<Value>, default_actions: &Value) -> Result<Value> {
    let Some(actions) = outcome_actions else {
        return Ok(default_actions.clone());
    };
    let Value::Array(mut items) = actions else {
        return Err(Error::type_mismatch("ruleSet.map is not a function"));
    };
    let default_rule = &default_actions[0];
    for rule in &mut items {
        if json_value::get(rule, "do").and_then(Value::as_str) != Some("checks") {
            continue;
        }
        let Value::Object(fields) = rule else {
            continue;
        };
        if !fields.contains_key("status") {
            fields.insert("status".to_string(), default_rule["status"].clone());
        }
        match fields.get_mut("payload") {
            None => {
                fields.insert("payload".to_string(), default_rule["payload"].clone());
            }
            Some(Value::Object(payload)) => {
                for key in ["title", "summary"] {
                    if !payload.contains_key(key) {
                        payload.insert(key.to_string(), default_rule["payload"][key].clone());
                    }
                }
            }
            Some(_) => {}
        }
    }
    Ok(Value::Array(items))
}

fn has_checks_action(outcome: Option<&Value>) -> bool {
    match outcome {
        Some(Value::Array(items)) => items
            .iter()
            .any(|item| json_value::get(item, "do").and_then(Value::as_str) == Some("checks")),
        _ => false,
    }
}

fn when_includes(when: Option<&Value>, needle: &str) -> Result<bool> {
    json_value::includes(when, &Value::String(needle.to_string()))
}

fn set_pull_request_defaults(config: &mut Value) -> Result<()> {
    let Some(Value::Array(recipes)) = config.get_mut("mergeable") else {
        return Ok(());
    };
    for recipe in recipes.iter_mut() {
        let Value::Object(fields) = recipe else {
            continue;
        };
        let when = fields.get("when");
        let targets_pull_requests = when_includes(when, "pull_request")?
            || when_includes(when, "pull_request_review")?
            || when_includes(when, "issue_comment")?;
        let has_no_outcomes = Status::ALL
            .iter()
            .all(|outcome| !fields.contains_key(outcome.as_str()));
        let should_apply_defaults = targets_pull_requests
            && (has_no_outcomes
                || Status::ALL
                    .iter()
                    .any(|outcome| has_checks_action(fields.get(outcome.as_str()))));
        for outcome in Status::ALL {
            let key = outcome.as_str();
            if should_apply_defaults {
                let defaulted = with_default_checks(
                    fields.get_mut(key).map(Value::take),
                    default_actions_for(outcome),
                )?;
                fields.insert(key.to_string(), defaulted);
            } else if !fields.contains_key(key) {
                fields.insert(key.to_string(), Value::Array(Vec::new()));
            }
        }
    }
    Ok(())
}

fn config_path(context: &Context) -> Result<String> {
    let configured = context.global_setting("config_path")?;
    Ok(if json_value::truthy(configured) {
        json_value::to_display_string(configured)
    } else {
        "mergeable.yml".to_string()
    })
}

pub async fn reset_cache(context: &Context) -> Result<()> {
    if context.event_name != "push" {
        return Ok(());
    }
    let Some(head_commit) = context
        .payload
        .get("head_commit")
        .filter(|commit| json_value::is_truthy(commit))
    else {
        return Ok(());
    };
    let config_file = Value::String(format!(".github/{}", config_path(context)?));
    let added = json_value::includes(head_commit.get("added"), &config_file)?;
    if !added && !json_value::includes(head_commit.get("modified"), &config_file)? {
        return Ok(());
    }
    let repository = context.repository_name()?;
    if repository.name == ".github" {
        let owner_prefix = format!("{}/", repository.owner);
        for key in context.app.configuration_cache.keys().await {
            if key.starts_with(&owner_prefix) {
                context.app.configuration_cache.delete(&key).await;
            }
        }
    } else {
        context
            .app
            .configuration_cache
            .delete(&repository.to_string())
            .await;
    }
    Ok(())
}

fn parse_yaml(text: Option<&str>) -> Result<Option<Value>, FetchError> {
    let source = text.unwrap_or("null");
    yaml::parse(source).map_err(|error| FetchError::Yaml(error.message))
}

async fn fetch_config_file(context: &Context) -> Result<Value, FetchError> {
    let repository = context.repository_name()?;
    let use_pull_request_config =
        context.global_setting("use_config_from_pull_request")? != Some(&Value::Bool(false));
    if use_pull_request_config
        && ["pull_request", "pull_request_review"].contains(&context.event_name.as_str())
    {
        let head_repository = json_value::to_display_string(json_value::read_path(
            &context.payload,
            &["pull_request", "head", "repo", "full_name"],
        )?);
        let base_repository = json_value::to_display_string(json_value::read_path(
            &context.payload,
            &["pull_request", "base", "repo", "full_name"],
        )?);
        if head_repository == base_repository {
            let pull_number = json_value::read_path(&context.payload, &["pull_request", "number"])?
                .cloned()
                .unwrap_or(Value::Null);
            let files = api::list_files(
                context,
                context.repo_params(json!({"pull_number": pull_number}))?,
            )
            .await?;
            let mut config_file_changed = false;
            for changed_file in &files {
                let status =
                    json_value::to_display_string(json_value::read(Some(changed_file), "status")?);
                if (status == "modified" || status == "added")
                    && json_value::to_display_string(changed_file.get("filename")) == FILE_NAME
                {
                    config_file_changed = true;
                }
            }
            if config_file_changed {
                let head_sha =
                    json_value::read_path(&context.payload, &["pull_request", "head", "sha"])?
                        .cloned()
                        .unwrap_or(Value::Null);
                let changed_config_content = api::fetch_content(
                    context,
                    json!({"owner": &repository.owner, "repo": &repository.name, "path": FILE_NAME, "ref": head_sha}),
                )
                .await?;
                return Ok(parse_yaml(changed_config_content.as_deref())?.unwrap_or(Value::Null));
            }
        }
    }
    let config_path = config_path(context)?;
    let use_cache = context.global_setting("use_config_cache")? == Some(&Value::Bool(true));
    let cache_key = repository.to_string();
    if use_cache {
        let reset_context = context.clone();
        tokio::spawn(
            async move {
                if let Err(error) = reset_cache(&reset_context).await {
                    tracing::warn!(%error, "configuration.cache_reset.failed");
                }
            }
            .in_current_span(),
        );
        if let Some(cached) = context.app.configuration_cache.get(&cache_key).await
            && json_value::is_truthy(&cached)
        {
            return Ok(cached);
        }
    }
    let mut org_config = None;
    if context.global_setting("use_org_as_default_config")? == Some(&Value::Bool(true)) {
        let org_config_content = api::fetch_content(
            context,
            json!({"owner": &repository.owner, "repo": ".github", "path": format!(".github/{config_path}")}),
        )
        .await?;
        org_config = parse_yaml(org_config_content.as_deref())?;
    }
    let config = repository_config::load(context, &config_path, org_config).await?;
    let config = match config {
        Some(found) if !found.is_null() => found,
        _ => return Err(FetchError::NotFound),
    };
    if use_cache {
        context
            .app
            .configuration_cache
            .set(&cache_key, config.clone())
            .await;
    }
    Ok(config)
}

pub async fn load(context: &Context) -> Configuration {
    let failure = match fetch_config_file(context).await {
        Ok(config) => match Configuration::from_settings(config) {
            Ok(configuration) => return configuration,
            Err(error) => FetchError::Other(error),
        },
        Err(error) => error,
    };
    let mut configuration = Configuration {
        errors: Vec::new(),
        settings: Value::Object(Map::new()),
    };
    match failure {
        FetchError::NotFound => configuration.set_error(ErrorCode::NoYml, "No Config File found".to_string()),
        FetchError::Yaml(message) => configuration.set_error(ErrorCode::BadYml, format!("Invalid YML format > {message}")),
        FetchError::Other(error) => configuration.set_error(
            ErrorCode::GithubApiError,
            format!("Github API Error occurred while fetching the config file at {FILE_NAME} \n Error from api: {error}"),
        ),
    }
    configuration
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_request_rule_without_outcomes_gets_default_checks() {
        let configuration = Configuration::from_settings(json!({
            "version": 2,
            "mergeable": [{"when": "pull_request.*", "validate": []}]
        }))
        .unwrap();
        assert_eq!(configuration.settings[0]["pass"][0]["do"], "checks");
        assert_eq!(configuration.settings[0]["fail"][0]["status"], "failure");
    }

    #[test]
    fn issue_rule_without_outcomes_gets_empty_outcomes() {
        let configuration = Configuration::from_settings(json!({
            "version": 2,
            "mergeable": [{"when": "issues.opened", "validate": []}]
        }))
        .unwrap();
        assert_eq!(configuration.settings[0]["pass"], json!([]));
    }

    #[test]
    fn checks_action_without_payload_inherits_default_title_and_summary() {
        let configuration = Configuration::from_settings(json!({
            "version": 2,
            "mergeable": [{"when": "pull_request.opened", "validate": [], "pass": [{"do": "checks", "payload": {"title": "custom"}}]}]
        }))
        .unwrap();
        let pass = &configuration.settings[0]["pass"][0];
        assert_eq!(pass["payload"]["title"], "custom");
        assert_eq!(pass["status"], "success");
        assert!(
            pass["payload"]["summary"]
                .as_str()
                .unwrap()
                .starts_with("All the validators")
        );
    }

    #[test]
    fn defaulted_outcomes_keep_their_position_in_the_rule() {
        let configuration = Configuration::from_settings(json!({
            "version": 2,
            "mergeable": [{"when": "pull_request.*", "fail": [{"do": "checks"}], "validate": []}]
        }))
        .unwrap();
        let keys: Vec<&String> = configuration.settings[0]
            .as_object()
            .map(|rule| rule.keys().collect())
            .unwrap_or_default();
        assert_eq!(keys, ["when", "fail", "validate", "pass", "error"]);
    }

    #[test]
    fn version_less_config_is_transformed_from_version_one() {
        let configuration = Configuration::from_settings(json!({
            "mergeable": {"pull_requests": {"title": "wip", "label": "do not merge"}}
        }))
        .unwrap();
        assert!(!configuration.has_errors());
        assert_eq!(configuration.settings[0]["when"], "pull_request.*");
        assert_eq!(configuration.settings[1]["when"], "pull_request_review.*");
        assert_eq!(
            configuration.settings[0]["validate"],
            json!([
                {"do": "title", "must_exclude": {"regex": "wip"}},
                {"do": "label", "must_exclude": {"regex": "do not merge"}}
            ])
        );
    }

    #[test]
    fn missing_when_and_validate_are_both_reported() {
        let configuration =
            Configuration::from_settings(json!({"version": 2, "mergeable": [{}]})).unwrap();
        let codes: Vec<ErrorCode> = configuration
            .errors
            .iter()
            .map(|error| error.code)
            .collect();
        assert_eq!(
            codes,
            vec![
                ErrorCode::MissingWhenKeyword,
                ErrorCode::MissingValidateKeyword
            ]
        );
    }
}
