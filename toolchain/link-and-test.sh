#!/usr/bin/env bash
# Link the freshly-built stage1 toolchain as a rustup toolchain, then build and
# run the test crate for x86_64-unknown-linux-fullrust, verifying the output is
# a statically-linked, libc-free ELF.
set -euo pipefail

VER="${1:-1.88}"
HERE="$(cd "$(dirname "$0")" && pwd)"
RUST="$HERE/rust-$VER"
STAGE1="$RUST/build/x86_64-unknown-linux-gnu/stage1"
TOOLCHAIN="fullrust-$VER"
TARGET="x86_64-unknown-linux-fullrust"

if [[ ! -x "$STAGE1/bin/rustc" ]]; then
  echo "stage1 rustc not found at $STAGE1/bin/rustc — build first" >&2
  exit 1
fi

echo "== linking toolchain $TOOLCHAIN -> $STAGE1 =="
rustup toolchain link "$TOOLCHAIN" "$STAGE1"

echo "== target std present? =="
ls "$STAGE1/lib/rustlib/$TARGET/lib/" | head || true

echo "== building test crate =="
cd "$HERE/test-hello"
cargo "+$TOOLCHAIN" build --release --target "$TARGET"

BIN="target/$TARGET/release/hello-fullrust"
echo "== file =="
file "$BIN"
echo "== ldd (expect: not a dynamic executable) =="
ldd "$BIN" || true
echo "== dynamic-section NEEDED (expect: none) =="
readelf -d "$BIN" 2>/dev/null | grep -i needed || echo "  (no NEEDED entries — no shared libs)"
echo "== run =="
./"$BIN" one two three
echo "== exit code: $? =="
