#!/usr/bin/env bash
# ci/rust-build.sh <ear|bridge> <native|musl> — the single rust build body for
# the CI DAG. .woodpecker.yml carries one thin step per crate x target pair;
# the body lives here because a real woodpecker `matrix:` expands to separate
# WORKFLOWS (one per combination), which would fork this repo's single
# workflow: 4 kaniko pushes, 4 tag attempts, and release-assets /
# ci-failure-report unable to depend on steps in another workflow.
#
# Laws carried from the inline step bodies:
# - the whole body tees into .ci/<asset>-build.log (nothing after cargo is
#   log-blind — pipeline history 2026-10-29)
# - native runs cargo test, musl does not (cross target, not runnable here)
# - musl gets its own CARGO_TARGET_DIR: concurrent native+musl builds of the
#   same crate would otherwise serialize on the target lock
# - smoke = run the artifact, expect the usage die (rc 2) + usage banner
set -euo pipefail

CRATE=${1:-}
TARGET=${2:-}
case "$CRATE" in ear|bridge) ;; *) echo "usage: $0 <ear|bridge> <native|musl>"; exit 2;; esac
case "$TARGET" in native|musl) ;; *) echo "usage: $0 <ear|bridge> <native|musl>"; exit 2;; esac

SUF=""
if [ "$TARGET" = "musl" ]; then SUF="-musl"; fi
ASSET="$CRATE$SUF"

mkdir -p .ci
{
  if [ "$TARGET" = "native" ]; then
    timeout 900 cargo build --release --manifest-path "$CRATE/Cargo.toml"
    timeout 600 cargo test --release --manifest-path "$CRATE/Cargo.toml"
    cp "$CRATE/target/release/chorux-$CRATE" ".ci/$ASSET"
  else
    apt-get update -qq && apt-get install -y -qq musl musl-tools >/dev/null
    rustup target add x86_64-unknown-linux-musl
    export CC_x86_64_unknown_linux_musl=musl-gcc
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc
    export CARGO_TARGET_DIR="$PWD/$CRATE/target-musl"
    timeout 1200 cargo build --release --target x86_64-unknown-linux-musl --manifest-path "$CRATE/Cargo.toml"
    cp "$CRATE/target-musl/x86_64-unknown-linux-musl/release/chorux-$CRATE" ".ci/$ASSET"
  fi
  set +e
  SMOKE=$(.ci/$ASSET 2>&1); RC=$?
  set -e
  [ "$RC" -eq 2 ] || { echo "::error::smoke failed: rc=$RC (want the usage die, rc 2)"; exit 1; }
  echo "$SMOKE" | grep -q "usage: chorux-$CRATE" || { echo "::error::smoke failed: no usage banner in output"; exit 1; }
  echo "smoke green: usage banner present, rc 2 ($TARGET)"
} 2>&1 | tee ".ci/$ASSET-build.log"
