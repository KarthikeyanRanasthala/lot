# Decisions

## 2026-07-15 — Publish native Homebrew bottles with the source release

`lot` remains a Homebrew formula in `KarthikeyanRanasthala/tap`. Its release
workflow packages the vendored source archive, builds native macOS and Linux
bottles from that exact archive, and publishes every artifact to the same
versioned GitHub Release. The tap formula is updated manually afterward with
the source checksum and generated bottle checksums. This eliminates the
Rust/LLVM build dependency for users with a matching bottle while retaining the
vendored source archive as a fallback. macOS builds use Homebrew's normal
ad-hoc signature path; no Developer ID certificate or notarization secret is
required.

## 2026-07-14 — Headless output is a finite raw-RGBA stream

`lot --headless` writes a single playback to standard output as contiguous `RGBA` frames, at the
requested output rate. It emits no headers or progress on standard output, so it composes directly
with raw-video consumers such as `ffmpeg`; diagnostics remain on standard error. The frame count
is the animation duration times the requested rate, rounded up, and looping is disabled so exports
terminate predictably. Container encoding, audio, and looping policy remain the consumer's job.

## 2026-07-14 — Metadata-first initial release (superseded)

The first TUI used `dotlottie-rs` v0.1.58 with only its `dotlottie` feature enabled. This let the
CLI read and validate dotLottie containers before a rendering backend was added.

The exact tag is fetched with `mise run fetch-dotlottie` as a shallow clone into the ignored
`deps/dotlottie-rs` directory. Cargo uses that local checkout so a normal build does not perform
an unconstrained Git fetch.

## 2026-07-14 — Keep `ratatui-image` out of the renderer

`ratatui-image` remains excluded. The app has an RGBA frame source now, but its terminal support
needs an explicit choice between stable and double-buffered Kitty image IDs. Keeping the small
protocol transport in-tree makes that lifecycle testable and avoids coupling Ratatui's redraws to
a generic image widget. It can be replaced later if a crate covers those semantics.

## 2026-07-14 — Own the Kitty static-frame presenter

The project implements a small Kitty graphics transport rather than depending on a protocol crate.
The protocol's image/placement lifecycle differs across terminals, so `lot` keeps single- and
double-buffer strategies behind a single presenter API and tests the emitted terminal bytes. This
module is intentionally independent of the metadata-only TUI until CPU rendering is added.

## 2026-07-14 — CPU rendering uses ThorVG's straight-alpha output

`dotlottie-rs` is built with its CPU ThorVG renderer plus PNG, JPEG, WebP, TTF, OTF, theming, and
Lottie-expression features. dotLottie packages embed their image and font assets before handing
the Lottie document to ThorVG. The renderer writes `ABGR8888S` pixels to a software buffer; `lot`
converts the numeric pixels to little-endian bytes to produce the Kitty protocol's RGBA stream.
The fetch task initializes the pinned ThorVG submodule shallowly to keep dependency downloads
metered. Animation background metadata is deliberately not applied yet.

## 2026-07-14 — Pace raw-image presentation at 30 fps

Static Kitty-image replacements do not have a terminal-managed playback clock. A full RGBA upload
for every renderer tick can queue faster than the terminal displays it, making animations appear
slow. `lot` advances dotlottie-rs no more than 30 times per second using the elapsed wall-clock
time, then sends the latest changed frame. This avoids unnecessary ThorVG work and drops
intermediate frames rather than extending the animation timeline.

## 2026-07-14 — Optimize the distributable without dropping renderer support

The release profile uses fat LTO, one codegen unit, aborting panics, and stripped symbols. The
dotlottie-rs feature list keeps themes, CPU ThorVG, image decoders, font loaders, and expressions;
each maps to a supported input capability. `theming` already enables `dotlottie`, and `tvg-cpu`
already enables `tvg`, so those redundant direct feature entries are omitted. Clap keeps parsing,
help, usage, derive, and error-context support but omits color and command-suggestion machinery.
