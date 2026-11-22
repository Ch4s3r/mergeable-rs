# Handoff

- Decisions: DECISIONS.md. Modules: MAP.md. Numbers: BENCHMARK.md. Usage: README.md.
- Environment: `devenv shell` at the repository root (Rust, cargo-edit, cargo-wizard, redis).
- Verify everything:
  - `cd rust && cargo fmt --all --check && cargo clippy --all-targets && cargo test`
  - `cargo build --release -p mergeable -p replay && cd .. && rust/target/release/replay check --bin rust/target/release/mergeable --jobs 8`
- Record a new case (from the repository root): add `rust/app-cases/cases/<name>.json` without `expected`, then
  `rust/target/release/replay record --missing-only --jobs 8 --reference-command <command that starts the reference app>`.
- The template, glob and YAML corpora in rust/app/tests/fixtures are fixed golden data (ADR-009).
- Possible next step: replace more `serde_json::Value` settings and results with typed structs where the shape is
  fixed (ADR-006).
