---
title: Scheduler
---

# Scheduler

With `MERGEABLE_SCHEDULER=true` mergeable runs the recipes for the event `schedule.repository` periodically for every
repository the app is installed on, for example to find [stale](../validators/stale.md) issues and pull requests.

## How it works

1. At start, the app lists its installations (`GET /app/installations?per_page=100`, authenticated as the App) and each
   installation's repositories (`GET /installation/repositories?per_page=100`, one installation at a time).
2. After a random delay within one interval, it runs the rules for every repository with the payload
   `{action: "repository", installation, repository}`, then again every `MERGEABLE_SCHEDULER_INTERVAL` seconds (default
   `3600`). `DISABLE_DELAY` (any non-empty value) disables the random initial delay. With `NODE_ENV=development` the
   interval is 2 seconds.
3. `installation.created` and `installation_repositories.added` webhooks schedule new repositories the same way;
   `installation.deleted` and `installation_repositories.removed` webhooks stop scheduling the installation's
   repositories or the removed repositories.

`IGNORED_ACCOUNTS` is a comma-separated list of account logins (case-insensitive) whose installations are skipped.

## Periodic installation resync

In case a webhook is missed, the scheduler resyncs every `MERGEABLE_INSTALLATION_RESYNC_INTERVAL` seconds (default
`604800`, one week; fractions are allowed, and zero, negative or invalid values use the default). The first resync
happens one interval after start. A resync:

- lists the installations and their repositories again, with the same requests as at start,
- schedules repositories it does not know yet,
- stops scheduling repositories that are no longer listed, including those of uninstalled or ignored installations.

Each resync logs `scheduler.resync.completed` with the counts `installations`, `repositories`, `added` and `removed`.

The resync is conservative: when listing the installations fails, nothing changes; when listing one installation's
repositories fails, that installation's repositories stay scheduled; and a repository scheduled by a webhook while the
resync was listing is kept. The scheduled payload of a repository that stays is not refreshed.

The cost is one installation list plus one repository list per installation per resync interval.

:::warning

Enable the scheduler in one replica only, see [Scaling with several replicas](./caching.md#scaling-with-several-replicas).

:::
