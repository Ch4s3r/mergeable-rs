# mergeable (Rust)

A Rust port of the [mergeable](../README.md) GitHub App. One binary, `mergeable`, serves the GitHub
webhook endpoint and the optional rate-limit and Prometheus routes. It reads the same
`.github/mergeable.yml` / `.github/mergeable.settings.yml` files and the same environment variables
as the Node/Probot app, so an existing GitHub App registration and deployment can switch without changes.

## Build

```sh
cd rust
cargo build --release -p mergeable
./target/release/mergeable
```

Docker (build context is the `rust/` directory):

```sh
docker build -t mergeable-rust rust
docker run --rm -p 3000:3000 \
  -e APP_ID=12345 -e WEBHOOK_SECRET=... -e PRIVATE_KEY="$(base64 < private-key.pem)" \
  mergeable-rust
```

Cloud Native Buildpacks (Paketo, same tooling as the Node image; configuration in `rust/project.toml` and
`rust/Procfile`):

```sh
cd rust
pack build mergeable-rust --path .            # or: --publish to push to a registry
docker run --rm -p 3000:3000 -e APP_ID=... -e WEBHOOK_SECRET=... -e PRIVATE_KEY=... mergeable-rust
```

The builder `paketobuildpacks/ubuntu-noble-builder-buildpackless` is multi-arch, so it builds natively on
Apple Silicon (the jammy builders are amd64-only and rustc crashes under qemu emulation). With Docker's
containerd image store, `pack` cannot export multi-arch buildpacks into the local daemon
(buildpacks/pack#2272); publish to a registry instead, e.g. a local one:
`docker run -d -p 5001:5000 registry:2` and
`pack build localhost:5001/mergeable-rust --path . --publish --insecure-registry localhost:5001 --network host`.

## Run and test

```sh
cargo test -p mergeable
cargo build -p mergeable -p replay
cd .. && rust/target/debug/replay check --bin rust/target/debug/mergeable
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
| `WEBHOOK_SECRET` | Webhook secret (`X-Hub-Signature-256` is verified) | `development` (as Probot) |
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
| `USE_CONFIG_FROM_PULL_REQUEST` | Use the config changed in the pull request (as in Node, the variable cannot turn it off) | on |
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
| `GITHUB_THROTTLE` | `false` disables the Probot-like request throttling (Rust only) | on |
| `GITHUB_RETRY_BASE_MS` | Base delay of the retry back-off for failed GitHub requests (Rust only) | `1000` |

## Routes

- `POST <WEBHOOK_PATH>`: GitHub webhooks. Other methods answer `404 {"error":"Unknown route: ..."}` like
  `@octokit/webhooks`.
- `GET /ping` (`PONG`), `GET /probot` (landing page), `GET /` (redirect to `/probot`).
- `GET /github-ratelimit/:installationId`: `resources.core` of `GET /rate_limit` for that installation, or the
  JSON string `installation error: <message>`.
- `GET /metrics`: `github_ratelimit_remaining_total` / `github_ratelimit_limit_total` gauges (label
  `installationId`, set from the `x-ratelimit-*` headers of every GitHub response), and the
  express-prometheus-middleware metrics `http_requests_total`, `http_request_duration_seconds`,
  `http_request_length_bytes`, `http_response_length_bytes` with labels `route` (ids masked as `#val`),
  `method`, `status` (`2XX`..`5XX`).

## Scheduler

With `MERGEABLE_SCHEDULER=true` the app lists its installations (`GET /app/installations?per_page=100`, as the
App) at start, lists each installation's repositories (`GET /installation/repositories?per_page=100`, one
installation at a time) and, after a random delay within one interval, runs the rules for the event
`schedule.repository` with the payload `{action: "repository", installation, repository}` every interval.
`installation.created` and `installation_repositories.added` webhooks schedule new repositories the same way.

## Differences to the Node app

- Configuration files without `version: 2` (the deprecated version-1 format) are reported with an "Invalid Configuration" check instead of being converted (ADR-003).
- YAML parse error texts come from serde_yaml_ng instead of js-yaml; the surrounding check text is the same.
- `/metrics` is rendered in the OpenMetrics text format (`application/openmetrics-text; version=1.0.0`) by
  `prometheus-client`, not the Prometheus 0.0.4 text format of prom-client. Prometheus scrapes both.
  Node-specific default metrics (`nodejs_*`, most `process_*`) are not exported; only
  `process_start_time_seconds` is.
- As in Node, only requests that no other route handles (unknown paths and `/metrics` itself, which is
  excluded) are counted in the HTTP metrics; webhooks, `/ping`, `/probot` and the rate-limit route are not.
- Redis: like the Node app, the value of `CACHE_REDIS_HOST` is sent as the Redis password
  (`CACHE_REDIS_PASSWORD` from `.env.template` was never read). Like ioredis, a server without a password
  that rejects the `AUTH` is used without one. `keys()` sends `KEYS ""` exactly like
  cache-manager-ioredis called without a pattern, so it finds nothing and deleting the cache of an
  organisation `.github` repository change only removes the exact keys. `CACHE_REDIS_REFRESH_THRESHOLD` has no
  effect (it never had: mergeable does not use `wrap()`).
- Redis errors are logged and treated as a cache miss instead of failing the webhook.
- The App-authenticated requests of the scheduler (`GET /app/installations`) are not throttled or retried.
