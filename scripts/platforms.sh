#!/usr/bin/env bash
# Builds and tests the app for Linux, starts it under a virtual screen, and lints the Windows build, inside Docker.
# GNU ld runs out of Docker's memory linking the app with WebKitGTK, so lld links. Windows cannot run here, so for it this proves that it compiles, not that a window opens: that is what CI's
# pictures are for. The start on Linux echoes the journal and draws a made-up mail with `--check-picture`.
set -euo pipefail

cd "$(dirname "$0")/.."
image=ubuntu:24.04

# Everything this run makes in Docker goes when it ends, however it ends: the volumes and the image. No image of our
# own is built, so no build cache is left behind either.
# The image goes only if this run fetched it: one that was here before may be someone else's.
had_image="$(docker image inspect "$image" >/dev/null 2>&1 && echo yes || true)"
cleanup() {
    docker volume rm -f pml-platforms-target pml-platforms-cargo pml-platforms-rustup >/dev/null 2>&1 || true
    [ -n "$had_image" ] || docker image rm "$image" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker run --rm \
    -v "$PWD:/source:ro" \
    -v pml-platforms-cargo:/root/.cargo/registry \
    -v pml-platforms-rustup:/root/.rustup \
    -v pml-platforms-target:/target \
    -e CARGO_TARGET_DIR=/target \
    -e CARGO_PROFILE_DEV_DEBUG=0 -e CARGO_PROFILE_TEST_DEBUG=0 -e CARGO_BUILD_JOBS=2 \
    -e CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=lld" \
    -e CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=lld" \
    "$image" bash -uc '
        failed=0
        apt-get update -qq && apt-get install -y -qq --no-install-recommends \
            build-essential curl ca-certificates file pkg-config git \
            libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev libxdo-dev libxkbcommon-dev libxkbcommon-x11-0 \
            libfontconfig1-dev libdbus-1-dev libssl-dev gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 \
            xvfb xauth lld >/dev/null
        curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none >/dev/null 2>&1
        export PATH=/root/.cargo/bin:$PATH
        step() {
            echo "== $1"; shift
            "$@" > /tmp/step.log 2>&1; code=$?
            grep -E "^test result|FAILED|panicked|^error|^warning|^ +-->|undefined reference|multiple definition|/usr/bin/ld|collect2|by signal|SIGKILL" /tmp/step.log | cut -c1-400 | head -60
            [ "$code" -eq 0 ] || { echo "-- failed with $code"; failed=1; }
        }
        mkdir -p /work && cd /work
        cp -r /source/Cargo.toml /source/Cargo.lock /source/rust-toolchain.toml /source/clippy.toml \
            /source/LICENSE /source/NOTICE /source/.cargo /source/core /source/app /source/vendor .
        rustup show active-toolchain >/dev/null 2>&1
        rustup component add clippy >/dev/null 2>&1
        rustup target add x86_64-pc-windows-gnu >/dev/null 2>&1
        step "Linux: tests" cargo test --workspace
        echo "== Linux: start"
        if cargo build -p proton-mail-light > /tmp/start-build.log 2>&1; then
            PML_JOURNAL_ECHO=1 RUST_BACKTRACE=1 xvfb-run -a -s "-screen 0 1440x900x24" \
                timeout 30 /target/debug/proton-mail-light --check-picture=/tmp/picture.png > /tmp/start.log 2>&1
            code=$?
            if [ "$code" -eq 124 ]; then echo "ran for 30 seconds"; else echo "-- quit by itself with $code"; failed=1; fi
            cat /tmp/picture.png.txt 2>/dev/null || { echo "-- no picture check"; failed=1; }
            grep -q "^width=[1-9]" /tmp/picture.png.txt 2>/dev/null || { echo "-- the mail picture was not drawn"; failed=1; }
            grep -viE "^\s*$" /tmp/start.log | cut -c1-300 | tail -40
        else
            grep -E "^error|collect2|by signal" /tmp/start-build.log | head -20; echo "-- failed to build"; failed=1
        fi
        step "Linux: clippy" cargo clippy --workspace --all-targets -- -D warnings
        step "Windows: clippy" cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
        exit $failed
    '
