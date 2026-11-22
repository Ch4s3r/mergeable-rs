use crate::cache::ConfigCache;
use crate::context::Context;
use crate::error::{Error, ErrorKind, Result};
use crate::github::api;
use crate::json_value;
use crate::yaml;
use serde_json::{Value, json};
use std::sync::LazyLock;

static SETTINGS_CACHE: LazyLock<ConfigCache> = LazyLock::new(ConfigCache::from_env);

#[must_use]
pub fn settings_path() -> String {
    match std::env::var("SETTINGS_PATH") {
        Ok(path) if !path.is_empty() => path,
        _ => "mergeable.settings.yml".to_string(),
    }
}

fn env_is_true(name: &str) -> bool {
    std::env::var(name).as_deref() == Ok("true")
}

fn use_settings_cache() -> bool {
    env_is_true("USE_SETTINGS_CACHE")
}

#[must_use]
pub fn default_config_path() -> String {
    std::env::var("CONFIG_PATH").unwrap_or_default()
}

fn apply_version_one_defaults(settings: &mut Value) {
    let Some(Value::Object(mergeable)) = settings.get_mut("mergeable") else {
        return;
    };
    let defaults = [
        ("use_config_from_pull_request", Value::Bool(true)),
        (
            "use_config_cache",
            Value::Bool(env_is_true("USE_CONFIG_CACHE")),
        ),
        (
            "use_org_as_default_config",
            Value::Bool(env_is_true("USE_ORG_AS_DEFAULT_CONFIG")),
        ),
        ("config_path", Value::String(default_config_path())),
    ];
    for (key, default_value) in defaults {
        if !mergeable.contains_key(key) {
            mergeable.insert(key.to_string(), default_value);
        }
    }
}

async fn fetch_settings_file(context: &Context) -> Result<Value> {
    let repository = context.repository_name()?;
    let settings_path = settings_path();
    let cache_key = format!("{repository}/settings");
    if use_settings_cache()
        && let Some(cached) = SETTINGS_CACHE.get(&cache_key).await
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
    apply_version_one_defaults(&mut settings);
    if use_settings_cache() {
        SETTINGS_CACHE.set(&cache_key, settings.clone()).await;
    }
    Ok(settings)
}

fn settings_from_file(mut settings: Value) -> Value {
    if json_value::truthy(settings.get("version"))
        && !settings.get("version").is_some_and(Value::is_number)
    {
        return json!({});
    }
    match settings.get_mut("mergeable").map(Value::take) {
        Some(mergeable @ (Value::Object(_) | Value::Array(_))) => mergeable,
        _ => json!({}),
    }
}

pub async fn load(context: &Context) -> Value {
    match fetch_settings_file(context).await {
        Ok(settings) => settings_from_file(settings),
        Err(error) => {
            tracing::warn!(%error, "settings.load.failed");
            json!({})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_one_defaults_fill_only_missing_keys() {
        let mut settings = json!({"mergeable": {"config_path": "custom.yml"}});
        apply_version_one_defaults(&mut settings);
        assert_eq!(settings["mergeable"]["config_path"], "custom.yml");
        assert_eq!(settings["mergeable"]["use_config_from_pull_request"], true);
    }

    #[test]
    fn non_numeric_version_discards_settings() {
        assert_eq!(
            settings_from_file(json!({"version": "1", "mergeable": {"a": 1}})),
            json!({})
        );
        assert_eq!(
            settings_from_file(json!({"version": 1, "mergeable": {"a": 1}})),
            json!({"a": 1})
        );
    }
}
