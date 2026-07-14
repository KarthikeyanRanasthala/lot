# lot cli

## requirements

- should use dotlottie-rs to render a lottie animation in cli `lot animation.lottie`, `lot animation.json` and `lot <url>`
- if loading from a `<url>` then show download progress
- `lot animation.lottie --headless --width <number> --height <number> --fps <number> --animation-id <string> --theme <string>` should emit raw frames which we can pipe to a tool like ffmpeg `| ffmpeg`

## tech

- rust (use std libs and public crates as much as possible)
- use ratatui; own the small terminal-image transport so its buffering policy can match each terminal
- dotlottie-rs should be fetched from https://github.com/lottiefiles/dotlottie-rs lib, choose the latest tag, use a shallow clone as we're operating in a metered network, may be create a mise bash script for it
- ensure we're only including necessary features in cargo.toml

## ui

- sidebar
    - show if dotlottie, hide if its a json
    - list of animations
    - followed by list of themes
    - mouse and keyboard scrollable
- preview
    - show the used terminal renderer
    - actual animation
    - canvas size, duration, fps
- use proper copy for url loading & error states, sidebar empty states
- use #019d91 for primary color and white text. maintain a dark theme

## fixtures

- multi animation: `https://lottie.host/294b684d-d6b4-4116-ab35-85ef566d4379/VkGHcqcMUI.lottie`
- themes: `https://lottie.host/884c11a9-e648-4b2f-9906-2c77279710b1/PalAqPKzRZ.lottie`
- json: `https://lottiefiles.github.io/dotlottie-web/lottie/threads.json`

## development

- leave proper code comments explaining why a certain decision is taken, what a block of code does. keep it short and precise
- keep a decision log with timestamps in a separate md file

### setup

Fetch the pinned `dotlottie-rs` release before building. The task shallow-clones the release and
its ThorVG submodule into the ignored `deps/` directory so Cargo can use a local, reproducible
dependency checkout without downloading its full history.

```sh
mise install
mise run fetch-dotlottie
cargo run -- animation.lottie
```

The TUI validates Lottie JSON and dotLottie containers, then renders frames through
`dotlottie-rs`'s CPU/ThorVG backend when the terminal supports the Kitty graphics protocol.
It follows a dotLottie manifest's default animation and initial theme; packaged images and fonts,
and ThorVG Lottie expressions, are enabled. Animation background metadata is intentionally ignored.
When packaged themes are available, the TUI also exposes `Default` to restore the animation's
unthemed source colors.
Kitty uses double-buffered image IDs; Ghostty and Warp use a stable single image ID. WezTerm can
be detected automatically and uses double-buffered image IDs.
Other terminals continue to show metadata and a clear renderer-unavailable state. Headless
raw-frame output is not implemented yet, so `--headless` reports that it is unavailable.
To avoid queuing raw image transfers faster than a terminal can display them, interactive image
updates are capped at 30 fps while the animation timeline continues at real time.
