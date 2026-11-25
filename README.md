# mergeable-rs

A Rust implementation of [mergeable](https://github.com/mergeability/mergeable), licensed under AGPL-3.0-only like the
original (it is a port of the original, so it cannot be relicensed without the consent of the upstream copyright
holders; see ADR-014).

**Documentation: [ch4s3r.github.io/mergeable-rs](https://ch4s3r.github.io/mergeable-rs/)** (rules reference, recipes,
deployment, operations). The site is built with Docusaurus from `website/` and deployed to GitHub Pages by
`.github/workflows/docs.yml`; preview it with `devenv shell -- docs-serve`. The [configuration table](#configuration)
below stays the source of truth for the environment variables.

A GitHub App that checks pull requests and issues against rules in `.github/mergeable.yml` and reports the
result as check runs, comments, labels and assignees. One Rust binary, `mergeable`, serves the GitHub webhook
endpoint and the optional rate-limit and Prometheus routes. It reads `.github/mergeable.yml` and
`.github/mergeable.settings.yml` from the repository and is configured through environment variables.

The repository is a single Cargo package, `mergeable` (`src/`): the server, the rule engine and the GitHub client.
The black-box contract tests (recorded webhook scenarios in `tests/cases`) are ordinary Rust integration tests
(`tests/contract_cases.rs`, support code in `tests/contract/`); see [Contract tests](#contract-tests).

Architecture decisions: [DECISIONS.md](DECISIONS.md). Module map: [MAP.md](MAP.md). Terms:
[GLOSSARY.md](GLOSSARY.md). Performance: [BENCHMARK.md](BENCHMARK.md).

## Development

The toolchain comes from [devenv](https://devenv.sh) (`devenv.nix`): Rust, cargo-deny, cargo-edit, cargo-nextest,
git-cliff, pack, redis, valkey and Node.js (for the documentation site in `website/`).

```sh
devenv shell
cargo build --release
./target/release/mergeable
```

## Container image

The image is built with Cloud Native Buildpacks (Paketo Rust buildpack; configuration in `project.toml` and
`Procfile`) through the devenv script `build-image <image> [pack options]`:

```sh
devenv shell -- build-image mergeable-rs
docker run --rm -p 3000:3000 \
  -e APP_ID=12345 -e WEBHOOK_SECRET=... -e PRIVATE_KEY="$(base64 < private-key.pem)" \
  mergeable-rs
```

The builder `paketobuildpacks/ubuntu-noble-builder-buildpackless` is multi-arch, so it builds natively on
Apple Silicon. With Docker's containerd image store, `pack` cannot export multi-arch buildpacks into the local
daemon (buildpacks/pack#2272); publish to a registry instead, e.g. a local one:
`docker run -d -p 5001:5000 registry:2` and
`devenv shell -- build-image localhost:5001/mergeable-rs --publish --insecure-registry localhost:5001 --network host`.

A plain `Dockerfile` (distroless) is kept as an alternative: `docker build -t mergeable-rs .`.

## Releases

Commits follow [Conventional Commits](https://www.conventionalcommits.org). Every push to `main` runs
`.github/workflows/release.yml`:

1. `devenv test` (format, clippy, unit and contract tests).
2. `git cliff --bumped-version` computes the next semantic version from the commits since the last `v*` tag
   (`feat` bumps the minor version, `fix`/`perf`/others the patch version, while below 1.0.0; the first release is
   0.1.0). Without releasable commits nothing is published.
3. `build-image` builds the image with Paketo and publishes `ghcr.io/<owner>/<repo>:<version>` and `:latest`.
4. The tag `v<version>` and a GitHub release with the generated changelog are created.

Pull requests run `.github/workflows/ci.yml` (`devenv test`). `main` is protected: every change goes through a pull
request with a passing `test` check and is squash-merged, so the pull request title becomes the conventional commit.

## Dependency updates

[Renovate](https://docs.renovatebot.com) (`renovate.json`, built from presets) tracks Cargo crates, GitHub Actions, the
Paketo builder and buildpacks in `project.toml`, the Dockerfile images, the npm dependencies of the documentation
site (`website/package.json`), and devenv: the devenv version (CLI in the
workflows and the module input in `devenv.yaml`) and the pinned `devenv-nixpkgs` commit. devenv re-locks
`devenv.lock` when `devenv.yaml` changes. Updates are opened on Mondays before 04:00 UTC; patch, minor, digest and
lock file updates are squash-merged automatically once all checks pass, major updates wait for review. Renovate also
opens PRs for direct dependencies with known vulnerabilities in the [OSV database](https://osv.dev) as soon as a fix
is available; vulnerability-fix PRs always ignore the weekly schedule.

Dry run locally:

```sh
GITHUB_COM_TOKEN="$(gh auth token)" RENOVATE_CONFIG_FILE=renovate.json \
  nix shell nixpkgs#renovate -c renovate --platform=local --dry-run=lookup
```

## Test

```sh
devenv test
```

or step by step inside `devenv shell`:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo deny check                  # licenses, RustSec advisories, duplicate crates, sources (deny.toml)
cargo nextest run                 # unit tests and the 381 recorded webhook cases, 32 in parallel
cargo test --doc
```

`tests/github_data_cache.rs` replays recorded cases twice against one app (and once each against two app replicas
sharing a [Valkey](https://valkey.io) server) and asserts which GitHub calls the cache saves. The Valkey test starts
its own `valkey-server` on a free port when the binary is on `PATH` (it is in `devenv shell`, so `devenv test` runs
it) and is skipped with a message otherwise:

```sh
cargo nextest run --test github_data_cache --no-capture
```

The Redis round trip test needs a local server and is ignored by default:

```sh
redis-server --port 6392 --save '' --daemonize yes
REDIS_TEST_PORT=6392 cargo test live_redis -- --ignored
```

## Dependency licenses

`cargo deny check` (part of `devenv test`, configured in `deny.toml`) accepts only crates available under Apache-2.0,
BSD-3-Clause, BSL-1.0, CDLA-Permissive-2.0, ISC, MIT, Unicode-3.0 or Zlib (all compatible with AGPL-3.0-only), fails on
RustSec advisories (ignored ones are listed with their reason in `deny.toml`) and yanked crates, allows crates.io as
the only source and warns about duplicate crate versions. The licenses we ship are listed on the
[Licenses](https://ch4s3r.github.io/mergeable-rs/contributing/licenses) page, generated by
`devenv shell -- license-inventory` (`website/scripts/license-inventory.mjs`) and regenerated by the docs workflow.

## Contract tests

Each file `tests/cases/<name>.json` is one recorded webhook scenario (ADR-005). `build.rs` generates one test
per case file in `tests/contract_cases.rs` (`action-assign-author` becomes `action_assign_author`); without a
`tests/cases` directory it generates none. Each test starts the mock GitHub API (`tests/contract/`) and the app
in-process (`mergeable::server::start` on port 0) with a fixed test environment (seeded test App key, `GHE_HOST` of the mock) plus the case `env`, sends the signed
webhook and compares the GitHub API calls the app makes and its HTTP response with the recorded expectation. A case
passes when the calls match, also when only the order differs; a case with `known_diff` does not fail on a
difference. A failing case prints the first differing call and a diff of all calls. Run them alone or pick one:

```sh
cargo nextest run --test contract_cases
cargo nextest run --test contract_cases validator_title_must_include_fail
cargo test --test contract_cases            # plain cargo test works too, in one process
```

The same test binary holds the scheduler resync tests (`tests/installation_resync/`), which drive the mock directly:
they change the stubbed installation and repository lists while the app runs and check which repositories the
scheduler keeps running (`cargo nextest run -E 'test(installation_resync)'`).

The 381 cases were recorded from the reference implementation. The former `replay` crate, whose CLI recorded new
cases (`replay record`), ran them against a built binary and benchmarked it (`replay bench`), has been removed;
recording new cases and the benchmark harness are no longer part of the repository.

### Case format

```jsonc
{
  "name": "title-must-include-fail",          // = file name without .json
  "description": "what this case proves",
  "env": { "APP_NAME": "Mergeable" },          // optional extra env vars for the app (this case only)
  "event": "pull_request",                     // X-GitHub-Event header
  "payload": { ... },                          // webhook body; must contain installation.id and repository (owner.login, name, full_name)
  "files": {                                   // sugar: repository files served by GET /repos/{owner}/{repo}/contents/{path}
    ".github/mergeable.yml": "version: 2\nmergeable: ...",      // key "path" = payload repository
    "acme/.github:.github/mergeable.settings.yml": "..."          // key "owner/repo:path" = other repository
  },                                           // a path not listed answers 404
  "github": [                                  // stubbed responses, first match wins
    { "method": "GET", "path": "/repos/acme/app/pulls/1/files", "query": {"page": "2"},   // query optional: subset match
      "status": 200, "headers": {"link": "..."}, "body": [ ... ] }
  ],
  "expected": {                                // recorded from the reference implementation, never written by hand
    "status": 200,                             // webhook HTTP response status
    "calls": [ { "method": "POST", "path": "/repos/acme/app/check-runs", "query": {}, "body": { ... } } ]
  },
  "unordered": false,                          // the recording runs disagreed on call order
  "quiet_ms": 12000,                           // optional: longer quiet window for cases that wait for retries
  "known_diff": null                           // or the reason for an approved divergence from the recording
}
```

### Mock GitHub rules

- Served under `/api/v3` (the app gets `GHE_HOST=127.0.0.1:<port>`, `GHE_PROTOCOL=http`); paths are stored without the
  prefix, percent-decoded (`%2F` -> `/`), query as an object with sorted keys.
- `POST /app/installations/{id}/access_tokens` and `GET /app/installations/...` are answered by the mock (token
  `ghs_test`) and are not recorded. A stub for `GET /app/installations` (the installation list) replaces the mock's
  default answer; the mock counts these list requests.
- Every response carries `x-ratelimit-limit`, `x-ratelimit-remaining` and `x-ratelimit-reset` headers.
- Anything unmatched answers `404 {"message":"Not Found","documentation_url":"https://docs.github.com/rest"}`.
- Recorded bodies: JSON parsed; ISO-8601 timestamps replaced by `"<timestamp>"`.
- Recorded strings: the mock origin `http://127.0.0.1:<port>` is replaced by `http://github.mock`; stack-trace
  lines inside error texts are ignored when comparing.
- A case ends when no request arrived for the quiet window (4 s, or the case's `quiet_ms` if longer) after the
  webhook was answered, or 500 ms after the expected number of calls arrived (cases without `quiet_ms`), at most
  40 s. The window covers the client's retries after 1 s, 4 s and 9 s and the 1 s spacing of writes (ADR-007).

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
| `USE_GITHUB_DATA_CACHE` | `true` caches team members, team memberships, collaborators, repository topics and the organisation default config (see [Caching](#caching)) | off |
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
  `http_response_length_bytes` with labels `route` (ids masked as `#val`), `method`, `status` (`2XX`..`5XX`), and
  `cache_lookups_total` with labels `cache` (`configuration`, `settings`, `topics`, `team_members`,
  `team_memberships`, `collaborators`, `repository_topics`, `organization_config`) and `result` (`hit`, `miss`).

## Caching

Every cache uses the backend chosen by `CACHE_STORAGE`: `memory` keeps it per app instance, `valkey` (recommended)
or `redis` shares it between replicas through one Redis-protocol server (`CACHE_REDIS_HOST`, `CACHE_REDIS_PORT`,
`CACHE_REDIS_DB`). Valkey is open source and protocol compatible, so the same client is used for both.

| Data | Switch | Key | Invalidation |
|---|---|---|---|
| Repository config (`.github/mergeable.yml`, merged) | `USE_CONFIG_CACHE` / `use_config_cache` | `owner/repo` | push changing the config file; `CACHE_TTL` |
| Settings (`.github/mergeable.settings.yml`) | `USE_SETTINGS_CACHE` | `owner/repo/settings` | push adding or modifying the settings file (organisation `.github` push: all `owner/` keys); `CACHE_TTL` |
| Repository topics | `use_config_cache` | `owner/repo/topics` | `CACHE_TTL` |
| Team members, team memberships, collaborators, repository topics | `USE_GITHUB_DATA_CACHE` | `github-data:<cache>:<id>` | `GITHUB_DATA_CACHE_TTL` |
| Organisation default config (`USE_ORG_AS_DEFAULT_CONFIG`) | `USE_GITHUB_DATA_CACHE` | `github-data:organization_config:owner/.github/<file>` | push to `owner/.github` changing the file; `GITHUB_DATA_CACHE_TTL` |
| Installation tokens | always | per process | renewed shortly before they expire |

Run Valkey locally with `devenv shell -- valkey-server --port 6379` and start the app with
`CACHE_STORAGE=valkey USE_GITHUB_DATA_CACHE=true`. Compare `cache_lookups_total` and `github_ratelimit_remaining_total`
on `/metrics` to see the saved requests.

## Scheduler

With `MERGEABLE_SCHEDULER=true` the app lists its installations (`GET /app/installations?per_page=100`, as the
App) at start, lists each installation's repositories (`GET /installation/repositories?per_page=100`, one
installation at a time) and, after a random delay within one interval, runs the rules for the event
`schedule.repository` with the payload `{action: "repository", installation, repository}` every interval.
`installation.created` and `installation_repositories.added` webhooks schedule new repositories the same way;
`installation.deleted` and `installation_repositories.removed` webhooks stop scheduling the installation's
repositories or the removed repositories.

In case a webhook is missed, the scheduler resyncs every `MERGEABLE_INSTALLATION_RESYNC_INTERVAL` seconds (default one
week, first resync one interval after start): it lists the installations and their repositories again, schedules
repositories it does not know yet and stops scheduling repositories that are no longer listed, also those of
uninstalled or ignored installations. Each resync logs `scheduler.resync.completed` with the counts `installations`,
`repositories`, `added` and `removed`. When listing the installations fails, nothing changes; when listing one
installation's repositories fails, that installation's repositories stay scheduled (ADR-012).

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
