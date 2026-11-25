# Module map

| Area | Rust module (app/src) |
|---|---|
| Entry point, environment snapshot, server startup | `main`, `app_config` (`Environment`), `server::start`, `logging` |
| Per-instance state (behaviour settings, caches) | `app_state` |
| HTTP routes, webhook verification, metrics | `server`, `metrics` |
| Event context | `context`, `event_aware` |
| GitHub client: requests, pagination, throttle, App auth | `github` |
| Repository config loading (`_extends`, organisation `.github` fallback) | `repository_config` |
| Configuration and settings files | `configuration` (incl. `v1_config`), `settings` |
| Rule engine | `flex`, `interceptors`, `registry` |
| Filters, validators and their options | `filters`, `validators` |
| Actions and annotations | `actions`, `annotations` |
| Validation results | `results` |
| Templates, globs, YAML | `template`, `glob`, `yaml` |
| JSON value helpers, value lists | `json_value`, `value_lists` |
| Caches | `cache`, `redis_cache` |
| Scheduler | `scheduler` |
| Errors | `error` |

Contract tests: tests/cases (381 recorded webhook scenarios), one generated Rust test each in
`app/tests/contract_cases.rs` (app/build.rs), run in-process with the mock GitHub API in `app/tests/contract/` (ADR-005).
Gate: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo nextest run --workspace`,
`cargo test --doc --workspace`.
