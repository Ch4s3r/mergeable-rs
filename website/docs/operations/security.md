---
title: Security
---

# Security

Report vulnerabilities privately through
[GitHub private vulnerability reporting](https://github.com/Ch4s3r/mergeable-rs/security/advisories/new), never in a
public issue. Only the latest release receives security fixes. The policy is in
[SECURITY.md](https://github.com/Ch4s3r/mergeable-rs/blob/main/SECURITY.md).

## Security-relevant settings

- `WEBHOOK_SECRET`: required unless `NODE_ENV=development` (which falls back to `development`); the server does not
  start without it. Use a long random value and set the same value in the GitHub App. Only the HMAC-SHA256 signature
  in `X-Hub-Signature-256` is accepted; it is compared in constant time against the raw request body before the body
  is parsed.
- `PRIVATE_KEY` / `PRIVATE_KEY_PATH`, `WEBHOOK_SECRET`, `JIRA_PASSWORD`, `CACHE_REDIS_PASSWORD`: pass them as secrets of your platform
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
- `CACHE_STORAGE=redis` / `valkey`: set `CACHE_REDIS_PASSWORD` (and `CACHE_REDIS_USERNAME` for an ACL user) as a
  secret and `CACHE_REDIS_TLS=true` to encrypt the connection and verify the server certificate. Without
  `CACHE_REDIS_PASSWORD` the value of `CACHE_REDIS_HOST` is sent as the `AUTH` password (behaviour of the original),
  and without `CACHE_REDIS_TLS` cached configuration travels unencrypted; then run the cache server on a private
  network reachable only by mergeable.
- `GHE_HOST`, `GHE_PROTOCOL`, `JIRA_HOST`, `JIRA_PROTOCOL`: the outgoing hosts are read from environment variables
  only, never from repository configuration. The Jira ticket id taken from a pull request is percent-encoded as one
  path segment.
- The container image runs as a non-root user (Paketo `cnb` user, distroless `nonroot` for the `Dockerfile`) and
  listens only on `PORT`.

Limits on input from repositories (configuration files and pull requests): YAML nesting depth and alias expansion are
bounded, templates may nest blocks and subexpressions at most 100 levels deep, a glob brace expansion yields at most
100 000 patterns, user regular expressions run on `fancy-regex` with its backtracking limit, and request bodies are
limited to 2 MB.

## Automated checks

- [CodeQL](https://codeql.github.com) for Rust and the GitHub Actions workflows on every pull request, on `main` and
  weekly (`.github/workflows/codeql.yml`).
- [OpenSSF Scorecard](https://scorecard.dev) on `main` and weekly (`.github/workflows/scorecard.yml`), results
  published and uploaded to code scanning.
- [Grype](https://github.com/anchore/grype) scans `ghcr.io/ch4s3r/mergeable-rs:latest` weekly and after every
  release run (`.github/workflows/image-scan.yml`) and uploads the findings to code scanning.
- Secret scanning with push protection is enabled for the repository.
- [Renovate](https://docs.renovatebot.com) pins every GitHub Action to a commit digest
  (`helpers:pinGitHubActionDigests`) and opens pull requests for dependencies with known vulnerabilities; every
  workflow declares least-privilege `permissions:`.
