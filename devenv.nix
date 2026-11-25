{ pkgs, ... }:

{
  languages.rust.enable = true;

  packages = [
    pkgs.cargo-edit
    pkgs.git-cliff
    pkgs.pack
    pkgs.redis
  ];

  scripts.build-image.exec = ''
    pack build "''${1:-mergeable-rs}" --path "$DEVENV_ROOT" "''${@:2}"
  '';

  scripts.contract-tests.exec = ''
    cd "$DEVENV_ROOT"
    cargo build --release -p mergeable -p replay
    if [ "$#" -eq 0 ]; then
      set -- --jobs 32
    fi
    ./target/release/replay check --bin ./target/release/mergeable "$@"
  '';

  enterTest = ''
    set -euo pipefail
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings
    cargo test 1>&2
    contract-tests 1>&2
  '';
}
