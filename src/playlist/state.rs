use crate::playlist::discover::natural_path_cmp;
use std::path::{Path, PathBuf};

/// A discovered animation file with a path relative to the playlist root for display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub path: PathBuf,
    pub relative: PathBuf,
}

impl PlaylistEntry {
    pub fn display_name(&self) -> String {
        self.relative.to_string_lossy().into_owned()
    }
}

/// Searchable, selection-stable playlist over a directory of animation files.
#[derive(Clone, Debug)]
pub struct Playlist {
    root: PathBuf,
    entries: Vec<PlaylistEntry>,
    /// Indices into `entries` that match the current filter, in display order.
    filtered: Vec<usize>,
    /// Selected index into `entries` (not the filtered view).
    selected: Option<usize>,
    filter: String,
}

impl Playlist {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            entries: Vec::new(),
            filtered: Vec::new(),
            selected: None,
            filter: String::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[PlaylistEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn filtered_len(&self) -> usize {
        self.filtered.len()
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub fn selected_entry(&self) -> Option<&PlaylistEntry> {
        self.selected.and_then(|index| self.entries.get(index))
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.selected_entry().map(|entry| entry.path.as_path())
    }

    /// Replace the entry list with a freshly discovered, sorted path set.
    ///
    /// Preserves the selected path when it still exists. When the selected file was
    /// removed, selects a nearby neighbor by previous index (clamped). When the
    /// previous selection is unknown, selects the first entry.
    pub fn replace_entries(&mut self, paths: Vec<PathBuf>) {
        let previous_path = self.selected_path().map(Path::to_path_buf);
        let previous_index = self.selected;

        self.entries = paths
            .into_iter()
            .map(|path| {
                let relative = path
                    .strip_prefix(&self.root)
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|_| {
                        path.file_name()
                            .map(PathBuf::from)
                            .unwrap_or_else(|| path.clone())
                    });
                PlaylistEntry { path, relative }
            })
            .collect();
        self.entries
            .sort_by(|a, b| natural_path_cmp(&a.relative, &b.relative));
        self.entries.dedup_by(|a, b| a.path == b.path);

        self.recompute_filtered();

        if let Some(previous_path) = previous_path {
            if let Some(index) = self
                .entries
                .iter()
                .position(|entry| entry.path == previous_path)
            {
                self.selected = Some(index);
            } else {
                self.selected = self.nearby_after_removal(previous_index);
            }
        } else if self.selected.is_none() && !self.filtered.is_empty() {
            self.selected = Some(self.filtered[0]);
        } else {
            self.ensure_selection_valid();
        }
    }

    /// Update the search filter. The selected path remains selected when it still
    /// matches; otherwise the first filtered entry is selected (if any).
    pub fn set_filter(&mut self, filter: impl Into<String>) {
        let previous_path = self.selected_path().map(Path::to_path_buf);
        self.filter = filter.into();
        self.recompute_filtered();

        if let Some(previous_path) = previous_path
            && let Some(index) = self
                .entries
                .iter()
                .position(|entry| entry.path == previous_path)
            && self.filtered.contains(&index)
        {
            self.selected = Some(index);
            return;
        }

        self.selected = self.filtered.first().copied();
    }

    pub fn push_filter_char(&mut self, c: char) {
        let mut filter = self.filter.clone();
        filter.push(c);
        self.set_filter(filter);
    }

    pub fn pop_filter_char(&mut self) {
        let mut filter = self.filter.clone();
        filter.pop();
        self.set_filter(filter);
    }

    /// Move selection within the filtered view. `cycle` wraps; otherwise clamps.
    pub fn select_next(&mut self, cycle: bool) {
        if self.filtered.is_empty() {
            self.selected = None;
            return;
        }
        let position = self.filtered_position();
        let next = match position {
            Some(pos) if cycle => Some((pos + 1) % self.filtered.len()),
            Some(pos) if pos + 1 < self.filtered.len() => Some(pos + 1),
            Some(pos) => Some(pos),
            None => Some(0),
        };
        if let Some(pos) = next {
            self.selected = Some(self.filtered[pos]);
        }
    }

    pub fn select_previous(&mut self, cycle: bool) {
        if self.filtered.is_empty() {
            self.selected = None;
            return;
        }
        let position = self.filtered_position();
        let previous = match position {
            Some(pos) if cycle => Some((pos + self.filtered.len() - 1) % self.filtered.len()),
            Some(pos) if pos > 0 => Some(pos - 1),
            Some(pos) => Some(pos),
            None => Some(0),
        };
        if let Some(pos) = previous {
            self.selected = Some(self.filtered[pos]);
        }
    }

    #[cfg(test)]
    pub fn select_filtered_index(&mut self, filtered_index: usize) {
        if let Some(&entry_index) = self.filtered.get(filtered_index) {
            self.selected = Some(entry_index);
        }
    }

    /// Visible entries for rendering (filtered).
    pub fn visible_entries(&self) -> impl Iterator<Item = (usize, &PlaylistEntry)> {
        self.filtered
            .iter()
            .copied()
            .map(|index| (index, &self.entries[index]))
    }

    /// Index into the filtered list of the current selection, if visible.
    pub fn filtered_position(&self) -> Option<usize> {
        let selected = self.selected?;
        self.filtered.iter().position(|&index| index == selected)
    }

    fn recompute_filtered(&mut self) {
        let needle = self.filter.to_ascii_lowercase();
        if needle.is_empty() {
            self.filtered = (0..self.entries.len()).collect();
            return;
        }

        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry
                    .relative
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
    }

    fn ensure_selection_valid(&mut self) {
        if let Some(selected) = self.selected {
            if selected >= self.entries.len() || !self.filtered.contains(&selected) {
                self.selected = self.filtered.first().copied();
            }
        } else if !self.filtered.is_empty() {
            self.selected = Some(self.filtered[0]);
        }
    }

    fn nearby_after_removal(&self, previous_index: Option<usize>) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        if self.filtered.is_empty() {
            return None;
        }

        let preferred = previous_index.unwrap_or(0).min(self.entries.len() - 1);
        // Prefer the entry now at the old index; if the old index is no longer in the
        // filtered view, fall back to the closest filtered entry by index distance.
        if self.filtered.contains(&preferred) {
            return Some(preferred);
        }

        self.filtered
            .iter()
            .copied()
            .min_by_key(|&index| index.abs_diff(preferred))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist_with(paths: &[&str]) -> Playlist {
        let root = PathBuf::from("/animations");
        let mut playlist = Playlist::new(root.clone());
        let paths = paths.iter().map(|name| root.join(name)).collect::<Vec<_>>();
        playlist.replace_entries(paths);
        playlist
    }

    #[test]
    fn natural_order_on_replace() {
        let playlist = playlist_with(&["animation10.json", "animation2.json", "animation1.json"]);
        let names: Vec<_> = playlist
            .entries()
            .iter()
            .map(|e| e.relative.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["animation1.json", "animation2.json", "animation10.json",]
        );
    }

    #[test]
    fn search_filters_by_filename() {
        let mut playlist = playlist_with(&["alpha.json", "beta.json", "alphabet.json"]);
        playlist.set_filter("alpha");
        let visible: Vec<_> = playlist
            .visible_entries()
            .map(|(_, e)| e.display_name())
            .collect();
        assert_eq!(visible, vec!["alpha.json", "alphabet.json"]);
    }

    #[test]
    fn search_keeps_selection_when_still_visible() {
        // Natural order: alpha.json, alphabet.json, beta.json
        let mut playlist = playlist_with(&["alpha.json", "beta.json", "alphabet.json"]);
        playlist.select_filtered_index(2); // beta.json
        assert_eq!(
            playlist.selected_path().unwrap().file_name().unwrap(),
            "beta.json"
        );
        playlist.set_filter("beta");
        assert_eq!(
            playlist.selected_path().unwrap().file_name().unwrap(),
            "beta.json"
        );
    }

    #[test]
    fn search_moves_selection_when_hidden() {
        // Natural order: alpha.json, alphabet.json, beta.json
        let mut playlist = playlist_with(&["alpha.json", "beta.json", "alphabet.json"]);
        playlist.select_filtered_index(2); // beta.json
        playlist.set_filter("alpha");
        // beta is hidden; first match is alpha.json
        assert_eq!(
            playlist.selected_path().unwrap().file_name().unwrap(),
            "alpha.json"
        );
    }

    #[test]
    fn preserves_selection_when_list_updates() {
        let mut playlist = playlist_with(&["a.json", "b.json", "c.json"]);
        playlist.select_filtered_index(1);
        let selected = playlist.selected_path().unwrap().to_path_buf();

        playlist.replace_entries(vec![
            PathBuf::from("/animations/a.json"),
            PathBuf::from("/animations/b.json"),
            PathBuf::from("/animations/c.json"),
            PathBuf::from("/animations/d.json"),
        ]);
        assert_eq!(playlist.selected_path(), Some(selected.as_path()));
    }

    #[test]
    fn selects_nearby_when_selected_file_deleted() {
        let mut playlist = playlist_with(&["a.json", "b.json", "c.json"]);
        playlist.select_filtered_index(1); // b.json
        playlist.replace_entries(vec![
            PathBuf::from("/animations/a.json"),
            PathBuf::from("/animations/c.json"),
        ]);
        // Nearby: index 1 now points at c.json (preferred after removal at old index 1).
        assert_eq!(
            playlist.selected_path().unwrap().file_name().unwrap(),
            "c.json"
        );
    }

    #[test]
    fn selects_none_when_all_files_removed() {
        let mut playlist = playlist_with(&["a.json"]);
        playlist.replace_entries(Vec::new());
        assert!(playlist.selected_path().is_none());
        assert!(playlist.is_empty());
    }

    #[test]
    fn navigation_cycles_and_clamps() {
        let mut playlist = playlist_with(&["a.json", "b.json", "c.json"]);
        assert_eq!(
            playlist.selected_path().unwrap().file_name().unwrap(),
            "a.json"
        );

        playlist.select_next(true);
        assert_eq!(
            playlist.selected_path().unwrap().file_name().unwrap(),
            "b.json"
        );
        playlist.select_next(true);
        playlist.select_next(true);
        assert_eq!(
            playlist.selected_path().unwrap().file_name().unwrap(),
            "a.json"
        );

        playlist.select_previous(false);
        // clamped at start after wrapping then previous without cycle from a
        playlist.select_previous(false);
        assert_eq!(
            playlist.selected_path().unwrap().file_name().unwrap(),
            "a.json"
        );
    }

    #[test]
    fn handles_rename_as_remove_and_add() {
        let mut playlist = playlist_with(&["old.json", "keep.json"]);
        playlist.select_filtered_index(0);
        playlist.replace_entries(vec![
            PathBuf::from("/animations/keep.json"),
            PathBuf::from("/animations/new.json"),
        ]);
        // old.json gone → nearby selection (index 0 → keep.json after natural sort: keep, new)
        assert_eq!(
            playlist.selected_path().unwrap().file_name().unwrap(),
            "keep.json"
        );
    }

    #[test]
    fn deduplicates_paths_on_replace() {
        let mut playlist = Playlist::new(PathBuf::from("/animations"));
        playlist.replace_entries(vec![
            PathBuf::from("/animations/a.json"),
            PathBuf::from("/animations/a.json"),
        ]);
        assert_eq!(playlist.len(), 1);
    }
}
