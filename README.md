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

Other terminals remain usable as metadata viewers and show a renderer-unavailable state. Headless
raw-frame output is planned but not implemented.

## Documentation

- [Product specification](docs/spec.md)
- [Technical decisions](DECISIONS.md)
- [Terminal rendering research](docs/research/terminal-rendering.md)
- [Terminal image-buffering research](docs/research/image-buffering.md)
