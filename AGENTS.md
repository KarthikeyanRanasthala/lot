# Repository Guidelines

## Project Structure & Module Organization

This repository is an early Rust CLI project named `lot`. `README.md` is the current product specification; review it before changing behavior. `mise.toml` pins the Rust toolchain. As the implementation is added, keep application code in `src/`, integration tests in `tests/`, and reusable sample `.lottie` and JSON animations in `fixtures/`. Keep terminal UI concerns separate from loading, rendering, and export logic (for example, `src/ui/`, `src/load/`, and `src/render/`). Record notable technical choices in `DECISIONS.md` with a timestamp and concise rationale.

## Build, Test, and Development Commands

Install the pinned toolchain with `mise install` (or ensure Rust 1.97.0 is active). Once `Cargo.toml` exists, use:

- `cargo run -- animation.lottie` — run the interactive CLI locally.
- `cargo test` — run unit and integration tests.
- `cargo fmt --check` — verify Rust formatting.
- `cargo clippy --all-targets -- -D warnings` — catch lint issues without allowing warnings.

Use `cargo fmt` before committing. When adding a headless rendering path, exercise it with the documented width, height, FPS, animation-ID, and theme flags.

## Coding Style & Naming Conventions

Follow standard Rust formatting (four-space indentation, `rustfmt`, trailing commas where formatter chooses them). Use `snake_case` for functions, modules, and files; `PascalCase` for types and traits; and descriptive error/context messages. Prefer small, focused modules and `Result`-based error propagation over panics in CLI paths. Add short comments only where they explain a non-obvious decision or behavior.

## Testing Guidelines

Place module tests beside the code they cover and end-to-end CLI tests in `tests/`. Name tests by observable behavior, such as `loads_json_animation` or `headless_output_uses_requested_dimensions`. Cover local files, URLs, dotLottie animation/theme selection, renderer fallback, and error states. Keep fixtures deterministic and small; do not require network access in automated tests.

## Commit & Pull Request Guidelines

The existing history uses Conventional Commit-style messages (`chore: initial commit`); continue with forms such as `feat: add JSON loader` or `fix: report unsupported renderer`. Keep commits narrowly scoped. Pull requests should describe behavior changes, list verification commands, link relevant issues, and include terminal captures for UI or rendering changes. Update `README.md` and `DECISIONS.md` when requirements or significant implementation choices change.
