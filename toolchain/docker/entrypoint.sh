#!/usr/bin/env bash
# Build (or test/run/check/…) the Cargo project in the working directory for the
# fullrust target, with the ecosystem `[patch.crates-io]` injected via
# `cargo --config` so the consumer's Cargo.toml is never touched.
#
#   docker run --rm -v "$PWD:/src" ghcr.io/karpeleslab/fullrust:1.88            # build --release
#   docker run --rm -v "$PWD:/src" ghcr.io/karpeleslab/fullrust:1.88 build
#   docker run --rm -v "$PWD:/src" ghcr.io/karpeleslab/fullrust:1.88 test
#   docker run --rm -v "$PWD:/src" -it ghcr.io/karpeleslab/fullrust:1.88 bash   # escape hatch
#
# Env:
#   FULLRUST_NO_ECOSYSTEM=1     don't inject the getrandom/socket2 patches
#   FULLRUST_TARGET=<triple>    override the target (default x86_64-unknown-linux-fullrust)
#   FULLRUST_TOOLCHAIN=<name>   rustup toolchain name (default "fullrust")
set -euo pipefail

target="${FULLRUST_TARGET:-x86_64-unknown-linux-fullrust}"
toolchain="${FULLRUST_TOOLCHAIN:-fullrust}"

# Ecosystem [patch.crates-io], one --config per bundled crate. A patch for a
# crate not in the dep graph is a harmless "patch not used" cargo warning.
patch_args=()
if [ -z "${FULLRUST_NO_ECOSYSTEM:-}" ] && [ -d "${FULLRUST_HOME}/ecosystem/crates" ]; then
  for dir in "${FULLRUST_HOME}"/ecosystem/crates/*/; do
    name="$(basename "$dir")"
    patch_args+=(--config "patch.crates-io.${name}.path=\"${dir%/}\"")
  done
fi

sub="${1:-build}"
case "$sub" in
  build|b|test|t|run|r|check|c|rustc|clippy|bench|doc|tree|metadata)
    shift || true
    exec cargo "+${toolchain}" "$sub" "$@" --target "$target" "${patch_args[@]}"
    ;;
  *)
    # Not a cargo subcommand — run verbatim (e.g. `bash`, a raw binary, …).
    exec "$@"
    ;;
esac
