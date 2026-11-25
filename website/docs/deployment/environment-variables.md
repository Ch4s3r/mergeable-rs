---
title: Environment variables
---

# Environment variables

mergeable-rs is configured through environment variables. The names and meanings are those of the original
implementation, including `NODE_ENV`. The source of truth is the configuration table in the repository's
[README](https://github.com/Ch4s3r/mergeable-rs#configuration); this page mirrors it.

| Variable | Meaning | Default |
|---|---|---|
| `APP_ID` | GitHub App id | required |
| `PRIVATE_KEY` | App private key: PEM text (`\n` escapes allowed) or base64-encoded PEM | - |
| `PRIVATE_KEY_PATH` | Path to the PEM file, used when `PRIVATE_KEY` is unset | - |
| `WEBHOOK_SECRET` | Webhook secret (the HMAC-SHA256 `X-Hub-Signature-256` is verified) | required; `development` with `NODE_ENV=development` |
| `WEBHOOK_PATH` | Webhook route | `/api/github/webhooks` |
| `HOST`, `PORT` | Listen address | `0.0.0.0`, `3000` |
| `GHE_HOST`, `GHE_PROTOCOL` | GitHub Enterprise host (API at `<protocol>://<host>/api/v3`) | api.github.com, `https` |
| `APP_NAME` | Name used for checks and the `@bot` annotation | `Mergeable` |
| `NODE_ENV` | `development` sets the scheduler interval to 2 seconds and allows a missing `WEBHOOK_SECRET` | production |
| `LOG_LEVEL` | `trace`, `debug`, `info`, `warn`, `error`, `fatal` | `info` |
| `LOG_FORMAT` | `pretty` for human-readable logs instead of JSON | JSON |
| `LOG_CONFIG` | Log the loaded configuration | off |
| `CONFIG_PATH`, `SETTINGS_PATH` | Config / settings file name below `.github/` | `mergeable.yml`, `mergeable.settings.yml` |
| `USE_CONFIG_CACHE`, `USE_SETTINGS_CACHE` | Cache config / settings per repository | off |
| `USE_CONFIG_FROM_PULL_REQUEST` | Use the config changed in the pull request (always on; the variable cannot turn it off) | on |
| `USE_ORG_AS_DEFAULT_CONFIG` | Merge the organisation `.github` repository config | off |
| `USE_GITHUB_DATA_CACHE` | `true` caches team members, team memberships, collaborators, repository topics and the organisation default config (see [Caching](../operations/caching.md)) | off |
| `GITHUB_DATA_CACHE_TTL` | Time to live in seconds of the GitHub data cache entries | `300` |
| `CACHE_STORAGE` | `memory`, or `valkey` / `redis` for a shared Redis-protocol server (both use the `CACHE_REDIS_*` settings) | `memory` |
| `CACHE_MEMORY_MAX`, `CACHE_TTL` | Memory cache size, entry time to live in seconds (also Redis `SETEX` ttl) | 500, none |
| `CACHE_REDIS_HOST`, `CACHE_REDIS_PORT`, `CACHE_REDIS_DB` | Redis connection | `localhost`, `6379`, `0` |
| `JIRA_HOST`, `JIRA_USERNAME`, `JIRA_PASSWORD`, `JIRA_PROTOCOL`, `JIRA_VERSION`, `JIRA_STRICT_SSL` | Jira option | - |
| `MERGEABLE_SCHEDULER` | `true` emits `schedule.repository` events for every installed repository | off |
| `MERGEABLE_SCHEDULER_INTERVAL` | Scheduler interval in seconds | `3600` |
| `MERGEABLE_INSTALLATION_RESYNC_INTERVAL` | Seconds between two scheduler resyncs of the installations and their repositories (fractions allowed; zero, negative or invalid values use the default) | `604800` (one week) |
| `IGNORED_ACCOUNTS` | Comma-separated account logins the scheduler skips (case-insensitive) | - |
| `DISABLE_DELAY` | Any non-empty value disables the random initial scheduler delay | delay on |
| `ENABLE_GITHUB_RATELIMIT_ENDPOINT` | `true` serves `GET /github-ratelimit/:installationId` | off |
| `ENABLE_METRICS_ENDPOINT` | `true` serves `GET /metrics` | off |
| `GITHUB_THROTTLE` | `false` disables the GitHub request throttling (ADR-007) | on |
| `GITHUB_RETRY_BASE_MS` | Base delay of the retry back-off for failed GitHub requests | `1000` |
| `MAX_CONCURRENT_WEBHOOKS` | Webhooks processed at the same time; a further webhook waits for a free slot and is answered `503` if none frees up within 9 s (zero or invalid values use the default) | `64` |
| `OUTGOING_HTTP_CONNECT_TIMEOUT`, `OUTGOING_HTTP_TIMEOUT` | Connect timeout and whole-request timeout in seconds of the requests to GitHub and Jira (zero or invalid values use the default) | `10`, `60` |

A variable that is set but empty counts as unset. Switches are on only for the exact value `true`, except
`LOG_CONFIG` and `DISABLE_DELAY` (any non-empty value) and `GITHUB_THROTTLE` (on unless it is `false`).

The [settings file](../configuration/settings-file.md) `mergeable.settings.yml` can override `CONFIG_PATH`,
`USE_CONFIG_CACHE` and `USE_ORG_AS_DEFAULT_CONFIG` per organisation or repository.
