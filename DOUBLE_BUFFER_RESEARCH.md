# Double-Buffered Terminal Image IDs

Researched 2026-07-14 through parallel terminal-specific investigations. This document evaluates
how a `lot` playback loop can replace a rendered static frame without blanking or leaving stale
images behind. It covers the Kitty graphics protocol and its current behavior in Kitty, WezTerm,
Ghostty, and Warp.

## Conclusion

There is no portable in-place static-image update in the Kitty graphics protocol.

The portable strategy for terminals that faithfully implement Kitty semantics is a **two-image-ID
swap**:

1. Keep the current frame (`front`) placed on screen.
2. Upload the next frame to the unplaced ID (`back`).
3. In a synchronized-update block, place `back` at the preview position and delete `front`'s
   placement.
4. Swap the application-side `front`/`back` roles.

This is required for Kitty and is the safe default for WezTerm. Ghostty has an observed,
implementation-specific in-place refresh that must not become the shared backend contract. Warp's
current source has incomplete delete behavior, so do not use a two-slot playback loop there until
it is verified in a released build.

## Terms

- **Image ID (`i`)**: application-chosen nonzero identifier for pixel data.
- **Placement ID (`p`)**: identifier for a particular display of that image. A placement is
  identified by the `(image ID, placement ID)` pair.
- **Front / back**: the currently displayed image ID and the unplaced ID receiving the next frame.
- **Synchronized update**: `CSI ? 2026 h` begins a buffered terminal update and `CSI ? 2026 l`
  flushes it. It limits visible intermediate text/graphics operations; it is not a protocol-level
  image-transaction primitive.

## Compatibility matrix

| Terminal | Same-ID static retransmit | Two-ID swap | Native Kitty animation | Recommendation |
| --- | --- | --- | --- | --- |
| Kitty | Replaces data **and deletes all placements** | Required | Yes | Two IDs + synchronized place/delete |
| WezTerm | Do not treat as a portable in-place refresh | Supported; validate visual atomicity | Current source handles frames, but feature/config/version-test it | Two IDs + synchronized place/delete |
| Ghostty | Observed extension updates in place | Portable fallback | No | Optional Ghostty-specific same-ID path; otherwise two IDs |
| Warp | Not a documented contract | Unsafe until deletion works in a released build | No | Stable-ID refresh only after a runtime test; otherwise static poster/text fallback |

## Kitty: why two IDs are necessary

