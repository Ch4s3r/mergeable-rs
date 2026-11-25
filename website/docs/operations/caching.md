---
title: Caching
---

# Caching

mergeable-rs can cache what it reads from GitHub again and again: the configuration and settings files, the
organisation default configuration, team members and memberships, collaborators and repository topics. Every cache
uses the backend chosen by `CACHE_STORAGE`.

## Backends

| `CACHE_STORAGE` | Backend |
|---|---|
| `memory` (default) | in-process memory, per app instance (size `CACHE_MEMORY_MAX`, default 500 entries) |
| `valkey` | a shared [Valkey](https://valkey.io) server (recommended for several replicas) |
| `redis` | a shared Redis server |

`valkey` and `redis` use the same client, because Valkey is open source and protocol compatible with Redis; both are
configured with `CACHE_REDIS_HOST`, `CACHE_REDIS_PORT` and `CACHE_REDIS_DB` (defaults `localhost`, `6379`, `0`).
`CACHE_TTL` sets the time to live in seconds of the configuration, settings and topic cache entries (also used as the
Redis `SETEX` ttl); without it they do not expire.

:::note

The value of `CACHE_REDIS_HOST` is also sent as the Redis `AUTH` password; a server without a password that rejects the
`AUTH` is used without one. There is no `CACHE_REDIS_PASSWORD` setting. Redis errors are logged and treated as a cache
miss instead of failing the webhook.

:::

## What is cached

| Data | Switch | Key | Invalidation |
|---|---|---|---|
| Repository config (`.github/mergeable.yml`, merged) | `USE_CONFIG_CACHE` / `use_config_cache` | `owner/repo` | push changing the config file (a push to the organisation `.github` repository: all `owner/` keys); `CACHE_TTL` |
| Settings (`.github/mergeable.settings.yml`) | `USE_SETTINGS_CACHE` | `owner/repo/settings` | `CACHE_TTL` |
| Repository topics | `use_config_cache` | `owner/repo/topics` | `CACHE_TTL` |
| Team members (`GET /orgs/{org}/teams/{slug}/members`) | `USE_GITHUB_DATA_CACHE` | `github-data:team_members:org/slug` | `GITHUB_DATA_CACHE_TTL` |
| Team membership (`GET .../memberships/{user}`) | `USE_GITHUB_DATA_CACHE` | `github-data:team_memberships:org/slug/user` | `GITHUB_DATA_CACHE_TTL` |
| Collaborators (`GET /repos/{owner}/{repo}/collaborators`) | `USE_GITHUB_DATA_CACHE` | `github-data:collaborators:owner/repo` | `GITHUB_DATA_CACHE_TTL` |
| Repository topics (`GET /repos/{owner}/{repo}/topics`) | `USE_GITHUB_DATA_CACHE` | `github-data:repository_topics:owner/repo` | `GITHUB_DATA_CACHE_TTL` |
| Organisation default config (`USE_ORG_AS_DEFAULT_CONFIG`) | `USE_GITHUB_DATA_CACHE` | `github-data:organization_config:owner/.github/<file>` | push to `owner/.github` adding or modifying the file; `GITHUB_DATA_CACHE_TTL` |
| Installation tokens | always | per process | renewed shortly before they expire |

## GitHub data cache

`USE_GITHUB_DATA_CACHE=true` turns on the cache for team members, team memberships, collaborators, repository topics and
the organisation default configuration. Its entries live for `GITHUB_DATA_CACHE_TTL` seconds (default `300`), so a
change to a team, the collaborators or the topics is seen after at most that time.

- Only successful responses are cached; errors (a 404 for an unknown team, a 5xx) are fetched again next time.
- Installation tokens are never stored in the shared cache: they are credentials and stay in the process.
- Pull request files, reviews, comments and check runs change with every event and are not cached.

The GitHub data cache is off by default, so the default behaviour is exactly that of the original implementation.

## Metrics

With `ENABLE_METRICS_ENDPOINT=true` every lookup counts a hit or a miss in `cache_lookups_total` with the labels
`cache` (`configuration`, `settings`, `topics`, `team_members`, `team_memberships`, `collaborators`,
`repository_topics`, `organization_config`) and `result` (`hit`, `miss`). Compare it with
`github_ratelimit_remaining_total` to see the saved requests.

## Scaling with several replicas

Run as many replicas behind a load balancer as you need; each webhook is handled by the replica that receives it. With
`CACHE_STORAGE=valkey` all replicas share one cache, so a team list or configuration fetched by one replica saves the
request on the others:

```bash
CACHE_STORAGE=valkey CACHE_REDIS_HOST=valkey.internal CACHE_REDIS_PORT=6379 \
USE_GITHUB_DATA_CACHE=true USE_CONFIG_CACHE=true \
  mergeable
```

:::warning

The [scheduler](./scheduler.md) runs in every replica where `MERGEABLE_SCHEDULER=true`, and the replicas do not
coordinate: with several replicas, enable it in one of them only, otherwise every `schedule.repository` run happens once
per replica.

:::

The request throttle (see [Operations](./overview.md#github-rate-limit-and-throttling)) is per process, so several
replicas together can send more write requests per second than one.

## Try it locally

Run Valkey from the devenv shell and start the app against it:

```bash
devenv shell -- valkey-server --port 6379
CACHE_STORAGE=valkey USE_GITHUB_DATA_CACHE=true ENABLE_METRICS_ENDPOINT=true ./target/release/mergeable
```

Then compare `cache_lookups_total` and `github_ratelimit_remaining_total` on `/metrics`.
