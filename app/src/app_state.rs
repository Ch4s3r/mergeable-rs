use crate::app_config::BehaviorSettings;
use crate::cache::ConfigCache;

pub struct AppState {
    pub behavior: BehaviorSettings,
    pub settings_cache: ConfigCache,
    pub configuration_cache: ConfigCache,
    pub topics_cache: ConfigCache,
}

impl AppState {
    #[must_use]
    pub fn new(behavior: BehaviorSettings) -> Self {
        AppState {
            settings_cache: ConfigCache::new(&behavior.cache),
            configuration_cache: ConfigCache::new(&behavior.cache),
            topics_cache: ConfigCache::new(&behavior.cache),
            behavior,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        AppState::new(BehaviorSettings::default())
    }
}
