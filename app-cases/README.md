# App cases

Each file `rust/app-cases/cases/<name>.json` is one webhook scenario. The `replay` crate (rust/replay) runs it
against an app (the Node original or the Rust binary) behind a mock GitHub API and compares the GitHub API
calls the app makes (ADR-005).

## Format

```jsonc
{
  "name": "title-must-include-fail",          // = file name without .json
  "description": "what this case proves",
  "env": { "APP_NAME": "Mergeable" },          // optional extra env vars for the app process
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
  "expected": {                                // written by `replay record` from the Node original, never by hand
    "status": 200,                             // webhook HTTP response status
    "calls": [ { "method": "POST", "path": "/repos/acme/app/check-runs", "query": {}, "body": { ... } } ]
  },
  "unordered": false,                          // set by record when two runs of the original disagree on order
  "known_diff": null                           // or a reason (approved) why Rust differs; see DECISIONS.md
}
```

## Mock GitHub rules

- Served under `/api/v3` (the app gets `GHE_HOST=127.0.0.1:<port>`, `GHE_PROTOCOL=http`); paths are stored without the
  prefix, percent-decoded (`%2F` -> `/`), query as an object with sorted keys.
- `POST /app/installations/{id}/access_tokens` and `GET /app/installations/...` are answered by the mock (token
  `ghs_test`) and are not recorded.
- Anything unmatched answers `404 {"message":"Not Found","documentation_url":"https://docs.github.com/rest"}`.
- Recorded bodies: JSON parsed; ISO-8601 timestamps replaced by `"<timestamp>"`.
- Recorded strings: the mock origin `http://127.0.0.1:<port>` is replaced by `http://github.mock`.
- A case ends when no request arrived for 400 ms after the webhook was answered (max 30 s: probot retries 5xx after 1 s, 4 s, 9 s and spaces writes 1 s apart).
