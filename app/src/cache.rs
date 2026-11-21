use serde_json::Value;
use std::time::Duration;

pub enum ConfigCache {
    Memory(moka::future::Cache<String, Value>),
    Redis(crate::redis_cache::RedisCache),
}

pub fn env_number(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
}

impl ConfigCache {
    pub fn from_env() -> Self {
        if std::env::var("CACHE_STORAGE").as_deref() == Ok("redis") {
            return ConfigCache::Redis(crate::redis_cache::RedisCache::from_env());
        }
        let mut builder = moka::future::Cache::builder();
        if let Some(max_entries) = env_number("CACHE_MEMORY_MAX") {
            builder = builder.max_capacity(max_entries);
        }
        if let Some(ttl_seconds) = env_number("CACHE_TTL") {
            builder = builder.time_to_live(Duration::from_secs(ttl_seconds));
        }
        ConfigCache::Memory(builder.build())
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
