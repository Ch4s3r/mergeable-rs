use crate::app_config::LogFormat;

fn crate_level(log_level: &str) -> &'static str {
    match log_level {
        "trace" => "trace",
        "debug" => "debug",
        "warn" => "warn",
        "error" | "fatal" => "error",
        _ => "info",
    }
}

pub fn init(log_level: &str, format: LogFormat) {
    let filter =
        tracing_subscriber::EnvFilter::new(format!("mergeable={},warn", crate_level(log_level)));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    match format {
        LogFormat::Json => builder.json().init(),
        LogFormat::Pretty => builder.init(),
    }
}
