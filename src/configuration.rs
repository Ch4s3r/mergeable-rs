use crate::context::Context;
use crate::error::{Error, Result};
use crate::github::api;
use crate::json_value;
use crate::repository_config;
use crate::results::Status;
use crate::yaml;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

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

fn push_changes_config_file(context: &Context) -> Result<bool> {
    if context.event_name != "push" {
        return Ok(false);
    }
    let Some(head_commit) = context
        .payload
        .get("head_commit")
        .filter(|commit| json_value::is_truthy(commit))
    else {
        return Ok(false);
    };
    let config_file = Value::String(format!(".github/{}", config_path(context)?));
    Ok(
        json_value::includes(head_commit.get("added"), &config_file)?
            || json_value::includes(head_commit.get("modified"), &config_file)?,
    )
}

async fn forget_changed_organization_config(context: &Context, config_file: &str) {
    if context.app.github_data.is_none() || !matches!(push_changes_config_file(context), Ok(true)) {
        return;
    }
    if let Ok(repository) = context.repository_name()
        && repository.name == ".github"
    {
        api::forget_organization_default_config(context, &repository.owner, config_file).await;
    }
}

pub async fn reset_cache(context: &Context) -> Result<()> {
    if !push_changes_config_file(context)? {
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
        if let Err(error) = reset_cache(context).await {
            tracing::warn!(%error, "configuration.cache_reset.failed");
        }
        if let Some(cached) = context.app.configuration_cache.get(&cache_key).await
            && json_value::is_truthy(&cached)
        {
            return Ok(cached);
        }
    }
    let mut org_config = None;
    if context.global_setting("use_org_as_default_config")? == Some(&Value::Bool(true)) {
        let org_config_file = format!(".github/{config_path}");
        forget_changed_organization_config(context, &org_config_file).await;
        let org_config_content =
            api::fetch_organization_default_config(context, &repository.owner, &org_config_file)
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
    use crate::app_config::{BehaviorSettings, Environment};
    use crate::app_state::AppState;
    use crate::filters::test_context::context_for;
    use std::sync::Arc;

    async fn push_to_organization_repository_with_cached_org_config(
        repository_name: &str,
        modified_files: Value,
    ) -> Context {
        let mut context = context_for(
            "push",
            json!({
                "repository": {"name": repository_name, "owner": {"login": "acme"}},
                "head_commit": {"added": [], "modified": modified_files},
            }),
        );
        context.app = Arc::new(AppState::new(BehaviorSettings::from_environment(
            &Environment::from_iter([("USE_GITHUB_DATA_CACHE", "true")]),
        )));
        if let Some(caches) = &context.app.github_data {
            caches
                .organization_config
                .set(
                    "github-data:organization_config:acme/.github/mergeable.yml",
                    json!({"content": "version: 2"}),
                )
                .await;
        }
        context
    }

    async fn cached_organization_config_keys(context: &Context) -> Vec<String> {
        match &context.app.github_data {
            Some(caches) => caches.organization_config.keys().await,
            None => Vec::new(),
        }
    }

    #[tokio::test]
    async fn push_changing_the_organization_config_forgets_its_cached_copy() {
        let context = push_to_organization_repository_with_cached_org_config(
            ".github",
            json!([".github/mergeable.yml"]),
        )
        .await;
        forget_changed_organization_config(&context, ".github/mergeable.yml").await;
        assert_eq!(
            cached_organization_config_keys(&context).await,
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn unrelated_pushes_keep_the_cached_organization_config() {
        let other_file =
            push_to_organization_repository_with_cached_org_config(".github", json!(["README.md"]))
                .await;
        forget_changed_organization_config(&other_file, ".github/mergeable.yml").await;
        assert_eq!(cached_organization_config_keys(&other_file).await.len(), 1);
        let other_repository = push_to_organization_repository_with_cached_org_config(
            "app",
            json!([".github/mergeable.yml"]),
        )
        .await;
        forget_changed_organization_config(&other_repository, ".github/mergeable.yml").await;
        assert_eq!(
            cached_organization_config_keys(&other_repository)
                .await
                .len(),
            1
        );
    }

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

    fn loaded_yaml(text: &str) -> Value {
        crate::yaml::parse(text).unwrap().unwrap()
    }

    fn configuration_from_yaml(text: &str) -> Configuration {
        Configuration::from_settings(loaded_yaml(text)).unwrap()
    }

    fn assert_single_error(configuration: &Configuration, code: ErrorCode) {
        assert_eq!(configuration.errors.len(), 1, "{:?}", configuration.errors);
        assert!(configuration.has_error(code), "{:?}", configuration.errors);
    }

    #[test]
    fn wrong_version_is_reported_as_unknown_version() {
        let configuration = configuration_from_yaml(
            "
version: not a number
mergeable:
  pull_request:
",
        );
        assert_single_error(&configuration, ErrorCode::UnknownVersion);
    }

    #[test]
    fn missing_mergeable_node_is_reported() {
        let configuration = configuration_from_yaml("version: 2\n");
        assert_single_error(&configuration, ErrorCode::MissingMergeableNode);
    }

    #[test]
    fn missing_rule_sets_are_reported() {
        let configuration = configuration_from_yaml("version: 2\nmergeable:\n");
        assert_single_error(&configuration, ErrorCode::MissingRuleSets);
    }

    #[test]
    fn v2_non_array_rule_set_is_reported() {
        let configuration = configuration_from_yaml(
            "
version: 2
mergeable:
  when: test
",
        );
        assert_single_error(&configuration, ErrorCode::NonArrayMergeable);
    }

    #[test]
    fn v2_missing_or_misspelled_validate_keyword_is_reported_for_multiple_rule_sets() {
        let misspelled_validate = configuration_from_yaml(
            "
version: 2
mergeable:
  - when: pull_requests.*
    validate:
      - do :
  - when: issues.*
    valdate:
",
        );
        assert_single_error(&misspelled_validate, ErrorCode::MissingValidateKeyword);
        let missing_validate = configuration_from_yaml(
            "
version: 2
mergeable:
  - when: pull_requests.*
",
        );
        assert_single_error(&missing_validate, ErrorCode::MissingValidateKeyword);
    }

    #[test]
    fn v2_non_array_validate_node_is_reported() {
        let empty_validate = configuration_from_yaml(
            "
version: 2
mergeable:
  - when: pull_requests.*
    validate:
",
        );
        assert_single_error(&empty_validate, ErrorCode::NonArrayValidate);
        let object_validate = configuration_from_yaml(
            "
version: 2
mergeable:
  - when: pull_requests.*
    validate:
      do : title
",
        );
        assert_single_error(&object_validate, ErrorCode::NonArrayValidate);
    }

    #[test]
    fn v2_missing_or_misspelled_when_keyword_is_reported() {
        let configuration = configuration_from_yaml(
            "
version: 2
mergeable:
  - validate:
      - do :
",
        );
        assert_single_error(&configuration, ErrorCode::MissingWhenKeyword);
    }

    #[test]
    fn multiple_errors_are_all_reported() {
        let configuration = configuration_from_yaml(
            "
version: foo
mergeably:
  when: bar
",
        );
        assert_eq!(configuration.errors.len(), 2);
    }

    const VERSION_ONE_APPROVALS_LABEL_AND_TITLE: &str = "
mergeable:
  approvals: 5
  label: 'label regex'
  title: 'title regex'
";

    fn validator<'rule>(rule_set: &'rule Value, validator_name: &str) -> Option<&'rule Value> {
        rule_set["validate"]
            .as_array()
            .and_then(|validators| validators.iter().find(|item| item["do"] == validator_name))
    }

    #[test]
    fn version_two_shape_loads_correctly_without_version() {
        let configuration = configuration_from_yaml(VERSION_ONE_APPROVALS_LABEL_AND_TITLE);
        assert!(configuration.settings[0].get("when").is_some());
        assert!(configuration.settings[0].get("validate").is_some());
        assert!(!configuration.has_errors());
    }

    #[test]
    fn version_one_constructor_loads_settings_correctly() {
        let configuration = configuration_from_yaml(VERSION_ONE_APPROVALS_LABEL_AND_TITLE);
        let rule_set = &configuration.settings[0];
        assert_eq!(validator(rule_set, "approvals").unwrap()["min"]["count"], 5);
        assert_eq!(
            validator(rule_set, "title").unwrap()["must_exclude"]["regex"],
            "title regex"
        );
        assert_eq!(
            validator(rule_set, "label").unwrap()["must_exclude"]["regex"],
            "label regex"
        );
    }

    #[test]
    fn version_one_defaults_are_not_injected_when_user_defined_configuration_exists() {
        let configuration = configuration_from_yaml("mergeable:\n  approvals: 1\n");
        let rule_set = &configuration.settings[0];
        assert_eq!(validator(rule_set, "approvals").unwrap()["min"]["count"], 1);
        assert!(validator(rule_set, "title").is_none());
        assert!(validator(rule_set, "label").is_none());
    }

    #[test]
    fn undefined_pass_fail_or_error_does_not_break_the_config() {
        let configuration = configuration_from_yaml(
            "
mergeable:
  issues:
    stale:
      days: 30
      message: 'There has not been any activity in the past month. Is there anything to follow-up?'
",
        );
        assert_eq!(configuration.settings.as_array().map(Vec::len), Some(2));
        assert!(configuration.settings[0].get("fail").is_some());
        assert!(configuration.settings[0].get("error").is_some());
    }

    fn transformed_v2(text: &str) -> Value {
        let mut config = loaded_yaml(text);
        set_pull_request_defaults(&mut config).unwrap();
        config["mergeable"].take()
    }

    fn assert_default_pull_request_outcomes(rule_set: &Value) {
        assert_eq!(rule_set["pass"], *default_actions_for(Status::Pass));
        assert_eq!(rule_set["fail"], *default_actions_for(Status::Fail));
        assert_eq!(rule_set["error"], *default_actions_for(Status::Error));
    }

    const TITLE_RULE_FOR_PULL_REQUESTS: &str = "
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: title
        must_exclude:
          regex: 'wip|work in progress'
          message: 'This PR is work in progress.'
";

    #[test]
    fn outcome_defaults_load_when_pull_request_event_is_specified() {
        let rule_sets = transformed_v2(TITLE_RULE_FOR_PULL_REQUESTS);
        assert_default_pull_request_outcomes(&rule_sets[0]);
    }

    #[test]
    fn outcome_defaults_load_for_pull_requests_without_overriding_user_specified_outcomes() {
        let pass_outcome = "    pass:
      - do: checks
        state: completed
        status: success
        payload:
          title: a title
          summary: a summary
          text: some text
";
        let fail_outcome = "    fail:
      - do: checks
        state: completed
        status: failure
        payload:
          title: a failed title
";
        let error_outcome = "    error:
      - do: checks
        state: completed
        status: error
        payload:
          title: an errored title
";
        let with_pass = format!("{TITLE_RULE_FOR_PULL_REQUESTS}{pass_outcome}");
        let rule_sets = transformed_v2(&with_pass);
        assert_eq!(rule_sets[0]["pass"][0]["payload"]["title"], "a title");
        assert_eq!(rule_sets[0]["fail"], *default_actions_for(Status::Fail));
        assert_eq!(rule_sets[0]["error"], *default_actions_for(Status::Error));

        let with_pass_and_fail = format!("{with_pass}{fail_outcome}");
        let rule_sets = transformed_v2(&with_pass_and_fail);
        assert_eq!(rule_sets[0]["pass"][0]["payload"]["title"], "a title");
        assert_eq!(
            rule_sets[0]["fail"][0]["payload"]["title"],
            "a failed title"
        );
        assert_eq!(rule_sets[0]["error"], *default_actions_for(Status::Error));

        let with_all_outcomes = format!("{with_pass_and_fail}{error_outcome}");
        let rule_sets = transformed_v2(&with_all_outcomes);
        assert_eq!(rule_sets[0]["pass"][0]["payload"]["title"], "a title");
        assert_eq!(
            rule_sets[0]["fail"][0]["payload"]["title"],
            "a failed title"
        );
        assert_eq!(
            rule_sets[0]["error"][0]["payload"]["title"],
            "an errored title"
        );
    }

    #[test]
    fn outcome_defaults_load_when_pull_request_is_mixed_with_other_events() {
        let rule_sets = transformed_v2(
            "
version: 2
mergeable:
  - when: issues.*, pull_request.*
    validate:
      - do: title
        must_exclude:
          regex: 'wip|work in progress'
          message: 'This PR is work in progress.'
",
        );
        assert_default_pull_request_outcomes(&rule_sets[0]);
    }

    #[test]
    fn outcome_defaults_load_when_issue_comment_event_is_specified() {
        let rule_sets = transformed_v2(
            "
version: 2
mergeable:
  - when: issue_comment.*
    validate:
      - do: lastComment
        must_exclude:
          regex: 'wip|work in progress'
",
        );
        assert_default_pull_request_outcomes(&rule_sets[0]);
    }

    #[test]
    fn outcome_defaults_load_when_pull_request_review_event_is_specified() {
        let rule_sets = transformed_v2(
            "
version: 2
mergeable:
  - when: pull_request_review.*
    validate:
      - do: payload
        review:
          state:
            must_exclude:
              regex: 'changes_requested'
",
        );
        assert_default_pull_request_outcomes(&rule_sets[0]);
    }

    #[test]
    fn outcome_defaults_ignore_recipes_that_are_not_for_pull_requests() {
        let rule_sets = transformed_v2(
            "
version: 2
mergeable:
  - when: issues.*
    validate:
      - do: title
        must_exclude:
          regex: 'wip|work in progress'
          message: 'This PR is work in progress.'
",
        );
        assert_eq!(rule_sets[0]["pass"], json!([]));
        assert_eq!(rule_sets[0]["fail"], json!([]));
        assert_eq!(rule_sets[0]["error"], json!([]));
    }

    const TITLE_RULE_FOR_PULL_REQUESTS_PLURAL: &str = "
version: 2
mergeable:
  - when: pull_requests.*
    validate:
      - do: title
        must_exclude:
          regex: 'wip|work in progress'
          message: 'This PR is work in progress.'
";

    #[test]
    fn default_checks_fill_in_missing_required_fields() {
        let rule_sets = transformed_v2(&format!(
            "{TITLE_RULE_FOR_PULL_REQUESTS_PLURAL}    pass:
      - do: checks
        payload:
          summary: 'test Summary'
          text: 'test text'
"
        ));
        assert_eq!(
            rule_sets[0]["pass"],
            json!([{
                "do": "checks",
                "status": "success",
                "payload": {
                    "title": "Mergeable Run has been Completed!",
                    "summary": "test Summary",
                    "text": "test text"
                }
            }])
        );
    }

    const COMMENT_ON_FAIL: &str = "    fail:
      - do: comment
        payload:
          body: 'test Body'
";

    #[test]
    fn adding_defaults_only_works_for_checks() {
        let rule_sets = transformed_v2(&format!(
            "{TITLE_RULE_FOR_PULL_REQUESTS_PLURAL}{COMMENT_ON_FAIL}"
        ));
        assert_eq!(
            rule_sets[0]["fail"],
            json!([{"do": "comment", "payload": {"body": "test Body"}}])
        );
    }

    #[test]
    fn defaults_are_not_added_to_all_outcomes_if_no_checks_exist() {
        let rule_sets = transformed_v2(&format!(
            "{TITLE_RULE_FOR_PULL_REQUESTS_PLURAL}{COMMENT_ON_FAIL}"
        ));
        assert_eq!(rule_sets[0]["pass"], json!([]));
        assert_eq!(rule_sets[0]["error"], json!([]));
    }

    #[test]
    fn defaults_are_added_to_all_outcomes_if_at_least_one_checks_exists() {
        let rule_sets = transformed_v2(&format!(
            "{TITLE_RULE_FOR_PULL_REQUESTS_PLURAL}    fail:
      - do: checks
        payload:
          body: 'test Body'
"
        ));
        assert_eq!(rule_sets[0]["pass"], *default_actions_for(Status::Pass));
        assert_eq!(rule_sets[0]["fail"][0]["payload"]["body"], "test Body");
        assert_eq!(rule_sets[0]["error"], *default_actions_for(Status::Error));
    }
}
