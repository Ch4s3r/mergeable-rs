use crate::app_config::Environment;
use crate::metrics::Metrics;
use crate::redis_cache::{RedisCache, RedisSettings};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

const SHARED_STORAGE_NAMES: [&str; 2] = ["redis", "valkey"];

#[derive(Debug, Clone, PartialEq)]
pub enum CacheSettings {
    Memory {
        max_entries: Option<u64>,
        time_to_live: Option<Duration>,
    },
    Redis(RedisSettings),
}

fn number_variable(environment: &Environment, name: &str) -> Option<u64> {
    environment
        .value(name)
        .and_then(|value| value.trim().parse::<u64>().ok())
}

impl CacheSettings {
    #[must_use]
    pub fn from_environment(environment: &Environment) -> Self {
        if environment
            .value("CACHE_STORAGE")
            .is_some_and(|storage| SHARED_STORAGE_NAMES.contains(&storage))
        {
            return CacheSettings::Redis(RedisSettings::from_environment(environment));
        }
        CacheSettings::Memory {
            max_entries: number_variable(environment, "CACHE_MEMORY_MAX"),
            time_to_live: number_variable(environment, "CACHE_TTL").map(Duration::from_secs),
        }
    }

    #[must_use]
    pub fn with_time_to_live(&self, time_to_live: Duration) -> Self {
        match self {
            CacheSettings::Memory { max_entries, .. } => CacheSettings::Memory {
                max_entries: *max_entries,
                time_to_live: Some(time_to_live),
            },
            CacheSettings::Redis(redis_settings) => CacheSettings::Redis(RedisSettings {
                ttl_seconds: Some(time_to_live.as_secs().max(1).to_string()),
                ..redis_settings.clone()
            }),
        }
    }
}

enum CacheStorage {
    Memory(moka::future::Cache<String, Value>),
    Redis(RedisCache),
}

pub struct NamedCache {
    name: &'static str,
    storage: CacheStorage,
    metrics: Option<Arc<Metrics>>,
}

impl NamedCache {
    #[must_use]
    pub fn new(
        name: &'static str,
        settings: &CacheSettings,
        metrics: Option<Arc<Metrics>>,
    ) -> Self {
        let storage = match settings {
            CacheSettings::Redis(redis_settings) => {
                CacheStorage::Redis(RedisCache::new(redis_settings.clone()))
            }
            CacheSettings::Memory {
                max_entries,
                time_to_live,
            } => {
                let mut builder = moka::future::Cache::builder();
                if let Some(max_entries) = max_entries {
                    builder = builder.max_capacity(*max_entries);
                }
                if let Some(time_to_live) = time_to_live {
                    builder = builder.time_to_live(*time_to_live);
                }
                CacheStorage::Memory(builder.build())
            }
        };
        NamedCache {
            name,
            storage,
            metrics,
        }
    }

    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    pub async fn get(&self, key: &str) -> Option<Value> {
        let cached = match &self.storage {
            CacheStorage::Memory(cache) => cache.get(key).await,
            CacheStorage::Redis(cache) => cache.get(key).await,
        };
        if let Some(metrics) = &self.metrics {
            metrics.record_cache_lookup(self.name, cached.is_some());
        }
        cached
    }

    pub async fn set(&self, key: &str, value: Value) {
        match &self.storage {
            CacheStorage::Memory(cache) => cache.insert(key.to_string(), value).await,
            CacheStorage::Redis(cache) => cache.set(key, value).await,
        }
    }

    pub async fn delete(&self, key: &str) {
        match &self.storage {
            CacheStorage::Memory(cache) => cache.invalidate(key).await,
            CacheStorage::Redis(cache) => cache.delete(key).await,
        }
    }

    pub async fn keys(&self) -> Vec<String> {
        match &self.storage {
            CacheStorage::Memory(cache) => {
                cache.iter().map(|(key, _)| key.as_ref().clone()).collect()
            }
            CacheStorage::Redis(cache) => cache.keys().await,
        }
    }

