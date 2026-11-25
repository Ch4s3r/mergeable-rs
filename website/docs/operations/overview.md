---
title: Operations
---

# Operations

This page covers running mergeable-rs in production: routes, health checks, metrics, logs, OpenTelemetry export and
the GitHub rate limit.
See also [Caching](./caching.md) (including scaling to several replicas) and [Scheduler](./scheduler.md).

## Routes

- `POST <WEBHOOK_PATH>` (default `/api/github/webhooks`): GitHub webhooks (headers `X-GitHub-Event`,
  `X-GitHub-Delivery`, `X-Hub-Signature-256`). Other methods answer `404 {"error":"Unknown route: ..."}`. A webhook
  still being processed after 9 s is answered `202` and finishes in the background. At most
  `MAX_CONCURRENT_WEBHOOKS` (64) webhooks are processed at once; a further one waits for a free slot and is
  answered `503 {"error":"Too many webhooks are being processed, try again later"}` if none frees up within 9 s.
  GitHub does not retry failed deliveries on its own: redeliver them from the GitHub App's advanced settings.
- `GET /ping`: answers `PONG`. Use it for liveness and readiness probes.
- `GET /probot` (landing page) and `GET /` (redirect to `/probot`), kept from the original.
- `GET /github-ratelimit/:installationId` with `ENABLE_GITHUB_RATELIMIT_ENDPOINT=true`: `resources.core` of GitHub's
  `GET /rate_limit` for that installation, or the JSON string `installation error: <message>`; ids that are not numbers
  are rejected without calling GitHub.
- `GET /metrics` with `ENABLE_METRICS_ENDPOINT=true`: Prometheus metrics, see below.

Both optional routes are unauthenticated; expose them only on an internal network, see [Security](./security.md).

## Health

`GET /ping` answers `200 PONG` as soon as the server listens. It does not call GitHub or the cache.

## Metrics

With `ENABLE_METRICS_ENDPOINT=true`, `GET /metrics` serves the metrics in the OpenMetrics text format
(`application/openmetrics-text; version=1.0.0`):

