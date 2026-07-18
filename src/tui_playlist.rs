//! Interactive directory playlist mode.

use crate::input::{AnimationInfo, LoadedInput, ThemeInfo};
use crate::playlist::{Playlist, PlaylistEvent, spawn_directory_watcher};
use crate::render::{KittyPlayback, kitty_strategy_from_environment};
use crate::terminal::kitty::PreviewArea;
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

const PRIMARY: Color = Color::Rgb(0, 106, 95);
const ESTIMATED_CELL_WIDTH_PX: u32 = 12;
const ESTIMATED_CELL_HEIGHT_PX: u32 = 24;
const PRESENT_INTERVAL: Duration = Duration::from_micros(1_000_000 / 30);
const PLAYLIST_WIDTH: u16 = 34;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Playlist,
    Animations,
    Themes,
}

pub fn run(root: PathBuf) -> Result<()> {
    let root = root.canonicalize()?;
    let (events, watcher_session) = spawn_directory_watcher(root.clone())?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal, root, events);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    drop(watcher_session);
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    root: PathBuf,
    events: Receiver<PlaylistEvent>,
) -> Result<()> {
    let mut app = DirectoryApp::new(root);
    let mut playback: Option<KittyPlayback> = None;
    let mut playback_error: Option<String> = None;
    let mut last_tick = Instant::now();
    let mut needs_present = false;

    let result = (|| -> Result<()> {
        loop {
            // Drain filesystem/scan events without blocking the render loop.
            while let Ok(event) = events.try_recv() {
                app.handle_playlist_event(event);
            }

            if app.consume_reload_request() {
                app.reload_selected();
                // Force playback rebuild for the (possibly new) loaded input.
                if let Some(active) = playback.as_mut() {
                    let _ = active.clear(terminal.backend_mut());
                }
                playback = None;
                playback_error = None;
                needs_present = false;
            }

            let has_input = app.loaded.is_some();
            if has_input {
                let input = app.loaded.as_ref().expect("checked");
                let preview = preview_content_area(preview_area(
                    terminal.size()?.into(),
                    true,
                    input.is_dotlottie(),
                ));
                let (render_width, render_height) =
                    target_dimensions(input, app.animation_index, preview);
                let target = preview_target(preview, render_width, render_height)?;
                let theme_id = app.selected_theme_id().map(str::to_owned);
                let path_key = app.selected_path_key();

                if playback.as_ref().is_none_or(|pb: &KittyPlayback| {
                    !pb.matches(app.animation_index, theme_id.as_deref(), target)
                        || app.playback_path_key.as_deref() != path_key.as_deref()
                }) {
                    if let Some(active) = playback.as_mut() {
                        active.clear(terminal.backend_mut())?;
                    }
                    match kitty_strategy_from_environment() {
                        Some(strategy) => match KittyPlayback::new(
                            input,
                            app.animation_index,
                            theme_id.as_deref(),
                            target,
                            render_width,
                            render_height,
                            strategy,
                        ) {
                            Ok(next) => {
                                playback = Some(next);
                                app.playback_path_key = path_key;
                                playback_error = None;
                                needs_present = true;
                            }
                            Err(error) => {
                                playback = None;
                                app.playback_path_key = None;
                                playback_error = Some(error.to_string());
                                needs_present = false;
                            }
                        },
                        None => {
                            playback = None;
                            app.playback_path_key = None;
                            playback_error = None;
                            needs_present = false;
                        }
                    }
                }
            } else {
                if let Some(active) = playback.as_mut() {
                    let _ = active.clear(terminal.backend_mut());
                }
                playback = None;
                app.playback_path_key = None;
                needs_present = false;
            }

            let progress = playback.as_ref().and_then(KittyPlayback::progress);
            let is_playing = playback.as_ref().map(KittyPlayback::is_playing);
            // Prefer file-load errors over renderer errors when both exist.
            let display_error = app.load_error.as_deref().or(playback_error.as_deref());

            terminal.draw(|frame| {
                draw(
                    frame,
                    &app,
                    playback.is_some(),
                    is_playing,
                    progress,
                    display_error,
                )
            })?;

            if let Some(active) = playback.as_mut() {
                if needs_present {
                    active.present(terminal.backend_mut())?;
                    needs_present = false;
                    last_tick = Instant::now();
                } else if active.is_playing() {
                    let now = Instant::now();
                    let elapsed = now.saturating_duration_since(last_tick);
                    if elapsed >= PRESENT_INTERVAL {
                        last_tick = now;
                        let frame_changed = active.advance(elapsed)?;
                        if frame_changed {
                            active.present(terminal.backend_mut())?;
                        }
                    }
                }
            }

            if !event::poll(Duration::from_millis(16))? {
                continue;
            }
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if app.searching {
                        if handle_search_key(&mut app, key) {
                            return Ok(());
                        }
                        continue;
                    }

                    if is_quit_key(key) {
                        return Ok(());
                    }

                    match key.code {
                        KeyCode::Char('/') => {
                            app.searching = true;
                        }
                        KeyCode::Down => app.next(true),
                        KeyCode::Up => app.previous(true),
                        KeyCode::Tab => app.toggle_focus(),
                        KeyCode::Char(' ') => {
                            if let Some(active) = playback.as_mut() {
                                active.toggle_pause()?;
                                last_tick = Instant::now();
                            }
                        }
                        KeyCode::Left | KeyCode::Right => {
                            if let Some(active) = playback.as_mut() {
                                let offset = if key.code == KeyCode::Left { -1 } else { 1 };
                                if active.step_frame(offset)? {
                                    active.present(terminal.backend_mut())?;
                                }
                                last_tick = Instant::now();
                            }
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => app.next(false),
                    MouseEventKind::ScrollUp => app.previous(false),
                    _ => {}
                },
                _ => {}
            }
        }
    })();

    if let Some(active) = playback.as_mut() {
        let _ = active.clear(terminal.backend_mut());
    }
    result
}