    pub async fn get_or_fetch<Error>(
        &self,
        key: &str,
        fetch: impl Future<Output = Result<Value, Error>>,
    ) -> Result<Value, Error> {
        if let Some(cached) = self.get(key).await {
            return Ok(cached);
        }
        let fetched = fetch.await?;
        if !fetched.is_null() {
            self.set(key, fetched.clone()).await;
        }
        Ok(fetched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn memory_settings() -> CacheSettings {
        CacheSettings::Memory {
            max_entries: None,
            time_to_live: None,
        }
    }

    #[test]
    fn valkey_is_an_alias_for_the_redis_protocol_backend() {
        let environment =
            Environment::from_iter([("CACHE_STORAGE", "valkey"), ("CACHE_REDIS_PORT", "6390")]);
        let CacheSettings::Redis(redis_settings) = CacheSettings::from_environment(&environment)
        else {
            panic!("valkey must select the shared backend");
        };
        assert_eq!(redis_settings.port, 6390);
    }

    #[test]
    fn unknown_storage_falls_back_to_memory() {
        let environment = Environment::from_iter([("CACHE_STORAGE", "memcached")]);
        assert!(matches!(
            CacheSettings::from_environment(&environment),
            CacheSettings::Memory { .. }
        ));
    }

    #[test]
    fn own_time_to_live_overrides_the_cache_ttl_for_both_backends() {
        let memory = CacheSettings::Memory {
            max_entries: Some(10),
            time_to_live: Some(Duration::from_secs(5)),
        };
        assert_eq!(
            memory.with_time_to_live(Duration::from_secs(300)),
            CacheSettings::Memory {
                max_entries: Some(10),
                time_to_live: Some(Duration::from_secs(300)),
            }
        );
        let redis =
            CacheSettings::from_environment(&Environment::from_iter([("CACHE_STORAGE", "redis")]));
        let CacheSettings::Redis(redis_settings) =
            redis.with_time_to_live(Duration::from_secs(300))
        else {
            panic!("the backend must not change");
        };
        assert_eq!(redis_settings.ttl_seconds.as_deref(), Some("300"));
    }

    #[tokio::test]
    async fn second_lookup_is_served_from_the_cache_without_fetching() {
        let cache = NamedCache::new("team_members", &memory_settings(), None);
        let fetch_count = AtomicUsize::new(0);
        let fetch = || async {
            fetch_count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, String>(json!(["alice"]))
        };
        assert_eq!(
            cache.get_or_fetch("acme/core", fetch()).await,
            Ok(json!(["alice"]))
        );
        assert_eq!(
            cache.get_or_fetch("acme/core", fetch()).await,
            Ok(json!(["alice"]))
        );
        assert_eq!(fetch_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_fetches_are_not_cached() {
        let cache = NamedCache::new("team_members", &memory_settings(), None);
        let failure = cache
            .get_or_fetch("acme/core", async { Err::<Value, _>("HttpError: boom") })
            .await;
        assert_eq!(failure, Err("HttpError: boom"));
        assert_eq!(cache.get("acme/core").await, None);
    }

    #[tokio::test]
    async fn lookups_are_counted_as_hits_and_misses_per_cache() {
        let metrics = Arc::new(Metrics::new());
        let cache = NamedCache::new(
            "collaborators",
            &memory_settings(),
            Some(Arc::clone(&metrics)),
        );
        cache
            .get_or_fetch("acme/app", async { Ok::<_, String>(json!(["bob"])) })
            .await
            .ok();
        cache.get("acme/app").await;
        let rendered = metrics.render();
        assert!(rendered.contains(r#"cache_lookups_total{cache="collaborators",result="miss"} 1"#));
        assert!(rendered.contains(r#"cache_lookups_total{cache="collaborators",result="hit"} 1"#));
    }
}
