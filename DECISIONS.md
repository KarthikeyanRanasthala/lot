# Decisions

## 2026-07-14 — Metadata-first initial release

`lot` uses `dotlottie-rs` v0.1.58 with only its `dotlottie` feature enabled. This lets the CLI
read and validate dotLottie containers without compiling or invoking a rendering backend. The
first TUI deliberately presents validated animation metadata and selection controls only; image
rendering and raw-frame headless output will be added together in a later renderer-focused change.

The exact tag is fetched with `mise run fetch-dotlottie` as a shallow clone into the ignored
`deps/dotlottie-rs` directory. Cargo uses that local checkout so a normal build does not perform
an unconstrained Git fetch.

## 2026-07-14 — Keep image dependencies out until a renderer exists

`ratatui-image` is not included yet because the application has no pixel source to display. Adding
it now would introduce an unused dependency and platform image-protocol setup. It will be added
when terminal-frame rendering is implemented.
