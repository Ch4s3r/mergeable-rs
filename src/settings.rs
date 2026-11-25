use crate::app_config::SettingsFileDefaults;
use crate::context::Context;
use crate::error::{Error, ErrorKind, Result};
use crate::github::api;
use crate::json_value;
use crate::yaml;
use serde_json::{Value, json};

pub const FILE_NAME: &str = ".github/mergeable.settings.yml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    BadYml,
    MissingMergeableNode,
    UnknownVersion,
    GithubApiError,
    MissingSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub errors: Vec<SettingsError>,
    pub mergeable: Value,
}

impl Settings {
    #[must_use]
    pub fn from_file(mut settings: Value) -> Settings {
        let errors = validation_errors(&settings);
        if !errors.is_empty() {
            return Settings {
                errors,
                mergeable: json!({}),
            };
        }
        let mergeable = match settings.get_mut("mergeable").map(Value::take) {
            Some(mergeable @ (Value::Object(_) | Value::Array(_))) => mergeable,
            _ => json!({}),
        };
        Settings { errors, mergeable }
    }

    fn with_error(code: ErrorCode, message: String) -> Settings {
        Settings {
            errors: vec![SettingsError { code, message }],
            mergeable: json!({}),
        }
    }

    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    #[must_use]
    pub fn has_error(&self, code: ErrorCode) -> bool {
        self.errors.iter().any(|error| error.code == code)
    }
}

fn validation_errors(settings: &Value) -> Vec<SettingsError> {
    let mut errors = Vec::new();
    if json_value::truthy(settings.get("version"))
        && !settings.get("version").is_some_and(Value::is_number)
    {
        errors.push(SettingsError {
            code: ErrorCode::UnknownVersion,
            message: "Invalid `version` found.".to_string(),
        });
    }
    match settings.get("mergeable") {
        None => errors.push(SettingsError {
            code: ErrorCode::MissingMergeableNode,
            message: "The `mergeable` node is missing.".to_string(),
        }),
        Some(Value::Null) => errors.push(SettingsError {
            code: ErrorCode::MissingSettings,
            message: "`mergeable` node does not contain any rule sets".to_string(),
        }),
        Some(_) => {}
    }
    errors
}

fn apply_version_one_defaults(settings: &mut Value, defaults: &SettingsFileDefaults) {
    let Some(Value::Object(mergeable)) = settings.get_mut("mergeable") else {
        return;
    };
    let default_values = [
        ("use_config_from_pull_request", Value::Bool(true)),
        ("use_config_cache", Value::Bool(defaults.use_config_cache)),
        (
            "use_org_as_default_config",
            Value::Bool(defaults.use_org_as_default_config),
        ),
        ("config_path", Value::String(defaults.config_path.clone())),
    ];
    for (key, default_value) in default_values {
        if !mergeable.contains_key(key) {
            mergeable.insert(key.to_string(), default_value);
        }
    }
}

fn push_changes_settings_file(context: &Context) -> bool {
    if context.event_name != "push" {
        return false;
    }
    let Some(head_commit) = context
        .payload
        .get("head_commit")
        .filter(|commit| json_value::is_truthy(commit))
    else {
        return false;
    };
    let settings_file = Value::String(format!(".github/{}", context.app.behavior.settings_path));
    matches!(
        json_value::includes(head_commit.get("added"), &settings_file),
        Ok(true)
    ) || matches!(
        json_value::includes(head_commit.get("modified"), &settings_file),
        Ok(true)
    )
}

pub async fn reset_cache(context: &Context) -> Result<()> {
    if !push_changes_settings_file(context) {
        return Ok(());
    }
    let repository = context.repository_name()?;
    let settings_cache = &context.app.settings_cache;
    if repository.name == ".github" {
        let owner_prefix = format!("{}/", repository.owner);
        for key in settings_cache.keys().await {
            if key.starts_with(&owner_prefix) {
                settings_cache.delete(&key).await;
            }
        }
    } else {
        settings_cache
            .delete(&format!("{repository}/settings"))
            .await;
    }
    Ok(())
}

