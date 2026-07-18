//! Directory playlist: discovery, natural sort, search, selection, and watching.

mod discover;
mod state;
mod watch;

pub use state::Playlist;
pub use watch::{PlaylistEvent, spawn_directory_watcher};