Kitty specifies that transmitting new pixel data for an existing image ID replaces the image data
and **deletes every placement** that uses that ID. The replacement is invisible until the client
places it again. Therefore retransmitting the front image ID cannot be a flicker-free static-frame
update. [Kitty display/placement semantics](https://sw.kovidgoyal.net/kitty/graphics-protocol/#display-images-on-screen)

The Kitty maintainer explicitly recommends two image IDs and a synchronized delete/place sequence
for this case; the maintainer also calls Ghostty's same-ID replacement behavior non-conforming for
Kitty compatibility. [Kitty issue #8701 clarification](https://github.com/kovidgoyal/kitty/issues/8701#issuecomment-2936864819)

### Kitty-compatible presentation algorithm

Reserve two application-owned image IDs, `A` and `B`, and one nonzero placement ID, `P`. Keep the
preview rectangle, cell dimensions, and z-index under application control.

1. Render the next dotLottie frame and fully transmit it to inactive ID `B` (`a=t`, raw RGBA).
   Do not place it yet. Kitty does not display an image until the final chunk is validated.
2. Begin synchronized output: `CSI ? 2026 h`.
3. Save cursor state and move to the preview origin.
4. Place `B` with `a=p,i=B,p=P,C=1` at the same cell rectangle as `A`. Give the incoming
   placement a higher explicit z-index while both placements overlap.
5. Delete the specific old placement: `a=d,d=i,i=A,p=P`. Use the uppercase deletion form only
   when the old backing data should be released immediately.
6. Restore cursor state, end synchronized output (`CSI ? 2026 l`), and swap `A`/`B` in app state.

Placing the same `(image ID, placement ID)` pair again is documented as a flicker-free
move/resize/replace of that *placement*. This is different from overwriting the image's pixel data.
[Kitty placement and deletion rules](https://sw.kovidgoyal.net/kitty/graphics-protocol/#deleting-images)

Do not interleave graphics commands while a chunked upload is in progress. Remote/direct payloads
have Kitty chunking restrictions, including a 4096-byte base64 chunk limit.
[Kitty remote-client transport rules](https://sw.kovidgoyal.net/kitty/graphics-protocol/#remote-client)

Kitty's `a=f`/`a=a` animation API could avoid placement churn, but it is intentionally excluded
from the common backend because the other terminals do not reliably support it.
[Kitty animation API](https://sw.kovidgoyal.net/kitty/graphics-protocol/#controlling-animations)

## WezTerm

WezTerm's Kitty support is opt-in (`enable_kitty_graphics=true`).
[WezTerm Kitty tracking issue](https://github.com/wez/wezterm/issues/986#L226-L232)

Its current implementation stores image data separately from `(image ID, placement ID)` state.
Re-transmitting an ID replaces its data record, but displayed cell attachments are created on a
placement operation. Do not assume a same-ID retransmit will visibly update an existing placement;
re-place the new frame. [WezTerm Kitty implementation](https://github.com/wez/wezterm/blob/main/term/src/terminalstate/kitty.rs#L21-L43)

The two-ID algorithm above is compatible with its explicit placement and deletion support. WezTerm
also supports synchronized output, so use the same buffered present sequence, then validate against
the exact released WezTerm version. [WezTerm synchronized output](https://wezterm.org/escape-sequences.html#mode-functions)

WezTerm's current source does handle Kitty frame-transmit/composition actions, despite older
release notes saying animation was unavailable. Treat that as a version-gated optimization, not as
the portable baseline. [WezTerm animation-frame handling](https://github.com/wez/wezterm/blob/main/term/src/terminalstate/kitty.rs#L540-L742)

## Ghostty

Ghostty officially supports the Kitty graphics protocol and aims for compatibility with the
originating protocol. [Ghostty features](https://ghostty.org/docs/features) and
[external-protocol policy](https://ghostty.org/docs/vt/external)

In a reported cross-terminal test, Ghostty accepts a same-ID `a=t` retransmit and updates the
already displayed image in place. Kitty rejects that as non-conforming replacement behavior.
[Cross-terminal behavior report](https://github.com/kovidgoyal/kitty/issues/8701#issue-3154012156)

That makes a single stable ID attractive for a **Ghostty-only, version-tested** fast path. It is
not suitable for the shared Kitty backend. Ghostty also lacks native Kitty frame-animation support,
so it still needs the dotLottie application clock. Its portable fallback is the two-ID static swap;
use synchronized output where supported, and test explicitly for overlap/flicker.

## Warp

Warp is listed by Kitty as an implementing terminal and its public source parses static Kitty
actions for transmit, transmit-and-display, place, query, and delete. It accepts RGB, RGBA, and
PNG input, but has no frame-upload (`a=f`) or animation-control (`a=a`) action.
[Kitty protocol implementers](https://sw.kovidgoyal.net/kitty/graphics-protocol/) and
[Warp Kitty action parser](https://github.com/warpdotdev/warp/blob/master/app/src/terminal/model/kitty.rs#L384-L453)

Warp's image map is keyed by `(image ID, placement ID)`, so two IDs/placements are represented.
[Warp image map](https://github.com/warpdotdev/warp/blob/master/app/src/terminal/model/image_map.rs#L19-L26)
However, the current public ANSI handler parses `Delete` and then applies it as a no-op. A two-ID
swap may therefore leave old placements or cached images behind.
[Warp delete application branch](https://github.com/warpdotdev/warp/blob/master/app/src/terminal/model/grid/ansi_handler.rs#L1746-L1976)

Until released Warp behavior proves otherwise:

- do not use terminal-owned animation;
- do not select the two-ID backend by default;
- optionally probe a bounded same-ID refresh at startup and cap its FPS if it works; and
- fall back to one static poster frame or text/metadata when it does not.

No separate public Warp graphics/animation protocol was found; its public implementation is a
Kitty-protocol parser.

## Engineering safeguards

- Allocate nonzero image/placement IDs from an application namespace; never use unscoped defaults
  during playback.
- Allow only one upload in flight. If a new dotLottie frame arrives first, discard the stale frame
  rather than queueing it.
- Cap pixel dimensions, FPS, image count, and retained bytes. Use two static RGBA buffers only for
  the common backend.
- On resize, hide/clear the old placement, rebuild the target buffer, then start with a fresh front
  frame. Do not mix a previous cell geometry with the new placement.
- On preview change, terminal reset, or TUI exit, delete both placements and release backing data
  only on terminals where that behavior has been verified.
- Test direct local use, SSH, tmux passthrough, alternate-screen redraw, resize, and rapid preview
  changes independently for each terminal/version. Capability queries establish parsing support,
  not correct double-buffer semantics.

## Proposed backend selection

1. **Kitty:** two-ID synchronized static backend.
2. **WezTerm:** same two-ID backend when Kitty graphics is explicitly enabled; otherwise use the
   separately researched iTerm image/static fallback.
3. **Ghostty:** default to the two-ID backend for compatibility; consider a measured same-ID
   optimization only behind a terminal/version-specific flag.
4. **Warp:** static poster/text fallback until released deletion and placement behavior passes a
   focused runtime conformance test.
