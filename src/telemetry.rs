use crate::app_config::Environment;
use crate::metrics::normalize_path;
use axum::http::HeaderMap;
use axum::http::header::{HeaderName, HeaderValue};
use opentelemetry::propagation::{Extractor, Injector, TextMapCompositePropagator};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{Key, KeyValue, global};
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub const DEFAULT_SERVICE_NAME: &str = "mergeable";
pub const INSTRUMENTATION_SCOPE: &str = "mergeable";
const UNDETECTED_SERVICE_NAME: &str = "unknown_service";
const SERVICE_NAME_KEY: &str = "service.name";
const SERVICE_VERSION_KEY: &str = "service.version";
const GENERIC_ENDPOINT_VARIABLE: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Traces,
    Metrics,
    Logs,
}

impl Signal {
    pub const ALL: [Signal; 3] = [Signal::Traces, Signal::Metrics, Signal::Logs];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Signal::Traces => "traces",
            Signal::Metrics => "metrics",
            Signal::Logs => "logs",
        }
    }

    fn exporter_variable(self) -> &'static str {
        match self {
            Signal::Traces => "OTEL_TRACES_EXPORTER",
            Signal::Metrics => "OTEL_METRICS_EXPORTER",
            Signal::Logs => "OTEL_LOGS_EXPORTER",
        }
    }

    fn endpoint_variable(self) -> &'static str {
        match self {
            Signal::Traces => "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
            Signal::Metrics => "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
            Signal::Logs => "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Propagator {
    TraceContext,
    Baggage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetrySettings {
    pub exported_signals: Vec<Signal>,
    pub propagators: Vec<Propagator>,
    pub ignored_values: Vec<String>,
}

impl Default for TelemetrySettings {
    fn default() -> Self {
        TelemetrySettings {
            exported_signals: Vec::new(),
            propagators: vec![Propagator::TraceContext, Propagator::Baggage],
            ignored_values: Vec::new(),
        }
    }
}

fn listed_values(list: &str) -> impl Iterator<Item = &str> {
    list.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

impl TelemetrySettings {
    #[must_use]
    pub fn from_environment(environment: &Environment) -> Self {
        let mut ignored_values = Vec::new();
        let sdk_disabled = environment
            .value("OTEL_SDK_DISABLED")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"));
        let exported_signals = Signal::ALL
            .into_iter()
            .filter(|signal| is_exported(*signal, environment, &mut ignored_values))
            .filter(|_| !sdk_disabled)
            .collect();
        let propagators = propagators(environment, &mut ignored_values);
        TelemetrySettings {
            exported_signals,
            propagators,
            ignored_values,
        }
    }

    #[must_use]
    pub fn exports(&self, signal: Signal) -> bool {
        self.exported_signals.contains(&signal)
    }

    pub fn log_summary(&self) {
        for ignored_value in &self.ignored_values {
            tracing::warn!(%ignored_value, "telemetry.setting.ignored");
        }
        if !self.exported_signals.is_empty() {
            let signals: Vec<&str> = self
                .exported_signals
                .iter()
                .map(|signal| signal.name())
                .collect();
            tracing::info!(signals = %signals.join(","), "telemetry.exporting");
        }
    }
}

fn is_exported(
    signal: Signal,
    environment: &Environment,
    ignored_values: &mut Vec<String>,
) -> bool {
    let Some(exporters) = environment.non_empty(signal.exporter_variable()) else {
        return [GENERIC_ENDPOINT_VARIABLE, signal.endpoint_variable()]
            .into_iter()
            .any(|variable| environment.non_empty(variable).is_some());
    };
    let mut otlp_requested = false;
    let mut none_requested = false;
    for exporter in listed_values(exporters) {
        match exporter {
            "otlp" => otlp_requested = true,
            "none" => none_requested = true,
            unsupported => {
                ignored_values.push(format!("{}={unsupported}", signal.exporter_variable()));
            }
        }
    }
    otlp_requested && !none_requested
}

fn propagators(environment: &Environment, ignored_values: &mut Vec<String>) -> Vec<Propagator> {
    let Some(configured) = environment.non_empty("OTEL_PROPAGATORS") else {
        return TelemetrySettings::default().propagators;
    };
    let mut selected = Vec::new();
    for name in listed_values(configured) {
        let propagator = match name {
            "tracecontext" => Propagator::TraceContext,
            "baggage" => Propagator::Baggage,
            "none" => return Vec::new(),
            unsupported => {
                ignored_values.push(format!("OTEL_PROPAGATORS={unsupported}"));
                continue;
            }
        };
        if !selected.contains(&propagator) {
            selected.push(propagator);
        }
    }
    selected
}

#[must_use]
pub fn service_resource() -> Resource {
    let detected = Resource::builder().build();
    let detected_value = |key: &'static str| {
        detected
            .get(&Key::from_static_str(key))
            .map(|value| value.to_string())
    };
    let mut builder = Resource::builder();
    if detected_value(SERVICE_NAME_KEY).is_none_or(|name| name.starts_with(UNDETECTED_SERVICE_NAME))
    {
        builder = builder.with_service_name(DEFAULT_SERVICE_NAME);
    }
    if detected_value(SERVICE_VERSION_KEY).is_none() {
        builder = builder.with_attribute(KeyValue::new(
            SERVICE_VERSION_KEY,
            env!("CARGO_PKG_VERSION"),
        ));
    }
    builder.build()
}

#[derive(Default)]
pub struct TelemetryProviders {
    pub tracer: Option<SdkTracerProvider>,
    pub meter: Option<SdkMeterProvider>,
    pub logger: Option<SdkLoggerProvider>,
}

#[derive(Default)]
pub struct Telemetry {
    providers: TelemetryProviders,
}

impl Telemetry {
    #[must_use]
    pub fn disabled() -> Self {
        Telemetry::default()
    }

    pub fn start(settings: &TelemetrySettings) -> Result<Self, String> {
        let resource = service_resource();
        let build_error = |signal: Signal| {
            move |error: opentelemetry_otlp::ExporterBuildError| {
                format!("cannot build the OTLP {} exporter: {error}", signal.name())
            }
        };
        let tracer = settings
            .exports(Signal::Traces)
            .then(|| SpanExporter::builder().build())
            .transpose()
            .map_err(build_error(Signal::Traces))?
            .map(|exporter| {
                SdkTracerProvider::builder()
                    .with_batch_exporter(exporter)
                    .with_resource(resource.clone())
                    .build()
            });
        let meter = settings
            .exports(Signal::Metrics)
            .then(|| MetricExporter::builder().build())
            .transpose()
            .map_err(build_error(Signal::Metrics))?
            .map(|exporter| {
                SdkMeterProvider::builder()
                    .with_periodic_exporter(exporter)
                    .with_resource(resource.clone())
                    .build()
            });
        let logger = settings
            .exports(Signal::Logs)
            .then(|| LogExporter::builder().build())
            .transpose()
            .map_err(build_error(Signal::Logs))?
            .map(|exporter| {
                SdkLoggerProvider::builder()
                    .with_batch_exporter(exporter)
                    .with_resource(resource)
                    .build()
            });
        Ok(Telemetry::install(
            TelemetryProviders {
                tracer,
                meter,
                logger,
            },
            &settings.propagators,
        ))
    }

    #[must_use]
    pub fn install(providers: TelemetryProviders, propagators: &[Propagator]) -> Self {
        if let Some(tracer_provider) = &providers.tracer {
            global::set_tracer_provider(tracer_provider.clone());
            global::set_text_map_propagator(composite_propagator(propagators));
        }
        if let Some(meter_provider) = &providers.meter {
            global::set_meter_provider(meter_provider.clone());
        }
        Telemetry { providers }
    }

    #[must_use]
    pub fn is_exporting(&self) -> bool {
        self.providers.tracer.is_some()
            || self.providers.meter.is_some()
            || self.providers.logger.is_some()
    }

    #[must_use]
    pub fn tracer(&self) -> Option<SdkTracer> {
        self.providers
            .tracer
            .as_ref()
            .map(|provider| provider.tracer(INSTRUMENTATION_SCOPE))
    }

    #[must_use]
    pub fn logger_provider(&self) -> Option<&SdkLoggerProvider> {
        self.providers.logger.as_ref()
    }

    pub fn force_flush(&self) {
        self.for_each_provider(
            SdkTracerProvider::force_flush,
            SdkMeterProvider::force_flush,
            SdkLoggerProvider::force_flush,
            "telemetry.flush.failed",
        );
    }

    pub fn shutdown(&self) {
        self.for_each_provider(
            SdkTracerProvider::shutdown,
            SdkMeterProvider::shutdown,
            SdkLoggerProvider::shutdown,
            "telemetry.shutdown.failed",
        );
    }

    fn for_each_provider(
        &self,
        on_tracer: fn(&SdkTracerProvider) -> OTelSdkResult,
        on_meter: fn(&SdkMeterProvider) -> OTelSdkResult,
        on_logger: fn(&SdkLoggerProvider) -> OTelSdkResult,
        failure_message: &'static str,
    ) {
        let outcomes = [
            (
                Signal::Traces,
                self.providers.tracer.as_ref().map(on_tracer),
            ),
            (Signal::Metrics, self.providers.meter.as_ref().map(on_meter)),
            (Signal::Logs, self.providers.logger.as_ref().map(on_logger)),
        ];
        for (signal, outcome) in outcomes {
            if let Some(Err(error)) = outcome {
                tracing::warn!(signal = signal.name(), %error, "{}", failure_message);
            }
        }
    }
}

fn composite_propagator(propagators: &[Propagator]) -> TextMapCompositePropagator {
    TextMapCompositePropagator::new(
        propagators
            .iter()
            .map(|propagator| -> Box<dyn opentelemetry::propagation::TextMapPropagator + Send + Sync> {
                match propagator {
                    Propagator::TraceContext => Box::new(TraceContextPropagator::new()),
                    Propagator::Baggage => Box::new(BaggagePropagator::new()),
                }
            })
            .collect(),
    )
}

struct IncomingHeaders<'headers>(&'headers HeaderMap);