fn handle_search_key(app: &mut DirectoryApp, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            app.searching = false;
            false
        }
        KeyCode::Enter => {
            app.searching = false;
            false
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        KeyCode::Backspace => {
            app.playlist.pop_filter_char();
            app.request_reload_if_selection_changed();
            false
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.playlist.push_filter_char(c);
            app.request_reload_if_selection_changed();
            false
        }
        KeyCode::Down => {
            app.playlist.select_next(true);
            app.request_reload_if_selection_changed();
            false
        }
        KeyCode::Up => {
            app.playlist.select_previous(true);
            app.request_reload_if_selection_changed();
            false
        }
        _ => false,
    }
}

fn is_quit_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc | KeyCode::Char('q' | 'Q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

struct DirectoryApp {
    playlist: Playlist,
    loaded: Option<LoadedInput>,
    load_error: Option<String>,
    animation_index: usize,
    theme_index: Option<usize>,
    focus: Focus,
    searching: bool,
    scanning: bool,
    status: Option<String>,
    last_generation: u64,
    /// Path string of the currently loaded animation (for change detection).
    loaded_path: Option<PathBuf>,
    loaded_mtime: Option<SystemTime>,
    /// Path key attached to the active Kitty playback instance.
    playback_path_key: Option<String>,
    /// Path that was selected when we last considered a reload.
    last_selected_path: Option<PathBuf>,
    reload_requested: bool,
}

impl DirectoryApp {
    fn new(root: PathBuf) -> Self {
        Self {
            playlist: Playlist::new(root),
            loaded: None,
            load_error: None,
            animation_index: 0,
            theme_index: None,
            focus: Focus::Playlist,
            searching: false,
            scanning: true,
            status: Some("Scanning…".into()),
            last_generation: 0,
            loaded_path: None,
            loaded_mtime: None,
            playback_path_key: None,
            last_selected_path: None,
            reload_requested: false,
        }
    }

    fn handle_playlist_event(&mut self, event: PlaylistEvent) {
        match event {
            PlaylistEvent::ScanComplete { generation, paths } => {
                if generation < self.last_generation {
                    return;
                }
                self.last_generation = generation;
                self.scanning = false;
                self.status = None;
                self.playlist.replace_entries(paths);
                self.sync_load_after_playlist_change();
            }
            PlaylistEvent::Error { message } => {
                self.scanning = false;
                self.status = Some(message);
            }
        }
    }

    fn sync_load_after_playlist_change(&mut self) {
        let selected = self.playlist.selected_path().map(Path::to_path_buf);
        match selected {
            None => {
                self.loaded = None;
                self.load_error = if self.playlist.is_empty() {
                    Some("No .json or .lottie files found in this directory.".into())
                } else {
                    Some("No playlist item matches the current search.".into())
                };
                self.loaded_path = None;
                self.loaded_mtime = None;
                self.last_selected_path = None;
                self.reload_requested = true;
            }
            Some(path) => {
                let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();
                let same_path = self.loaded_path.as_ref() == Some(&path);
                let mtime_changed = same_path && self.loaded_mtime != mtime;
                if !same_path || mtime_changed || self.loaded.is_none() {
                    self.last_selected_path = Some(path);
                    self.reload_requested = true;
                } else {
                    self.last_selected_path = Some(path);
                }
            }
        }
    }

    fn request_reload_if_selection_changed(&mut self) {
        let selected = self.playlist.selected_path().map(Path::to_path_buf);
        if selected != self.last_selected_path {
            self.last_selected_path = selected;
            self.reload_requested = true;
        }
    }

    fn consume_reload_request(&mut self) -> bool {
        let pending = self.reload_requested;
        self.reload_requested = false;
        pending
    }

    fn reload_selected(&mut self) {
        let Some(path) = self.playlist.selected_path().map(Path::to_path_buf) else {
            self.loaded = None;
            self.load_error = if self.playlist.is_empty() {
                Some("No .json or .lottie files found in this directory.".into())
            } else {
                Some("No playlist item matches the current search.".into())
            };
            self.loaded_path = None;
            self.loaded_mtime = None;
            self.animation_index = 0;
            self.theme_index = None;
            return;
        };

        match LoadedInput::from_path(&path) {
            Ok(input) => {
                self.animation_index = input.default_animation_index();
                self.theme_index = input.initial_theme_index(self.animation_index);
                self.loaded = Some(input);
                self.load_error = None;
                self.loaded_path = Some(path.clone());
                self.loaded_mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();
                if self.focus != Focus::Playlist
                    && !self.loaded.as_ref().is_some_and(|i| i.is_dotlottie())
                {
                    self.focus = Focus::Playlist;
                }
            }
            Err(error) => {
                self.loaded = None;
                self.load_error = Some(format!("Could not load {}: {error}", path.display()));
                self.loaded_path = Some(path);
                self.loaded_mtime = None;
                self.animation_index = 0;
                self.theme_index = None;
            }
        }
    }

    fn selected_path_key(&self) -> Option<String> {
        self.playlist
            .selected_path()
            .map(|path| path.to_string_lossy().into_owned())
    }

    fn next(&mut self, cycle: bool) {
        match self.focus {
            Focus::Playlist => {
                if cycle {
                    self.playlist.select_next(true);
                } else {
                    self.playlist.select_next(false);
                }
                self.request_reload_if_selection_changed();
            }
            Focus::Animations => {
                if let Some(input) = self.loaded.as_ref() {
                    let len = input.animations().len();
                    if len == 0 {
                        return;
                    }
                    if cycle {
                        self.animation_index = (self.animation_index + 1) % len;
                    } else if self.animation_index + 1 < len {
                        self.animation_index += 1;
                    } else {
                        return;
                    }
                    self.theme_index = input.initial_theme_index(self.animation_index);
                }
            }
            Focus::Themes => {
                if let Some(input) = self.loaded.as_ref() {
                    let len = input.themes().len().saturating_add(1);
                    if len == 0 {
                        return;
                    }
                    let selected = self.theme_index.map_or(0, |index| index + 1);
                    let next = if cycle {
                        (selected + 1) % len
                    } else if selected + 1 < len {
                        selected + 1
                    } else {
                        return;
                    };
                    self.theme_index = next.checked_sub(1);
                }
            }
        }
    }

    fn previous(&mut self, cycle: bool) {
        match self.focus {
            Focus::Playlist => {
                if cycle {
                    self.playlist.select_previous(true);
                } else {
                    self.playlist.select_previous(false);
                }
                self.request_reload_if_selection_changed();
            }
            Focus::Animations => {
                if let Some(input) = self.loaded.as_ref() {
                    let len = input.animations().len();
                    if len == 0 {
                        return;
                    }
                    if cycle {
                        self.animation_index = (self.animation_index + len - 1) % len;
                    } else if self.animation_index > 0 {
                        self.animation_index -= 1;
                    } else {
                        return;
                    }
                    self.theme_index = input.initial_theme_index(self.animation_index);
                }
            }
            Focus::Themes => {
                if let Some(input) = self.loaded.as_ref() {
                    let len = input.themes().len().saturating_add(1);
                    if len == 0 {
                        return;
                    }
                    let selected = self.theme_index.map_or(0, |index| index + 1);
                    if cycle {
                        let previous = (selected + len - 1) % len;
                        self.theme_index = previous.checked_sub(1);
                    } else if selected > 0 {
                        self.theme_index = (selected - 1).checked_sub(1);
                    }
                }
            }
        }
    }

    fn toggle_focus(&mut self) {
        let has_dotlottie = self.loaded.as_ref().is_some_and(LoadedInput::is_dotlottie);
        let has_themes = self
            .loaded
            .as_ref()
            .is_some_and(|input| !input.themes().is_empty());
        self.focus = match self.focus {
            Focus::Playlist if has_dotlottie => Focus::Animations,
            Focus::Playlist => Focus::Playlist,
            Focus::Animations if has_themes => Focus::Themes,
            Focus::Animations => Focus::Playlist,
            Focus::Themes => Focus::Playlist,
        };
    }

    fn selected_theme_id(&self) -> Option<&str> {
        let input = self.loaded.as_ref()?;
        self.theme_index
            .and_then(|index| input.themes().get(index))
            .map(|theme| theme.id.as_str())
    }
}

fn draw(
    frame: &mut Frame,
    app: &DirectoryApp,
    is_rendering: bool,
    is_playing: Option<bool>,
    progress: Option<f64>,
    error: Option<&str>,
) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY))
        .title(title_text(app));
    frame.render_widget(outer, frame.area());
    let (area, footer) = app_layout(frame.area());

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(PLAYLIST_WIDTH), Constraint::Min(28)])
        .split(area);

    draw_playlist_panel(frame, columns[0], app);

    let right = columns[1];
    if app.loaded.as_ref().is_some_and(LoadedInput::is_dotlottie) {
        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(28)])
            .split(right);
        draw_animation_sidebar(frame, sections[0], app);
        draw_preview(
            frame,
            sections[1],
            app,
            is_rendering,
            is_playing,
            progress,
            error,
        );
    } else {
        draw_preview(frame, right, app, is_rendering, is_playing, progress, error);
    }

    frame.render_widget(
        Paragraph::new(controls_text(app, is_playing))
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Left),
        footer,
    );
}

