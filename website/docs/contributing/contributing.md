---
title: Contributing
---

# Contributing

We need your help:

- Have an **idea** for a new **feature**? Please create an [issue](https://github.com/Ch4s3r/mergeable-rs/issues/new)
  and tell us!
- Fix a **bug**, implement a new **validator** or **action** and open a
  [pull request](https://github.com/Ch4s3r/mergeable-rs/pulls)!

The repository is a single Cargo package, `mergeable` (`src/`): the server, the rule engine and the GitHub client.
Architecture decisions are recorded in [DECISIONS.md](https://github.com/Ch4s3r/mergeable-rs/blob/main/DECISIONS.md),
the module map is [MAP.md](https://github.com/Ch4s3r/mergeable-rs/blob/main/MAP.md).

## Development environment

The toolchain comes from [devenv](https://devenv.sh) (`devenv.nix`): Rust, cargo-edit, cargo-nextest, git-cliff, pack,
Redis, Valkey and Node.js (for this documentation site).

```bash
devenv shell
cargo build --release
./target/release/mergeable
```

See [Running locally](../deployment/running-locally.md) for a GitHub App to test against.

## Tests

```bash
devenv test
```

runs the whole gate, the same as CI. Step by step inside `devenv shell`:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo nextest run                 # unit tests, the ported upstream unit tests and the 381 recorded webhook cases
cargo test --doc
```

Clippy runs with `clippy::pedantic`; a deliberate exception uses a scoped `#[expect(lint, reason = "...")]`.

### Contract tests

The observable behaviour of the app is the GitHub API traffic it produces for a webhook and the HTTP response it gives.
Each file `tests/cases/<name>.json` is one webhook scenario recorded from the original implementation (381 cases).
`build.rs` generates one test per case file in `tests/contract_cases.rs` (`action-assign-author` becomes
`action_assign_author`). Each test starts a mock GitHub API and the app in-process, sends the signed webhook and compares
the GitHub API calls the app makes and its HTTP status with the recorded expectation. A case passes when the calls
match, also when only their order differs; a failing case prints the first differing call and a diff of all calls.

```bash
cargo nextest run --test contract_cases
cargo nextest run --test contract_cases validator_title_must_include_fail
```

A case file looks like this (shortened):

```json
{
  "name": "title-must-include-pass",
  "description": "A v2 config validating the title with must_include passes and the checks pass action reports success.",
  "event": "pull_request",
  "payload": { "action": "opened", "number": 1, "pull_request": { "title": "feat: add replay" }, "installation": { "id": 176535 } },
  "files": {
    ".github/mergeable.yml": "version: 2\nmergeable:\n  - when: pull_request.*\n    validate:\n      - do: title\n        must_include:\n          regex: 'feat|fix'\n"
  },
  "github": [
    { "method": "GET", "path": "/repos/acme/app/pulls/1/files", "status": 200, "body": [] },
    { "method": "POST", "path": "/repos/acme/app/check-runs", "status": 201, "body": { "id": 555 } }
  ],
  "expected": {
    "status": 200,
    "calls": [
      { "method": "GET", "path": "/repos/acme/.github/contents/.github/mergeable.settings.yml", "query": {}, "body": null },
      { "method": "GET", "path": "/repos/acme/app/contents/.github/mergeable.yml", "query": {}, "body": null },
      { "method": "POST", "path": "/repos/acme/app/check-runs", "query": {}, "body": { "status": "in_progress" } }
    ]
  }
}
```

- `files` are repository files served by the mock (`owner/repo:path` for another repository); a path not listed answers
  404.
- `github` are stubbed responses, the first match wins.
- `expected` was recorded from the original implementation and is never written by hand. Optional fields: `env`
  (extra environment variables), `unordered`, `quiet_ms` and `known_diff` (the reason for an approved difference).

The full format and the mock rules are described in the
[README](https://github.com/Ch4s3r/mergeable-rs#contract-tests).

Further suites: `tests/github_data_cache.rs` replays cases against one app and two replicas sharing a Valkey server
(it starts its own `valkey-server` when it is on `PATH`, as in `devenv shell`), `tests/installation_resync/` covers the
scheduler resync, and the golden corpora in `tests/fixtures` pin the template, glob and YAML engines.

## Commits and pull requests

- Commits follow [Conventional Commits](https://www.conventionalcommits.org) (`feat:`, `fix:`, `docs:`, `test:`,
  `refactor:`, `ci:`, `build:`, `perf:`, `chore:`). The commit types decide the next version, see
  [Releases](./releases.md).
- `main` is protected: every change goes through a pull request with a passing `test` check and is squash-merged, so
  the pull request title becomes the conventional commit on `main`.
- A change to the observable behaviour (configuration files, environment variables, routes, GitHub calls) shows up as
  a failing contract case and must be deliberate.

## Documentation

This site lives in `website/` (Docusaurus). Preview it with the devenv script `docs-serve`:

```bash
devenv shell -- docs-serve
```

or `cd website && npm ci && npm start`. Pull requests that change `website/` build the site in CI; pushes to `main`
deploy it to GitHub Pages.
