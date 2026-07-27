#!/usr/bin/env bash
# Thin convenience wrapper for the fullrust toolchain image.
#
# rustc, the target, the getrandom/socket2 ecosystem [patch.crates-io], and
# RUSTC_BOOTSTRAP are all baked into the image env + CARGO_HOME config, so a
# plain `cargo build` is already a fullrust build (the same build a GitHub
# `container:` job gets). This wrapper only chooses cargo-vs-verbatim and honors
# the FULLRUST_NO_ECOSYSTEM escape hatch.
#
#   docker run --rm -v "$PWD:/src" ghcr.io/karpeleslab/fullrust:1.88            # build --release
#   docker run --rm -v "$PWD:/src" ghcr.io/karpeleslab/fullrust:1.88 test
#   docker run --rm -v "$PWD:/src" -it ghcr.io/karpeleslab/fullrust:1.88 bash   # escape hatch
#
# Env:
#   FULLRUST_NO_ECOSYSTEM=1   build against upstream getrandom/socket2 (no patch)
#   CARGO_BUILD_TARGET=…      override the target triple
set -euo pipefail

# Opt out of the ecosystem patches by pointing at a config-less CARGO_HOME
# (rustc + target still come from the image env).
[ -n "${FULLRUST_NO_ECOSYSTEM:-}" ] && export CARGO_HOME=/opt/fullrust/cargo-noeco

# No args at all → `cargo build` (the Dockerfile CMD normally supplies these).
[ $# -eq 0 ] && set -- build

sub="$1"
case "$sub" in
  build|b|test|t|run|r|check|c|rustc|clippy|bench|doc|tree|metadata|update|fetch|add|remove|fix|vendor)
    exec cargo "$@"
    ;;
  *)
    # Not a cargo subcommand — run verbatim (e.g. `bash`, a raw binary, …).
    exec "$@"
    ;;
esac