fn title_text(app: &DirectoryApp) -> String {
    let root = app.playlist.root().display();
    if app.scanning {
        format!(" lot · {root} · scanning ")
    } else if let Some(status) = app.status.as_deref() {
        format!(" lot · {root} · {status} ")
    } else {
        format!(" lot · {root} ")
    }
}

fn draw_playlist_panel(frame: &mut Frame, area: Rect, app: &DirectoryApp) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(area);

    let filter_title = if app.searching {
        " Search (Enter/Esc) "
    } else {
        " Search (/) "
    };
    let filter_value = if app.playlist.filter().is_empty() && !app.searching {
        "type / to filter by filename".to_owned()
    } else {
        format!("/{}", app.playlist.filter())
    };
    let filter_style = if app.searching {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(
        Paragraph::new(filter_value)
            .style(filter_style)
            .block(panel(
                filter_title,
                app.searching || matches!(app.focus, Focus::Playlist),
            )),
        sections[0],
    );

    let count = app.playlist.filtered_len();
    let total = app.playlist.len();
    let title = if app.playlist.filter().is_empty() {
        format!(" {} ", count_label(total, "file"))
    } else {
        format!(" {count}/{total} files ")
    };

    if count == 0 {
        let block = panel(
            &title,
            matches!(app.focus, Focus::Playlist) && !app.searching,
        );
        let content = block.inner(sections[1]);
        frame.render_widget(block, sections[1]);
        let message = if app.scanning {
            "Scanning…"
        } else if app.playlist.filter().is_empty() {
            "No animations found"
        } else {
            "No matches"
        };
        frame.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
            content,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .playlist
        .visible_entries()
        .map(|(_, entry)| ListItem::new(Line::from(entry.display_name())))
        .collect();
    let mut state = ListState::default();
    state.select(app.playlist.filtered_position());
    let list = List::new(items)
        .block(panel(
            &title,
            matches!(app.focus, Focus::Playlist) && !app.searching,
        ))
        .highlight_style(
            Style::default()
                .bg(PRIMARY)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, sections[1], &mut state);
}

fn draw_animation_sidebar(frame: &mut Frame, area: Rect, app: &DirectoryApp) {
    let Some(input) = app.loaded.as_ref() else {
        return;
    };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Min(4)])
        .split(area);

    let anim_items: Vec<ListItem> = input.animations().iter().map(animation_item).collect();
    let mut anim_state = ListState::default();
    anim_state.select(Some(app.animation_index));
    let anim_title = format!(" {} ", count_label(input.animations().len(), "animation"));
    let anim_list = List::new(anim_items)
        .block(panel(&anim_title, matches!(app.focus, Focus::Animations)))
        .highlight_style(
            Style::default()
                .bg(PRIMARY)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(anim_list, sections[0], &mut anim_state);

    let themes = input.themes();
    let theme_title = format!(" {} ", count_label(themes.len(), "theme"));
    if themes.is_empty() {
        let block = panel(&theme_title, false);
        let content = block.inner(sections[1]);
        frame.render_widget(block, sections[1]);
        frame.render_widget(
            Paragraph::new("No themes available")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
            content,
        );
        return;
    }

    let theme_items = std::iter::once(ListItem::new(Line::from("Default")))
        .chain(themes.iter().map(theme_item))
        .collect::<Vec<_>>();
    let mut theme_state = ListState::default();
    theme_state.select(Some(app.theme_index.map_or(0, |index| index + 1)));
    let theme_list = List::new(theme_items)
        .block(panel(&theme_title, matches!(app.focus, Focus::Themes)))
        .highlight_style(
            Style::default()
                .bg(PRIMARY)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(theme_list, sections[1], &mut theme_state);
}

fn draw_preview(
    frame: &mut Frame,
    area: Rect,
    app: &DirectoryApp,
    is_rendering: bool,
    is_playing: Option<bool>,
    progress: Option<f64>,
    error: Option<&str>,
) {
    let animation = app
        .loaded
        .as_ref()
        .map(|input| input.selected_animation(app.animation_index));
    let block = panel(" Preview ", true).title_bottom(
        Line::from(
            animation
                .map(metadata_values)
                .unwrap_or_else(|| " — ".into()),
        )
        .centered()
        .style(Style::default().fg(Color::Gray)),
    );
    let content = block.inner(area);
    let progress_area = Rect::new(
        content.x,
        content.y + content.height.saturating_sub(1),
        content.width,
        content.height.min(1),
    );
    let message_height = content.height.min(6);
    let message_area = Rect::new(
        content.x,
        content.y + (content.height - message_height) / 2,
        content.width,
        message_height,
    );
    frame.render_widget(block, area);

    if let Some(progress) = progress {
        let duration = animation.and_then(|a| a.duration_seconds);
        let label = playback_progress_label(progress, duration, is_playing.unwrap_or(true));
        frame.render_widget(
            Gauge::default()
                .ratio(progress)
                .label(label)
                .use_unicode(true)
                .style(Style::default().fg(Color::DarkGray))
                .gauge_style(Style::default().fg(PRIMARY)),
            progress_area,
        );
    }

    if !is_rendering {
        let (headline, detail) = if let Some(error) = error {
            ("Unable to play selection", error)
        } else if app.scanning {
            ("Scanning directory", "Discovering .json and .lottie files…")
        } else if app.playlist.is_empty() {
            (
                "Empty playlist",
                "No .json or .lottie files found under this directory.",
            )
        } else if app.loaded.is_none() {
            (
                "Select an animation",
                "Use ↑/↓ to choose a file from the playlist.",
            )
        } else {
            (
                "Rendering unavailable",
                "This terminal does not expose a supported graphics protocol.",
            )
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(headline),
                Line::from(""),
                Line::from(detail),
            ])
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: true }),
            message_area,
        );
    }
}

