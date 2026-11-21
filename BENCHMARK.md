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
