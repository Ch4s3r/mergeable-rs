#!/usr/bin/env bash
set -euo pipefail

step() {
  echo
  echo "== $1"
  shift
  printf '%s\n' "$@"
  read -r -p "Press Enter when done (Ctrl-C to stop)... " _
}

IMAGE="${IMAGE:-ghcr.io/your-org/mergeable-rust:latest}"
RUST_URL="${RUST_URL:-}"

echo "Mergeable cutover: Node/Probot -> Rust. The Node deployment stays untouched until the last step."

step "1. Build and push the image" \
  "docker build -t \"$IMAGE\" rust && docker push \"$IMAGE\"" \
  "(or let CI do it; the build context is the rust/ directory)"

step "2. Deploy the Rust app beside the Node app" \
  "Use the same env vars and secrets as the Node deployment: APP_ID, PRIVATE_KEY, WEBHOOK_SECRET," \
  "GHE_HOST (if any), CACHE_*, JIRA_*, MERGEABLE_SCHEDULER*, ENABLE_*_ENDPOINT." \
  "Keep MERGEABLE_SCHEDULER=false on the Rust instance until step 5, so scheduled runs do not happen twice." \
  "Helm: set image.repository/tag to \"$IMAGE\"; probes on /probot keep working."

if [ -z "$RUST_URL" ]; then
  read -r -p "Base URL of the new Rust instance (e.g. https://mergeable-rust.example.com): " RUST_URL
fi
if curl -fsS "$RUST_URL/ping" | grep -q PONG; then
  echo "Rust instance answers /ping."
else
  echo "Rust instance does not answer $RUST_URL/ping - check the deployment before continuing."
  exit 1
fi

step "3. Smoke test with a real repository" \
  "Open a test pull request in a repository with a .github/mergeable.yml and redeliver its webhook to" \
  "$RUST_URL/api/github/webhooks from the GitHub App's 'Advanced -> Recent deliveries' page." \
  "Expect a 'Mergeable' check run with the same result as the Node app produced." \
  "Way back: nothing to undo, the Node app still receives all webhooks."

step "4. Switch the webhook URL" \
  "GitHub App settings -> Webhook URL -> $RUST_URL/api/github/webhooks (or switch your ingress/DNS)." \
  "Way back: set the Webhook URL back to the Node deployment."

step "5. Move the scheduler" \
  "Set MERGEABLE_SCHEDULER=true on the Rust deployment and false on the Node deployment." \
  "Way back: flip both values again."

step "6. Watch for a day" \
  "GitHub App -> Recent deliveries: all 200 (or 202 for slow runs); /metrics if ENABLE_METRICS_ENDPOINT=true." \
  "Way back: steps 4 and 5 in reverse."

step "7. Retire the Node deployment" \
  "Scale the Node deployment to zero, then delete it once you are confident." \
  "Keep lib/ in the repository as the reference implementation until you decide otherwise."

echo
echo "Cutover complete."
