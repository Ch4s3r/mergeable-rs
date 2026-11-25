use crate::app_config::Environment;
use crate::redis_cache::{RedisCache, RedisSettings};
use serde_json::Value;
use std::time::Duration;

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
        if environment.value("CACHE_STORAGE") == Some("redis") {
            return CacheSettings::Redis(RedisSettings::from_environment(environment));
        }
        CacheSettings::Memory {
            max_entries: number_variable(environment, "CACHE_MEMORY_MAX"),
            time_to_live: number_variable(environment, "CACHE_TTL").map(Duration::from_secs),
        }
    }
}

pub enum ConfigCache {
    Memory(moka::future::Cache<String, Value>),
    Redis(RedisCache),
}

impl ConfigCache {
    #[must_use]
    pub fn new(settings: &CacheSettings) -> Self {
        match settings {
            CacheSettings::Redis(redis_settings) => {
                ConfigCache::Redis(RedisCache::new(redis_settings.clone()))
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
                ConfigCache::Memory(builder.build())
            }
        }
    }

    pub async fn get(&self, key: &str) -> Option<Value> {
        match self {
            ConfigCache::Memory(cache) => cache.get(key).await,
            ConfigCache::Redis(cache) => cache.get(key).await,
        }
    }

    pub async fn set(&self, key: &str, value: Value) {
        match self {
            ConfigCache::Memory(cache) => cache.insert(key.to_string(), value).await,
            ConfigCache::Redis(cache) => cache.set(key, value).await,
        }
    }

    pub async fn delete(&self, key: &str) {
        match self {
            ConfigCache::Memory(cache) => cache.invalidate(key).await,
            ConfigCache::Redis(cache) => cache.delete(key).await,
        }
    }

    pub async fn keys(&self) -> Vec<String> {
        match self {
            ConfigCache::Memory(cache) => {
                cache.iter().map(|(key, _)| key.as_ref().clone()).collect()
            }
            ConfigCache::Redis(cache) => cache.keys().await,
        }
    }
}
