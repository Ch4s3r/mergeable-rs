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

## ADR-012: Periodic resync of the scheduled installations and repositories
Status: accepted
Context: With `MERGEABLE_SCHEDULER=true` the scheduler listed installations and repositories only at start and then
relied on the `installation` and `installation_repositories` webhooks. A missed webhook left a new repository
unscheduled, and removed repositories or uninstalled installations stayed scheduled until a restart.
Decision: The scheduler also handles `installation.deleted` and `installation_repositories.removed`, and every
`MERGEABLE_INSTALLATION_RESYNC_INTERVAL` seconds (default 604800, one week) lists all installations and their repositories again
with the same requests as at start. Unknown repositories are scheduled, scheduled repositories that were not listed
are unscheduled (their timer task is aborted). The resync is conservative: a failed installation list changes
nothing, a failed repository list keeps that installation's repositories, and a repository scheduled by a webhook
while the resync was listing is kept (every scheduling gets a sequence number; only repositories scheduled before
the resync started can be removed). The scheduled payload of a repository that stays is not refreshed.
Consequences: Missed webhooks heal within one resync interval at the cost of one installation list plus one
repository list per installation per interval. Without `MERGEABLE_SCHEDULER` nothing changes, so the recorded
cases are unaffected.

## ADR-013: One cache layer for GitHub data, Valkey as the shared backend
Status: accepted
Context: Every webhook fetches the same GitHub data again (config and settings files, the organisation `.github`
config, team memberships, collaborators, topics), which costs rate limit and latency. Replicas cannot share an
in-memory cache. The recorded cases (ADR-005) fix the calls the app makes with the default environment.
Decision: All caches are `cache::NamedCache` instances on the backend of `CACHE_STORAGE`: `memory` (moka, per
instance) or a Redis-protocol server, where `valkey` is an alias of `redis` and the documented, tested shared
backend (open source, protocol compatible with the existing `redis` client). Every lookup counts a hit or miss in
`cache_lookups_total{cache,result}` when metrics are enabled. Inventory:

| Data | Switch | Key | Invalidation |
|---|---|---|---|
| Merged repository config | `use_config_cache` | `owner/repo` | push changing the config file (organisation `.github` push: all `owner/` keys); `CACHE_TTL` |
| Settings file | `USE_SETTINGS_CACHE` | `owner/repo/settings` | push adding or modifying the settings file (organisation `.github` push: all `owner/` keys); `CACHE_TTL` |
| Repository topics (existing) | `use_config_cache` | `owner/repo/topics` | `CACHE_TTL` |
| Team members (`GET /orgs/{org}/teams/{slug}/members`) | `USE_GITHUB_DATA_CACHE` | `github-data:team_members:org/slug` | `GITHUB_DATA_CACHE_TTL` (300 s) |
| Team membership (`GET .../memberships/{user}`) | `USE_GITHUB_DATA_CACHE` | `github-data:team_memberships:org/slug/user` | `GITHUB_DATA_CACHE_TTL` |
| Collaborators (`GET /repos/{owner}/{repo}/collaborators`) | `USE_GITHUB_DATA_CACHE` | `github-data:collaborators:owner/repo` | `GITHUB_DATA_CACHE_TTL` |
| Repository topics (`GET /repos/{owner}/{repo}/topics`) | `USE_GITHUB_DATA_CACHE` | `github-data:repository_topics:owner/repo` | `GITHUB_DATA_CACHE_TTL` |
| Organisation default config (`USE_ORG_AS_DEFAULT_CONFIG`) | `USE_GITHUB_DATA_CACHE` | `github-data:organization_config:owner/.github/<file>` | push to `owner/.github` adding or modifying the file; `GITHUB_DATA_CACHE_TTL` |
| Installation tokens | always | per process (`GithubApp`) | renewed before `expires_at` |

Only successful responses are cached; errors (404 team, 5xx) are fetched again. The GitHub data cache is off by
default, so the recorded cases are unaffected; with it, a cold cache still reproduces the recordings
(`tests/github_data_cache.rs`). Installation tokens stay per process: they are credentials and would otherwise be
stored in the shared server. Pull-request files, reviews, comments and check runs change with every event and are
not cached. Conditional requests (`If-None-Match`/ETag, a 304 does not count against the rate limit) are a
follow-up: they need the ETag stored next to the cached content and the mock to answer 304.
Consequences: Repeated events on the same repository save the cached reads (measured by the integration tests and
`cache_lookups_total`); team, collaborator and topic changes are seen after at most `GITHUB_DATA_CACHE_TTL`.
Several replicas share one cache through Valkey (`CACHE_STORAGE=valkey`).

## ADR-014: An empty approvals limit does not filter approvals
Status: accepted
Context: With `limit` set, the `approvals` validator only counts approvals from the allowed set (`limit.teams`
members, `limit.users`, CODEOWNERS of `limit.owners`). When that set is empty, for example `limit.owners: true`
in a repository without a CODEOWNERS file, every approval was dropped and `min.count` failed, while
`required.owners` already passes without a CODEOWNERS file. Upstream changed this in
Ch4s3r/mergeable@528691515800307ac14685d05d68c1b4c63507ca.
Decision: Follow upstream: `approvals::only_allowed_reviewers` narrows the approvals down to the allowed set only
when the set is not empty; the info output "Only approvals from following sources are counted" stays.
Consequences: A `limit` that resolves to nobody behaves like no `limit`. A team that does not exist or has no
approving member no longer blocks approvals on its own. No recorded case (ADR-005) covers an empty allowed set;
the upstream test "returns proper error when team provided is not found" mocks a non-approving team member that
the membership lookup cannot return, so its Rust port asserts the lifted limit instead.

