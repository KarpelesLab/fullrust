# fullrust toolchain image — compile any Cargo project into a static, libc-free
# `x86_64-unknown-linux-fullrust` binary.
#
# This is a *packaging* Dockerfile: it copies a prebuilt stage1 compiler (built
# by ../build-fork.sh) rather than building the toolchain from source, which
# would rebuild LLVM (hours). CI builds the toolchain once, then runs this. The
# build context is produced by ./build-image.sh (stage1 + ecosystem + scripts).
FROM rust:1.88-slim-bookworm

# A host toolchain for build scripts / proc-macros, which compile for the *host*
# (x86_64-unknown-linux-gnu). The fullrust *target* itself links with the
# bundled rust-lld and pulls in no C — the produced binary stays libc-free.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates git \
    && rm -rf /var/lib/apt/lists/*

# The patched stage1 compiler: rustc + host & fullrust sysroots + rust-lld. The
# `fullrust` target is compiled into this rustc (no target JSON needed).
COPY stage1    /opt/fullrust/stage1
# The ecosystem bundle (getrandom / socket2 forks), auto-applied via --config.
COPY ecosystem /opt/fullrust/ecosystem
COPY entrypoint.sh /usr/local/bin/fullrust-build

ENV FULLRUST_HOME=/opt/fullrust \
    FULLRUST_TARGET=x86_64-unknown-linux-fullrust \
    RUSTC_BOOTSTRAP=1

RUN chmod +x /usr/local/bin/fullrust-build \
    # Link the patched compiler as a rustup toolchain (cargo falls back to the
    # base image's cargo, driving this rustc). stage1 ships no cargo of its own.
    && rustup toolchain link fullrust /opt/fullrust/stage1 \
    # Sanity: the target really is built into this rustc.
    && /opt/fullrust/stage1/bin/rustc --print target-list \
        | grep -q '^x86_64-unknown-linux-fullrust$'

WORKDIR /src
ENTRYPOINT ["/usr/local/bin/fullrust-build"]
CMD ["build", "--release"]
