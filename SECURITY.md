# Security policy

## Reporting a vulnerability

Please report vulnerabilities privately through
[GitHub private vulnerability reporting](https://github.com/Ch4s3r/mergeable-rs/security/advisories/new)
(repository **Security** tab, **Report a vulnerability**). Do not open a public issue or pull request for it.
Include the affected version, the configuration needed to reproduce it and its impact. Fixed vulnerabilities are
published as GitHub security advisories.

## Supported versions

Only the latest release (the newest `v*` tag and the image `ghcr.io/ch4s3r/mergeable-rs:latest`) receives security
fixes. Releases are cut automatically from `main`, so a fix is available as soon as it is merged.

## Security-relevant settings

- `WEBHOOK_SECRET`: required unless `NODE_ENV=development` (which falls back to `development`); the server does not
  start without it. Use a long random value and set the same value in the GitHub App. Only the HMAC-SHA256 signature
  in `X-Hub-Signature-256` is accepted; it is compared in constant time against the raw request body before the body
  is parsed.
- `PRIVATE_KEY` / `PRIVATE_KEY_PATH`, `WEBHOOK_SECRET`, `JIRA_PASSWORD`: pass them as secrets of your platform
  (Kubernetes secrets, Docker secrets, ...). They are not logged; the `Debug` output of the environment lists variable
  names only and the Jira and Redis settings redact their passwords.
- `ENABLE_GITHUB_RATELIMIT_ENDPOINT` and `ENABLE_METRICS_ENDPOINT`: both routes are unauthenticated.
  `/github-ratelimit/:installationId` creates an installation token for any numeric installation id of the App and
  reveals its rate limit; `/metrics` reveals installation ids, request counts and cache statistics. Leave them off, or
  expose them only on an internal network (for example a Kubernetes `NetworkPolicy` or an ingress that does not route
  them) and expose only `WEBHOOK_PATH` publicly.
- Configuration from pull requests: a `.github/mergeable.yml` changed in a pull request from the same repository is
  used for that pull request, so anyone who can push a branch can change the rules that check their own pull request
  (for example drop a required approval). Configurations from forks are never loaded. Turn it off with
  `use_config_from_pull_request: false` in the settings file, and do not rely on mergeable as the only merge gate: use
  branch protection or rulesets with required reviews as well.
- `CACHE_STORAGE=redis` / `valkey`: the value of `CACHE_REDIS_HOST` is also sent as the Redis `AUTH` password
  (behaviour of the original) and the connection is not encrypted. Run the cache server on a private network
  reachable only by mergeable.
- `GHE_HOST`, `GHE_PROTOCOL`, `JIRA_HOST`, `JIRA_PROTOCOL`: the outgoing hosts are read from environment variables
  only, never from repository configuration. The Jira ticket id taken from a pull request is percent-encoded as one
  path segment.
- The container image runs as a non-root user (Paketo `cnb` user, distroless `nonroot` for the `Dockerfile`) and
  listens only on `PORT`. Its run image is the Paketo tiny run image (glibc, CA certificates, tzdata; no shell, no
  package manager), pinned in `project.toml` and updated by Renovate, and every update releases a new image.

Limits on input from repositories (configuration files and pull requests): YAML nesting depth and alias expansion are
bounded, templates may nest blocks and subexpressions at most 100 levels deep, a glob brace expansion yields at most
100 000 patterns, user regular expressions run on `fancy-regex` with its backtracking limit, and request bodies are
limited to 2 MB.

Limits on load: at most `MAX_CONCURRENT_WEBHOOKS` (default 64) webhooks are processed at once, further deliveries
wait at most 9 s for a free slot and are then answered `503`. Requests to GitHub and Jira time out after
`OUTGOING_HTTP_CONNECT_TIMEOUT` (10 s) without a connection and `OUTGOING_HTTP_TIMEOUT` (60 s) per request, so a
hanging upstream cannot keep a handler alive indefinitely.

## Automated checks

- [CodeQL](https://codeql.github.com) for Rust and the GitHub Actions workflows on every pull request, on `main` and
  weekly (`.github/workflows/codeql.yml`).
- [OpenSSF Scorecard](https://scorecard.dev) on `main` and weekly (`.github/workflows/scorecard.yml`), results
  published and uploaded to code scanning.
- [Grype](https://github.com/anchore/grype) scans `ghcr.io/ch4s3r/mergeable-rs:latest` weekly and after every
  release run (`.github/workflows/image-scan.yml`) and uploads the findings to code scanning. Findings in packages
  the binary does not load (for example `openssl`/`libssl3t64`: the binary uses rustls and links only `libc`,
  `libm` and `libgcc_s`) are dismissed as "won't fix" with that reason.
- Secret scanning with push protection is enabled for the repository.
- [Renovate](https://docs.renovatebot.com) pins every GitHub Action to a commit digest
  (`helpers:pinGitHubActionDigests`) and opens pull requests for dependencies with known vulnerabilities; every
  workflow declares least-privilege `permissions:`.