impl Extractor for IncomingHeaders<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(HeaderName::as_str).collect()
    }
}

struct OutgoingHeaders<'headers>(&'headers mut HeaderMap);

impl Injector for OutgoingHeaders<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            self.0.insert(name, value);
        }
    }
}

pub fn continue_incoming_trace(span: &Span, headers: &HeaderMap) {
    let incoming_context =
        global::get_text_map_propagator(|propagator| propagator.extract(&IncomingHeaders(headers)));
    let _ = span.set_parent(incoming_context);
}

pub fn propagate_trace(span: &Span, headers: &mut HeaderMap) {
    let span_context = span.context();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&span_context, &mut OutgoingHeaders(headers));
    });
}

#[must_use]
pub fn github_request_span(method: &reqwest::Method, path: &str) -> Span {
    tracing::info_span!(
        "github.request",
        otel.kind = "client",
        http.request.method = %method,
        url.template = %normalize_path(path),
        http.response.status_code = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    )
}

pub fn record_response_status(span: &Span, status: Option<u16>, failed: bool) {
    if let Some(status) = status {
        span.record("http.response.status_code", status);
    }
    if failed {
        span.record("otel.status_code", "ERROR");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(variables: &[(&str, &str)]) -> TelemetrySettings {
        TelemetrySettings::from_environment(&variables.iter().copied().collect())
    }

    #[test]
    fn nothing_is_exported_without_otel_variables() {
        let unconfigured = settings(&[]);
        assert_eq!(unconfigured.exported_signals, Vec::<Signal>::new());
        assert_eq!(unconfigured.ignored_values, Vec::<String>::new());
        let telemetry = Telemetry::start(&unconfigured);
        assert!(telemetry.is_ok_and(|telemetry| !telemetry.is_exporting()
            && telemetry.tracer().is_none()
            && telemetry.logger_provider().is_none()));
    }

    #[test]
    fn an_otlp_endpoint_exports_traces_metrics_and_logs() {
        let configured = settings(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318")]);
        assert_eq!(configured.exported_signals, Signal::ALL.to_vec());
    }

    #[test]
    fn sdk_disabled_wins_over_an_endpoint_and_explicit_exporters() {
        let disabled = settings(&[
            ("OTEL_SDK_DISABLED", "TRUE"),
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
            ("OTEL_TRACES_EXPORTER", "otlp"),
        ]);
        assert_eq!(disabled.exported_signals, Vec::<Signal>::new());
    }

    #[test]
    fn exporter_none_disables_only_that_signal() {
        let without_logs = settings(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
            ("OTEL_LOGS_EXPORTER", "none"),
        ]);
        assert_eq!(
            without_logs.exported_signals,
            vec![Signal::Traces, Signal::Metrics]
        );
    }

    #[test]
    fn a_signal_endpoint_or_otlp_exporter_enables_only_that_signal() {
        let traces_endpoint = settings(&[(
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
            "http://collector:4318/v1/traces",
        )]);
        assert_eq!(traces_endpoint.exported_signals, vec![Signal::Traces]);
        let metrics_exporter = settings(&[("OTEL_METRICS_EXPORTER", "otlp")]);
        assert_eq!(metrics_exporter.exported_signals, vec![Signal::Metrics]);
    }

    #[test]
    fn unsupported_exporters_are_reported_and_export_nothing() {
        let console = settings(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
            ("OTEL_TRACES_EXPORTER", "console"),
        ]);
        assert_eq!(
            console.exported_signals,
            vec![Signal::Metrics, Signal::Logs]
        );
        assert_eq!(console.ignored_values, vec!["OTEL_TRACES_EXPORTER=console"]);
    }

    #[test]
    fn propagators_default_to_tracecontext_and_baggage_and_none_disables_them() {
        assert_eq!(
            settings(&[]).propagators,
            vec![Propagator::TraceContext, Propagator::Baggage]
        );
        assert_eq!(
            settings(&[("OTEL_PROPAGATORS", "none")]).propagators,
            Vec::<Propagator>::new()
        );
        let only_tracecontext = settings(&[("OTEL_PROPAGATORS", "tracecontext, b3")]);
        assert_eq!(
            only_tracecontext.propagators,
            vec![Propagator::TraceContext]
        );
        assert_eq!(
            only_tracecontext.ignored_values,
            vec!["OTEL_PROPAGATORS=b3"]
        );
    }

    #[test]
    fn service_is_named_mergeable_unless_otel_service_name_is_set() {
        let resource = service_resource();
        let attribute = |key: &'static str| {
            resource
                .get(&Key::from_static_str(key))
                .map(|value| value.to_string())
        };
        let expected_name = std::env::var("OTEL_SERVICE_NAME")
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| DEFAULT_SERVICE_NAME.to_string());
        assert_eq!(attribute(SERVICE_NAME_KEY), Some(expected_name));
        assert!(attribute(SERVICE_VERSION_KEY).is_some());
    }
}
