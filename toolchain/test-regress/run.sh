#!/usr/bin/env bash
# Build + run the regression checks (see src/main.rs) for one fullrust toolchain:
#   ./run.sh 1.88
# Builds twice — with and without an extra 8-byte thread-local — and requires
# that at least one build has round_up(PT_TLS memsz, p_align) NOT a multiple of
# 16 (exactly the layout the 1.88/1.89 max(16) TLS rounding broke), checked with
# `readelf -l`.
set -u
cd "$(dirname "$0")"
V="${1:?usage: run.sh <minor, e.g. 1.88>}"
T=x86_64-unknown-linux-fullrust
# Under run-tests.sh / CI there is no rustup link: it exports RUSTC (the stage1
# rustc) and CARGO (the bootstrap cargo). Otherwise use the linked toolchain.
if [ -n "${RUSTC:-}" ] && [ -n "${CARGO:-}" ]; then CARGO_CMD=("$CARGO"); else CARGO_CMD=(cargo "+fullrust-$V"); fi
fail=0; odd=0
for variant in plain pad8; do
    flags=""; [ "$variant" = pad8 ] && flags="--cfg tls_pad8"
    rm -rf target
    RUSTFLAGS="$flags" "${CARGO_CMD[@]}" build --release --target $T >build.log 2>&1 || {
        echo "FAIL build ($variant)"; cat build.log; exit 1; }
    BIN=target/$T/release/regress-fullrust
    if readelf -d "$BIN" 2>/dev/null | grep -q NEEDED; then echo "FAIL $variant: has NEEDED"; fail=1; fi
    memsz=$(readelf -lW "$BIN" | awk '$1=="TLS"{print $6}')
    align=$(readelf -lW "$BIN" | awk '$1=="TLS"{print $8}')
    rounded=$(( (memsz + align - 1) / align * align ))
    m=$((rounded % 16))
    [ "$m" != 0 ] && odd=1
    echo "== $V $variant: PT_TLS memsz=$((memsz)) align=$((align)) round_up(memsz,align)=$rounded (%16=$m)"
    "$BIN"; code=$?
    [ "$code" = 0 ] || { echo "FAIL $variant: exit $code"; fail=1; }
done
rm -f build.log
if [ "$odd" = 0 ]; then echo "FAIL neither build had round_up(memsz, p_align) % 16 != 0"; fail=1; fi
[ "$fail" = 0 ] && echo "REGRESS $V: ALL OK" || echo "REGRESS $V: FAILED"
exit $fail