## ADR-015: Security fixes from Renovate, Dependabot only for alerts
Status: accepted
Context: Renovate already keeps the dependencies up to date and raises fix PRs for OSV vulnerabilities. Dependabot
security updates would open a second, differently formatted PR for the same vulnerability.
Decision: Dependabot alerts are enabled for the repository, Dependabot security updates stay disabled. Renovate reads
the Dependabot alerts (the Mend Renovate app holds `read` on Dependabot alerts) next to `osvVulnerabilityAlerts` and
opens the fix PRs. `vulnerabilityAlerts` labels them `security` and enables automerge, so a fix (also a major
update) is squash-merged as soon as all required checks pass.
Consequences: One PR per vulnerability, merged without waiting for review or the weekly schedule; the required
checks are the only gate. A wrong alert could produce a bad automerged fix, which the release changelog makes
visible.

## ADR-016: Untrusted input limits and a mandatory webhook secret
Status: accepted
Context: Repository configuration (`.github/mergeable.yml`, also from same-repository pull requests) and pull request
data are untrusted. The security review (#20) found that a template nested about 20 000 levels deep overflowed the
stack and aborted the whole process, that a glob brace range such as `{1..100000000}` or a long `{a,b}` product
expanded without bound (a range ending at `i64::MAX` never terminated), that a production server without
`WEBHOOK_SECRET` accepted webhooks signed with the public default `development`, that `X-Hub-Signature-256` also
accepted SHA-1 digests, and that path segments from callers reached GitHub (`/github-ratelimit/:installationId`) and
Jira (ticket id) URLs unencoded.
Decision: The template parser rejects blocks, `else if` chains and subexpressions nested more than 100 levels deep
with `RangeError: Maximum call stack size exceeded` (the error the YAML loader already uses); a brace expansion stops
at 100 000 patterns, like the `max` of the JavaScript `brace-expansion`; `WEBHOOK_SECRET` is required unless
`NODE_ENV=development`; only `sha256=` signatures are verified; the rate-limit route accepts numeric installation ids
only and the Jira ticket id is percent-encoded as one path segment (`.` and `..` are never valid tickets).
Consequences: The recorded cases are unchanged (none comes near the limits, all are signed with SHA-256 and set
`WEBHOOK_SECRET`). A deployment without `WEBHOOK_SECRET` no longer starts and must set it. Larger follow-ups (a
separate Redis password and TLS, bounded concurrent webhook processing and outbound timeouts) are tracked as
separate issues labelled `security`.

## ADR-017: License stays AGPL-3.0-only
Status: accepted
Context: Issue #17 asked whether mergeable-rs can move to MIT OR Apache-2.0. mergeable-rs was ported from
mergeability/mergeable: modules translated from the original source, error and message texts reproduced verbatim,
template and YAML behaviour reproduced in detail, the documentation converted from the original docs and the
upstream unit tests ported. A translation into another language is a derivative work, so the upstream license
applies (AGPL-3.0 sections 5 and 10). Upstream was ISC, then BSD-3-Clause (April 2018), and has been AGPL-3.0-only
since January 2019 (upstream #134). Its history has 55 author names (one of them a bot); 54 of them committed after
the switch to AGPL, 36 of them to `lib/`, and the five most active authors wrote 557 of the 727 commits since then.
Upstream has no contributor license agreement, so every author keeps the copyright in their contribution. Only the
behaviour and the interface (configuration format, webhook responses, the recorded cases) are not protected; a
different license would need a clean-room reimplementation by people who have not read the original source.
Decision: mergeable-rs stays AGPL-3.0-only (`LICENSE`, `Cargo.toml` `license`, the image label `BP_OCI_LICENSES`).
A relicensing needs the written consent of all upstream copyright holders after January 2019: ask the upstream
maintainers in a mergeability/mergeable discussion, then each author with a commit since upstream #134 in a
tracking issue with a checklist (consent to "MIT OR Apache-2.0" for their contributions, given as an issue comment
or a signed-off statement); contributions of authors who decline or cannot be reached are rewritten clean-room
before the switch. The decision whether to start this process stays with the owner (issue #17, label
`needs-decision`). This is not legal advice.
Consequences: Dependencies must be compatible with AGPL-3.0-only (ADR-018). Anyone running a modified mergeable-rs
as a network service must offer its source to its users.

## ADR-018: Dependency policy checked by cargo-deny
Status: accepted
Context: The licenses of the crates we ship must be compatible with AGPL-3.0-only (ADR-017), and known
vulnerable, unmaintained or yanked crates should not slip in through lock file updates.
Decision: `cargo deny check` with `deny.toml` runs in `devenv test`, so in the required `test` check of every pull
request and in the release workflow. Licenses: an allowlist of the licenses the dependency tree uses that are
compatible with AGPL-3.0-only (Apache-2.0, BSD-3-Clause, BSL-1.0, CDLA-Permissive-2.0, ISC, MIT, Unicode-3.0, Zlib);
for an `OR` expression one allowed alternative is enough; our own unpublished crate is not checked. Advisories:
the RustSec database (fetched from GitHub), yanked crates denied; an ignored advisory needs a reason and a follow-up
issue in `deny.toml` (chrono and chrono-tz, issue #24). Bans: duplicate versions warn, wildcard requirements are
denied. Sources: crates.io only. The license inventory page (`website/docs/contributing/licenses.md`) is generated
by `website/scripts/license-inventory.mjs` from `cargo tree` and `website/package-lock.json` and regenerated by
the docs workflow before each build, so it is not checked in `devenv test` (Renovate lock file updates would
otherwise fail).
Consequences: A new dependency under another license, or a new RustSec advisory, fails the gate until it is
reviewed. `devenv test` needs network access to GitHub to update the advisory database.
||||||| d753e54
||||||| 9007066
||||||| 38e5ed5
