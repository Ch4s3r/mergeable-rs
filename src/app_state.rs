use crate::app_config::BehaviorSettings;
use crate::cache::{CacheSettings, NamedCache};
use crate::metrics::Metrics;
use std::sync::Arc;

pub struct GithubDataCaches {
    pub team_members: NamedCache,
    pub team_memberships: NamedCache,
    pub collaborators: NamedCache,
    pub repository_topics: NamedCache,
    pub organization_config: NamedCache,
}

impl GithubDataCaches {
    #[must_use]
    pub fn new(settings: &CacheSettings, metrics: Option<&Arc<Metrics>>) -> Self {
        let cache = |name| NamedCache::new(name, settings, metrics.cloned());
        GithubDataCaches {
            team_members: cache("team_members"),
            team_memberships: cache("team_memberships"),
            collaborators: cache("collaborators"),
            repository_topics: cache("repository_topics"),
            organization_config: cache("organization_config"),
        }
    }
}

pub struct AppState {
    pub behavior: BehaviorSettings,
    pub settings_cache: NamedCache,
    pub configuration_cache: NamedCache,
    pub topics_cache: NamedCache,
    pub github_data: Option<GithubDataCaches>,
}

impl AppState {
    #[must_use]
    pub fn new(behavior: BehaviorSettings) -> Self {
        AppState::with_metrics(behavior, None)
    }

    #[must_use]
    pub fn with_metrics(behavior: BehaviorSettings, metrics: Option<&Arc<Metrics>>) -> Self {
        let cache = |name| NamedCache::new(name, &behavior.cache, metrics.cloned());
        AppState {
            settings_cache: cache("settings"),
            configuration_cache: cache("configuration"),
            topics_cache: cache("topics"),
            github_data: behavior.github_data_cache_ttl.map(|time_to_live| {
                GithubDataCaches::new(&behavior.cache.with_time_to_live(time_to_live), metrics)
            }),
            behavior,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        AppState::new(BehaviorSettings::default())
    }
}
