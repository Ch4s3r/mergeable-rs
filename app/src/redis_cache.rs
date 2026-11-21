use crate::logging;
use redis::aio::MultiplexedConnection;
use redis::{AsyncCommands, IntoConnectionInfo};
use serde_json::{Value, json};
use tokio::sync::Mutex;

const DEFAULT_HOST: &str = "localhost";
const DEFAULT_PORT: u16 = 6379;
const KEYS_PATTERN_OF_UNDEFINED: &str = "";

#[derive(Debug, Clone, PartialEq)]
pub struct RedisSettings {
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
    pub database: i64,
    pub ttl_seconds: Option<String>,
}

fn non_empty_variable(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

impl RedisSettings {
    pub fn from_env() -> Self {
        Self::from_variables(non_empty_variable)
    }

    pub fn from_variables(variable: impl Fn(&str) -> Option<String>) -> Self {
        let host = variable("CACHE_REDIS_HOST");
        RedisSettings {
            host: host.clone().unwrap_or_else(|| DEFAULT_HOST.to_string()),
            port: variable("CACHE_REDIS_PORT")
                .and_then(|port| port.trim().parse().ok())
                .unwrap_or(DEFAULT_PORT),
            password: host,
            database: variable("CACHE_REDIS_DB")
                .and_then(|database| database.trim().parse().ok())
                .unwrap_or(0),
            ttl_seconds: variable("CACHE_TTL"),
        }
    }

    fn connection_info(&self, with_password: bool) -> redis::RedisResult<redis::ConnectionInfo> {
        let mut redis_settings = redis::RedisConnectionInfo::default().set_db(self.database);
        if with_password && let Some(password) = &self.password {
            redis_settings = redis_settings.set_password(password);
        }
        Ok(redis::ConnectionAddr::Tcp(self.host.clone(), self.port)
            .into_connection_info()?
            .set_redis_settings(redis_settings))
    }
}

pub fn serialize_value(value: &Value) -> Option<String> {
    (!value.is_null()).then(|| crate::js::stringify(value))
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
    logging::error(
        "cache",
        || json!({"operation": operation, "message": error.to_string()}),
    );
}

impl RedisCache {
    pub fn from_env() -> Self {
        Self::new(RedisSettings::from_env())
    }

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
                if self.settings.password.is_some()
                    && error.kind() == redis::ErrorKind::AuthenticationFailed =>
            {
                logging::warn(
                    "cache",
                    || json!({"message": "Redis server does not require a password, but a password was supplied"}),
                );
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
        let ttl_seconds = self.settings.ttl_seconds.clone();
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
            connection
                .keys::<_, Vec<String>>(KEYS_PATTERN_OF_UNDEFINED)
                .await
        })
        .await
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
    fn redis_password_is_the_host_like_the_javascript_cache() {
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
                password: Some("redis.internal".to_string()),
                database: 2,
                ttl_seconds: Some("60".to_string()),
            }
        );
    }

    #[test]
    fn redis_defaults_to_localhost_without_password_and_ttl() {
        let settings = settings_from(&[]);
        assert_eq!(settings.host, "localhost");
        assert_eq!(settings.port, 6379);
        assert_eq!(settings.password, None);
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
            password: Some("127.0.0.1".to_string()),
            database: 0,
            ttl_seconds: Some("60".to_string()),
        });
        cache.set("acme/app", json!({"version": 2})).await;
        assert_eq!(cache.get("acme/app").await, Some(json!({"version": 2})));
        assert_eq!(cache.keys().await, Vec::<String>::new());
        cache.delete("acme/app").await;
        assert_eq!(cache.get("acme/app").await, None);
    }
}
