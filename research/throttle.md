# Request throttling on GitHub Enterprise (research 2026-11-21)

Sources (docs.github.com, @latest):
- GHES admin, configuring rate limits: https://docs.github.com/en/enterprise-server@latest/admin/configuring-settings/configuring-user-applications-for-your-enterprise/configuring-rate-limits
- GHES REST rate limits: https://docs.github.com/en/enterprise-server@latest/rest/using-the-rest-api/rate-limits-for-the-rest-api
- GHES REST best practices: https://docs.github.com/en/enterprise-server@latest/rest/using-the-rest-api/best-practices-for-using-the-rest-api
- GHE Cloud REST rate limits: https://docs.github.com/en/enterprise-cloud@latest/rest/using-the-rest-api/rate-limits-for-the-rest-api
- Probot/Octokit source: node_modules/@octokit/plugin-throttling/dist-src/{index,wrap-request}.js

## GitHub Enterprise Server (self-hosted)
- Primary limits are **off by default**: "Rate limits are disabled by default for GitHub Enterprise Server." The site
  admin enables them in the Management Console (Rate Limiting -> Enable HTTP API Rate Limiting), with prefilled
  defaults; users can be exempted with `ghe-config`.
- Secondary limits: admin option "Enable Secondary Rate Limiting" (Total Requests, CPU Limit, CPU Limit for
  Searching). The docs give no default values and don't say whether it is on by default.
- The GHES REST page still documents the secondary rules: max 80 content-generating requests/min and 500/h;
  points: GET 1, POST/PATCH/PUT/DELETE 5, max 900 points/min (REST).
- Best practices (GHES): make requests serially (queue), and for many writes "wait at least one second between each request".
- Signals: 403 or 429; `retry-after` -> wait that long; `x-ratelimit-remaining: 0` -> wait until `x-ratelimit-reset`;
  otherwise wait >= 1 min, then exponential backoff. Continuing while limited "may result in the banning of your integration".

## GitHub Enterprise Cloud
- Installation tokens: 15,000 requests/h (vs 5,000-12,500 on github.com).
- Secondary: 100 concurrent requests, 900 points/min REST, 90 s CPU per 60 s, 80 content-creating requests/min, 500/h.
- Data residency (ghe.com) is not covered separately in these pages.

## What mergeable does (Node and Rust, identical; ADR-011)
- Writes >= 1 s apart (60/min = 300 points/min, below 900), notification paths (comments, merges, review
  requests) >= 3 s apart (20/min, below 80/min), search >= 2 s apart, max 10 concurrent requests, process-wide.
- Octokit matches these paths on the route template, so on GHES (`/api/v3` base) they apply exactly as on github.com.
- Not implemented in either: honouring `retry-after` / `x-ratelimit-reset` on 403/429. Probot's onRateLimit
  retries primary-limit hits twice; secondary-limit hits only log a warning.

## Conclusions for a GHES installation
1. If the admin has not enabled rate limits, the throttle protects only the appliance's load; turning it off
   (`GITHUB_THROTTLE=false`) is safe for GitHub's rules but puts burst load on the server.
2. If secondary limits are enabled, the 1 s / 3 s spacing keeps mergeable well inside 900 points/min and 80
   content requests/min; it could be loosened to ~0.35 s for writes (180 writes/min) but 500 content requests/h is
   the real ceiling for comments.
3. The 1 write/s global queue is the throughput bottleneck (about 0.5-1.5 webhooks/s), not the CPU.
