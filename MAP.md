# Port map: mergeable -> Rust

Goal: standalone Rust GitHub App binary equivalent to the Probot app (DECISIONS.md ADR-001..012).
Proof: rust/app-cases (381 recorded webhook scenarios) replayed by rust/replay (ADR-005, ADR-009, ADR-012).
Benchmark: BENCHMARK.md. Handoff: HANDOFF.md.

| Wave | Units | Rust module | State |
|---|---|---|---|
| 0 | actions/lib, cache, configuration/lib, errors, settings/lib, stats, utils, scheduler | annotations, cache, redis_cache, error, settings, results, server (rate limit), scheduler | done 2026-11-21 |
| 1 | actions/handlebars, settings/transformers | template (own handlebars.js-compatible interpreter), settings | done 2026-11-21 |
| 2 | tangled core (lib, actions, configuration, flex, github, interceptors, settings, validators/options_processor) | context, github/{client,api,throttle,app_auth}, flex, interceptors, actions, configuration, probot_config, registry, validators/options_processor | done 2026-11-21 |
| 3 | index.js, configuration/transformers (v2), filters options lib, validators/lib, validators/options | main, server, configuration, filters::options, validators::{lib,options} | done 2026-11-21 |
| 4 | filters/lib, filters/options_processor, validators | filters, validators, minimatch, lodash | done 2026-11-21 |
| 5 | filters | filters | done 2026-11-21 |

Results: replay 381/381 identical to Node, no known diffs; fallback rate 0%.
Gate: cargo fmt --check, cargo clippy --all-targets -D warnings, cargo test (332 + 14 tests) all pass.

## Out of scope
- v1 config transformer (ADR-003).

## Open questions
- none
