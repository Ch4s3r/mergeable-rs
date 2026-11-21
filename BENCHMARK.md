# Benchmark: Node/Probot original vs Rust port (2026-11-21)

Harness: `replay bench` (rust/replay) — starts the app, a mock GitHub API answering instantly, fires N signed
webhooks drawn round-robin from the listed recorded cases with C in flight. Same machine (Apple Silicon, macOS),
Node 22.23.5 (devenv), Rust release build. Results files: produced with `--json`.

## Read path (10 recorded cases whose rules make only GET calls: filters, merge blocked, no matching rule, ...)
`replay bench --node|--bin rust/target/release/mergeable --case <10 cases> --requests 2000 --concurrency 32`

| Metric | Node (Probot) | Rust | Factor |
|---|---|---|---|
| Throughput | 81.1 webhooks/s | 7,282 webhooks/s | ~90x |
| Latency p50 / p95 / p99 | 357 / 527 / 540 ms | 3.9 / 5.6 / 12.7 ms | ~90x |
| GitHub calls served | 381 /s | 34,226 /s | ~90x |
| Peak RSS | 239.6 MB | 20.3 MB | ~12x less |
| Startup to first webhook | 490 ms | 30 ms | ~16x |

## Write path (8 approvals cases: check run create + update per webhook)
Probot spaces write requests 1 s apart process-wide; the Rust port keeps that throttle by default (ADR-011).

| Metric | Node (Probot) | Rust (throttle on, default) | Rust (`GITHUB_THROTTLE=false`) |
|---|---|---|---|
| Requests / concurrency | 40 / 16 | 40 / 16 | 2000 / 32 |
| Throughput | 1.5 webhooks/s | 1.5 webhooks/s | 4,249 webhooks/s |
| Latency p50 | 9,004 ms (202 after 9 s) | 9,002 ms (202 after 9 s) | 6.7 ms |
| Peak RSS | 139.0 MB | 19.0 MB | 25.4 MB |

With the throttle on, both apps are bound by GitHub's pacing rule, so throughput is identical by design; the
unthrottled column shows the engine's own capacity and is not an apples-to-apples comparison.

## Footprint
- Rust release binary: 16.5 MB, no runtime dependencies (distroless image, rust/Dockerfile).
- Node: node_modules 138 MB plus the Node 22 runtime.

## Rust-to-Rust: release profile tuning (2026-11-22)

Method: `replay bench` with GITHUB_THROTTLE=false, 20,000 webhooks, 32 in flight, read set (10 GET-only cases)
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

Chosen: `[profile.release] lto = true, codegen-units = 1` plus log payloads built only when the level is enabled.
Not adopted: `-Ctarget-cpu=native` (no measurable gain, binaries would not run on other CPUs / in the Docker image),
`panic = "abort"` (1-3% more, but a panic in one webhook would abort the whole server instead of failing that
webhook). Replay after the change: 379/381 identical, 2 known diffs (unchanged).
