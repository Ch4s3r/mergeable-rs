use crate::contract::case::Case;
use crate::contract::mock::MockServer;
use crate::contract::runner;
use crate::contract::webhook::WEBHOOK_SECRET;
use anyhow::{Context as _, Result, bail};
use mergeable::app_config::{AppConfig, Environment, LogFormat};
use mergeable::logging;
use mergeable::server;
use mergeable::telemetry::{Propagator, Telemetry, TelemetryProviders};
use opentelemetry::trace::TraceId;
use opentelemetry_sdk::logs::in_memory_exporter::LogDataWithResource;
use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLoggerProvider};
use opentelemetry_sdk::metrics::data::{ResourceMetrics, ScopeMetrics};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use std::path::PathBuf;

const INSTALLATION_TOKEN: &str = "ghs_test";

struct InMemoryExporters {
    spans: InMemorySpanExporter,
    logs: InMemoryLogExporter,
    metrics: InMemoryMetricExporter,
}

impl InMemoryExporters {
    fn new() -> Self {
        InMemoryExporters {
            spans: InMemorySpanExporter::default(),
            logs: InMemoryLogExporter::default(),
            metrics: InMemoryMetricExporter::default(),
        }
    }

    fn telemetry(&self) -> Telemetry {
        Telemetry::install(
            TelemetryProviders {
                tracer: Some(
                    SdkTracerProvider::builder()
                        .with_simple_exporter(self.spans.clone())
                        .build(),
                ),
                meter: Some(
                    SdkMeterProvider::builder()
                        .with_periodic_exporter(self.metrics.clone())
                        .build(),
                ),
                logger: Some(
                    SdkLoggerProvider::builder()
                        .with_simple_exporter(self.logs.clone())
                        .build(),
                ),
            },
            &[Propagator::TraceContext],
        )
    }
}

struct ExportedTelemetry {
    spans: Vec<SpanData>,
    logs: Vec<LogDataWithResource>,
    metric_names: Vec<String>,
    metric_attribute_values: Vec<String>,
}

impl ExportedTelemetry {
    fn collect(exporters: &InMemoryExporters, telemetry: &Telemetry) -> Result<Self> {
        telemetry.force_flush();
        let finished_metrics = exporters.metrics.get_finished_metrics()?;
        let exported_metrics = finished_metrics
            .iter()
            .flat_map(ResourceMetrics::scope_metrics)
            .flat_map(ScopeMetrics::metrics);
        let mut metric_names = Vec::new();
        let mut metric_attribute_values = Vec::new();
        for metric in exported_metrics {
            metric_names.push(metric.name().to_owned());
            metric_attribute_values.push(format!("{:?}", metric.data()));
        }
        Ok(ExportedTelemetry {
            spans: exporters.spans.get_finished_spans()?,
            logs: exporters.logs.get_emitted_logs()?,
            metric_names,
            metric_attribute_values,
        })
    }

    fn span_named(&self, name: &str) -> Result<&SpanData> {
        self.spans
            .iter()
            .find(|span| span.name == name)
            .with_context(|| format!("no exported span named {name}"))
    }

    fn span_names_in_trace(&self, trace_id: TraceId) -> Vec<&str> {
        self.spans
            .iter()
            .filter(|span| span.span_context.trace_id() == trace_id)
            .map(|span| span.name.as_ref())
            .collect()
    }

    fn log_trace_ids(&self) -> Vec<TraceId> {
        self.logs
            .iter()
            .filter_map(|log| log.record.trace_context())
            .map(|trace_context| trace_context.trace_id)
            .collect()
    }

    fn every_recorded_value(&self) -> Vec<String> {
        let span_values = self.spans.iter().flat_map(|span| {
            span.attributes
                .iter()
                .map(|attribute| attribute.value.to_string())
                .chain(span.events.iter().flat_map(|event| {
                    event
                        .attributes
                        .iter()
                        .map(|attribute| attribute.value.to_string())
                }))
        });
        let log_values = self.logs.iter().flat_map(|log| {
            log.record
                .attributes_iter()
                .map(|(_, value)| format!("{value:?}"))
                .chain(log.record.body().map(|body| format!("{body:?}")))
        });
        span_values
            .chain(log_values)
            .chain(self.metric_attribute_values.iter().cloned())
            .collect()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn webhook_is_exported_as_a_trace_with_correlated_log_records_and_metric_data() -> Result<()>
{
    let case = Case::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cases/action-assign-author.json"),
    )?;
    let exporters = InMemoryExporters::new();
    let telemetry = exporters.telemetry();
    let _subscriber =
        tracing::subscriber::set_default(logging::subscriber("info", LogFormat::Json, &telemetry));
    let mock = MockServer::start().await?;
    let mut environment_variables = runner::in_process_app_environment(mock.port, &case)?;
    environment_variables.insert("OTEL_METRICS_EXPORTER".to_owned(), "otlp".to_owned());
    let private_key = environment_variables
        .get("PRIVATE_KEY")
        .cloned()
        .unwrap_or_default();
    let environment: Environment = environment_variables.into_iter().collect();
    let config = AppConfig::from_environment(&environment).map_err(anyhow::Error::msg)?;
    let webhook_path = config.webhook_path.clone();
    let running_app = server::start(config).await?;
    let case_run = runner::exercise_case(
        &mock,
        running_app.address().port(),
        &webhook_path,
        &case,
        runner::case_quiet_period(&case, runner::DEFAULT_QUIET_PERIOD),
    )
    .await;
    running_app.shutdown().await?;
    mock.stop();
    if case_run?.status != 200 {
        bail!("the webhook was not handled");
    }
    let exported = ExportedTelemetry::collect(&exporters, &telemetry)?;

    let webhook_trace_id = exported.span_named("webhook")?.span_context.trace_id();
    let spans_of_the_webhook = exported.span_names_in_trace(webhook_trace_id);
    for expected_span in ["event", "rule", "validator", "action", "github.request"] {
        assert!(
            spans_of_the_webhook.contains(&expected_span),
            "span {expected_span} is missing from the webhook trace: {spans_of_the_webhook:?}"
        );
    }
    assert!(
        exported.log_trace_ids().contains(&webhook_trace_id),
        "no exported log record carries the trace id of the webhook"
    );
    for expected_metric in ["github.ratelimit.remaining", "github.ratelimit.limit"] {
        assert!(
            exported
                .metric_names
                .iter()
                .any(|name| name == expected_metric),
            "metric {expected_metric} was not exported: {:?}",
            exported.metric_names
        );
    }
    let secrets = [private_key.as_str(), WEBHOOK_SECRET, INSTALLATION_TOKEN];
    for recorded_value in exported.every_recorded_value() {
        for secret in secrets {
            assert!(
                !recorded_value.contains(secret),
                "a secret was exported in {recorded_value}"
            );
        }
    }
    telemetry.shutdown();
    Ok(())
}
