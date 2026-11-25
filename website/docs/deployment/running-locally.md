---
title: Running locally
---

# Running locally

To develop or try mergeable-rs on your machine:

1. [Register a GitHub App](./github-app.md) for development (for example on a personal account with a test repository).
2. Create a webhook proxy channel so GitHub can reach your machine, for example on [smee.io](https://smee.io), and
   use the channel URL as the app's webhook URL. Forward the channel to `http://localhost:3000/api/github/webhooks`
   with the proxy's client. Any other tunnel (for example cloudflared or ngrok) works too.
3. Build and start the server from the repository, with the toolchain from [devenv](https://devenv.sh):

   ```bash
   devenv shell
   cargo build --release
   APP_ID=12345 WEBHOOK_SECRET=... PRIVATE_KEY_PATH=./private-key.pem LOG_FORMAT=pretty \
     ./target/release/mergeable
   ```

   or run the [container image](./container-image.md).
4. Install the app on your test repository ([application settings](https://github.com/settings/installations)) and
   add a `.github/mergeable.yml`.
5. Open a test pull request to check that everything works.

`NODE_ENV=development` shortens the scheduler interval to 2 seconds, which helps when testing
[scheduled recipes](../operations/scheduler.md).

:::note

To use a different configuration file name than `mergeable.yml`, set the `CONFIG_PATH` environment variable. The file
is always looked up below `.github/`.

:::

## Possible issues

**`400 Bad Request` with `signature does not match event payload and secret`**: the webhook secret of the GitHub App
and `WEBHOOK_SECRET` differ. Set the same value on both sides. A request without the `X-GitHub-Event`,
`X-Hub-Signature-256` or `X-GitHub-Delivery` header is rejected with `400` as well.

**The server does not start (`missing APP_ID` or a private key error)**: set `APP_ID` and either `PRIVATE_KEY` or
`PRIVATE_KEY_PATH`.

**Errors from the GitHub API with GitHub Enterprise Server**: set `GHE_HOST` to the host name of your Enterprise
instance, e.g. `GHE_HOST=github.your-company.com`.

For all variables see [Environment variables](./environment-variables.md).
