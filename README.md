# lot cli

## requirements

- should use dotlottie-rs to render a lottie animation in cli `lot animation.lottie`, `lot animation.json` and `lot <url>`
- if loading from a `<url>` then show download progress
- `lot animation.lottie --headless --width <number> --height <number> --fps <number> --animation-id <string> --theme <string>` should emit raw frames which we can pipe to a tool like ffmpeg `| ffmpeg`

## tech

- rust (use std libs and public crates as much as possible)
- use ratatui & ratatui-image lib
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
