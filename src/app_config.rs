use crate::cache::CacheSettings;
use crate::scheduler::SchedulerOptions;
use base64::Engine as _;
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

const DEVELOPMENT_SCHEDULER_INTERVAL_SECONDS: &str = "2";
const DEFAULT_SCHEDULER_INTERVAL_SECONDS: &str = "3600";
const DEFAULT_SETTINGS_PATH: &str = "mergeable.settings.yml";
const DEFAULT_CONFIG_FILE_NAME: &str = "mergeable.yml";

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Environment {
    variables: BTreeMap<String, String>,
}

impl fmt::Debug for Environment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Environment")
            .field("variable_names", &self.variables.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Environment {
    #[must_use]
    pub fn from_process() -> Self {
        std::env::vars_os()
            .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
            .collect()
    }

    #[must_use]
    pub fn value(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(String::as_str)
    }

    #[must_use]
    pub fn non_empty(&self, name: &str) -> Option<&str> {
        self.value(name).filter(|value| !value.is_empty())
    }

    #[must_use]
    pub fn is_true(&self, name: &str) -> bool {
        self.value(name) == Some("true")
    }
}

impl<Name: Into<String>, Value: Into<String>> FromIterator<(Name, Value)> for Environment {
    fn from_iter<Variables: IntoIterator<Item = (Name, Value)>>(variables: Variables) -> Self {
        Environment {
            variables: variables
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Pretty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Development,
    Production,
}

#[derive(Clone, PartialEq, Eq)]
pub struct JiraSettings {
    pub protocol: String,
    pub host: Option<String>,
    pub api_version: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl fmt::Debug for JiraSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JiraSettings")
            .field("protocol", &self.protocol)
            .field("host", &self.host)
            .field("api_version", &self.api_version)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl Default for JiraSettings {
    fn default() -> Self {
        JiraSettings::from_environment(&Environment::default())
    }
}

impl JiraSettings {
    #[must_use]
    pub fn from_environment(environment: &Environment) -> Self {
        let owned = |name: &str| environment.non_empty(name).map(str::to_string);
        JiraSettings {
            protocol: owned("JIRA_PROTOCOL").unwrap_or_else(|| "https".to_string()),
            host: owned("JIRA_HOST"),
            api_version: owned("JIRA_VERSION").unwrap_or_else(|| "2".to_string()),
            username: owned("JIRA_USERNAME"),
            password: owned("JIRA_PASSWORD"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFileDefaults {
    pub config_path: String,
    pub use_config_cache: bool,
    pub use_org_as_default_config: bool,
}

impl SettingsFileDefaults {
    #[must_use]
    pub fn from_environment(environment: &Environment) -> Self {
        SettingsFileDefaults {
            config_path: environment
                .value("CONFIG_PATH")
                .unwrap_or_default()
                .to_string(),
            use_config_cache: environment.is_true("USE_CONFIG_CACHE"),
            use_org_as_default_config: environment.is_true("USE_ORG_AS_DEFAULT_CONFIG"),
        }
    }

    #[must_use]
    pub fn config_file_name(&self) -> &str {
        if self.config_path.is_empty() {
            DEFAULT_CONFIG_FILE_NAME
        } else {
            &self.config_path
        }
    }
}

#[derive(Debug, Clone)]
pub struct BehaviorSettings {
    pub app_name: Option<String>,
    pub settings_path: String,
    pub use_settings_cache: bool,
    pub settings_file_defaults: SettingsFileDefaults,
    pub log_config: bool,
    pub jira: JiraSettings,
    pub cache: CacheSettings,
}

impl Default for BehaviorSettings {
    fn default() -> Self {
        BehaviorSettings::from_environment(&Environment::default())
    }
}

impl BehaviorSettings {
    #[must_use]
    pub fn from_environment(environment: &Environment) -> Self {
        BehaviorSettings {
            app_name: environment.non_empty("APP_NAME").map(str::to_string),
            settings_path: environment
                .non_empty("SETTINGS_PATH")
                .unwrap_or(DEFAULT_SETTINGS_PATH)
                .to_string(),
            use_settings_cache: environment.is_true("USE_SETTINGS_CACHE"),
            settings_file_defaults: SettingsFileDefaults::from_environment(environment),
            log_config: environment.non_empty("LOG_CONFIG").is_some(),
            jira: JiraSettings::from_environment(environment),
            cache: CacheSettings::from_environment(environment),
        }
    }
}

pub struct AppConfig {
    pub app_id: String,
    pub private_key_pem: String,
    pub webhook_secret: String,
    pub webhook_path: String,
    pub host: String,
    pub port: u16,
    pub github_base_url: String,
    pub log_level: String,
    pub log_format: LogFormat,
    pub run_mode: RunMode,
    pub scheduler: Option<SchedulerOptions>,
    pub rate_limit_endpoint_enabled: bool,
    pub metrics_endpoint_enabled: bool,
    pub github_throttle_enabled: bool,
    pub github_retry_base_delay: Duration,
    pub behavior: BehaviorSettings,
}

fn read_private_key(environment: &Environment) -> Result<String, String> {
    if let Some(key) = environment.non_empty("PRIVATE_KEY") {
        if key.contains("-----BEGIN") {
            return Ok(key.replace("\\n", "\n"));
        }
        return base64::engine::general_purpose::STANDARD
            .decode(key.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .filter(|decoded| decoded.contains("-----BEGIN"))
            .ok_or_else(|| "PRIVATE_KEY is neither a PEM key nor base64-encoded PEM".to_string());
    }
    if let Some(path) = environment.non_empty("PRIVATE_KEY_PATH") {
        return std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read PRIVATE_KEY_PATH: {error}"));
    }
    Err("missing PRIVATE_KEY or PRIVATE_KEY_PATH".to_string())
}

fn scheduler_interval_milliseconds(environment: &Environment, run_mode: RunMode) -> Option<f64> {
    if !environment.is_true("MERGEABLE_SCHEDULER") {
        return None;
    }
    let interval_seconds = match run_mode {
        RunMode::Development => DEVELOPMENT_SCHEDULER_INTERVAL_SECONDS,
        RunMode::Production => environment
            .non_empty("MERGEABLE_SCHEDULER_INTERVAL")
            .unwrap_or(DEFAULT_SCHEDULER_INTERVAL_SECONDS),
    };
    Some(crate::json_value::parse_decimal_number(interval_seconds) * 1000.0)
}

impl AppConfig {
    pub fn from_environment(environment: &Environment) -> Result<Self, String> {
        let variable = |name: &str| environment.non_empty(name).map(str::to_string);
        let github_base_url = match variable("GHE_HOST") {
            Some(host) => format!(
                "{}://{host}/api/v3",
                variable("GHE_PROTOCOL").unwrap_or_else(|| "https".to_string())
            ),
            None => "https://api.github.com".to_string(),
        };
        let run_mode = if variable("NODE_ENV").as_deref() == Some("development") {
            RunMode::Development
        } else {
            RunMode::Production
        };
        let log_format = if variable("LOG_FORMAT").as_deref() == Some("pretty") {
            LogFormat::Pretty
        } else {
            LogFormat::Json
        };
        Ok(AppConfig {
            app_id: variable("APP_ID").ok_or("missing APP_ID")?,
            private_key_pem: read_private_key(environment)?,
            webhook_secret: variable("WEBHOOK_SECRET").unwrap_or_else(|| "development".to_string()),
            webhook_path: variable("WEBHOOK_PATH")
                .unwrap_or_else(|| "/api/github/webhooks".to_string()),
            host: variable("HOST").unwrap_or_else(|| "0.0.0.0".to_string()),
            port: variable("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(3000),
            github_base_url,
            log_level: variable("LOG_LEVEL").unwrap_or_else(|| "info".to_string()),
            log_format,
            run_mode,
            scheduler: scheduler_interval_milliseconds(environment, run_mode).map(
                |interval_milliseconds| {
                    SchedulerOptions::from_environment(environment, interval_milliseconds)
                },
            ),
            rate_limit_endpoint_enabled: environment.is_true("ENABLE_GITHUB_RATELIMIT_ENDPOINT"),
            metrics_endpoint_enabled: environment.is_true("ENABLE_METRICS_ENDPOINT"),
            github_throttle_enabled: variable("GITHUB_THROTTLE").as_deref() != Some("false"),
            github_retry_base_delay: Duration::from_millis(
                variable("GITHUB_RETRY_BASE_MS")
                    .and_then(|millis| millis.parse().ok())
                    .unwrap_or(1000),
            ),
            behavior: BehaviorSettings::from_environment(environment),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_variables_count_as_unset_where_the_app_ignores_them() {
        let environment = Environment::from_iter([("APP_NAME", ""), ("CONFIG_PATH", "")]);
        let behavior = BehaviorSettings::from_environment(&environment);
        assert_eq!(behavior.app_name, None);
        assert_eq!(
            behavior.settings_file_defaults.config_file_name(),
            "mergeable.yml"
        );
        assert_eq!(behavior.settings_path, "mergeable.settings.yml");
    }

    #[test]
    fn flags_are_enabled_only_by_the_exact_word_true() {
        let environment =
            Environment::from_iter([("USE_CONFIG_CACHE", "true"), ("USE_SETTINGS_CACHE", "TRUE")]);
        let behavior = BehaviorSettings::from_environment(&environment);
        assert!(behavior.settings_file_defaults.use_config_cache);
        assert!(!behavior.use_settings_cache);
    }

    #[test]
    fn debug_output_lists_variable_names_but_never_values() {
        let environment = Environment::from_iter([("WEBHOOK_SECRET", "hunter2")]);
        let debug_output = format!("{environment:?}");
        assert!(debug_output.contains("WEBHOOK_SECRET"));
        assert!(!debug_output.contains("hunter2"));
    }

    #[test]
    fn config_is_built_from_an_injected_environment() {
        let environment = Environment::from_iter([
            ("APP_ID", "7"),
            ("PRIVATE_KEY", "-----BEGIN KEY-----\\nabc"),
            ("GHE_HOST", "127.0.0.1:9"),
            ("GHE_PROTOCOL", "http"),
            ("PORT", "0"),
        ]);
        let config =
            AppConfig::from_environment(&environment).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(config.app_id, "7");
        assert_eq!(config.private_key_pem, "-----BEGIN KEY-----\nabc");
        assert_eq!(config.github_base_url, "http://127.0.0.1:9/api/v3");
        assert_eq!(config.port, 0);
        assert!(config.scheduler.is_none());
    }
}
