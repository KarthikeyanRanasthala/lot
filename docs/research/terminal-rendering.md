# Terminal Rendering Research

Researched 2026-07-14. This document describes how `lot` should turn dotLottie frames into
terminal output. It is a design/research record, not an implementation commitment.

## Executive summary

`dotlottie-rs` can render a Lottie frame into a software pixel buffer. The right transport then
depends on the terminal:

| Terminal | Static pixel frames | Native animation | Required transport |
| --- | --- | --- | --- |
| Kitty | Direct RGB/RGBA | Yes | Kitty graphics protocol; raw RGBA is preferred |
| Ghostty | Direct RGB/RGBA | No | Kitty graphics protocol; application plays frames |
| WezTerm | Direct RGB/RGBA | Not through Kitty | Kitty graphics plus application playback, or an encoded animated APNG/GIF through its iTerm2 protocol |
| iTerm2 | No raw-frame API | Animated GIF documented | One complete encoded image file through OSC 1337 |
| Apple Terminal.app | No | No | Text/metadata fallback only |

Use an explicit terminal capability layer. Do not assume `TERM`, a terminal name, or a graphics
protocol implies animation support.

## Renderer input from dotlottie-rs

The current project deliberately enables only dotLottie-container reading. Rendering will require
enabling the CPU ThorVG features in `dotlottie-rs` (for example `tvg` and `tvg-cpu`) and creating a
`Player` with a software target.

The relevant API is:

1. Allocate a `Vec<u32>` of `width * height` pixels.
2. Call `Player::set_sw_target(&mut buffer, width, height, color_space)`.
3. Load the JSON or dotLottie bytes, advance with `Player::tick(dt)`, and call `Player::render()`.
4. Convert the resulting buffer into an explicitly byte-oriented RGBA representation before a
   terminal backend consumes it.

The exact byte order must be verified with a small known-color fixture when choosing
`ColorSpace`; do not assume a `u32`'s in-memory byte order. `dotlottie-rs` exposes `ABGR8888`,
`ABGR8888S`, `ARGB8888`, and `ARGB8888S`; its WebAssembly software path currently uses
`ABGR8888S`. Keep rendered pixels in application state as `Rgba<u8>` (or an equivalent typed
buffer) rather than leaking the renderer's `u32` layout into protocol code.

## Terminal capability matrix

### Kitty

Kitty is the preferred high-fidelity backend.

