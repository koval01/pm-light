#!/bin/bash
# Builds with network, then runs the hostile inputs with none, read-only files and hard limits.
# Only ever run this in Docker, never on the host.
set -euo pipefail

IMAGE=rust:1.100-slim
SOURCE="$(cd "$(dirname "$0")/.." && pwd)"

# Everything this run makes in Docker goes when it ends, however it ends.
# The image goes only if this run fetched it: one that was here before may be someone else's.
had_image="$(docker image inspect "$IMAGE" >/dev/null 2>&1 && echo yes || true)"
cleanup() {
    docker volume rm -f pml-attacks-target pml-attacks-cargo >/dev/null 2>&1 || true
    [ -n "$had_image" ] || docker image rm "$IMAGE" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker volume create pml-attacks-target >/dev/null
docker volume create pml-attacks-cargo >/dev/null

docker run --rm -v "$SOURCE":/src:ro -v pml-attacks-target:/target -v pml-attacks-cargo:/usr/local/cargo/registry \
  -w /src -e CARGO_TARGET_DIR=/target "$IMAGE" \
  cargo test --release -p proton-core --no-run --config 'build.rustflags=[]' 2>&1 | tail -2

docker run --rm --network none --memory 1g --memory-swap 1g --cpus 2 --pids-limit 64 --read-only \
  --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges -v pml-attacks-target:/target:ro "$IMAGE" \
  bash -c 'timeout 300 "$(ls -t /target/release/deps/proton_core-* | grep -v "\.d$" | head -1)" --ignored --skip live_ --nocapture 2>&1' |
  grep -E "KB ->|^test result|panicked|took|FAILED|mails parsed|turned away|^mail:|^editor:"
