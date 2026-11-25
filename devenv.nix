{ pkgs, ... }:

{
  languages.rust.enable = true;

  packages = [
    pkgs.cargo-edit
    pkgs.cargo-nextest
    pkgs.git-cliff
    pkgs.nodejs_22
    pkgs.pack
    pkgs.redis
    pkgs.valkey
  ];

  scripts.build-image.exec = ''
    pack build "''${1:-mergeable-rs}" --path "$DEVENV_ROOT" "''${@:2}"
  '';

  scripts.docs-serve.exec = ''
    cd "$DEVENV_ROOT/website" && npm ci && npm start
  '';

  enterTest = ''
    set -euo pipefail
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings
    cargo nextest run 1>&2
    cargo test --doc 1>&2
  '';
}