fn panel(title: &str, active: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if active { PRIMARY } else { Color::DarkGray }))
        .title(title.to_owned())
}

fn animation_item(animation: &AnimationInfo) -> ListItem<'_> {
    let label = animation.name.as_deref().unwrap_or(&animation.id);
    ListItem::new(Line::from(label.to_owned()))
}

fn theme_item(theme: &ThemeInfo) -> ListItem<'_> {
    let label = theme.name.as_deref().unwrap_or(&theme.id);
    ListItem::new(Line::from(label.to_owned()))
}

fn metadata_values(animation: &AnimationInfo) -> String {
    let canvas = match (animation.width, animation.height) {
        (Some(width), Some(height)) => format!("{width} × {height}"),
        _ => "—".into(),
    };
    let duration = animation
        .duration_seconds
        .map(|seconds| format!("{seconds:.2}s"))
        .unwrap_or_else(|| "—".into());
    let fps = animation
        .fps
        .map(|fps| format!("{fps:.2} fps"))
        .unwrap_or_else(|| "—".into());
    format!(" {canvas}  ·  {duration}  ·  {fps} ")
}

fn playback_progress_label(
    progress: f64,
    duration_seconds: Option<f64>,
    is_playing: bool,
) -> String {
    let state = (!is_playing).then_some("Paused · ");
    let Some(duration) =
        duration_seconds.filter(|duration| duration.is_finite() && *duration > 0.0)
    else {
        return state.unwrap_or_default().to_owned();
    };
    let elapsed = duration * progress;
    format!(
        "{}{} / {}",
        state.unwrap_or_default(),
        format_playback_time(elapsed),
        format_playback_time(duration),
    )
}

