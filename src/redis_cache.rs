use redis::aio::MultiplexedConnection;
use redis::{AsyncCommands, IntoConnectionInfo};
use serde_json::Value;
use std::fmt;
use tokio::sync::Mutex;

const DEFAULT_HOST: &str = "localhost";
const DEFAULT_PORT: u16 = 6379;
const EMPTY_KEYS_PATTERN: &str = "";

#[derive(Clone, PartialEq, Eq)]
pub enum RedisPassword {
    Configured(String),
    HostNameFallback(String),
}

impl RedisPassword {
    fn secret(&self) -> &str {
        match self {
            RedisPassword::Configured(password) | RedisPassword::HostNameFallback(password) => {
                password
            }
        }
    }
}

impl fmt::Debug for RedisPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source = match self {
            RedisPassword::Configured(_) => "Configured",
            RedisPassword::HostNameFallback(_) => "HostNameFallback",
        };
        formatter.debug_tuple(source).field(&"<redacted>").finish()
    }
}

#[derive(Clone, PartialEq)]
pub struct RedisSettings {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<RedisPassword>,
    pub use_tls: bool,
    pub database: i64,
    pub ttl_seconds: Option<String>,
}

impl fmt::Debug for RedisSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RedisSettings")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &self.password)
            .field("use_tls", &self.use_tls)
            .field("database", &self.database)
            .field("ttl_seconds", &self.ttl_seconds)
            .finish()
    }
}

impl RedisSettings {
    #[must_use]
    pub fn from_environment(environment: &crate::app_config::Environment) -> Self {
        Self::from_variables(|name| environment.non_empty(name).map(str::to_string))
    }

    pub fn from_variables(variable: impl Fn(&str) -> Option<String>) -> Self {
        let host = variable("CACHE_REDIS_HOST");
        RedisSettings {
            host: host.clone().unwrap_or_else(|| DEFAULT_HOST.to_string()),
            port: variable("CACHE_REDIS_PORT")
                .and_then(|port| port.trim().parse().ok())
                .unwrap_or(DEFAULT_PORT),
            username: variable("CACHE_REDIS_USERNAME"),
            password: variable("CACHE_REDIS_PASSWORD")
                .map(RedisPassword::Configured)
                .or_else(|| host.map(RedisPassword::HostNameFallback)),
            use_tls: variable("CACHE_REDIS_TLS").as_deref() == Some("true"),
            database: variable("CACHE_REDIS_DB")
                .and_then(|database| database.trim().parse().ok())
                .unwrap_or(0),
            ttl_seconds: variable("CACHE_TTL"),
        }
    }

    fn connection_info(&self, with_password: bool) -> redis::RedisResult<redis::ConnectionInfo> {
        let mut redis_settings = redis::RedisConnectionInfo::default().set_db(self.database);
        if with_password && let Some(password) = &self.password {
            redis_settings = redis_settings.set_password(password.secret());
            if let Some(username) = &self.username {
                redis_settings = redis_settings.set_username(username);
            }
        }
        Ok(self
            .connection_address()
            .into_connection_info()?
            .set_redis_settings(redis_settings))
    }

    fn connection_address(&self) -> redis::ConnectionAddr {
        if self.use_tls {
            redis::ConnectionAddr::TcpTls {
                host: self.host.clone(),
                port: self.port,
                insecure: false,
                tls_params: None,
            }
        } else {
            redis::ConnectionAddr::Tcp(self.host.clone(), self.port)
        }
    }

    fn may_retry_without_password(&self) -> bool {
        matches!(self.password, Some(RedisPassword::HostNameFallback(_)))
    }
}

#[must_use]
pub fn serialize_value(value: &Value) -> Option<String> {
    (!value.is_null()).then(|| crate::json_value::stringify(value))
}

pub fn deserialize_value(stored: Option<String>) -> Result<Option<Value>, serde_json::Error> {
    match stored {
        None => Ok(None),
        Some(text) => {
            serde_json::from_str::<Value>(&text).map(|parsed| (!parsed.is_null()).then_some(parsed))
        }
    }
}

pub struct RedisCache {
    settings: RedisSettings,
    connection: Mutex<Option<MultiplexedConnection>>,
}

fn log_cache_error(operation: &str, error: impl std::fmt::Display) {
    tracing::error!(operation, %error, "redis.cache.failed");
}

impl RedisCache {
    #[must_use]
    pub fn new(settings: RedisSettings) -> Self {
        RedisCache {
            settings,
            connection: Mutex::new(None),
        }
    }

