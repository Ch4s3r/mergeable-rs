# Module map

| Area | Rust module (rust/app/src) |
|---|---|
| Entry point, environment | `main`, `app_config`, `logging` |
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

Contract tests: rust/app-cases (381 recorded webhook scenarios) replayed by rust/replay (ADR-005).
Gate: `cargo fmt --all --check`, `cargo clippy --all-targets`, `cargo test`, `replay check`.
