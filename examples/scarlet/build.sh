#!/bin/sh
# Deliberately outside the host Cargo workspace. Only this command selects the fork.
set -eu
[ "$APPCOAT_PLATFORM" = scarlet ] || { echo 'Scarlet adapter requires --platform scarlet' >&2; exit 1; }
: "${SCARLET_RUSTC:?set SCARLET_RUSTC to the explicitly selected Scarlet compiler}"
case "$APPCOAT_TARGET" in aarch64-unknown-scarlet|riscv64gc-unknown-scarlet) ;; *) exit 1;; esac
mkdir -p build
"$SCARLET_RUSTC" --edition=2024 --crate-name appcoat_runtime --crate-type=rlib \
  --target "$APPCOAT_TARGET" ../../crates/appcoat_runtime/src/lib.rs -o build/libappcoat_runtime.rlib
"$SCARLET_RUSTC" --edition=2024 --target "$APPCOAT_TARGET" main.rs \
  --extern appcoat_runtime=build/libappcoat_runtime.rlib -o build/scarlet-reader
