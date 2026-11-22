use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::{Histogram, exponential_buckets};
use prometheus_client::registry::Registry;
use regex::Regex;
use reqwest::header::HeaderMap;
use std::sync::atomic::AtomicU64;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const METRICS_PATH: &str = "/metrics";
pub const CONTENT_TYPE: &str = "application/openmetrics-text; version=1.0.0; charset=utf-8";
const PATH_VALUE_PLACEHOLDER: &str = "#val";

static PATH_VALUE_MASKS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"^-?[0-9]+$",
        r"^([0-9]{2}|[0-9]{4})-[0-9][0-9]-[0-9][0-9]$",
        r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        r"^[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}$",
        r"^[0-9a-f]{7,}$",
        r"^[0-9A-F]{7,}$",
        r"^[a-zA-Z0-9\-_]{18,}\.[a-zA-Z0-9\-_]{3,}\.[a-zA-Z0-9\-_]{39,}$",
        r"^[a-zA-Z0-9\-_]{66,}$",
        r"^(?:[A-Za-z0-9+/]{4}){16,}",
    ]
    .iter()
    .filter_map(|pattern| Regex::new(pattern).ok())
    .collect()
});

type InstallationLabels = Vec<(String, String)>;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct HttpRequestLabels {
    pub route: String,
    pub method: String,
    pub status: String,
}

pub fn normalize_path(path: &str) -> String {
    let mut normalized = String::with_capacity(path.len() + 1);
    for chunk in path.split('/').filter(|chunk| !chunk.is_empty()) {
        normalized.push('/');
        if PATH_VALUE_MASKS.iter().any(|mask| mask.is_match(chunk)) {
            normalized.push_str(PATH_VALUE_PLACEHOLDER);
        } else {
            normalized.push_str(chunk);
        }
    }
    if normalized.is_empty() {
        normalized.push('/');
    }
    normalized
}

#[must_use]
pub fn normalize_status_code(status: u16) -> &'static str {
    match status {
        200..=299 => "2XX",
        300..=399 => "3XX",
        400..=499 => "4XX",
        _ => "5XX",
    }
}

fn header_number(headers: &HeaderMap, name: &str) -> f64 {
    crate::github::client::header_text(headers, name).map_or(f64::NAN, crate::scheduler::js_number)
}

pub struct Metrics {
    registry: Mutex<Registry>,
    rate_limit_remaining: Family<InstallationLabels, Gauge<f64, AtomicU64>>,
    rate_limit_limit: Family<InstallationLabels, Gauge<f64, AtomicU64>>,
    http_requests: Family<HttpRequestLabels, Counter>,
    http_request_duration: Family<HttpRequestLabels, Histogram>,
}

fn new_duration_histogram() -> Histogram {
    Histogram::new(exponential_buckets(0.05, 1.75, 8))
}

