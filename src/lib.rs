//! Library surface for `lot`.
//!
//! Integration tests import production playlist, discovery, and watching types from this crate
//! so they cannot pass against duplicated test-only logic.

pub mod app;
pub mod cli;
pub mod input;
pub mod playlist;
pub mod render;
pub mod terminal;
pub mod tui;
pub mod tui_playlist;
