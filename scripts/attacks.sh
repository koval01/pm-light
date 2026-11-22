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

# Cargo names the test program it built, wherever its layout puts it, and the path is kept beside the build. Stylo, the
# CSS engine of the mail's page, writes part of itself with Python when it is built: the image has none of its own.
docker run --rm -v "$SOURCE":/src:ro -v pml-attacks-target:/target -v pml-attacks-cargo:/usr/local/cargo/registry \
  -w /src -e CARGO_TARGET_DIR=/target "$IMAGE" \
  bash -c "apt-get update -qq >/dev/null && apt-get install -y -qq --no-install-recommends python3 >/dev/null \\
    && cargo test --release -p proton-core --lib --no-run --config 'build.rustflags=[]' --message-format=json \
    | sed -n 's/.*\"executable\":\"\([^\"]*\)\".*/\1/p' | tail -n 1 > /target/attacks-program"

docker run --rm --network none --memory 1g --memory-swap 1g --cpus 2 --pids-limit 64 --read-only \
  --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges -v pml-attacks-target:/target:ro "$IMAGE" \
  bash -c 'program=$(cat /target/attacks-program); test -x "$program" || { echo "no test program was built"; exit 1; }
    timeout 300 "$program" --ignored --skip live_ --nocapture 2>&1' |
  grep -E "KB ->|^test result|panicked|took|FAILED|mails parsed|turned away|^mail:|^editor:|^page:"
