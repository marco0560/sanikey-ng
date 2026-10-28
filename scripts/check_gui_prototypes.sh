#!/bin/sh
set -eu

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"

for MANIFEST in \
    prototypes/gui-protocol/Cargo.toml \
    prototypes/gui-egui/Cargo.toml \
    prototypes/gui-slint/Cargo.toml
do
    echo "==> Formattazione: $MANIFEST"
    cargo fmt --manifest-path "$MANIFEST" --all -- --check
    echo "==> Test: $MANIFEST"
    cargo test --manifest-path "$MANIFEST" --locked
done
