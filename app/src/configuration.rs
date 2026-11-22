use crate::cache::ConfigCache;
use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::github::api;
use crate::js;
use crate::probot_config;
use crate::yaml;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

mod v1_config;

pub const FILE_NAME: &str = ".github/mergeable.yml";

pub mod error_codes {
    pub const BAD_YML: u16 = 10;
    pub const MISSING_MERGEABLE_NODE: u16 = 20;
    pub const UNKNOWN_VERSION: u16 = 30;
    pub const GITHUB_API_ERROR: u16 = 50;
    pub const NO_YML: u16 = 60;
    pub const MISSING_RULE_SETS: u16 = 70;
    pub const NON_ARRAY_MERGEABLE: u16 = 80;
    pub const MISSING_WHEN_KEYWORD: u16 = 90;
    pub const MISSING_VALIDATE_KEYWORD: u16 = 100;
    pub const NON_ARRAY_VALIDATE: u16 = 110;
}

static CONFIG_CACHE: LazyLock<ConfigCache> = LazyLock::new(ConfigCache::from_env);

#[derive(Debug, Clone, Default)]
pub struct Configuration {
    pub errors: Vec<(u16, String)>,
    pub settings: Value,
}

enum FetchError {
    NotFound,
    Yaml(String),
    Other(JsError),
}

impl From<JsError> for FetchError {
    fn from(error: JsError) -> Self {
        FetchError::Other(error)
    }
}

impl Configuration {
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    #[must_use]
    pub fn has_error(&self, code: u16) -> bool {
        self.errors.iter().any(|(existing, _)| *existing == code)
    }

    fn set_error(&mut self, code: u16, message: &str) {
        match self
            .errors
            .iter_mut()
            .find(|(existing, _)| *existing == code)
        {
            Some(entry) => entry.1 = message.to_string(),
            None => self.errors.push((code, message.to_string())),
        }
    }

    pub fn from_settings(settings: Value) -> JsResult<Configuration> {
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
            other => Err(JsError::error(format!(
                "Cannot find module './transformers/v{other}Config'"
            ))),
        }
    }

    fn validate(&mut self) -> JsResult<()> {
        for (code, message) in validation_errors(&self.settings)? {
            self.set_error(code, message);
        }
        Ok(())
    }
}

