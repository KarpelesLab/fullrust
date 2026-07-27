# fullrust toolchain image — compile any Cargo project into a static, libc-free
# `x86_64-unknown-linux-fullrust` binary.
#
# Like KarpelesLab/decryptd, this image is a thin package around a published
# release asset: it downloads the prebuilt fullrust stage1 toolchain tarball
# (rustc + host & fullrust sysroots + rust-lld + the bootstrap cargo) and adds
# the ecosystem bundle + entrypoint. The heavy toolchain build runs once in the
# `fullrust toolchain` workflow (which attaches the tarball to a release); this
# Dockerfile just repackages it, so the image rebuilds in seconds — e.g. when
# only the ecosystem bundle changes — without rebuilding the compiler.
#
# Build context is `toolchain/`; built in CI by docker/fullrust-image.yml via
# docker/build-push-action with --build-arg FULLRUST_TOOLCHAIN_URL=<asset>.
#
# rustc, target, the ecosystem patches, and RUSTC_BOOTSTRAP are baked into the
# image env + cargo config, so a plain `cargo build` here is already a fullrust
# build — the composite action (`docker run …`) and a GitHub `container:` job
# running a raw `cargo build` produce the IDENTICAL build.

# Node.js (what actions/checkout@v6 targets — node 24) so GitHub `container:`
# jobs can run JS actions in-container. Lean: just the node binary; its
# libstdc++6/libc deps come from build-essential. The bookworm-built node binary
# runs fine on the newer trixie base (forward-compatible glibc/libstdc++).
FROM node:24-bookworm-slim AS node

# Trixie (glibc 2.41 / libstdc++ from GCC 14), NOT bookworm: the host rustc/cargo
# shipped in the toolchain asset are built on the ubuntu-24.04 CI runner and
# require GLIBC_2.39 + GLIBCXX_3.4.32, which bookworm (glibc 2.36 / GCC 12) lacks.
# `fullrust-toolchain.yml` pins runs-on: ubuntu-24.04 so this floor stays fixed.
FROM debian:trixie-slim

# Host toolchain for build scripts / proc-macros + cargo's libgcc_s + node's
# libstdc++6 + TLS roots + git; curl to fetch the toolchain tarball at build.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates git curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=node /usr/local/bin/node /usr/local/bin/node

# The prebuilt stage1 toolchain + bootstrap cargo, a GitHub release asset (the
# tarball extracts to `stage1/` + `cargo`). Override to pin a specific version.
ARG FULLRUST_TOOLCHAIN_URL
RUN set -eux; \
    test -n "${FULLRUST_TOOLCHAIN_URL}"; \
    mkdir -p /opt/fullrust; \
    curl -fSL --retry 5 --retry-delay 3 --retry-all-errors "${FULLRUST_TOOLCHAIN_URL}" \
      | tar -xz --no-same-owner -C /opt/fullrust; \
    mv /opt/fullrust/cargo /usr/local/bin/cargo; \
    chmod +x /usr/local/bin/cargo

# Ecosystem bundle (getrandom 0.2/0.4, socket2 forks) + entrypoint, from the
# repo (context = toolchain/). Generate the cargo config that patches those
# crates for fullrust: patch KEY = dir name (so two majors of one crate coexist),
# `package` = the real crate name cargo matches by version.
COPY fullrust-ecosystem /opt/fullrust/ecosystem
COPY docker/entrypoint.sh /usr/local/bin/fullrust-build
RUN set -eux; \
    mkdir -p /opt/fullrust/cargo /opt/fullrust/cargo-noeco; \
    { echo "# Generated: ecosystem [patch.crates-io] for the fullrust target."; \
      echo "[patch.crates-io]"; \
      for d in /opt/fullrust/ecosystem/crates/*/; do \
        dir="$(basename "$d")"; \
        pkg="$(sed -n 's/^name *= *"\([^"]*\)".*/\1/p' "$d/Cargo.toml" | head -1)"; \
        echo "\"$dir\" = { path = \"/opt/fullrust/ecosystem/crates/$dir\", package = \"$pkg\" }"; \
      done; } > /opt/fullrust/cargo/config.toml; \
    chmod +x /usr/local/bin/fullrust-build

ENV FULLRUST_HOME=/opt/fullrust \
    RUSTC=/opt/fullrust/stage1/bin/rustc \
    CARGO_BUILD_TARGET=x86_64-unknown-linux-fullrust \
    CARGO_HOME=/opt/fullrust/cargo \
    RUSTC_BOOTSTRAP=1

RUN set -eux; \
    "$RUSTC" --print target-list | grep -q '^x86_64-unknown-linux-fullrust$'; \
    cargo --version >/dev/null; \
    node --version >/dev/null

WORKDIR /src
ENTRYPOINT ["/usr/local/bin/fullrust-build"]
CMD ["build", "--release"]
