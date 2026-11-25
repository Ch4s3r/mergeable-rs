---
title: Container image
---

# Container image

Every release publishes a container image to the GitHub Container Registry:

```text
ghcr.io/ch4s3r/mergeable-rs:<version>   # e.g. ghcr.io/ch4s3r/mergeable-rs:0.4.2
ghcr.io/ch4s3r/mergeable-rs:latest      # the most recent release
```

Versions follow semantic versioning, see [Releases and versioning](../contributing/releases.md); the image of a
release is named in its [GitHub release](https://github.com/Ch4s3r/mergeable-rs/releases) notes. Pin a version in
production.

## Run

The server listens on port 3000 (`PORT`) and is configured through
[environment variables](./environment-variables.md):

```bash
docker run --rm -p 3000:3000 \
  -e APP_ID=12345 \
  -e WEBHOOK_SECRET=... \
  -e PRIVATE_KEY="$(base64 < private-key.pem)" \
  ghcr.io/ch4s3r/mergeable-rs:latest
```

Check that it is up with `curl http://localhost:3000/ping` (answers `PONG`). Put the server behind HTTPS and point the
GitHub App's webhook URL at `https://<your-host>/api/github/webhooks`.

The image contains the single `mergeable` binary (no Node.js runtime) and runs as a non-root user. For health checks,
metrics, logs and scaling see [Operations](../operations/overview.md).

:::note

A Helm chart and a Docker Compose file are not provided. The image runs on any container platform; use `/ping` for
liveness and readiness probes and, for several replicas, a shared Valkey cache (see
[Caching](../operations/caching.md#scaling-with-several-replicas)).

:::

## Build the image yourself

The image is built with [Cloud Native Buildpacks](https://buildpacks.io) (the Paketo Rust buildpack, configured in
`project.toml` and `Procfile`) through the [devenv](https://devenv.sh) script `build-image <image> [pack options]`:

```bash
devenv shell -- build-image mergeable-rs
docker run --rm -p 3000:3000 \
  -e APP_ID=12345 -e WEBHOOK_SECRET=... -e PRIVATE_KEY="$(base64 < private-key.pem)" \
  mergeable-rs
```

The builder `paketobuildpacks/ubuntu-noble-builder-buildpackless` is multi-arch, so it builds natively on Apple
Silicon. With Docker's containerd image store, `pack` cannot export multi-arch buildpacks into the local daemon
([buildpacks/pack#2272](https://github.com/buildpacks/pack/issues/2272)); publish to a registry instead, for example a
local one:

```bash
docker run -d -p 5001:5000 registry:2
devenv shell -- build-image localhost:5001/mergeable-rs --publish --insecure-registry localhost:5001 --network host
```

### Dockerfile

A plain `Dockerfile` (a Rust build stage and a distroless runtime image, running as non-root) is kept as an
alternative:

```bash
docker build -t mergeable-rs .
```
