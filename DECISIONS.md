# Architecture decisions

## ADR-001: One standalone HTTP server binary
Status: accepted
Context: mergeable receives GitHub webhooks and optionally serves /github-ratelimit/:installationId and /metrics.
Decision: One binary `mergeable` (package `mergeable`) built on axum and tokio serves the webhook endpoint and the
optional routes. The library target exposes the engine so tests and the binary share it.
Consequences: Deployment is a single executable in a distroless image or a buildpack image.

## ADR-002: Thin GitHub REST client, own webhook verification and App authentication
Status: accepted
Context: The app needs webhook HMAC verification, the App JWT, installation tokens, ~30 REST endpoints and
repository config loading with `_extends` and the organisation `.github` repository fallback. Templates read
arbitrary fields of GitHub responses.
Decision: A reqwest (rustls) client returning `serde_json::Value`; jsonwebtoken (RS256, `rust_crypto` backend) for the App JWT;
hmac + sha2 for `X-Hub-Signature-256`; config loading in `repository_config`. octocrab was rejected because its
typed models drop fields that templates read.
Consequences: All GitHub calls go through one client (authentication, Link-header pagination, error text
`HttpError: <message>` as it appears in check output).

## ADR-003: Full feature scope, including version-less configurations
Status: accepted
Context: Configurations without `version` (version-1 format) are deprecated but still in use.
Decision: Support version-1 and version-2 configurations, the settings file, all validators, filters, actions and
options, the Jira option, the scheduler, memory and Redis caches, Prometheus metrics and the rate-limit endpoint.
A configuration without `version` is converted to version 2 by `configuration::v1_config`.
Consequences: Existing configurations keep working without edits.

## ADR-004: Stable configuration files and environment interface
Status: accepted
Context: Installations are configured through repository files and environment variables; both are part of the
deployment contract (Docker image, Helm chart).
Decision: `.github/mergeable.yml` and `.github/mergeable.settings.yml` keep their semantics and defaults; the
environment variables listed in README.md (including `NODE_ENV`) keep their names and meaning; the routes
`/ping`, `/probot`, `/` and the webhook path keep their responses.
Consequences: Changing any of these is a breaking change and needs a new recorded case.

## ADR-005: Recorded webhook scenarios are the contract
Status: accepted
Context: The observable behaviour of the app is the GitHub API traffic it produces for a webhook and the HTTP
response it gives.
Decision: A case is a webhook plus stubbed GitHub responses plus the expected calls and status
(tests/cases). Every case is a generated Rust test (`tests/contract_cases.rs`, `cargo nextest run`)
that starts the server in-process behind the mock GitHub API of the test support code (`tests/contract/`); it passes when the app
makes the same calls (method, path, query, JSON body) and answers with the same status; a differing call
order alone is reported but passes. The app reads its environment once into an immutable `Environment`
snapshot, and caches are per server instance, so cases with different env run side by side in one process.
Comment and check texts match byte for byte; ISO timestamps and the mock origin are normalised; logs are not
compared. Expectations were recorded from the reference implementation, never written by hand. The former
`replay` crate, which recorded cases, ran them against a built binary and benchmarked it, was removed; the
contract test support code lives in the integration tests.
Consequences: Internals can be refactored freely; a change to the contract shows up as a failing case and is
either fixed or re-recorded deliberately. The quiet window of a case (`quiet_ms`) covers the retry schedule.

## ADR-006: Dynamic JSON values at the template boundary
Status: accepted
Context: Settings, webhook payloads and validation results flow into user-written templates that may reference
any field.
Decision: Payloads, settings and results are `serde_json::Value` with `preserve_order`, so request bodies keep
the configured key order. Truthiness, string conversion and type tests over values live in `json_value`.
The registry maps each `do:` name to a trait object (`registry::Registry`).
Consequences: Typed structs are introduced where a shape is fixed; free-form data stays `Value`.

## ADR-007: Request throttling, retries and the 9 s webhook timeout
Status: accepted
Context: GitHub enforces secondary rate limits on bursts of writes.
Decision: The client spaces write requests 1 s apart process-wide, 2 s for /search and 3 s for
notification-triggering writes on github.com, allows at most 10 concurrent requests, and retries any status
>= 400 except 400/401/403/404/422/451 three times after 1 s, 4 s and 9 s. A webhook still running after 9 s is
answered 202 and completes in the background. `GITHUB_THROTTLE=false` disables the throttle and
`GITHUB_RETRY_BASE_MS` changes the retry base delay.
Consequences: Throughput under write-heavy load is bounded by GitHub's pacing; the benchmark reports throttled
and unthrottled numbers separately.

## ADR-008: Crates
Status: accepted
Decision: tokio, axum, reqwest (rustls), async-trait, serde_json (`preserve_order`), an own YAML loader (`yaml`) whose
parse errors are part of the check output, an own template interpreter (`template`), regex + fancy-regex
(lookbehind in user patterns), an own glob matcher (`glob`), chrono + chrono-tz, moka (memory cache and
compiled pattern cache), redis, prometheus-client, jsonwebtoken, hmac, sha1, sha2, tracing + tracing-subscriber.
Consequences: the rustls stack pulls in aws-lc-rs and ring (assembly/C crypto) for TLS; no equally mature
pure-Rust TLS provider exists, so this is accepted.

## ADR-009: Golden corpora for the template, glob and YAML engines
Status: accepted
Context: The template language, glob matching and YAML parsing have many edge cases whose output is visible in
check texts.
Decision: tests/fixtures holds fixed golden data (templates.json, globs.json, yaml.json) asserted by
the test suite.
Consequences: A behaviour change in these engines fails a test before it reaches a recorded case.

## ADR-010: Release profile
Status: accepted
Decision: `lto = true`, `codegen-units = 1`, `panic = "unwind"`; no `target-cpu=native` (see BENCHMARK.md).
Consequences: A panic fails one webhook instead of aborting the server; the binary runs on any CPU of the
target architecture.

## ADR-011: Lints
Status: accepted
Decision: `clippy::pedantic` at warn crate-wide (`[lints.clippy]` in Cargo.toml) (`missing_errors_doc`, `missing_panics_doc` allowed);
deliberate exceptions use scoped `#[expect(lint, reason = "...")]`, never `#[allow]`.
Consequences: New warnings are fixed or justified at the site.
