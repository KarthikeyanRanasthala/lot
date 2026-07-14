#!/usr/bin/env bash
set -euo pipefail

# Keep the native dependency out of Git while making its exact upstream release reproducible.
readonly TAG="v0.1.58"
readonly REPOSITORY="https://github.com/LottieFiles/dotlottie-rs.git"
readonly DESTINATION="deps/dotlottie-rs"

if [[ -d "$DESTINATION/.git" ]]; then
    current_tag="$(git -C "$DESTINATION" describe --tags --exact-match HEAD 2>/dev/null || true)"
    if [[ "$current_tag" == "$TAG" ]]; then
        git -C "$DESTINATION" submodule update --init --depth 1 --recommend-shallow
        echo "dotlottie-rs $TAG is already available at $DESTINATION"
        exit 0
    fi

    echo "dotlottie-rs at $DESTINATION is not $TAG; remove it and run this task again." >&2
    exit 1
fi

mkdir -p deps
git clone --depth 1 --shallow-submodules --recurse-submodules --branch "$TAG" "$REPOSITORY" "$DESTINATION"
