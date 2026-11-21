use serde_json::Value;

pub fn info(name: &str, message: impl FnOnce() -> Value) {
    if tracing::enabled!(tracing::Level::INFO) {
        tracing::info!(name, message = %crate::js::stringify(&message()));
    }
}

pub fn debug(name: &str, message: impl FnOnce() -> Value) {
    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!(name, message = %crate::js::stringify(&message()));
    }
}

pub fn warn(name: &str, message: impl FnOnce() -> Value) {
    if tracing::enabled!(tracing::Level::WARN) {
        tracing::warn!(name, message = %crate::js::stringify(&message()));
    }
}

pub fn error(name: &str, message: impl FnOnce() -> Value) {
    if tracing::enabled!(tracing::Level::ERROR) {
        tracing::error!(name, message = %crate::js::stringify(&message()));
    }
}

pub fn init(log_level: &str, json_format: bool) {
    let level = match log_level {
        "trace" => "trace",
        "debug" => "debug",
        "warn" => "warn",
        "error" | "fatal" => "error",
        _ => "info",
    };
    let filter = tracing_subscriber::EnvFilter::new(format!("mergeable={level},warn"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);
    if json_format {
        builder.json().init();
    } else {
        builder.init();
    }
}
