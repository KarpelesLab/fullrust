#!/usr/bin/env bash
# Stage a build context (prebuilt stage1 + ecosystem + entrypoint) and build the
# fullrust toolchain image. Run ../build-fork.sh first so the stage1 exists.
#
# Usage:  ./build-image.sh [minor] [tag]
#   ./build-image.sh 1.88 ghcr.io/karpeleslab/fullrust:1.88
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
MINOR="${1:-1.88}"
TAG="${2:-fullrust:$MINOR}"
STAGE1="$ROOT/rust-$MINOR/build/x86_64-unknown-linux-gnu/stage1"

[[ -d "$STAGE1" ]] || { echo "missing $STAGE1 — run $ROOT/build-fork.sh first" >&2; exit 1; }

CTX="$HERE/context"
rm -rf "$CTX"; mkdir -p "$CTX"
# Hardlink the 700 MB+ stage1 into the context (instant, no extra disk).
cp -al "$STAGE1" "$CTX/stage1"
cp -a  "$ROOT/fullrust-ecosystem" "$CTX/ecosystem"
cp -a  "$HERE/entrypoint.sh" "$CTX/entrypoint.sh"
cp -a  "$HERE/Dockerfile"    "$CTX/Dockerfile"

echo "== docker build $TAG =="
docker build -t "$TAG" "$CTX"
rm -rf "$CTX"
echo "== built $TAG =="
