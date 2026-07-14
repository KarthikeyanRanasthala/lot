# Source-release maintenance

This document is for maintainers of `lot`'s Homebrew source releases. The published formula works
with Homebrew on macOS and Linux; end users should install it with:

```sh
brew install KarthikeyanRanasthala/tap/lot
```

## Release workflow

Pushing to the `release` branch produces a vendored source archive for the version in
`Cargo.toml`. The workflow rejects an existing version tag, packages the pinned dotlottie-rs and
ThorVG sources with Cargo's locked dependency set, and validates an offline release build on
`macos-26`. It then builds native bottles on macOS Apple Silicon, macOS Intel,
Linux x86_64, and Linux ARM64 from that same archive. After every build succeeds,
it creates the matching version tag and GitHub Release, attaching the source
archive, its SHA-256 checksum, bottle archives, and bottle metadata.

## Publish Homebrew bottles

After the release workflow has published `v<version>`, update the tap's
`Formula/lot.rb` source URL and checksum, then merge the bottle JSON metadata
from that same release into the formula's `bottle do` block. The release
workflow intentionally does not write to the separate tap repository.

## Create and validate an archive locally

```sh
mise run fetch-dotlottie
mise run package-source-release
scripts/validate-source-release.sh dist/lot-<version>-source.tar.gz
```
