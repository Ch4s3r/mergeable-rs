# Handoff

State (2026-11-21): the port is complete and unlifted from Node — rust/app is a pure Rust binary; the Node app in
lib/ stays as reference. Nothing is committed yet (branch rust-rewrite in its own worktree).

- Decisions: DECISIONS.md (ADR-001..012). Plan/state: MAP.md. Numbers: BENCHMARK.md. Usage: README.md.
- Environment: `devenv shell` at the repo root (Node 22, Rust, cargo-edit, redis).
- Verify everything:
  - `cd rust && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test`
  - `cargo build --release -p mergeable -p replay && cd .. && rust/target/release/replay check --bin rust/target/release/mergeable --jobs 8`
- Re-record after changing lib/ or adding cases: `rust/target/release/replay record --missing-only --jobs 8` (runs the Node app).
- Regenerate the template / minimatch corpora: rust/tools/generate-*-corpus.js (run with node from the repo root).
- Known diffs: config-invalid-yaml-in-pr (YAML parser message), config-version-1-old-format (v1 config dropped).
- Next steps for the user: cutover (rust/cutover.sh), optional reshape pass (typed settings/results instead of serde_json::Value).
- Skill to load in a new session: rust-anything (whole-app.md).