fn validation_errors(settings: &Value) -> JsResult<Vec<(u16, &'static str)>> {
    let mut errors = Vec::new();
    if js::truthy(settings.get("version")) && !settings.get("version").is_some_and(Value::is_number)
    {
        errors.push((error_codes::UNKNOWN_VERSION, "Invalid `version` found."));
    }
    match settings.get("mergeable") {
        None => {
            errors.push((
                error_codes::MISSING_MERGEABLE_NODE,
                "The `mergeable` node is missing.",
            ));
            return Ok(errors);
        }
        Some(Value::Null) => {
            errors.push((
                error_codes::MISSING_RULE_SETS,
                "`mergeable` node does not contain any rule sets",
            ));
            return Ok(errors);
        }
        Some(_) => {}
    }
    if config_version(settings) == "2" {
        let Some(Value::Array(rule_sets)) = settings.get("mergeable") else {
            errors.push((
                error_codes::NON_ARRAY_MERGEABLE,
                "`mergeable` must be an array for version 2 config",
            ));
            return Ok(errors);
        };
        for rule_set in rule_sets {
            if js::read(Some(rule_set), "when")?.is_none() {
                errors.push((
                    error_codes::MISSING_WHEN_KEYWORD,
                    "One or more rule set is missing `when` keyword",
                ));
            }
            match js::read(Some(rule_set), "validate")? {
                None => {
                    errors.push((
                        error_codes::MISSING_VALIDATE_KEYWORD,
                        "One or more rule set is missing `validate` keyword",
                    ));
                }
                Some(Value::Array(_)) => {}
                Some(_) => {
                    errors.push((
                        error_codes::NON_ARRAY_VALIDATE,
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
        Some(version) if js::is_truthy(version) => js::to_js_string(Some(version)),
        _ => "1".to_string(),
    }
}

static DEFAULT_PASS_ACTIONS: LazyLock<Value> =
    LazyLock::new(|| default_pull_request_actions("pass"));
static DEFAULT_FAIL_ACTIONS: LazyLock<Value> =
    LazyLock::new(|| default_pull_request_actions("fail"));
static DEFAULT_ERROR_ACTIONS: LazyLock<Value> =
    LazyLock::new(|| default_pull_request_actions("error"));

fn default_actions_for(outcome: &str) -> &'static Value {
    match outcome {
        "pass" => &DEFAULT_PASS_ACTIONS,
        "fail" => &DEFAULT_FAIL_ACTIONS,
        _ => &DEFAULT_ERROR_ACTIONS,
    }
}

fn default_pull_request_actions(outcome: &str) -> Value {
    match outcome {
        "pass" => json!([{
            "do": "checks",
            "state": "completed",
            "status": "success",
            "payload": {
                "title": "Mergeable Run has been Completed!",
                "summary": "All the validators have returned 'pass'! \n Here are some stats of the run: \n {{validationCount}} validations were run"
            }
        }]),
        "fail" => json!([{
            "do": "checks",
            "state": "completed",
            "status": "failure",
            "payload": {
                "title": "{{failCount}}/{{validationCount}} Fail(s): {{#each failures}} {{toUpperCase name}}{{^@last}}, {{/@last}}{{/each}}",
                "summary": "### Status: {{toUpperCase validationStatus}}\n\n        Here are some stats of the run:\n        {{validationCount}} validations were run.\n        {{passCount}} PASSED\n        {{failCount}} FAILED\n      ",
                "text": "{{#each validationSuites}}\n#### {{{statusIcon status}}} Validator: {{toUpperCase name}}\n{{#each validations }} * {{{statusIcon status}}} ***{{{ description }}}***\n       Input : {{{details.input}}}\n       Settings : {{{displaySettings details.settings}}}\n       {{/each}}\n{{/each}}"
            }
        }]),
        _ => json!([{
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

fn check_and_set_default(
    outcome_actions: Option<Value>,
    default_actions: &Value,
) -> JsResult<Value> {
    let Some(actions) = outcome_actions else {
        return Ok(default_actions.clone());
    };
    let Value::Array(mut items) = actions else {
        return Err(JsError::type_error("ruleSet.map is not a function"));
    };
    let default_rule = &default_actions[0];
    for rule in &mut items {
        if js::get(rule, "do").and_then(Value::as_str) != Some("checks") {
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
            .any(|item| js::get(item, "do").and_then(Value::as_str) == Some("checks")),
        _ => false,
    }
}

fn when_includes(when: Option<&Value>, needle: &str) -> JsResult<bool> {
    js::includes(when, &Value::String(needle.to_string()))
}

fn set_pull_request_defaults(config: &mut Value) -> JsResult<()> {
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
        let should_apply_defaults = targets_pull_requests
            && ((!fields.contains_key("pass")
                && !fields.contains_key("fail")
                && !fields.contains_key("error"))
                || has_checks_action(fields.get("pass"))
                || has_checks_action(fields.get("fail"))
                || has_checks_action(fields.get("error")));
        for outcome in ["pass", "fail", "error"] {
            if should_apply_defaults {
                let defaulted = check_and_set_default(
                    fields.get_mut(outcome).map(Value::take),
                    default_actions_for(outcome),
                )?;
                fields.insert(outcome.to_string(), defaulted);
            } else if !fields.contains_key(outcome) {
                fields.insert(outcome.to_string(), Value::Array(Vec::new()));
            }
        }
    }
    Ok(())
}

fn config_path(context: &Context) -> JsResult<String> {
    let configured = context.global_setting("config_path")?;
    Ok(if js::truthy(configured) {
        js::to_js_string(configured)
    } else {
        "mergeable.yml".to_string()
    })
}

pub async fn reset_cache(context: &Context) -> JsResult<()> {
    if context.event_name != "push" {
        return Ok(());
    }
    let Some(head_commit) = context
        .payload
        .get("head_commit")
        .filter(|commit| js::is_truthy(commit))
    else {
        return Ok(());
    };
    let config_file = Value::String(format!(".github/{}", config_path(context)?));
    let added = js::includes(head_commit.get("added"), &config_file)?;
    if !added && !js::includes(head_commit.get("modified"), &config_file)? {
        return Ok(());
    }
    let (owner, repo) = context.repo()?;
    if repo == ".github" {
        let owner_prefix = format!("{owner}/");
        for key in CONFIG_CACHE.keys().await {
            if key.starts_with(&owner_prefix) {
                CONFIG_CACHE.delete(&key).await;
            }
        }
    } else {
        CONFIG_CACHE.delete(&format!("{owner}/{repo}")).await;
    }
    Ok(())
}

fn parse_yaml(text: Option<&str>) -> Result<Option<Value>, FetchError> {
    let source = text.unwrap_or("null");
    yaml::parse(source).map_err(|error| FetchError::Yaml(error.message))
}

async fn fetch_config_file(context: &Context) -> Result<Value, FetchError> {
    let (owner, repo) = context.repo()?;
    let use_pull_request_config =
        context.global_setting("use_config_from_pull_request")? != Some(&Value::Bool(false));
    if use_pull_request_config
        && ["pull_request", "pull_request_review"].contains(&context.event_name.as_str())
    {
        let head_repository = js::to_js_string(js::read_path(
            &context.payload,
            &["pull_request", "head", "repo", "full_name"],
        )?);
        let base_repository = js::to_js_string(js::read_path(
            &context.payload,
            &["pull_request", "base", "repo", "full_name"],
        )?);
        if head_repository == base_repository {
            let pull_number = js::read_path(&context.payload, &["pull_request", "number"])?
                .cloned()
                .unwrap_or(Value::Null);
            let files = api::list_files(
                context,
                context.repo_params(json!({"pull_number": pull_number}))?,
            )
            .await?;
            let mut config_file_changed = false;
            for changed_file in &files {
                let status = js::to_js_string(js::read(Some(changed_file), "status")?);
                if (status == "modified" || status == "added")
                    && js::to_js_string(changed_file.get("filename")) == FILE_NAME
                {
                    config_file_changed = true;
                }
            }
            if config_file_changed {
                let head_sha = js::read_path(&context.payload, &["pull_request", "head", "sha"])?
                    .cloned()
                    .unwrap_or(Value::Null);
                let changed_config_content = api::get_content(
                    context,
                    json!({"owner": owner, "repo": repo, "path": FILE_NAME, "ref": head_sha}),
                )
                .await?;
                return Ok(parse_yaml(changed_config_content.as_deref())?.unwrap_or(Value::Null));
            }
        }
    }
    let config_path = config_path(context)?;
    let use_cache = context.global_setting("use_config_cache")? == Some(&Value::Bool(true));
    let cache_key = format!("{owner}/{repo}");
    if use_cache {
        let reset_context = context.clone();
        tokio::spawn(async move {
            let _ignored = reset_cache(&reset_context).await;
        });
        if let Some(cached) = CONFIG_CACHE.get(&cache_key).await
            && js::is_truthy(&cached)
        {
            return Ok(cached);
        }
    }
    let mut org_config = None;
    if context.global_setting("use_org_as_default_config")? == Some(&Value::Bool(true)) {
        let org_config_content = api::get_content(
            context,
            json!({"owner": owner, "repo": ".github", "path": format!(".github/{config_path}")}),
        )
        .await?;
        org_config = parse_yaml(org_config_content.as_deref())?;
    }
    let config = probot_config::load(context, &config_path, org_config).await?;
    let config = match config {
        Some(found) if !found.is_null() => found,
        _ => return Err(FetchError::NotFound),
    };
    if use_cache {
        CONFIG_CACHE.set(&cache_key, config.clone()).await;
    }
    Ok(config)
}

pub async fn instance_with_context(context: &Context) -> Configuration {
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
        FetchError::NotFound => configuration.set_error(error_codes::NO_YML, "No Config File found"),
        FetchError::Yaml(message) => configuration.set_error(error_codes::BAD_YML, &format!("Invalid YML format > {message}")),
        FetchError::Other(error) => configuration.set_error(
            error_codes::GITHUB_API_ERROR,
            &format!("Github API Error occurred while fetching the config file at {FILE_NAME} \n Error from api: {error}"),
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
        let codes: Vec<u16> = configuration.errors.iter().map(|(code, _)| *code).collect();
        assert_eq!(
            codes,
            vec![
                error_codes::MISSING_WHEN_KEYWORD,
                error_codes::MISSING_VALIDATE_KEYWORD
            ]
        );
    }
}