    async fn open_connection(
        &self,
        with_password: bool,
    ) -> redis::RedisResult<MultiplexedConnection> {
        redis::Client::open(self.settings.connection_info(with_password)?)?
            .get_multiplexed_async_connection()
            .await
    }

    async fn connection(&self) -> redis::RedisResult<MultiplexedConnection> {
        let mut cached = self.connection.lock().await;
        if let Some(connection) = cached.as_ref() {
            return Ok(connection.clone());
        }
        let connection = match self.open_connection(true).await {
            Err(error)
                if self.settings.may_retry_without_password()
                    && error.kind() == redis::ErrorKind::AuthenticationFailed =>
            {
                tracing::warn!("redis.connection.password_not_required");
                self.open_connection(false).await?
            }
            outcome => outcome?,
        };
        *cached = Some(connection.clone());
        Ok(connection)
    }

    async fn forget_connection(&self) {
        *self.connection.lock().await = None;
    }

    async fn run<Output>(
        &self,
        operation: &str,
        command: impl AsyncFnOnce(MultiplexedConnection) -> redis::RedisResult<Output>,
    ) -> Option<Output> {
        let outcome = match self.connection().await {
            Ok(connection) => command(connection).await,
            Err(error) => Err(error),
        };
        match outcome {
            Ok(output) => Some(output),
            Err(error) => {
                if error.is_io_error() || error.is_connection_dropped() {
                    self.forget_connection().await;
                }
                log_cache_error(operation, error);
                None
            }
        }
    }

    pub async fn get(&self, key: &str) -> Option<Value> {
        let stored = self
            .run("get", async |mut connection| {
                connection.get::<_, Option<String>>(key).await
            })
            .await?;
        deserialize_value(stored).unwrap_or_else(|error| {
            log_cache_error("get", error);
            None
        })
    }

    pub async fn set(&self, key: &str, value: Value) {
        let Some(serialized) = serialize_value(&value) else {
            log_cache_error("set", "\"null\" is not a cacheable value");
            return;
        };
        let ttl_seconds = self.settings.ttl_seconds.as_deref();
        self.run("set", async |mut connection| match ttl_seconds {
            Some(ttl_seconds) => {
                redis::cmd("SETEX")
                    .arg(key)
                    .arg(ttl_seconds)
                    .arg(serialized)
                    .query_async::<()>(&mut connection)
                    .await
            }
            None => connection.set::<_, _, ()>(key, serialized).await,
        })
        .await;
    }

    pub async fn delete(&self, key: &str) {
        self.run("del", async |mut connection| {
            connection.del::<_, ()>(key).await
        })
        .await;
    }