pub async fn fetch_settings_file(context: &Context) -> Result<Value> {
    let repository = context.repository_name()?;
    let behavior = &context.app.behavior;
    let settings_cache = &context.app.settings_cache;
    let settings_path = &behavior.settings_path;
    let cache_key = format!("{repository}/settings");
    if behavior.use_settings_cache {
        reset_cache(context).await?;
    }
    if behavior.use_settings_cache
        && let Some(cached) = settings_cache.get(&cache_key).await
        && json_value::is_truthy(&cached)
    {
        return Ok(cached);
    }
    let file_path = format!(".github/{settings_path}");
    let mut settings_file_content = api::fetch_content(
        context,
        json!({"owner": &repository.owner, "repo": ".github", "path": file_path}),
    )
    .await?;
    if settings_file_content.is_none() {
        settings_file_content = api::fetch_content(
            context,
            json!({"owner": &repository.owner, "repo": &repository.name, "path": file_path}),
        )
        .await?;
    }
    let parsed = match settings_file_content {
        Some(text) => {
            yaml::parse(&text).map_err(|error| Error::new(ErrorKind::Yaml, error.message))?
        }
        None => None,
    };
    let mut settings = match parsed {
        Some(value) if json_value::truthy(value.get("mergeable")) => value,
        _ => json!({"version": 1, "mergeable": {}}),
    };
    let version = match settings.get("version") {
        Some(version) if json_value::is_truthy(version) => {
            json_value::to_display_string(Some(version))
        }
        _ => "1".to_string(),
    };
    if version != "1" {
        return Err(Error::general(format!(
            "unsupported settings version {version}"
        )));
    }
    apply_version_one_defaults(&mut settings, &behavior.settings_file_defaults);
    if behavior.use_settings_cache {
        settings_cache.set(&cache_key, settings.clone()).await;
    }
    Ok(settings)
}

pub async fn instance_with_context(context: &Context) -> Settings {
    match fetch_settings_file(context).await {
        Ok(settings) => Settings::from_file(settings),
        Err(error) => {
            tracing::warn!(%error, "settings.load.failed");
            if error.kind() == ErrorKind::Yaml {
                Settings::with_error(
                    ErrorCode::BadYml,
                    format!("Invalid YML format > {}", error.message()),
                )
            } else {
                Settings::with_error(
                    ErrorCode::GithubApiError,
                    format!(
                        "Github API Error occurred while fetching the settings file at {FILE_NAME} \n Error from api: {error}"
                    ),
                )
            }
        }
    }
}

pub async fn load(context: &Context) -> Value {
    instance_with_context(context).await.mergeable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_one_defaults_fill_only_missing_keys() {
        let mut settings = json!({"mergeable": {"config_path": "custom.yml"}});
        apply_version_one_defaults(
            &mut settings,
            &SettingsFileDefaults::from_environment(&crate::app_config::Environment::default()),
        );
        assert_eq!(settings["mergeable"]["config_path"], "custom.yml");
        assert_eq!(settings["mergeable"]["use_config_from_pull_request"], true);
    }

    #[test]
    fn non_numeric_version_discards_settings() {
        assert_eq!(
            Settings::from_file(json!({"version": "1", "mergeable": {"a": 1}})).mergeable,
            json!({})
        );
        assert_eq!(
            Settings::from_file(json!({"version": 1, "mergeable": {"a": 1}})).mergeable,
            json!({"a": 1})
        );
    }

    #[test]
    fn non_numeric_version_is_reported_as_unknown_version() {
        let settings = Settings::from_file(json!({
            "version": "not a number",
            "mergeable": {
                "config_path": "",
                "use_config_cache": false,
                "use_org_as_default_config": false,
                "use_config_from_pull_request": true,
            },
        }));
        assert_eq!(settings.errors.len(), 1);
        assert!(settings.has_error(ErrorCode::UnknownVersion));
    }

    #[test]
    fn settings_without_mergeable_node_report_missing_mergeable_node() {
        let settings = Settings::from_file(json!({"version": 1}));
        assert_eq!(settings.errors.len(), 1);
        assert!(settings.has_error(ErrorCode::MissingMergeableNode));
    }

    #[test]
    fn empty_mergeable_node_reports_missing_rule_sets() {
        let settings = Settings::from_file(json!({"version": 1, "mergeable": null}));
        assert_eq!(settings.errors.len(), 1);
        assert!(settings.has_error(ErrorCode::MissingSettings));
    }
}
