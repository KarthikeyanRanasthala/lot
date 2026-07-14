# Product specification

## Inputs

`lot` accepts a local dotLottie file, Lottie JSON file, or URL:

```sh
lot animation.lottie
lot animation.json
lot <url>
```

URL loading should report download progress. dotLottie containers and standalone JSON are
validated with dotlottie-rs before entering the TUI.

The planned headless interface is:

```sh
lot animation.lottie --headless --width <number> --height <number> --fps <number> \
  --animation-id <string> --theme <string>
```

It will emit raw frames suitable for piping to tools such as `ffmpeg`. It is not implemented yet.

## Rendering

- Use dotlottie-rs with its CPU ThorVG backend.
- Support packaged images and fonts, Lottie expressions, and dotLottie themes.
- Honour the manifest-default animation; use the first animation when no manifest default exists.
- Honour an animation's optional `initialTheme`. When themes are available, expose `Default` to
  restore the unthemed source colors.
- Ignore animation background metadata for now.
- Tick/render ThorVG only when a frame can be presented. Limit image presentation to 30 fps while
  advancing the source timeline with elapsed wall-clock time.

## Terminal UI

- For dotLottie, show an animation list followed by a theme list. Hide the sidebar for standalone
  JSON.
- Lists are keyboard- and mouse-scrollable.
- The preview shows the active terminal renderer, the animation, and centered canvas dimensions,
  duration, and frame rate along the bottom border.
- Empty theme state: `No themes available`, centered in its panel.
- Use `#019d91` as the primary color with white text on a dark theme.
- Use concise copy for loading, error, and unsupported-renderer states.

## Terminal graphics policy

- Kitty uses double-buffered image IDs.
- WezTerm uses double-buffered image IDs.
- Ghostty uses a stable single image ID.
- Warp uses a stable single image ID.

Keep the terminal transport separate from loading, rendering, and TUI layout so buffering
strategies can be replaced independently.

## Fixtures

- Multi-animation: `https://lottie.host/294b684d-d6b4-4116-ab35-85ef566d4379/VkGHcqcMUI.lottie`
- Themes: `https://lottie.host/884c11a9-e648-4b2f-9906-2c77279710b1/PalAqPKzRZ.lottie`
- JSON: `https://lottiefiles.github.io/dotlottie-web/lottie/threads.json`