fn format_playback_time(seconds: f64) -> String {
    if seconds < 60.0 {
        return format!("{seconds:.1}s");
    }
    let whole_seconds = seconds.floor() as u64;
    format!("{}:{:02}", whole_seconds / 60, whole_seconds % 60)
}

fn count_label(count: usize, singular: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{count} {singular}{suffix}")
}

fn controls_text(app: &DirectoryApp, is_playing: Option<bool>) -> String {
    if app.searching {
        return "Type to filter  ·  ↑/↓ Select  ·  Enter/Esc Done  ·  Ctrl-C Quit".into();
    }
    let play = match is_playing {
        Some(true) => "Space Pause",
        Some(false) => "Space Play",
        None => "",
    };
    let step = if is_playing.is_some() {
        "  ·  ←/→ Step"
    } else {
        ""
    };
    let play_part = if play.is_empty() {
        String::new()
    } else {
        format!("  ·  {play}")
    };
    format!("↑/↓ Navigate  ·  / Search  ·  Tab Focus{step}{play_part}  ·  q/Esc Quit")
}

fn app_layout(area: Rect) -> (Rect, Rect) {
    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    (sections[0], sections[1])
}

fn preview_area(area: Rect, directory: bool, dotlottie: bool) -> Rect {
    let (inner, _) = app_layout(area);
    if directory {
        let right = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(PLAYLIST_WIDTH), Constraint::Min(28)])
            .split(inner)[1];
        if dotlottie {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(28), Constraint::Min(28)])
                .split(right)[1]
        } else {
            right
        }
    } else if dotlottie {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(30), Constraint::Min(28)])
            .split(inner)[1]
    } else {
        inner
    }
}

