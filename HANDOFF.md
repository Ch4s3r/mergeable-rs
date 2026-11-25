# Handoff

- Decisions: DECISIONS.md. Modules: MAP.md. Numbers: BENCHMARK.md. Usage: README.md.
- Environment: `devenv shell` (Rust, cargo-edit, cargo-nextest, git-cliff, pack, redis).
- Verify everything: `devenv test` (fmt, clippy, `cargo nextest run --workspace` with unit and contract tests,
  doctests). Contract tests alone: `cargo nextest run --test contract_cases` or `cargo test --test contract_cases`.
- The recorded cases live in tests/cases (format: README.md, Contract tests). Recording new cases and the
  benchmark harness were removed together with the former `replay` crate; a new case needs its `expected` section
  from the reference implementation.
- The template, glob and YAML corpora in app/tests/fixtures are fixed golden data (ADR-009).
- Releases: conventional commits on `main` drive the version (git-cliff); see README.md.
- Possible next step: replace more `serde_json::Value` settings and results with typed structs where the shape is
  fixed (ADR-006).
