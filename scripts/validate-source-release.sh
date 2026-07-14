#!/usr/bin/env bash
set -euo pipefail

archive="${1:?Usage: scripts/validate-source-release.sh <source-archive>}"
workspace="$(mktemp -d "${TMPDIR:-/tmp}/lot-source-validation.XXXXXX")"
trap 'rm -rf "$workspace"' EXIT

tar -xzf "$archive" -C "$workspace"
source_dir="$(find "$workspace" -mindepth 1 -maxdepth 1 -type d -name 'lot-*' -print -quit)"

if [[ -z "$source_dir" || ! -f "$source_dir/Cargo.toml" ]]; then
    echo "The archive does not contain a lot source directory." >&2
    exit 1
fi

cargo build --release --offline --locked --manifest-path "$source_dir/Cargo.toml"