    pub async fn keys(&self) -> Vec<String> {
        self.run("keys", async |mut connection| {
            connection.keys::<_, Vec<String>>(EMPTY_KEYS_PATTERN).await
        })
        .await
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn random_password() -> String {
        hex::encode(rand::random::<[u8; 16]>())
    }

    fn settings_from(variables: &[(&str, &str)]) -> RedisSettings {
        let variables: HashMap<String, String> = variables
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        RedisSettings::from_variables(|name| {
            variables
                .get(name)
                .cloned()
                .filter(|value| !value.is_empty())
        })
    }

    #[test]
    fn cached_values_are_stored_as_json_text() {
        let config = json!({"version": 2, "mergeable": [{"when": "pull_request.*"}]});
        assert_eq!(
            serialize_value(&config).as_deref(),
            Some(r#"{"version":2,"mergeable":[{"when":"pull_request.*"}]}"#)
        );
        assert_eq!(serialize_value(&json!("x")).as_deref(), Some(r#""x""#));
    }

    #[test]
    fn null_is_not_a_cacheable_value() {
        assert_eq!(serialize_value(&Value::Null), None);
    }

    #[test]
    fn stored_json_is_parsed_back_and_a_missing_key_is_a_miss() {
        assert_eq!(
            deserialize_value(Some(r#"{"a":1}"#.to_string())).ok(),
            Some(Some(json!({"a": 1})))
        );
        assert_eq!(deserialize_value(None).ok(), Some(None));
        assert!(deserialize_value(Some("not json".to_string())).is_err());
    }

    #[test]
    fn redis_password_falls_back_to_the_host_setting_without_a_configured_password() {
        let settings = settings_from(&[
            ("CACHE_REDIS_HOST", "redis.internal"),
            ("CACHE_REDIS_PORT", "6380"),
            ("CACHE_REDIS_DB", "2"),
            ("CACHE_TTL", "60"),
        ]);
        assert_eq!(
            settings,
            RedisSettings {
                host: "redis.internal".to_string(),
                port: 6380,
                username: None,
                password: Some(RedisPassword::HostNameFallback(
                    "redis.internal".to_string()
                )),
                use_tls: false,
                database: 2,
                ttl_seconds: Some("60".to_string()),
            }
        );
    }

    #[test]
    fn configured_redis_password_and_username_replace_the_host_fallback() {
        let password = random_password();
        let settings = settings_from(&[
            ("CACHE_REDIS_HOST", "redis.internal"),
            ("CACHE_REDIS_USERNAME", "mergeable"),
            ("CACHE_REDIS_PASSWORD", &password),
        ]);
        assert_eq!(settings.username.as_deref(), Some("mergeable"));
        assert_eq!(settings.password, Some(RedisPassword::Configured(password)));
    }

    #[test]
    fn only_the_host_fallback_password_is_retried_without_authentication() {
        let host_fallback = settings_from(&[("CACHE_REDIS_HOST", "redis.internal")]);
        let configured = settings_from(&[
            ("CACHE_REDIS_HOST", "redis.internal"),
            ("CACHE_REDIS_PASSWORD", &random_password()),
        ]);
        assert!(host_fallback.may_retry_without_password());
        assert!(!configured.may_retry_without_password());
        assert!(!settings_from(&[]).may_retry_without_password());
    }

    #[test]
    fn tls_is_enabled_only_by_the_exact_word_true() {
        let with_tls = settings_from(&[
            ("CACHE_REDIS_HOST", "redis.internal"),
            ("CACHE_REDIS_TLS", "true"),
        ]);
        assert!(with_tls.use_tls);
        assert!(matches!(
            with_tls.connection_address(),
            redis::ConnectionAddr::TcpTls { ref host, port: 6379, insecure: false, .. } if host == "redis.internal"
        ));
        let without_tls = settings_from(&[("CACHE_REDIS_TLS", "TRUE")]);
        assert!(!without_tls.use_tls);
        assert!(matches!(
            without_tls.connection_address(),
            redis::ConnectionAddr::Tcp(..)
        ));
    }

    #[test]
    fn redis_settings_debug_output_redacts_the_password() {
        let settings = settings_from(&[("CACHE_REDIS_HOST", "secret-host")]);
        let debug_output = format!("{settings:?}");
        assert!(debug_output.contains("<redacted>"));
        assert_eq!(debug_output.matches("secret-host").count(), 1);
        let password = random_password();
        let configured = settings_from(&[("CACHE_REDIS_PASSWORD", &password)]);
        assert!(!format!("{configured:?}").contains(&password));
    }

    #[test]
    fn redis_defaults_to_localhost_without_password_and_ttl() {
        let settings = settings_from(&[]);
        assert_eq!(settings.host, "localhost");
        assert_eq!(settings.port, 6379);
        assert_eq!(settings.username, None);
        assert_eq!(settings.password, None);
        assert!(!settings.use_tls);
        assert_eq!(settings.ttl_seconds, None);
    }

    #[tokio::test]
    #[ignore = "needs a redis-server on REDIS_TEST_PORT"]
    async fn live_redis_round_trip_with_host_as_password_on_a_passwordless_server() {
        let port = std::env::var("REDIS_TEST_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let cache = RedisCache::new(RedisSettings {
            host: "127.0.0.1".to_string(),
            port,
            username: None,
            password: Some(RedisPassword::HostNameFallback("127.0.0.1".to_string())),
            use_tls: false,
            database: 0,
            ttl_seconds: Some("60".to_string()),
        });
        cache.set("acme/app", json!({"version": 2})).await;
        assert_eq!(cache.get("acme/app").await, Some(json!({"version": 2})));
        assert_eq!(cache.keys().await, Vec::<String>::new());
        cache.delete("acme/app").await;
        assert_eq!(cache.get("acme/app").await, None);
    }

    #[tokio::test]
    #[ignore = "needs a redis-server with TLS on REDIS_TLS_TEST_PORT, a password and a trusted certificate for localhost"]
    async fn live_redis_round_trip_over_tls_with_a_configured_password() {
        let port = std::env::var("REDIS_TLS_TEST_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let password = std::env::var("REDIS_TEST_PASSWORD").unwrap_or_default();
        let cache = RedisCache::new(RedisSettings {
            host: "localhost".to_string(),
            port,
            username: None,
            password: Some(RedisPassword::Configured(password)),
            use_tls: true,
            database: 0,
            ttl_seconds: Some("60".to_string()),
        });
        cache.set("acme/tls", json!({"version": 2})).await;
        assert_eq!(cache.get("acme/tls").await, Some(json!({"version": 2})));
        cache.delete("acme/tls").await;
        assert_eq!(cache.get("acme/tls").await, None);
    }
}
