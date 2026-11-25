{ pkgs, ... }:

{
  languages.rust.enable = true;

  packages = [
    pkgs.cargo-deny
    pkgs.cargo-edit
    pkgs.cargo-nextest
    pkgs.crane
    pkgs.git-cliff
    pkgs.nodejs_22
    pkgs.pack
    pkgs.redis
    pkgs.syft
    pkgs.valkey
  ];

  scripts.build-image.exec = ''
    pack build "''${1:-mergeable-rs}" --path "$DEVENV_ROOT" "''${@:2}"
  '';

  scripts.license-inventory.exec = ''
    node "$DEVENV_ROOT/website/scripts/license-inventory.mjs"
  '';

  scripts.docs-serve.exec = ''
    cd "$DEVENV_ROOT/website" && npm ci && npm start
  '';

  enterTest = ''
    set -euo pipefail
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings
    cargo deny check 1>&2
    cargo nextest run 1>&2
    cargo test --doc 1>&2
  '';
}
