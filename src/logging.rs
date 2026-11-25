use crate::app_config::LogFormat;
use crate::telemetry::Telemetry;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use tracing::Subscriber;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

const EXPORTER_INTERNAL_TARGETS: [&str; 11] = [
    "opentelemetry",
    "opentelemetry_sdk",
    "opentelemetry_otlp",
    "opentelemetry_http",
    "tonic",
    "tower",
    "h2",
    "hyper",
    "hyper_util",
    "reqwest",
    "rustls",
];

fn crate_level(log_level: &str) -> &'static str {
    match log_level {
        "trace" => "trace",
        "debug" => "debug",
        "warn" => "warn",
        "error" | "fatal" => "error",
        _ => "info",
    }
}

fn stdout_filter(log_level: &str) -> EnvFilter {
    EnvFilter::new(format!("mergeable={},warn", crate_level(log_level)))
}

fn exported_log_filter(log_level: &str) -> EnvFilter {
    let exporter_internals_off: Vec<String> = EXPORTER_INTERNAL_TARGETS
        .iter()
        .map(|target| format!("{target}=off"))
        .collect();
    EnvFilter::new(format!(
        "mergeable={},warn,{}",
        crate_level(log_level),
        exporter_internals_off.join(",")
    ))
}

fn exported_span_filter() -> Targets {
    Targets::new().with_target("mergeable", LevelFilter::INFO)
}

#[must_use]
pub fn subscriber(
    log_level: &str,
    format: LogFormat,
    telemetry: &Telemetry,
) -> impl Subscriber + Send + Sync + 'static {
    let stdout_layer = match format {
        LogFormat::Json => tracing_subscriber::fmt::layer().json().boxed(),
        LogFormat::Pretty => tracing_subscriber::fmt::layer().boxed(),
    }
    .with_filter(stdout_filter(log_level));
    let span_export_layer = telemetry.tracer().map(|tracer| {
        tracing_opentelemetry::layer()
            .with_tracer(tracer)
            .with_filter(exported_span_filter())
    });
    let log_export_layer = telemetry.logger_provider().map(|logger_provider| {
        OpenTelemetryTracingBridge::new(logger_provider).with_filter(exported_log_filter(log_level))
    });
    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(span_export_layer)
        .with(log_export_layer)
}

pub fn init(log_level: &str, format: LogFormat, telemetry: &Telemetry) {
    subscriber(log_level, format, telemetry).init();
}
