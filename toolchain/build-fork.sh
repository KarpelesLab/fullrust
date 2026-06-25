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
  git clone --depth 1 --branch "$VER" https://github.com/rust-lang/rust.git "$RUST"
fi

# Apply the overlay unless it is already applied (reverse-check succeeds).
if git -C "$RUST" apply --check --reverse "$PATCH" 2>/dev/null; then
  echo "== overlay already applied =="
else
  echo "== applying overlay $PATCH =="
  git -C "$RUST" apply "$PATCH"
fi

cp "$CONFIG" "$RUST/bootstrap.toml"

echo "== building stage1 + std for x86_64-unknown-linux-fullrust =="
cd "$RUST"
BOOTSTRAP_SKIP_TARGET_SANITY=1 \
  python3 x.py build library --target x86_64-unknown-linux-fullrust --stage 1

echo "== done. verify with: $HERE/link-and-test.sh $MINOR =="