fn preview_content_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(3),
    )
}

fn preview_target(area: Rect, render_width: u32, render_height: u32) -> Result<PreviewArea> {
    let columns = cells_for_pixels(render_width, ESTIMATED_CELL_WIDTH_PX).min(area.width);
    let rows = cells_for_pixels(render_height, ESTIMATED_CELL_HEIGHT_PX).min(area.height);
    PreviewArea::new(
        area.x
            .saturating_add(area.width.saturating_sub(columns) / 2)
            .saturating_add(1),
        area.y
            .saturating_add(area.height.saturating_sub(rows) / 2)
            .saturating_add(1),
        columns,
        rows,
    )
    .map_err(Into::into)
}

fn cells_for_pixels(pixels: u32, estimated_cell_pixels: u32) -> u16 {
    u16::try_from(pixels.div_ceil(estimated_cell_pixels)).unwrap_or(u16::MAX)
}

fn target_dimensions(input: &LoadedInput, animation_index: usize, area: Rect) -> (u32, u32) {
    let animation = input.selected_animation(animation_index);
    let source_width = animation
        .width
        .and_then(|width| u32::try_from(width).ok())
        .unwrap_or(320);
    let source_height = animation
        .height
        .and_then(|height| u32::try_from(height).ok())
        .unwrap_or(180);
    let fallback_width = u32::from(area.width)
        .saturating_mul(ESTIMATED_CELL_WIDTH_PX)
        .clamp(1, 640);
    let fallback_height = u32::from(area.height)
        .saturating_mul(ESTIMATED_CELL_HEIGHT_PX)
        .clamp(1, 480);
    let scale = (fallback_width as f64 / source_width as f64)
        .min(fallback_height as f64 / source_height as f64)
        .min(1.0);
    (
        (source_width as f64 * scale).round().max(1.0) as u32,
        (source_height as f64 * scale).round().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn search_mode_does_not_quit_on_escape() {
        let mut app = DirectoryApp::new(PathBuf::from("/tmp"));
        app.searching = true;
        assert!(!handle_search_key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        ));
        assert!(!app.searching);
    }

    #[test]
    fn playlist_navigation_updates_selection() {
        let mut app = DirectoryApp::new(PathBuf::from("/animations"));
        app.playlist.replace_entries(vec![
            PathBuf::from("/animations/a.json"),
            PathBuf::from("/animations/b.json"),
        ]);
        app.focus = Focus::Playlist;
        app.next(true);
        assert_eq!(
            app.playlist.selected_path().unwrap().file_name().unwrap(),
            "b.json"
        );
    }

    #[test]
    fn corrupt_selection_sets_load_error_without_loaded_input() {
        let root = std::env::temp_dir().join(format!(
            "lot-tui-corrupt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let bad = root.join("bad.json");
        fs::write(&bad, b"not-json").unwrap();

        let mut app = DirectoryApp::new(root.clone());
        app.playlist
            .replace_entries(vec![bad.canonicalize().unwrap()]);
        app.reload_selected();

        assert!(app.loaded.is_none());
        assert!(
            app.load_error
                .as_ref()
                .is_some_and(|e| e.contains("Could not load"))
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn deleted_selection_moves_nearby_and_can_reload() {
        let mut app = DirectoryApp::new(PathBuf::from("/animations"));
        app.playlist.replace_entries(vec![
            PathBuf::from("/animations/a.json"),
            PathBuf::from("/animations/b.json"),
            PathBuf::from("/animations/c.json"),
        ]);
        app.playlist.select_filtered_index(1);
        app.playlist.replace_entries(vec![
            PathBuf::from("/animations/a.json"),
            PathBuf::from("/animations/c.json"),
        ]);
        assert_eq!(
            app.playlist.selected_path().unwrap().file_name().unwrap(),
            "c.json"
        );
    }

    #[test]
    fn focus_toggle_stays_on_playlist_for_json() {
        let mut app = DirectoryApp::new(PathBuf::from("/animations"));
        app.loaded = Some(LoadedInput::Json {
            data: Arc::from([]),
            animation: AnimationInfo {
                id: "animation".into(),
                name: None,
                initial_theme_id: None,
                width: None,
                height: None,
                fps: None,
                duration_seconds: None,
            },
        });
        app.focus = Focus::Playlist;
        app.toggle_focus();
        assert_eq!(app.focus, Focus::Playlist);
    }
}