fn new_length_histogram() -> Histogram {
    Histogram::new(std::iter::empty())
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Registry::default();
        let rate_limit_remaining = Family::<InstallationLabels, Gauge<f64, AtomicU64>>::default();
        let rate_limit_limit = Family::<InstallationLabels, Gauge<f64, AtomicU64>>::default();
        let http_requests = Family::<HttpRequestLabels, Counter>::default();
        let http_request_duration = Family::<HttpRequestLabels, Histogram>::new_with_constructor(
            new_duration_histogram as fn() -> Histogram,
        );
        let http_request_length = Family::<HttpRequestLabels, Histogram>::new_with_constructor(
            new_length_histogram as fn() -> Histogram,
        );
        let http_response_length = Family::<HttpRequestLabels, Histogram>::new_with_constructor(
            new_length_histogram as fn() -> Histogram,
        );
        let process_start_time = Gauge::<f64, AtomicU64>::default();
        process_start_time.set(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs_f64())
                .unwrap_or_default()
                .floor(),
        );
        registry.register(
            "github_ratelimit_remaining_total",
            "The total amount of Rate Limit requests remaining",
            rate_limit_remaining.clone(),
        );
        registry.register(
            "github_ratelimit_limit_total",
            "The total amount of Rate Limit requests limit",
            rate_limit_limit.clone(),
        );
        registry.register(
            "http_requests",
            "Counter for total requests received",
            http_requests.clone(),
        );
        registry.register(
            "http_request_duration_seconds",
            "Duration of HTTP requests in seconds",
            http_request_duration.clone(),
        );
        registry.register(
            "http_request_length_bytes",
            "Content-Length of HTTP request",
            http_request_length,
        );
        registry.register(
            "http_response_length_bytes",
            "Content-Length of HTTP response",
            http_response_length,
        );
        registry.register(
            "process_start_time_seconds",
            "Start time of the process since unix epoch in seconds",
            process_start_time,
        );
        Metrics {
            registry: Mutex::new(registry),
            rate_limit_remaining,
            rate_limit_limit,
            http_requests,
            http_request_duration,
        }
    }

    pub fn record_rate_limit(&self, installation_id: u64, headers: &HeaderMap) {
        let labels: InstallationLabels =
            vec![("installationId".to_string(), installation_id.to_string())];
        self.rate_limit_remaining
            .get_or_create(&labels)
            .set(header_number(headers, "x-ratelimit-remaining"));
        self.rate_limit_limit
            .get_or_create(&labels)
            .set(header_number(headers, "x-ratelimit-limit"));
    }

    pub fn record_http_request(&self, method: &str, path: &str, status: u16, seconds: f64) {
        let route = normalize_path(path);
        if route == METRICS_PATH {
            return;
        }
        let labels = HttpRequestLabels {
            route,
            method: method.to_string(),
            status: normalize_status_code(status).to_string(),
        };
        self.http_requests.get_or_create(&labels).inc();
        self.http_request_duration
            .get_or_create(&labels)
            .observe(seconds);
    }

    pub fn render(&self) -> String {
        let mut output = String::new();
        if let Ok(registry) = self.registry.lock() {
            let _encoding_result = encode(&mut output, &registry);
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_in_request_paths_are_masked_like_url_value_parser() {
        assert_eq!(normalize_path("/nope/123"), "/nope/#val");
        assert_eq!(
            normalize_path("/github-ratelimit/4242"),
            "/github-ratelimit/#val"
        );
        assert_eq!(normalize_path("/a/2024-01-15/b"), "/a/#val/b");
        assert_eq!(
            normalize_path("/x/0b9f3d7e-1c2a-4b3d-9e8f-0123456789ab"),
            "/x/#val"
        );
        assert_eq!(normalize_path("/commit/deadbeef"), "/commit/#val");
        assert_eq!(normalize_path("/commit/DeadBeef"), "/commit/DeadBeef");
        assert_eq!(normalize_path("//probot//"), "/probot");
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn status_codes_are_grouped_by_class() {
        assert_eq!(normalize_status_code(200), "2XX");
        assert_eq!(normalize_status_code(302), "3XX");
        assert_eq!(normalize_status_code(404), "4XX");
        assert_eq!(normalize_status_code(500), "5XX");
        assert_eq!(normalize_status_code(101), "5XX");
    }

    #[test]
    fn fall_through_requests_are_counted_with_route_method_and_status() {
        let metrics = Metrics::new();
        metrics.record_http_request("GET", "/nope/42", 404, 0.01);
        metrics.record_http_request("GET", "/metrics", 200, 0.01);
        let rendered = metrics.render();
        assert!(
            rendered
                .contains(r#"http_requests_total{route="/nope/#val",method="GET",status="4XX"} 1"#)
        );
        assert!(!rendered.contains(r#"route="/metrics""#));
        assert!(rendered.contains(r#"http_request_duration_seconds_bucket{le="0.05",route="/nope/#val",method="GET",status="4XX"} 1"#));
    }

    #[test]
    fn rate_limit_gauges_are_labelled_by_installation_and_missing_headers_give_nan() {
        let metrics = Metrics::new();
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-ratelimit-remaining",
            reqwest::header::HeaderValue::from_static("4999"),
        );
        metrics.record_rate_limit(4242, &headers);
        let rendered = metrics.render();
        assert!(
            rendered.contains(r#"github_ratelimit_remaining_total{installationId="4242"} 4999.0"#)
        );
        assert!(rendered.contains(r#"github_ratelimit_limit_total{installationId="4242"} NaN"#));
    }
}
