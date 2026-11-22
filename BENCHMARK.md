# Benchmark

Harness: `replay bench` (rust/replay) starts the app and a mock GitHub API that answers instantly, then fires N
signed webhooks drawn round-robin from the listed recorded cases with C in flight. Apple Silicon, macOS, release
build. Result files are produced with `--json`.

## Read path (2026-11-21)

10 recorded cases whose rules make only GET calls (filters, merge blocked, no matching rule, ...).
`replay bench --bin rust/target/release/mergeable --case <10 cases> --requests 2000 --concurrency 32`

| Metric | Value |
|---|---|
| Throughput | 7,282 webhooks/s |
| Latency p50 / p95 / p99 | 3.9 / 5.6 / 12.7 ms |
| GitHub calls served | 34,226 /s |
| Peak RSS | 20.3 MB |
| Startup to first webhook | 30 ms |

## Write path (2026-11-21)

8 approvals cases: check run create + update per webhook. The client spaces write requests 1 s apart
process-wide by default (ADR-007).

| Metric | Throttle on (default) | `GITHUB_THROTTLE=false` |
|---|---|---|
| Requests / concurrency | 40 / 16 | 2000 / 32 |
| Throughput | 1.5 webhooks/s | 4,249 webhooks/s |
| Latency p50 | 9,002 ms (202 after 9 s) | 6.7 ms |
| Peak RSS | 19.0 MB | 25.4 MB |

With the throttle on, throughput is bound by GitHub's pacing rule by design; the unthrottled column shows the
engine's own capacity.

## Footprint

Release binary 12.3 MB, no runtime dependencies (distroless image, rust/Dockerfile).

## Release profile tuning (2026-11-22)

Method: `replay bench` with `GITHUB_THROTTLE=false`, 20,000 webhooks, 32 in flight, read set (10 GET-only cases)
and write set (8 approvals cases, check run create + update). Throughput is limited by the load generator and mock
GitHub on the same machine (all variants ~7,500 read / ~4,300 write webhooks/s), so the metric is the app process's
own CPU time per webhook (sampled with ps), median of 5 interleaved runs.

| Variant | Read µs CPU/webhook | Write µs CPU/webhook | Binary | Release build |
|---|---|---|---|---|
| Baseline (default release profile) | 404 | 697 | 16.5 MB | 24 s |
| cargo-wizard fast-runtime + target-cpu=native | 392 | 663 | 10.2 MB | 61 s |
| cargo-wizard fast-runtime, portable (no native) | 380 | 664 | 10.3 MB | 62 s |
| fat LTO + codegen-units=1 + lazy log payloads, panic=unwind (**chosen**) | 392 (-3%) | 654 (-6%) | 12.1 MB (-27%) | 45 s |
| same with panic=abort | 380 (-6%) | 649 (-7%) | 10.3 MB | 42 s |

Chosen: `[profile.release] lto = true, codegen-units = 1` plus log payloads built only when the level is enabled
(ADR-010). Not adopted: `-Ctarget-cpu=native` (no measurable gain, binaries would not run on other CPUs / in the
Docker image), `panic = "abort"` (1-3% more, but a panic in one webhook would abort the whole server instead of
failing that webhook). Contract tests unchanged.

## Code optimization round (2026-11-22)

Same method as above, comparing the tuned release build (`84243c6`) with this round.

| Build | Read µs CPU/webhook | Write µs CPU/webhook | Binary |
|---|---|---|---|
| `84243c6` (release profile tuning) | 385 | 658 | 12.1 MB |
| this round | 281 (-27%) | 544 (-17%) | 12.4 MB |

Main changes: user regex and glob patterns compiled once and cached (moka) instead of per webhook, CODEOWNERS globs
compiled once per file, the webhook payload shared copy-on-write across tasks instead of deep-copied, rule lists and
settings borrowed instead of cloned in flex/registry/configuration, GitHub request URLs built and parsed once with
the token header cached, the template lexer made linear, and logging switched to `tracing` events with fields.
Contract tests unchanged.

## Clippy pedantic and borrowing round (2026-11-22)

`clippy::pedantic` enabled workspace-wide (ADR-011). JSON value helpers return borrowed slices and iterators
instead of copied arrays, the global settings are shared via `Arc`, GitHub responses are moved instead of cloned,
and payload fields are borrowed: non-test `.clone()`/`.cloned()` in app/src went from 302 to 202.

| Build | Read µs CPU/webhook | Write µs CPU/webhook | Binary |
|---|---|---|---|
| previous round | 285 | 538 | 12.4 MB |
| this round | 296 (noise, runs 271-300) | 516 (-4%) | 12.3 MB |

The benchmark payloads are small, so the effect of not copying large payloads (big PRs, file lists, reviews) is
mostly not visible here. Contract tests unchanged.
