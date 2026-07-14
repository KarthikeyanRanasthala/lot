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
`macos-26`. After a successful validation, it creates the matching version tag and GitHub Release,
attaching the archive and its SHA-256 checksum.

## Create and validate an archive locally

```sh
mise run fetch-dotlottie
mise run package-source-release
scripts/validate-source-release.sh dist/lot-<version>-source.tar.gz
```
