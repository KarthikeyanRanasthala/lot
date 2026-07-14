# lot

`lot` previews Lottie JSON and dotLottie animations in terminals that support the Kitty graphics
protocol.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](LICENSE)

## Quick start

The pinned `dotlottie-rs` release and its ThorVG submodule are fetched shallowly into the ignored
`deps/` directory before building.

```sh
mise install
mise run fetch-dotlottie
cargo run -- animation.lottie
```

## Homebrew source releases

Pushing to the `release` branch produces a vendored source archive for the version in
`Cargo.toml`. The workflow rejects an existing version tag, packages the pinned dotlottie-rs and
ThorVG sources with Cargo's locked dependency set, and validates an offline release build on
`macos-26`. After a successful validation, it creates the matching version tag and GitHub Release,
attaching the archive and its SHA-256 checksum.

To create the same archive locally:

```sh
mise run fetch-dotlottie
mise run package-source-release
scripts/validate-source-release.sh dist/lot-<version>-source.tar.gz
```

The input may be a local `.lottie` file, a Lottie JSON file, or a URL. URL loading reports
download progress.

## Controls

- Up / Down or mouse wheel — change the focused animation or theme
- Tab — switch between animations and themes
- Esc — exit

## Terminal rendering

`lot` renders with dotlottie-rs and ThorVG, then sends static RGBA frames through the Kitty
graphics protocol. Frames are capped at 30 fps while their timeline continues at wall-clock time.

| Terminal | Image-ID strategy |
| --- | --- |
| Kitty | Double-buffered |
| WezTerm | Double-buffered |
| Ghostty | Stable single ID |
| Warp | Stable single ID |

Other terminals remain usable as metadata viewers and show a renderer-unavailable state.

## Headless output

Use `--headless` to write one playback as tightly-packed RGBA frames to standard output. All of
`--width`, `--height`, and `--fps` are required. The stream has no container headers: every frame
is exactly `width × height × 4` bytes in RGBA byte order. Diagnostics, including download progress
for URL inputs, remain on standard error so they do not corrupt the video stream.

```sh
cargo run -- assets/animation.lottie --headless --width 512 --height 512 --fps 30 \
  | ffmpeg -y -f rawvideo -pixel_format rgba -video_size 512x512 -framerate 30 -i - \
      -c:v libx264 -pix_fmt yuv420p output.mp4
```

For dotLottie inputs, `--animation-id` selects a manifest animation and `--theme` selects a
manifest theme. Without either flag, the file's default animation and initial theme are used.
The renderer stops after one animation pass; use the output tool to loop or otherwise package the
video. `--animation-id` and `--theme` are rejected for standalone Lottie JSON inputs.

## Documentation

- [Product specification](docs/spec.md)
- [Technical decisions](DECISIONS.md)
- [Terminal rendering research](docs/research/terminal-rendering.md)
- [Terminal image-buffering research](docs/research/image-buffering.md)