- Its graphics protocol requires support for raw 24-bit RGB (`f=24`), raw 32-bit RGBA (`f=32`,
  the default), and PNG (`f=100`). Raw input supplies pixel width/height as `s`/`v`; it must be
  sRGB. Payloads can be zlib-compressed before base64 encoding (`o=z`).
  [Kitty pixel formats, compression, and transport](https://sw.kovidgoyal.net/kitty/graphics-protocol/#transferring-pixel-data)
- `lot` should send dotLottie-rendered RGBA directly (`f=32`, `s=<width>`, `v=<height>`) using
  direct transport (`t=d`). Chunk payloads correctly; when transmitting remotely, use the
  protocol's chunking rules. No PNG, APNG, or GIF conversion is needed.
- Kitty also has a real animation protocol. Upload a root image, add frames with `a=f`, set each
  frame delay in milliseconds (`z`), and start terminal-driven looping with `a=a`. It supports
  delta frames and frame composition, but a first version should upload full frames and enforce
  conservative pixel/frame limits.
  [Kitty animation control and frame composition](https://sw.kovidgoyal.net/kitty/graphics-protocol/#controlling-animations)
- This backend can be terminal-driven: the CLI may upload all frames, then stop rendering. It
  should delete image IDs when a preview changes or the TUI exits because Kitty has image-storage
  quotas. [Kitty image quotas](https://sw.kovidgoyal.net/kitty/graphics-protocol/#image-persistence-and-storage-quotas)
- Query the graphics protocol at startup rather than trusting environment variables. This also
  detects when a multiplexer prevents the escape sequence from reaching Kitty.

### Ghostty

- Ghostty officially supports the Kitty graphics protocol, so it can show static raw RGBA output
  without any intermediary format. [Ghostty features](https://ghostty.org/docs/features)
- It does **not** implement the iTerm2 OSC 1337 image extensions; it only parses their sequences.
  [Ghostty 1.3 release notes](https://ghostty.org/docs/install/release-notes/1-3-0)
- Do not use Kitty's terminal-owned animation API yet. Ghostty's open tracker says it parses
  animation frames but does not handle them. [Ghostty issue #5255](https://github.com/ghostty-org/ghostty/issues/5255)
- Therefore `lot` must own the playback clock: render one dotLottie frame per tick and replace the
  static Kitty image/placement. Direct RGBA remains the best transport; encoding APNG/GIF would
  not help because Ghostty has no iTerm2 image protocol.

### WezTerm

- WezTerm supports the Kitty graphics protocol, experimental SIXEL, and the iTerm2 image
  protocol. [WezTerm features](https://wezterm.org/features.html)
- Static dotLottie output can use direct raw RGBA over Kitty graphics, just as with Ghostty.
- WezTerm documents that Kitty animation support is not implemented. Use application-driven frame
  updates for the raw-RGBA backend.
  [WezTerm Kitty animation limitation](https://wezterm.org/changelog.html#20220101-133340-7edc5a5)
- WezTerm also animates GIF and APNG sent through its iTerm2-compatible `imgcat`/OSC 1337 path.
  This is the only requested terminal with a documented intermediary-format, terminal-owned
  animation route. It requires encoding the entire animation first; it is not a direct dotLottie
  frame path. [WezTerm iTerm image protocol](https://wezterm.org/imgcat.html) and
  [animated GIF/APNG changelog entry](https://wezterm.org/changelog.html#20210314-114017-04b7cedd)
- Prefer raw-RGBA application playback for a common Kitty implementation. Offer APNG/GIF as an
  optional WezTerm-specific optimization only after measuring output size, latency, and alpha
  fidelity. SIXEL is a static, palette-oriented fallback, not an animation solution.

### iTerm2

- iTerm2's inline-image API is OSC 1337 with a base64-encoded **complete file**:
  `OSC 1337;File=<arguments>:<base64-data>`. It supports multipart delivery for large files, but
  has no documented raw RGBA payload, stable image ID, in-place frame replacement, or
  terminal-driven frame-update API.
  [iTerm2 inline images protocol](https://iterm2.com/documentation-images.html)
- Animated GIFs are explicitly documented as supported. iTerm2 can display image formats macOS
  understands, but its documentation does not explicitly guarantee APNG animation; GIF is the
  safe supported choice. [iTerm2 utilities / `imgcat`](https://iterm2.com/documentation-utilities.html)
- To animate in iTerm2, render all needed frames, encode one animated GIF, and send it once. GIF
  has a 256-colour palette and limited transparency, so use a static PNG or a text fallback when
  fidelity matters. Streaming individual PNGs is not a usable preview-update mechanism because
  the protocol does not document placement replacement.
- iTerm2 accepts output size in cells, pixels, percentages, or `auto`, and can report cell size.
  [iTerm2 image dimensions and cell-size reporting](https://iterm2.com/documentation-images.html)

### Apple Terminal.app

Apple Terminal.app has no supported Kitty graphics, iTerm2 inline-image, or SIXEL graphics
protocol. A current capability probe reports no response for all three families.
[Apple Terminal capability result](https://ucs-detect.readthedocs.io/sw_results/appleterminal.html)

No APNG or GIF encoding can create inline graphics where the terminal has no image protocol.
Show the existing metadata panel and a concise message instead. A later optional fallback could
use Unicode half-block colour cells, but should be opt-in because it is lower fidelity and costly
to repaint at animation frame rates.

## Ratatui and ratatui-image

Ratatui supplies the retained text-cell UI; it does not select or implement a terminal image
protocol. Its `Cell::set_skip` support lets image widgets protect graphics-covered cells from the
text diff, which is important when graphics and text occupy the same frame.
[Ratatui skipped cells](https://ratatui.rs/highlights/v023/)

`ratatui-image` v11.0.6 is the appropriate static-image integration layer:

- `Picker` queries capabilities and picks `Kitty`, `Sixel`, `Iterm2`, or a Unicode-half-block
  fallback. It should be initialized after entering the alternate screen and before starting
  normal event reads. [Picker source](https://docs.rs/ratatui-image/latest/src/ratatui_image/picker.rs.html)
- Feed it `image::DynamicImage`; a dotLottie frame therefore becomes
  `ImageBuffer<Rgba<u8>, _>` and then `DynamicImage::ImageRgba8`.
- Its generated output is protocol-specific: Kitty data, SIXEL, iTerm2 base64 PNG, or half-block
  text. It does not expose Kitty's native animation commands and has no animation scheduler or
  APNG/GIF encoder. [ratatui-image README and compatibility table](https://github.com/ratatui/ratatui-image#readme)
- Re-encoding a changing image can block the draw loop. Use `ThreadProtocol`/a worker for each
  new frame, retain the latest completed protocol state, and never perform a full PNG/SIXEL
  encode in the input-event path. [StatefulImage blocking warning](https://docs.rs/ratatui-image/latest/ratatui_image/struct.StatefulImage.html)
- The crate's compatibility guidance identifies Kitty for Kitty and Ghostty, iTerm2 for iTerm2,
  and iTerm2 as WezTerm's most reliable path. It is still a static-image abstraction, so its
  WezTerm iTerm2 choice does not automatically produce a one-file animated GIF/APNG.

`ratatui-image` should be the default renderer for static preview and fallback output. Add a
small custom Kitty backend alongside it if native Kitty animation is a product goal; forcing that
protocol through `ratatui-image` would re-encode/redraw every frame and lose Kitty's frame API.

## Recommended implementation order

1. **CPU-frame renderer:** enable the minimal `dotlottie-rs` CPU feature set; add a known-colour
   fixture and a unit test proving the buffer-to-RGBA conversion.
2. **Capability model:** define `NoPixels`, `Halfblocks`, `Sixel`, `ItermStatic`, `KittyStatic`,
   and `KittyAnimated`. Probe actively, provide a configuration override, and preserve safe
   fallbacks when SSH/tmux blocks queries.
3. **Static preview:** use `ratatui-image` with `Picker` and a worker. This covers a first frame
   in Kitty, Ghostty, WezTerm, iTerm2, compatible SIXEL terminals, and text fallback.
4. **Software playback:** add an application tick that advances dotLottie and replaces the static
   image for Ghostty and WezTerm. Cap FPS/resolution and drop stale worker results instead of
   queueing them.
5. **Kitty-native animation:** upload a bounded sequence of direct RGBA frames with frame delays,
   start terminal looping, and clean up image IDs. Activate only after a successful Kitty query
   and only for Kitty itself, not merely a terminal that recognizes the static protocol.
6. **Optional encoded animation:** add GIF encoding for iTerm2. Add APNG/GIF selection for
   WezTerm only behind an explicit capability/configuration path; verify transparency and remote
   throughput before making it default.

## Validation plan

- Test each requested terminal locally and record version, OS, local/SSH/tmux context, negotiated
  protocol, source dimensions, target cells/pixels, FPS, CPU, and terminal memory.
- Unit-test protocol bytes with fixed small RGBA fixtures, including transparent and known-colour
  pixels; never test byte order only visually.
- Integration-test capability detection with mocked replies, plus forced backend selection.
- Test resize, alt-screen exit, selection changes, terminal reset, and cleanup of protocol image
  IDs. Confirm no stale images survive after leaving the TUI.
