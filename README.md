# lot

Preview Lottie JSON and dotLottie animations in Kitty-graphics-compatible terminals, or render
them as raw RGBA frames for tools such as `ffmpeg`.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](LICENSE)

## Install

### Homebrew (macOS and Linux)

Install `lot` from the published tap:

```sh
brew install KarthikeyanRanasthala/tap/lot
```

Homebrew builds `lot` from its released source archive on your machine.

### Build from source

The pinned `dotlottie-rs` release and its ThorVG submodule are fetched shallowly into the ignored
`deps/` directory before building.

```sh
mise install
mise run fetch-dotlottie
cargo run -- animation.lottie
```

## Preview an animation

Pass a local `.lottie` file, a Lottie JSON file, or an `http(s)` URL:

```sh
lot animation.lottie
lot animation.json
lot https://example.com/animation.lottie
```

For dotLottie files, use the arrow keys or mouse wheel to choose an animation or theme. URL
downloads report progress in the terminal.

## Create an MP4

`lot` can pipe rendered frames to any tool that accepts raw RGBA video. To create an MP4, pipe
the output to `ffmpeg`:

```sh
lot animation.lottie --headless --width 512 --height 512 --fps 30 \
  | ffmpeg -y -f rawvideo -pixel_format rgba -video_size 512x512 -framerate 30 -i - \
      -c:v libx264 -pix_fmt yuv420p output.mp4
```

`--headless` writes one animation pass as tightly-packed RGBA frames to standard output. All of
`--width`, `--height`, and `--fps` are required. The stream has no container headers: every frame
is exactly `width × height × 4` bytes in RGBA byte order. Diagnostics, including URL-download
progress, remain on standard error so they do not corrupt the video stream.

For dotLottie inputs, `--animation-id` selects a manifest animation and `--theme` selects a
manifest theme. Without either flag, the file's default animation and initial theme are used.
The renderer stops after one animation pass; use the output tool to loop or otherwise package the
video. `--animation-id` and `--theme` are rejected for standalone Lottie JSON inputs.

## Controls

- Up / Down or mouse wheel — change the focused animation or theme
- Tab — switch between animations and themes
- Esc — exit

## Terminal preview support

`lot` uses the Kitty graphics protocol to display rendered RGBA frames. It supports terminals
with a compatible Kitty-graphics implementation that `lot` currently recognizes:

| Terminal | Support | Preview strategy |
| --- | --- | --- |
| Kitty | Supported | Double-buffered frame updates |
| WezTerm | Supported; Kitty graphics are enabled by default | Double-buffered frame updates |
| Ghostty | Supported | Stable single-image updates |
| Warp | Supported | Stable single-image updates |

Frames are capped at 30 fps while the animation timeline follows wall-clock time. Other terminals
remain usable as metadata viewers and show a renderer-unavailable state.

### In action

<p align="center">
  <img src="docs/images/terminals/wezterm.png" alt="lot previewing an emoji animation in WezTerm" width="900">
</p>
<p align="center"><em>WezTerm</em></p>

<p align="center">
  <img src="docs/images/terminals/kitty.png" alt="lot previewing an emoji animation in Kitty" width="900">
</p>
<p align="center"><em>Kitty</em></p>

<p align="center">
  <img src="docs/images/terminals/ghostty.png" alt="lot previewing an emoji animation in Ghostty" width="900">
</p>
<p align="center"><em>Ghostty</em></p>

<p align="center">
  <img src="docs/images/terminals/warp.png" alt="lot previewing an emoji animation in Warp" width="900">
</p>
<p align="center"><em>Warp</em></p>

## Develop and release

Run the checks before contributing:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

See [source-release maintenance](docs/source-releases.md) for the vendored source archive and
release-branch workflow.

## Documentation

- [Product specification](docs/spec.md)
- [Technical decisions](DECISIONS.md)
- [Terminal rendering research](docs/research/terminal-rendering.md)
- [Terminal image-buffering research](docs/research/image-buffering.md)
