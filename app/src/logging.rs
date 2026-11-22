pub fn init(log_level: &str, json_format: bool) {
    let level = match log_level {
        "trace" => "trace",
        "debug" => "debug",
        "warn" => "warn",
        "error" | "fatal" => "error",
        _ => "info",
    };
    let filter = tracing_subscriber::EnvFilter::new(format!("mergeable={level},warn"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json_format {
        builder.json().init();
    } else {
        builder.init();
    }
}
