---
title: GitHub App setup
---

# GitHub App setup

mergeable-rs runs as your own GitHub App: you register the app, run the server (see
[Container image](./container-image.md)) and install the app on your repositories or organisation.

[Create a GitHub App](https://github.com/settings/apps/new) (or, for an organisation, under the organisation's
*Settings → Developer settings → GitHub Apps*) and configure it as follows.

## Settings

- **GitHub App name**: your app name. Set `APP_NAME` to the same name, it is used for the `@bot` annotation and to
  recognise the app's own comments.
- **Webhook URL**: the public URL of your instance followed by the webhook path, by default
  `https://<your-host>/api/github/webhooks` (`WEBHOOK_PATH`). For local development use a webhook proxy, see
  [Running locally](./running-locally.md).
- **Webhook secret**: a random secret; set the same value as `WEBHOOK_SECRET`. Every delivery's
  `X-Hub-Signature-256` is verified against it.

## Repository permissions

| Permission | Access |
|---|---|
| Checks | Read & write |
| Contents | Read-only (Read & write if you use the [merge](../actions/merge.md) action) |
| Issues | Read & write |
| Metadata | Read-only |
| Pull requests | Read & write |
| Commit statuses | Read-only |
| Projects | Read-only (for the [project](../validators/project.md) validator) |

## Organization permissions

| Permission | Access |
|---|---|
| Members | Read-only (for teams in [approvals](../validators/approvals.md), [author](../validators/author.md) and [one_of / none_of](../options/one-of-none-of.md)) |

## Events

Subscribe to the following events:

- Check suite
- Issue comment
- Issues
- Pull request
- Pull request review
- Push (re-runs open pull requests when the configuration file changes)
- Status

The `installation` and `installation_repositories` events are always delivered to GitHub Apps; the
[scheduler](../operations/scheduler.md) uses them.

## Private key

After the app has been registered, generate a private key on the app's settings page and pass it to the server, either
as `PRIVATE_KEY` (the PEM text, with `\n` escapes allowed, or the base64-encoded PEM) or as a file with
`PRIVATE_KEY_PATH`. Set `APP_ID` to the app's ID. See [Environment variables](./environment-variables.md).

Finally, install the app on the repositories or the organisation you want to automate (*Install App* on the app's
settings page) and add a [configuration](../configuration/configuration.md).

## GitHub Enterprise Server

Register the app on your GitHub Enterprise Server and set `GHE_HOST` to its host name (for example
`github.your-company.com`); the API is then called at `https://<GHE_HOST>/api/v3` (`GHE_PROTOCOL` changes the scheme).
