# mergeable

A GitHub App that checks pull requests and issues against rules in `.github/mergeable.yml` and reports the
result as check runs, comments, labels and assignees. One Rust binary, `mergeable`, serves the GitHub webhook
endpoint and the optional rate-limit and Prometheus routes. It reads `.github/mergeable.yml` and
`.github/mergeable.settings.yml` from the repository and is configured through environment variables.

The workspace has two crates:

- `app` (package `mergeable`): the server, the rule engine and the GitHub client.
- `replay`: black-box contract tests that replay recorded webhook scenarios against the binary behind a mock
  GitHub API (see [app-cases/README.md](app-cases/README.md)).

Architecture decisions: [DECISIONS.md](DECISIONS.md). Module map: [MAP.md](MAP.md). Terms:
[GLOSSARY.md](GLOSSARY.md). Performance: [BENCHMARK.md](BENCHMARK.md).

## Build

```sh
cd rust
cargo build --release -p mergeable
./target/release/mergeable
```

Docker (build context is the `rust/` directory):

```sh
docker build -t mergeable rust
docker run --rm -p 3000:3000 \
  -e APP_ID=12345 -e WEBHOOK_SECRET=... -e PRIVATE_KEY="$(base64 < private-key.pem)" \
  mergeable
```

Cloud Native Buildpacks (Paketo; configuration in `rust/project.toml` and `rust/Procfile`):

```sh
cd rust
pack build mergeable --path .            # or: --publish to push to a registry
docker run --rm -p 3000:3000 -e APP_ID=... -e WEBHOOK_SECRET=... -e PRIVATE_KEY=... mergeable
```

