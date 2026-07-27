#!/usr/bin/env bash
# Build the fullrust fork toolchain for a given Rust version:
#   1. clone rust-lang/rust at the tag (if not already present),
#   2. apply our source overlay (patches/fullrust-<minor>.patch),
#   3. drop in the build config (bootstrap-<minor>.toml),
#   4. build stage1 + std for x86_64-unknown-linux-fullrust.
#
# Usage:  ./build-fork.sh 1.88.0
# Then verify with:  ./link-and-test.sh 1.88
set -euo pipefail

VER="${1:?usage: build-fork.sh <version, e.g. 1.88.0>}"
MINOR="${VER%.*}"                      # 1.88.0 -> 1.88
HERE="$(cd "$(dirname "$0")" && pwd)"
RUST="$HERE/rust-$MINOR"
PATCH="$HERE/patches/fullrust-$MINOR.patch"
CONFIG="$HERE/bootstrap-$MINOR.toml"

[[ -f "$PATCH" ]]  || { echo "missing overlay $PATCH" >&2; exit 1; }
[[ -f "$CONFIG" ]] || { echo "missing config $CONFIG" >&2; exit 1; }

if [[ ! -d "$RUST/.git" ]]; then
  echo "== cloning rust $VER =="
  # A restored build cache may have recreated $RUST/build without the source
  # (no .git), and git clone refuses a non-empty target. Preserve the cached
  # build/ (same-fs temp → the move is a rename, not a 2 GB copy), clone the
  # source fresh, then fold the build dir back in so the compile stays incremental.
  saved_build=""
  if [[ -d "$RUST/build" ]]; then
    saved_build="$(mktemp -d -p "$HERE")"
    mv "$RUST/build" "$saved_build/build"
  fi
  rm -rf "$RUST"
  git clone --depth 1 --branch "$VER" https://github.com/rust-lang/rust.git "$RUST"
  if [[ -n "$saved_build" ]]; then
    mv "$saved_build/build" "$RUST/build"
    rmdir "$saved_build"
  fi
fi

# The overlay patches vendored submodules (backtrace, stdarch); they must be
# checked out BEFORE the patch (x.py only initializes them later, at build time,
# which is too late for `git apply`). Keep this list in sync with regen-overlay.sh.
echo "== initializing vendored submodules the overlay patches =="
git -C "$RUST" submodule update --init library/backtrace library/stdarch

# Apply the overlay unless it is already applied (reverse-check succeeds).
if git -C "$RUST" apply --check --reverse "$PATCH" 2>/dev/null; then
  echo "== overlay already applied =="
else
  echo "== applying overlay $PATCH =="
  git -C "$RUST" apply "$PATCH"
fi

cp "$CONFIG" "$RUST/bootstrap.toml"

echo "== building stage1 + std for HOST (x86_64-unknown-linux-gnu) + fullrust =="
cd "$RUST"
# Build BOTH targets: the host std/core must be in the sysroot too, or cargo
# can't compile build scripts / proc-macros (they run on the host) — real crates
# (serde, thiserror, …) then fail with `can't find crate for std`. Building the
# fullrust target ALONE drops the host libs from the stage1 sysroot.
BOOTSTRAP_SKIP_TARGET_SANITY=1 \
  python3 x.py build library \
    --target x86_64-unknown-linux-gnu,x86_64-unknown-linux-fullrust --stage 1

echo "== done. verify with: $HERE/link-and-test.sh $MINOR =="
