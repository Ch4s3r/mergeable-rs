---
slug: /
title: Introduction
sidebar_label: Introduction
---

# mergeable-rs

Mergeable helps automate your team's GitHub workflow without a single line of code.

It is a GitHub App that checks pull requests and issues against rules (recipes) in `.github/mergeable.yml` and reports
the result as check runs, comments, labels and assignees. It can also close, merge or request reviews, and it can
look for stale issues and pull requests on a schedule.

**mergeable-rs** is a Rust implementation of [mergeable](https://github.com/mergeability/mergeable). It reads the same
configuration files (`.github/mergeable.yml` and `.github/mergeable.settings.yml`) with the same semantics, and it
produces the same GitHub API calls for a webhook as the original: 381 webhook scenarios recorded from the original
implementation run as contract tests on every change. Existing configurations keep working without edits.

## What is different from the original

- **One binary, one container image.** The server is a single Rust executable, published as
  `ghcr.io/ch4s3r/mergeable-rs`. There is no Node.js runtime and nothing to install with npm. See
  [Container image](./deployment/container-image.md).
- **Configured through environment variables.** The variable names of the original are kept (including `NODE_ENV`),
  see [Environment variables](./deployment/environment-variables.md).
- **Shared cache with Valkey.** Configuration, settings and frequently read GitHub data (team members,
  collaborators, topics, the organisation default config) can be cached in memory or in a shared Valkey/Redis server, so
  several replicas share one cache. See [Caching](./operations/caching.md).
- **Periodic installation resync.** With the scheduler on, installations and repositories are listed again once a week
  by default, so a missed `installation` webhook heals by itself. See [Scheduler](./operations/scheduler.md).
- **Structured logs and metrics.** Logs are JSON by default (`LOG_FORMAT=pretty` for humans), and `/metrics` exports
  GitHub rate-limit, HTTP and cache metrics. See [Operations](./operations/overview.md).

## Where to go next

- [Usage](./getting-started/usage.md): set up the app and write your first recipe.
- [Configuration](./configuration/configuration.md): how recipes are structured.
- [Recipes](./recipes.md): ready-to-use examples.
- [Deployment](./deployment/github-app.md): run your own instance.

## Credits and license

mergeable-rs is a rewrite of [mergeable](https://github.com/mergeability/mergeable) by the mergeability contributors.
The rule semantics, the configuration format and most of this documentation come from the original project, whose
documentation was converted from reStructuredText and updated for this implementation. Like the original,
mergeable-rs is licensed under the [GNU Affero General Public License v3.0](https://github.com/Ch4s3r/mergeable-rs/blob/main/LICENSE).
