#!/usr/bin/env bash
set -euo pipefail

readonly DOTLOTTIE_TAG="v0.1.58"
readonly DOTLOTTIE_DIR="deps/dotlottie-rs"
readonly THORVG_DIR="$DOTLOTTIE_DIR/dotlottie-rs/deps/thorvg"

root="$(git rev-parse --show-toplevel)"
cd "$root"

version="$(awk -F ' = ' '
    $1 == "version" {
        gsub(/"/, "", $2)
        print $2
        exit
    }
' Cargo.toml)"

if [[ -z "$version" ]]; then
    echo "Could not read the package version from Cargo.toml." >&2
    exit 1
fi

if [[ "$(git -C "$DOTLOTTIE_DIR" describe --tags --exact-match HEAD 2>/dev/null || true)" != "$DOTLOTTIE_TAG" ]]; then
    echo "Expected $DOTLOTTIE_DIR at $DOTLOTTIE_TAG; run mise run fetch-dotlottie first." >&2
    exit 1
fi

if ! git -C "$THORVG_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "ThorVG is missing; run mise run fetch-dotlottie first." >&2
    exit 1
fi

output="${1:-dist/lot-$version-source.tar.gz}"
if [[ "$output" != /* ]]; then
    output="$root/$output"
fi

mkdir -p "$(dirname "$output")"
workspace="$(mktemp -d "${TMPDIR:-/tmp}/lot-source-release.XXXXXX")"
trap 'rm -rf "$workspace"' EXIT

stage="$workspace/lot-$version"
mkdir -p "$stage/deps/dotlottie-rs/dotlottie-rs/deps/thorvg" "$stage/.cargo"

# Git archives exclude repository metadata and retain the exact source revisions.
git archive --format=tar HEAD | tar -x -C "$stage"
git -C "$DOTLOTTIE_DIR" archive --format=tar HEAD | tar -x -C "$stage/deps/dotlottie-rs"
git -C "$THORVG_DIR" archive --format=tar HEAD | tar -x -C "$stage/deps/dotlottie-rs/dotlottie-rs/deps/thorvg"

# Homebrew builds run without network access, so include every locked Cargo crate.
cargo vendor --locked --manifest-path "$stage/Cargo.toml" "$stage/vendor" > "$stage/.cargo/config.toml"

tar -C "$workspace" -czf "$output" "lot-$version"
shasum -a 256 "$output" > "$output.sha256"

echo "Created $output"
