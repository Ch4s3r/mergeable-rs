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
    ./target/release/replay check --bin ./target/release/mergeable "$@"
  '';

  enterTest = ''
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings
    cargo test
    contract-tests
  '';
}
