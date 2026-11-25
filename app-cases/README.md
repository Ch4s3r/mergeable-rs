# App cases

Black-box contract tests. Each file `app-cases/cases/<name>.json` is one recorded webhook scenario. The
`replay` crate starts the app behind a mock GitHub API, sends the signed webhook and compares the GitHub API
calls the app makes and its HTTP response with the recorded expectation (ADR-005).

- `cargo nextest run --test contract_cases` (or `cargo test --test contract_cases`) runs every case as an
  ordinary Rust test. `app/build.rs` generates one test per case file (`action-assign-author` becomes
  `action_assign_author`); each test starts the mock and the app in-process (`mergeable::server::start` on port 0)
  with the same environment as below plus the case `env`. A case passes when the calls match, also when only
  the order differs; a case with `known_diff` does not fail on a difference. A failing case prints the first
  differing call and a diff of all calls.
- `replay check --bin <mergeable binary>` runs the cases against a built binary, one process per case.
- `replay record --reference-command <command…>` records `expected` from any reference implementation that
  speaks the same HTTP and environment interface. Each case is run twice; if the two runs disagree only on call
  order, the case is marked `unordered`.
- `replay bench` fires many webhooks drawn from the cases for throughput and latency (BENCHMARK.md).

The `replay` commands run from the repository root; `--case <name>` selects cases, `--jobs` sets parallelism.

## Format

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
  "expected": {                                // written by `replay record`, never by hand
    "status": 200,                             // webhook HTTP response status
    "calls": [ { "method": "POST", "path": "/repos/acme/app/check-runs", "query": {}, "body": { ... } } ]
  },
  "unordered": false,                          // set by record when two recording runs disagree on order
  "quiet_ms": 12000,                           // optional: longer quiet window for cases that wait for retries
  "known_diff": null                           // or the reason for an approved divergence from the recording
}
```

## Mock GitHub rules

- Served under `/api/v3` (the app gets `GHE_HOST=127.0.0.1:<port>`, `GHE_PROTOCOL=http`); paths are stored without the
  prefix, percent-decoded (`%2F` -> `/`), query as an object with sorted keys.
- `POST /app/installations/{id}/access_tokens` and `GET /app/installations/...` are answered by the mock (token
  `ghs_test`) and are not recorded.
- Every response carries `x-ratelimit-limit`, `x-ratelimit-remaining` and `x-ratelimit-reset` headers.
- Anything unmatched answers `404 {"message":"Not Found","documentation_url":"https://docs.github.com/rest"}`.
- Recorded bodies: JSON parsed; ISO-8601 timestamps replaced by `"<timestamp>"`.
- Recorded strings: the mock origin `http://127.0.0.1:<port>` is replaced by `http://github.mock`; stack-trace
  lines inside error texts are ignored when comparing.
- A case ends when no request arrived for the quiet window (`--quiet-ms`, default 4 s, or the case's `quiet_ms`
  if longer) after the webhook was answered, at most 40 s. The window covers the client's retries after 1 s, 4 s
  and 9 s and the 1 s spacing of writes (ADR-007).
