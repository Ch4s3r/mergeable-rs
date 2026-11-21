# Decisions

## ADR-001: Port as a standalone HTTP server
Status: accepted (2026-11-21)
Context: mergeable is a Probot GitHub App (webhook receiver + optional /github-ratelimit/:installationId and /metrics routes); rust-anything analyze called it a library because it has no main.
Decision: One Rust binary `mergeable` (crate rust/app) built on axum + tokio that serves the webhook endpoint and the optional routes.
Consequences: No napi bridge, no shadows; the Node app stays only as reference.

## ADR-002: Thin GitHub REST client and own webhook/auth instead of Probot
Status: accepted (2026-11-21)
Context: Probot provides webhook HMAC verification, App JWT, installation tokens, an Octokit client and context.config() (with _extends and .github-repo fallback). Mergeable reads raw JSON shapes of ~30 REST endpoints.
Decision: reqwest (rustls) GitHub client returning serde_json::Value, jsonwebtoken (RS256) for the App JWT, hmac+sha2 for X-Hub-Signature-256, own port of probot config loading. octocrab was considered (Q2) but its typed models drop fields mergeable templates read; a Value client mirrors Octokit 1:1.
Consequences: All GitHub calls go through one client (auth, Link-header pagination, Octokit-style error text 'HttpError: <message>').

## ADR-003: Scope: every feature, v1 config format dropped
Status: accepted (2026-11-21)
Context: v1 config (version-less mergeable.yml) is deprecated since 2.0.
Decision: Port everything: v2 config, settings file, all validators/filters/actions/options, Jira option, scheduler, memory+Redis cache, Prometheus metrics, rate-limit endpoint. A config without `version: 2` gets a configuration-error check.
Consequences: The v1 transformer and its tests are not ported.

## ADR-004: Config, settings and env vars stay compatible
Status: accepted (2026-11-21)
Context: Existing installations must switch without changes.
Decision: Same .github/mergeable.yml and .github/mergeable.settings.yml semantics and defaults; same env var names (APP_ID, PRIVATE_KEY, PRIVATE_KEY_PATH, WEBHOOK_SECRET, WEBHOOK_PATH, PORT, HOST, GHE_HOST, GHE_PROTOCOL, APP_NAME, NODE_ENV, LOG_LEVEL, LOG_CONFIG, CONFIG_PATH, SETTINGS_PATH, USE_CONFIG_*, USE_ORG_AS_DEFAULT_CONFIG, USE_SETTINGS_CACHE, CACHE_*, JIRA_*, MERGEABLE_SCHEDULER*, IGNORED_ACCOUNTS, DISABLE_DELAY, ENABLE_GITHUB_RATELIMIT_ENDPOINT, ENABLE_METRICS_ENDPOINT).
Consequences: Drop-in for the Docker image / Helm chart.

## ADR-005: Equivalence bar: same GitHub API calls
Status: accepted (2026-11-21)
Context: The observable behaviour is the GitHub API traffic produced for a webhook.
Decision: A case = webhook in + stubbed GitHub responses. It passes when the Rust app makes the same calls (method, path, query, JSON body; key order ignored) in the same order and answers the webhook with the same status. Comment/check text matches byte for byte. ISO timestamps are normalised. Logs are not compared.
Consequences: Texts that embed JS-runtime internals (TypeError text, stack traces, 'Cannot find module' paths) are normalised per case and listed as known diffs.

## ADR-006: Dynamic JSON value model, faithful first
Status: accepted (2026-11-21)
Context: Validators/filters/actions pass loosely typed JS objects (settings, payloads, results) into handlebars templates.
Decision: Settings, payloads and results are serde_json::Value (preserve_order). JS semantics (truthiness, String(x), typeof) live in module `js`. Registries map the `do:` name to a trait object (replaces the computed require()).
Consequences: Typed structs only in a later reshape pass.

## ADR-007: Crates
Status: accepted (2026-11-21)
Context: Dependency research in rust/research/.
Decision: tokio, axum, reqwest(rustls), serde_json(preserve_order), serde_yaml_ng, handlebars (helpers ported), regex + fancy-regex (lookbehind), globset (minimatch), chrono + chrono-tz, moka (memory cache), redis (Redis cache), prometheus-client, jsonwebtoken, hmac, sha2, tracing.
Consequences: rustls/jsonwebtoken pull ring (asm/C crypto): no equally mature pure-Rust alternative, accepted.

## ADR-008: No per-wave stops; one final report with benchmark
Status: accepted (2026-11-21)
Context: User instruction.
Decision: Port all waves in one go; the only checkpoint is the final report: replay results, gate (fmt, clippy -D warnings, tests), benchmark Node vs Rust.
Consequences: HANDOFF.md is written at the end.

## ADR-009: Whole engine ported directly, not through the front-door proxy
Status: accepted (2026-11-21)
Context: All units are classes with async GitHub calls (analyze: 0 bridgeable exports) and the app has one webhook route, so route-level fallback cannot move units one at a time.
Decision: Build rust/app as the full engine; prove it with app-level replay of recorded webhook cases (rust/app-cases, harness rust/replay) plus unit tests ported from the JS suite.
Consequences: Per-unit bridge/record/diff are not used; replay is the gate.

## ADR-010: Reference runtime Node 22 via devenv
Status: accepted (2026-11-21)
Context: nixpkgs removed Node 20 (EOL); devenv is the project environment (user instruction).
Decision: Record the original on Node 22 inside devenv (devenv.nix: nodejs_22, rust, cargo-edit, redis). JS suite: 643/643 pass on Node 22.
Consequences: Recorded cases reflect Node 22 behaviour.

## ADR-011: Keep Probot request throttling, retries and the 9 s webhook timeout
Status: accepted (2026-11-21)
Context: Probot's octokit spaces all write requests 1 s apart process-wide (plugin-throttling, id 'no-id'), 2 s for /search, 3 s for notification-triggering writes on github.com, max 10 concurrent requests, and retries any status >= 400 except 400/401/403/404/422/451 three times after 1 s, 4 s, 9 s. The webhook answers 202 'still processing' after 9 s. These protect installations from GitHub secondary rate limits.
Decision: The Rust client reproduces the throttle lanes, the retry schedule and the 202 timeout by default. GITHUB_THROTTLE=false disables the throttle and GITHUB_RETRY_BASE_MS changes the retry base delay (new, optional env vars).
Consequences: Node and Rust have the same GitHub-facing pacing, so throughput under write-heavy load is bounded by GitHub's rules in both; the benchmark therefore reports throttled and unthrottled numbers separately.

## ADR-012: Known differences and how replay compares runtime texts
Status: accepted (2026-11-21)
Context: Some recorded texts embed details of the Node runtime: V8 stack frames ("    at ..."), Node's "Require stack:" list of absolute paths, and js-yaml's parser messages. Some cases depend on retry timing around the quiet window.
Decision: replay ignores JS stack frames and Require-stack lists when comparing (the error message before them is still compared exactly). A case may set "quiet_ms" when Probot's retry schedule (1 s, 4 s, 9 s) straddles the default 4 s quiet window. Remaining differences are marked per case with "known_diff": config-invalid-yaml-in-pr (YAML parser message text) and config-version-1-old-format (ADR-003).
Consequences: Every other recorded text, including TypeError messages, must match byte for byte.
