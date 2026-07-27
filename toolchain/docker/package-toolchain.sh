#!/usr/bin/env bash
# Package a built stage1 + the bootstrap cargo into the release-asset tarball the
# Dockerfile downloads. Run ../build-fork.sh first. The tarball extracts to
# `stage1/` + `cargo` (matching the Dockerfile's `tar -xz -C /opt/fullrust`).
#
# Usage:  ./package-toolchain.sh [minor] [out.tar.gz]
#   ./package-toolchain.sh 1.88 fullrust-toolchain-1.88-x86_64.tar.gz
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
MINOR="${1:-1.88}"
OUT="${2:-fullrust-toolchain-$MINOR-x86_64.tar.gz}"
BUILD="$ROOT/rust-$MINOR/build/x86_64-unknown-linux-gnu"

[[ -d "$BUILD/stage1" ]]        || { echo "missing $BUILD/stage1 — run $ROOT/build-fork.sh first" >&2; exit 1; }
[[ -x "$BUILD/stage0/bin/cargo" ]] || { echo "missing bootstrap cargo $BUILD/stage0/bin/cargo" >&2; exit 1; }

# Tar directly from the build tree (no copy — /tmp is often a different fs, so a
# hardlink staging fails cross-device). Multiple `-C` place `stage1/` and `cargo`
# at the archive root.
tar -czf "$OUT" \
  -C "$BUILD"            stage1 \
  -C "$BUILD/stage0/bin" cargo
echo "wrote $OUT ($(du -h "$OUT" | cut -f1))"
