#!/usr/bin/env bash
# Everything a build must pass first, in the order that fails fastest. scripts/bundle-macos.sh and CI
# run it before they build: a build that skips it is not one to hand anyone.
#
#   dependencies  cargo-deny: known vulnerabilities, yanked crates, licenses, where crates come from
#   lints         clippy over every crate and test, warnings as errors; the rules are in Cargo.toml
#                 and forbid unwrap, expect, panic and indexing that can fail
#   tests         every test, including the dictionaries and the English-only check
set -euo pipefail
cd "$(dirname "$0")/.."
# Old builds go however this ends: a failed check builds as much as a passing one.
trap 'python3 -I scripts/prune-target.py' EXIT

echo "== dependencies"
cargo deny check

echo "== lints"
cargo clippy --workspace --all-targets --locked -- -D warnings

echo "== tests"
cargo test --workspace --locked

echo "== all checks passed"
