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

The headless interface is:

```sh
lot animation.lottie --headless --width <number> --height <number> --fps <number> \
  --animation-id <string> --theme <string>
```

It emits one animation pass as tightly-packed RGBA frames suitable for piping to tools such as
`ffmpeg`. `--width`, `--height`, and `--fps` are required. Output contains no container headers,
progress, or terminal escape sequences: each frame is exactly `width × height × 4` bytes in RGBA
byte order. Consumers must be given the same dimensions and frame rate passed to `lot`; diagnostics
are written to standard error. Headless playback does not loop and ends after the selected
animation's duration.

```sh
lot animation.lottie --headless --width 512 --height 512 --fps 30 \
  | ffmpeg -f rawvideo -pixel_format rgba -video_size 512x512 -framerate 30 -i - \
      -c:v libx264 -pix_fmt yuv420p output.mp4
```

`--animation-id` and `--theme` select IDs declared in a dotLottie manifest. They are invalid for
standalone Lottie JSON input; omitting them uses the manifest-default animation and its optional
initial theme.

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
- Use `#006a5f` as the primary color with white text on a dark theme.
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