| Metric | Labels | Meaning |
|---|---|---|
| `github_ratelimit_remaining_total`, `github_ratelimit_limit_total` | `installationId` | gauges set from the `x-ratelimit-*` headers of every GitHub response |
| `http_requests_total`, `http_request_duration_seconds`, `http_request_length_bytes`, `http_response_length_bytes` | `route` (ids masked as `#val`), `method`, `status` (`2XX`..`5XX`) | HTTP metrics |
| `cache_lookups_total` | `cache`, `result` (`hit`, `miss`) | cache lookups, see [Caching](./caching.md#metrics) |
| `process_start_time_seconds` | | the only process metric exported |

:::note

Only requests that no other route handles (unknown paths and `/metrics` itself, which is excluded) are counted in the
HTTP metrics; webhooks, `/ping`, `/probot` and the rate-limit route are not.

:::

## Logs

Logs go to standard output as structured JSON, one event per line, with named fields (for example
`scheduler.resync.completed` with the fields `installations`, `repositories`, `added` and `removed`). Set
`LOG_FORMAT=pretty` for human-readable lines during development.

`LOG_LEVEL` sets the level of the app's own logs: `trace`, `debug`, `info` (default), `warn`, `error` or `fatal`
(same as `error`). Logs of libraries are shown from `warn`. `LOG_CONFIG` (any non-empty value) logs the loaded
configuration.

## OpenTelemetry

mergeable-rs exports traces, metrics and logs over OTLP to an OpenTelemetry Collector or any OTLP backend. It is
configured only through the standard `OTEL_*` environment variables of the OpenTelemetry SDK; without any of them
nothing is exported and stdout logs and `/metrics` behave as before.

| Variable | Meaning | Default |
|---|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Collector endpoint for all signals; setting it turns on the export of traces, metrics and logs | off |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`, `..._METRICS_ENDPOINT`, `..._LOGS_ENDPOINT` | Endpoint of one signal; setting it turns on the export of that signal | - |
| `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER`, `OTEL_LOGS_EXPORTER` | `otlp` turns the signal on (default endpoint `http://localhost:4318`, `http://localhost:4317` for gRPC), `none` turns it off; other exporters are not supported and logged as ignored | `otlp` when an endpoint is set |
| `OTEL_SDK_DISABLED` | `true` turns every export off | `false` |
| `OTEL_EXPORTER_OTLP_PROTOCOL` and `OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_PROTOCOL` | `http/protobuf` or `grpc` | `http/protobuf` |
| `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_EXPORTER_OTLP_TIMEOUT`, `OTEL_EXPORTER_OTLP_INSECURE` and their per-signal variants | Headers (for example an API key; never recorded in telemetry), timeout in milliseconds, plaintext gRPC (`true`) | -, `10000`, `false` |
| `OTEL_SERVICE_NAME` | `service.name` resource attribute | `mergeable` |
| `OTEL_RESOURCE_ATTRIBUTES` | Extra resource attributes (`key=value,...`) | - |
| `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG` | Sampler, for example `parentbased_traceidratio` with `0.1` | `parentbased_always_on` |
| `OTEL_PROPAGATORS` | `tracecontext`, `baggage` or `none` (comma-separated) | `tracecontext,baggage` |
| `OTEL_BSP_*`, `OTEL_BLRP_*` | Batch span / log record processor (schedule delay, queue and batch size, export timeout) | SDK defaults |
| `OTEL_METRIC_EXPORT_INTERVAL`, `OTEL_METRIC_EXPORT_TIMEOUT` | Metric export interval and timeout in milliseconds | `60000`, SDK default |

- **Traces**: a `webhook` server span per delivery (continuing an incoming W3C `traceparent`), the `event` span of
  the workflow, `rule.filter` and `rule` spans per rule with `filter`, `validator` and `action` children, and a
  `github.request` client span per GitHub API call (method, templated path, status). The trace context is sent to
  GitHub as `traceparent`.
- **Logs**: every log event that goes to stdout is also exported, with the trace and span id of the span it was logged in.
- **Metrics**: the metrics of [Metrics](#metrics) are also exported (`/metrics` keeps working): `github.ratelimit.remaining`,
  `github.ratelimit.limit` (attribute `github.installation.id`), `http.server.request.duration` (`http.route`,
  `http.request.method`, `http.response.status_code`) and `cache.lookups` (`cache.name`, `cache.result`).

Secrets (the private key, the webhook secret, installation tokens, the OTLP headers) are never recorded as span or log
attributes. Buffered telemetry is flushed when the process stops on `SIGTERM` or `SIGINT`. Example for a Collector
in the same pod:

```shell
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
OTEL_RESOURCE_ATTRIBUTES=deployment.environment.name=production
```

## GitHub rate limit and throttling

The GitHub client spaces write requests 1 s apart per process (2 s for `/search`, 3 s for notification-triggering
writes on github.com), allows at most 10 concurrent requests, and retries failed requests (any status `>= 400` except
400, 401, 403, 404, 422 and 451) three times after 1 s, 4 s and 9 s. `GITHUB_THROTTLE=false` disables the throttle
and `GITHUB_RETRY_BASE_MS` changes the retry base delay. Requests to GitHub and Jira give up after 10 s without a
connection (`OUTGOING_HTTP_CONNECT_TIMEOUT`) and after 60 s per request (`OUTGOING_HTTP_TIMEOUT`); a timed-out GitHub
request counts as a failed request with status 500 and is retried like one. The App-authenticated requests of the scheduler
(`GET /app/installations`) are not throttled or retried.

Watch `github_ratelimit_remaining_total` on `/metrics`, or ask `/github-ratelimit/:installationId` for one
installation. The [GitHub data cache](./caching.md) saves requests that repeat across webhooks.
