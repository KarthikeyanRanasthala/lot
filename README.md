# lot

`lot` is `cat`, but for Lottie: preview Lottie JSON and dotLottie animations in
Kitty-graphics-compatible terminals, or render them as raw RGBA frames for tools such as `ffmpeg`.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](LICENSE)

## Launch trailer



https://github.com/user-attachments/assets/984b81ec-18d4-4cc6-a7d0-6579ed346820



## Install

### Homebrew (macOS and Linux)

Install `lot` from the published tap:

```sh
brew install KarthikeyanRanasthala/tap/lot
```

Homebrew downloads a prebuilt bottle on supported platforms. Where no matching
bottle is available, it builds `lot` from its released source archive on your
machine.

## Preview an animation

Pass a local `.lottie` file, a Lottie JSON file, a directory of animations, or an `http(s)` URL:

```sh
lot animation.lottie
lot animation.json
lot ./animations
lot https://lottie.host/294b684d-d6b4-4116-ab35-85ef566d4379/VkGHcqcMUI.lottie
```

For a single file, use the arrow keys or mouse wheel to choose a dotLottie animation or theme.
Mouse wheel selection stops at the first and last item; arrow-key selection cycles. URL downloads
show a progress bar, percentage, and adaptive units such as B, KB, and MB.

### Directory playlist mode

When the input path is a directory, `lot` opens an interactive playlist:

- Recursively discovers `.json` and `.lottie` files (case-insensitive extensions)
- Sorts entries with natural ordering (`animation2` before `animation10`)
- Shows a searchable file list beside the preview
- Watches the directory for adds, modifications, renames, and deletes, and refreshes the playlist
  without restarting
- Keeps the current selection (and playback of an unchanged file) when possible after updates
- Skips directory symlinks so recursive links cannot loop
- Surfaces load errors for corrupt or unsupported files without leaving the playlist

Directory mode requires an interactive terminal. Headless mode rejects directory input with a clear
error because the playlist UI is interactive-only.

## Controls

### Single file

- Up / Down or mouse wheel — change the focused animation or theme
- Tab — switch between animations and themes
- Space — pause or resume playback
- Left / Right — pause and step backward or forward one frame
- q, Esc, or Ctrl-C — exit

### Directory playlist

- Up / Down or mouse wheel — move through the focused list (playlist, animations, or themes)
- `/` — enter search mode and filter the playlist by filename (substring, case-insensitive)
- Enter or Esc — leave search mode (filter remains until cleared with Backspace)
- Tab — cycle focus between the playlist and, for dotLottie selections, animations/themes
- Space — pause or resume playback of the loaded file
- Left / Right — pause and step backward or forward one frame
- q, Esc, or Ctrl-C — exit (Esc leaves search mode first when searching)

## Create an MP4

`lot` can pipe rendered frames to any tool that accepts raw RGBA video. To create an MP4, pipe
the output to `ffmpeg`:

```sh
lot https://lottiefiles.github.io/dotlottie-web/lottie/threads.json \
  --headless --width 512 --height 512 --fps 30 \
  | ffmpeg -y -f rawvideo -pixel_format rgba -video_size 512x512 -framerate 30 -i - \
      -c:v libx264 -pix_fmt yuv420p output.mp4
```

`--headless` writes one animation pass as tightly-packed RGBA frames to standard output. All of
`--width`, `--height`, and `--fps` are required. The stream has no container headers: every frame
is exactly `width × height × 4` bytes in RGBA byte order. Diagnostics, including URL-download
progress, remain on standard error so they do not corrupt the video stream. Directory inputs are
rejected in headless mode; pass a single file or URL instead.

For dotLottie inputs, `--animation-id` selects a manifest animation and `--theme` selects a
manifest theme. Without either flag, the file's default animation and initial theme are used.
The renderer stops after one animation pass; use the output tool to loop or otherwise package the
video. `--animation-id` and `--theme` are rejected for standalone Lottie JSON inputs.

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

## Build from source

The pinned `dotlottie-rs` release and its ThorVG submodule are fetched shallowly into the ignored
`deps/` directory before building.

```sh
mise install
mise run fetch-dotlottie
cargo run -- animation.lottie
```

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