The builder `paketobuildpacks/ubuntu-noble-builder-buildpackless` is multi-arch, so it builds natively on
Apple Silicon (the jammy builders are amd64-only and rustc crashes under qemu emulation). With Docker's
containerd image store, `pack` cannot export multi-arch buildpacks into the local daemon
(buildpacks/pack#2272); publish to a registry instead, e.g. a local one:
`docker run -d -p 5001:5000 registry:2` and
`pack build localhost:5001/mergeable --path . --publish --insecure-registry localhost:5001 --network host`.

## Test

```sh
cd rust
cargo fmt --all --check
cargo clippy --all-targets
cargo test
```

Contract tests (run from the repository root, where the case paths resolve):

```sh
cargo build --release --manifest-path rust/Cargo.toml -p mergeable -p replay
rust/target/release/replay check --bin rust/target/release/mergeable --jobs 8
```

The Redis round trip test needs a local server and is ignored by default:

```sh
redis-server --port 6392 --save '' --daemonize yes
REDIS_TEST_PORT=6392 cargo test -p mergeable live_redis -- --ignored
```

## Configuration

| Variable | Meaning | Default |
|---|---|---|
| `APP_ID` | GitHub App id | required |
| `PRIVATE_KEY` | App private key: PEM text (`\n` escapes allowed) or base64-encoded PEM | - |
| `PRIVATE_KEY_PATH` | Path to the PEM file, used when `PRIVATE_KEY` is unset | - |
| `WEBHOOK_SECRET` | Webhook secret (`X-Hub-Signature-256` is verified) | `development` |
| `WEBHOOK_PATH` | Webhook route | `/api/github/webhooks` |
| `HOST`, `PORT` | Listen address | `0.0.0.0`, `3000` |
| `GHE_HOST`, `GHE_PROTOCOL` | GitHub Enterprise host (API at `<protocol>://<host>/api/v3`) | api.github.com, `https` |
| `APP_NAME` | Name used for checks and the `@bot` annotation | `Mergeable` |
| `NODE_ENV` | `development` sets the scheduler interval to 2 seconds | production |
| `LOG_LEVEL` | `trace`, `debug`, `info`, `warn`, `error`, `fatal` | `info` |
| `LOG_FORMAT` | `pretty` for human-readable logs instead of JSON | JSON |
| `LOG_CONFIG` | Log the loaded configuration | off |
| `CONFIG_PATH`, `SETTINGS_PATH` | Config / settings file name below `.github/` | `mergeable.yml`, `mergeable.settings.yml` |
| `USE_CONFIG_CACHE`, `USE_SETTINGS_CACHE` | Cache config / settings per repository | off |
| `USE_CONFIG_FROM_PULL_REQUEST` | Use the config changed in the pull request (always on; the variable cannot turn it off) | on |
| `USE_ORG_AS_DEFAULT_CONFIG` | Merge the organisation `.github` repository config | off |
| `CACHE_STORAGE` | `memory` or `redis` | `memory` |
| `CACHE_MEMORY_MAX`, `CACHE_TTL` | Memory cache size, entry time to live in seconds (also Redis `SETEX` ttl) | 500, none |
| `CACHE_REDIS_HOST`, `CACHE_REDIS_PORT`, `CACHE_REDIS_DB` | Redis connection | `localhost`, `6379`, `0` |
| `JIRA_HOST`, `JIRA_USERNAME`, `JIRA_PASSWORD`, `JIRA_PROTOCOL`, `JIRA_VERSION`, `JIRA_STRICT_SSL` | Jira option | - |
| `MERGEABLE_SCHEDULER` | `true` emits `schedule.repository` events for every installed repository | off |
| `MERGEABLE_SCHEDULER_INTERVAL` | Scheduler interval in seconds | `3600` |
| `IGNORED_ACCOUNTS` | Comma-separated account logins the scheduler skips (case-insensitive) | - |
| `DISABLE_DELAY` | Any non-empty value disables the random initial scheduler delay | delay on |
| `ENABLE_GITHUB_RATELIMIT_ENDPOINT` | `true` serves `GET /github-ratelimit/:installationId` | off |
| `ENABLE_METRICS_ENDPOINT` | `true` serves `GET /metrics` | off |
| `GITHUB_THROTTLE` | `false` disables the GitHub request throttling (ADR-007) | on |
| `GITHUB_RETRY_BASE_MS` | Base delay of the retry back-off for failed GitHub requests | `1000` |

## Routes

- `POST <WEBHOOK_PATH>`: GitHub webhooks (headers `X-GitHub-Event`, `X-GitHub-Delivery`,
  `X-Hub-Signature-256`). Other methods answer `404 {"error":"Unknown route: ..."}`. A webhook still being
  processed after 9 s is answered `202` and finishes in the background.
- `GET /ping` (`PONG`), `GET /probot` (landing page), `GET /` (redirect to `/probot`).
- `GET /github-ratelimit/:installationId`: `resources.core` of `GET /rate_limit` for that installation, or the
  JSON string `installation error: <message>`.
- `GET /metrics`: `github_ratelimit_remaining_total` / `github_ratelimit_limit_total` gauges (label
  `installationId`, set from the `x-ratelimit-*` headers of every GitHub response), and the HTTP metrics
  `http_requests_total`, `http_request_duration_seconds`, `http_request_length_bytes`,
  `http_response_length_bytes` with labels `route` (ids masked as `#val`), `method`, `status` (`2XX`..`5XX`).

## Scheduler

With `MERGEABLE_SCHEDULER=true` the app lists its installations (`GET /app/installations?per_page=100`, as the
App) at start, lists each installation's repositories (`GET /installation/repositories?per_page=100`, one
installation at a time) and, after a random delay within one interval, runs the rules for the event
`schedule.repository` with the payload `{action: "repository", installation, repository}` every interval.
`installation.created` and `installation_repositories.added` webhooks schedule new repositories the same way.

## Behaviour notes

- A configuration without `version` (the deprecated version-1 format) is converted to version 2 before it is
  evaluated (ADR-003).
- `/metrics` is rendered in the OpenMetrics text format (`application/openmetrics-text; version=1.0.0`) by
  `prometheus-client`. Of the process metrics only `process_start_time_seconds` is exported.
- Only requests that no other route handles (unknown paths and `/metrics` itself, which is excluded) are
  counted in the HTTP metrics; webhooks, `/ping`, `/probot` and the rate-limit route are not.
- Redis: the value of `CACHE_REDIS_HOST` is also sent as the Redis `AUTH` password; a server without a password
  that rejects the `AUTH` is used without one. Listing keys sends `KEYS ""`, so deleting the cache after a
  change in an organisation `.github` repository only removes the exact keys. There is no
  `CACHE_REDIS_PASSWORD` or `CACHE_REDIS_REFRESH_THRESHOLD` setting.
- Redis errors are logged and treated as a cache miss instead of failing the webhook.
- The App-authenticated requests of the scheduler (`GET /app/installations`) are not throttled or retried.
