# fullrust toolchain image — compile any Cargo project into a static, libc-free
# `x86_64-unknown-linux-fullrust` binary.
#
# Packaging image (built by ./build-image.sh from a prebuilt stage1; building the
# toolchain from source would rebuild LLVM). Slim base: debian-slim + the cargo
# from the rust bootstrap + our patched stage1 — no unused stock rustc/std.
#
# rustc, target, the ecosystem patches, and RUSTC_BOOTSTRAP are baked into the
# image env + cargo config, so a plain `cargo build` here is already a fullrust
# build. That means the composite action (`docker run …`) and a GitHub
# `container:` job that runs a raw `cargo build` produce the IDENTICAL build.
FROM debian:bookworm-slim

# Host toolchain for build scripts / proc-macros (compiled for the host) +
# cargo's libgcc_s + TLS roots + git for git deps. The fullrust *target* links
# with the bundled rust-lld and pulls in no C — the binary stays libc-free.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates git \
    && rm -rf /var/lib/apt/lists/*

COPY cargo         /usr/local/bin/cargo
COPY stage1        /opt/fullrust/stage1
COPY ecosystem     /opt/fullrust/ecosystem
COPY config.toml   /opt/fullrust/cargo/config.toml
COPY entrypoint.sh /usr/local/bin/fullrust-build

ENV FULLRUST_HOME=/opt/fullrust \
    RUSTC=/opt/fullrust/stage1/bin/rustc \
    CARGO_BUILD_TARGET=x86_64-unknown-linux-fullrust \
    CARGO_HOME=/opt/fullrust/cargo \
    RUSTC_BOOTSTRAP=1

RUN chmod +x /usr/local/bin/fullrust-build /usr/local/bin/cargo \
    && mkdir -p /opt/fullrust/cargo-noeco \
    # Sanity: the target is built into this rustc and cargo drives it.
    && "$RUSTC" --print target-list | grep -q '^x86_64-unknown-linux-fullrust$' \
    && cargo --version >/dev/null

WORKDIR /src
ENTRYPOINT ["/usr/local/bin/fullrust-build"]
CMD ["build", "--release"]
