use std::time::Duration;

pub struct AppConfig {
    pub app_id: String,
    pub private_key_pem: String,
    pub webhook_secret: String,
    pub webhook_path: String,
    pub host: String,
    pub port: u16,
    pub github_base_url: String,
    pub log_level: String,
    pub log_json: bool,
    pub development_mode: bool,
    pub scheduler_enabled: bool,
    pub scheduler_interval_seconds_text: String,
    pub scheduler_interval_milliseconds: f64,
    pub ratelimit_endpoint_enabled: bool,
    pub metrics_endpoint_enabled: bool,
    pub github_throttle_enabled: bool,
    pub github_retry_base_delay: Duration,
}

fn variable(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn read_private_key() -> Result<String, String> {
    if let Some(key) = variable("PRIVATE_KEY") {
        if key.contains("-----BEGIN") {
            return Ok(key.replace("\\n", "\n"));
        }
        use base64::Engine as _;
        return base64::engine::general_purpose::STANDARD
            .decode(key.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .filter(|decoded| decoded.contains("-----BEGIN"))
            .ok_or_else(|| "PRIVATE_KEY is neither a PEM key nor base64-encoded PEM".to_string());
    }
    if let Some(path) = variable("PRIVATE_KEY_PATH") {
        return std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read PRIVATE_KEY_PATH: {error}"));
    }
    Err("missing PRIVATE_KEY or PRIVATE_KEY_PATH".to_string())
}

impl AppConfig {
    pub fn from_env() -> Result<Self, String> {
        let github_base_url = match variable("GHE_HOST") {
            Some(host) => format!(
                "{}://{host}/api/v3",
                variable("GHE_PROTOCOL").unwrap_or_else(|| "https".to_string())
            ),
            None => "https://api.github.com".to_string(),
        };
        let development_mode = variable("NODE_ENV").as_deref() == Some("development");
        let scheduler_interval_seconds_text = if development_mode {
            "2".to_string()
        } else {
            variable("MERGEABLE_SCHEDULER_INTERVAL").unwrap_or_else(|| "3600".to_string())
        };
        let scheduler_interval_milliseconds =
            crate::scheduler::js_number(&scheduler_interval_seconds_text) * 1000.0;
        Ok(AppConfig {
            app_id: variable("APP_ID").ok_or("missing APP_ID")?,
            private_key_pem: read_private_key()?,
            webhook_secret: variable("WEBHOOK_SECRET").unwrap_or_else(|| "development".to_string()),
            webhook_path: variable("WEBHOOK_PATH")
                .unwrap_or_else(|| "/api/github/webhooks".to_string()),
            host: variable("HOST").unwrap_or_else(|| "0.0.0.0".to_string()),
            port: variable("PORT")
                .and_then(|port| port.parse().ok())
                .unwrap_or(3000),
            github_base_url,
            log_level: variable("LOG_LEVEL").unwrap_or_else(|| "info".to_string()),
            log_json: variable("LOG_FORMAT").as_deref() != Some("pretty"),
            development_mode,
            scheduler_enabled: variable("MERGEABLE_SCHEDULER").as_deref() == Some("true"),
            scheduler_interval_milliseconds,
            scheduler_interval_seconds_text,
            ratelimit_endpoint_enabled: variable("ENABLE_GITHUB_RATELIMIT_ENDPOINT").as_deref()
                == Some("true"),
            metrics_endpoint_enabled: variable("ENABLE_METRICS_ENDPOINT").as_deref()
                == Some("true"),
            github_throttle_enabled: variable("GITHUB_THROTTLE").as_deref() != Some("false"),
            github_retry_base_delay: Duration::from_millis(
                variable("GITHUB_RETRY_BASE_MS")
                    .and_then(|millis| millis.parse().ok())
                    .unwrap_or(1000),
            ),
        })
    }
}
