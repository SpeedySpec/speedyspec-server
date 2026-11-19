#!/usr/bin/env bash
# Runs a command in a Linux musl container with this repository mounted, with the static PHP built for musl
# (copied from packages/speedyspec-cli/scripts/linux.sh).
#
#   scripts/linux-php.sh scripts/build-php.sh
#   scripts/linux-php.sh cargo test -p speedyspec-php --test extensions
#
# The container's PHP build lives in .php-build-linux/ on the host, mounted over /work/.php-build. On macOS the cargo
# target is a Docker volume (speedyspec-server-php-target-<arch>); SPEEDYSPEC_LINUX_TARGET=bind uses target-linux/.
# SPEEDYSPEC_LINUX_PLATFORM picks the platform (default: the host's architecture, natively). The container is thrown away after each run,
# so build-php.sh may install missing tools in it (SPEEDYSPEC_ALLOW_HOST_INSTALL=1).
set -euo pipefail

if [ "$#" -eq 0 ]; then
    echo "usage: scripts/linux.sh <command...>" >&2
    exit 2
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# The host's own architecture by default: under emulation (linux/amd64 on an arm64 Mac) the static ICU crashes as
# soon as intl is used, so a native container is the only one whose results mean anything.
case "$(uname -m)" in
    arm64|aarch64) native="linux/arm64" ;;
    *)             native="linux/amd64" ;;
esac
platform="${SPEEDYSPEC_LINUX_PLATFORM:-$native}"
suffix=""
if [ "$platform" != "linux/amd64" ]; then
    suffix="-${platform##*/}"
fi
php_build_dir="$repo/.php-build-linux$suffix"
target_dir="$repo/target-linux$suffix"
image="speedyspec-server-linux:1.99-${platform##*/}"

case "$platform" in
    linux/amd64) rust_target="x86_64-unknown-linux-musl" ;;
    linux/arm64) rust_target="aarch64-unknown-linux-musl" ;;
    *) echo "scripts/linux.sh: unsupported platform $platform" >&2; exit 2 ;;
esac

# Build tools for static-php-cli (it would otherwise `apk add` them on every
# run, since each container starts fresh) and libclang for ext-php-rs's bindgen.
#
# Host artifacts (build scripts, proc macros) must link musl dynamically:
# bindgen dlopen()s libclang, and a static musl binary cannot dlopen. Cargo has
# no stable way to give host artifacts their own flags, so the container sets
# CARGO_BUILD_TARGET (target artifacts get `--target`, host ones do not) and a
# rustc wrapper turns crt-static off only for invocations without `--target`.
# Target artifacts, including every test binary, stay fully static.
docker build --quiet --platform "$platform" --tag "$image" - >/dev/null <<'EOF'
FROM rust:1.99-alpine
RUN apk add --no-cache \
    autoconf automake bash binutils bison build-base bzip2 ca-certificates \
    clang-dev cmake curl file flex g++ gettext-dev git gzip libtool \
    linux-headers m4 make patch perl pkgconf re2c tar unzip wget xz \
    && rustup component add rustfmt
RUN printf '%s\n' \
    '#!/bin/sh' \
    'rustc="$1"; shift' \
    'for arg in "$@"; do [ "$arg" = "--target" ] && exec "$rustc" "$@"; done' \
    'exec "$rustc" "$@" -C target-feature=-crt-static' \
    > /usr/local/bin/rustc-host-dynamic \
    && chmod +x /usr/local/bin/rustc-host-dynamic
ENV RUSTC_WRAPPER=/usr/local/bin/rustc-host-dynamic
EOF

mkdir -p "$php_build_dir"
target_mount="$target_dir"
if [ "$(uname -s)" = "Darwin" ] && [ "${SPEEDYSPEC_LINUX_TARGET:-volume}" != "bind" ]; then
    target_mount="speedyspec-server-php-target-${platform##*/}"
else
    mkdir -p "$target_dir"
fi

tty_flag=()
if [ -t 0 ] && [ -t 1 ]; then
    tty_flag=(--tty --interactive)
fi

# spc authenticates its GitHub API calls with GITHUB_TOKEN when it is set (CI sets it to
# avoid the anonymous rate limit). Passed by name so the value never appears in a command line.
token_flags=()
if [ -n "${GITHUB_TOKEN:-}" ]; then
    token_flags=(--env GITHUB_TOKEN)
fi

# --init: an init (tini) as PID 1 reaps orphans, as a host does; the keeper tests watch orphaned generations exit.
exec docker run --rm --init ${tty_flag[@]+"${tty_flag[@]}"} \
    --platform "$platform" \
    --volume "$repo:/work" \
    --volume "$php_build_dir:/work/.php-build" \
    --volume "$target_mount:/work/target" \
    --volume "speedyspec-server-php-registry-${platform##*/}:/usr/local/cargo/registry" \
    ${token_flags[@]+"${token_flags[@]}"} \
    --env CARGO_BUILD_TARGET="$rust_target" \
    --env SPEEDYSPEC_ALLOW_HOST_INSTALL=1 \
    --workdir /work \
    "$image" "$@"
