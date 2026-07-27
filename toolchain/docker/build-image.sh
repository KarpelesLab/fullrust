#!/usr/bin/env bash
# Build the fullrust toolchain image locally, mirroring what the CI does: package
# the prebuilt stage1 into the tarball the Dockerfile downloads, then build. CI
# fetches the tarball from a release asset; here we serve it over localhost so the
# same URL-based Dockerfile works unchanged. Run ../build-fork.sh first.
#
# Usage:  ./build-image.sh [minor] [tag]
#   ./build-image.sh 1.88 ghcr.io/karpeleslab/fullrust:1.88
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
MINOR="${1:-1.88}"
TAG="${2:-fullrust:$MINOR}"
PORT="${FULLRUST_HTTP_PORT:-8231}"

TARBALL="$HERE/fullrust-toolchain-$MINOR-x86_64.tar.gz"
"$HERE/package-toolchain.sh" "$MINOR" "$TARBALL"

# Serve the tarball so the Dockerfile's curl can fetch it (build runs with
# --network host so it can reach 127.0.0.1).
python3 -m http.server "$PORT" --directory "$HERE" --bind 127.0.0.1 >/dev/null 2>&1 &
srv=$!
trap 'kill "$srv" 2>/dev/null || true; rm -f "$TARBALL"' EXIT
sleep 1

echo "== docker build $TAG =="
docker build --network host \
  --build-arg "FULLRUST_TOOLCHAIN_URL=http://127.0.0.1:${PORT}/$(basename "$TARBALL")" \
  -f "$HERE/Dockerfile" -t "$TAG" "$ROOT"
echo "== built $TAG =="
