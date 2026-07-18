//! Directory playlist: discovery, natural sort, search, selection, and watching.

pub mod discover;
pub mod state;
pub mod watch;

pub use discover::{discover_animations, is_animation_path, natural_path_cmp, natural_str_cmp};
pub use state::{Playlist, PlaylistEntry};
pub use watch::{PlaylistEvent, WatcherSession, spawn_directory_watcher};
