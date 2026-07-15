use crate::input::{AnimationInfo, LoadedInput, ThemeInfo};
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
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use std::{
    io,
    time::{Duration, Instant},
};

const PRIMARY: Color = Color::Rgb(0, 106, 95);
// Until terminal pixel metrics are queried, these values keep the render target and Kitty cell
// placement in one consistent, conservative coordinate system.
const ESTIMATED_CELL_WIDTH_PX: u32 = 12;
const ESTIMATED_CELL_HEIGHT_PX: u32 = 24;
const PRESENT_INTERVAL: Duration = Duration::from_micros(1_000_000 / 30);

pub fn run(input: LoadedInput) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal, input);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    input: LoadedInput,
) -> Result<()> {
    let mut app = App::new(input);
    let mut playback = None;
    let mut playback_error = None;
    let mut last_tick = Instant::now();
    let mut needs_present = false;

    let result = (|| -> Result<()> {
        loop {
            let preview = preview_content_area(preview_area(terminal.size()?.into(), &app.input));
            let (render_width, render_height) =
                target_dimensions(&app.input, app.animation_index, preview);
            let target = preview_target(preview, render_width, render_height)?;
            let theme_id = app.selected_theme_id().map(str::to_owned);
            if playback.as_ref().is_none_or(|playback: &KittyPlayback| {
                !playback.matches(app.animation_index, theme_id.as_deref(), target)
            }) {
                if let Some(playback) = playback.as_mut() {
                    playback.clear(terminal.backend_mut())?;
                }
                match kitty_strategy_from_environment() {
                    Some(strategy) => match KittyPlayback::new(
                        &app.input,
                        app.animation_index,
                        theme_id.as_deref(),
                        target,
                        render_width,
                        render_height,
                        strategy,
                    ) {
                        Ok(next_playback) => {
                            playback = Some(next_playback);
                            playback_error = None;
                            needs_present = true;
                        }
                        Err(error) => {
                            playback = None;
                            playback_error = Some(error.to_string());
                            needs_present = false;
                        }
                    },
                    None => {
                        playback = None;
                        playback_error = None;
                        needs_present = false;
                    }
                }
            }

            terminal
                .draw(|frame| draw(frame, &app, playback.is_some(), playback_error.as_deref()))?;
            if let Some(playback) = playback.as_mut() {
                if needs_present {
                    playback.present(terminal.backend_mut())?;
                    needs_present = false;
                    last_tick = Instant::now();
                } else {
                    let now = Instant::now();
                    let elapsed = now.saturating_duration_since(last_tick);
                    if elapsed >= PRESENT_INTERVAL {
                        // Render only when an image can be presented. The elapsed wall-clock
                        // delta still includes prior rendering and I/O, so playback stays real-time.
                        last_tick = now;
                        let frame_changed = playback.advance(elapsed)?;
                        if frame_changed {
                            playback.present(terminal.backend_mut())?;
                        }
                    }
                }
            }

            if !event::poll(Duration::from_millis(16))? {
                continue;
            }
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if is_quit_key(key) {
                        return Ok(());
                    }

                    match key.code {
                        KeyCode::Down => app.next(),
                        KeyCode::Up => app.previous(),
                        KeyCode::Tab => app.toggle_focus(),
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => app.next(),
                    MouseEventKind::ScrollUp => app.previous(),
                    _ => {}
                },
                _ => {}
            }
        }
    })();

    if let Some(playback) = playback.as_mut() {
        let _ = playback.clear(terminal.backend_mut());
    }
    result
}

fn is_quit_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc | KeyCode::Char('q' | 'Q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

#[derive(Clone, Copy)]
enum Focus {
    Animations,
    Themes,
}

struct App {
    input: LoadedInput,
    animation_index: usize,
    theme_index: Option<usize>,
    focus: Focus,
}

impl App {
    fn new(input: LoadedInput) -> Self {
        let animation_index = input.default_animation_index();
        Self {
            theme_index: input.initial_theme_index(animation_index),
            input,
            animation_index,
            focus: Focus::Animations,
        }
    }

    fn next(&mut self) {
        match self.focus {
            Focus::Animations => {
                let len = self.input.animations().len();
                if len > 0 {
                    self.animation_index = (self.animation_index + 1) % len;
                    self.theme_index = self.input.initial_theme_index(self.animation_index);
                }
            }
            Focus::Themes => {
                let len = self.input.themes().len().saturating_add(1);
                if len > 0 {
                    let selected = self.theme_index.map_or(0, |index| index + 1);
                    let next = (selected + 1) % len;
                    self.theme_index = next.checked_sub(1);
                }
            }
        }
    }

    fn previous(&mut self) {
        match self.focus {
            Focus::Animations => {
                let len = self.input.animations().len();
                if len > 0 {
                    self.animation_index = (self.animation_index + len - 1) % len;
                    self.theme_index = self.input.initial_theme_index(self.animation_index);
                }
            }
            Focus::Themes => {
                let len = self.input.themes().len().saturating_add(1);
                if len > 0 {
                    let selected = self.theme_index.map_or(0, |index| index + 1);
                    let previous = (selected + len - 1) % len;
                    self.theme_index = previous.checked_sub(1);
                }
            }
        }
    }

    fn selected_theme_id(&self) -> Option<&str> {
        self.theme_index
            .and_then(|index| self.input.themes().get(index))
            .map(|theme| theme.id.as_str())
    }

    fn toggle_focus(&mut self) {
        if self.input.themes().is_empty() {
            return;
        }
        self.focus = match self.focus {
            Focus::Animations => Focus::Themes,
            Focus::Themes => Focus::Animations,
        };
    }
}

fn draw(frame: &mut Frame, app: &App, is_rendering: bool, playback_error: Option<&str>) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY))
        .title(" lot ");
    frame.render_widget(outer, frame.area());
    let (area, footer) = app_layout(frame.area());

    if app.input.is_dotlottie() {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(30), Constraint::Min(28)])
            .split(area);
        draw_sidebar(frame, columns[0], app);
        draw_preview(frame, columns[1], app, is_rendering, playback_error);
    } else {
        draw_preview(frame, area, app, is_rendering, playback_error);
    }

    frame.render_widget(
        Paragraph::new(controls_text(&app.input))
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Left),
        footer,
    );
}

fn draw_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Min(4)])
        .split(area);
    draw_animations(frame, sections[0], app);
    draw_themes(frame, sections[1], app);
}

fn draw_animations(frame: &mut Frame, area: Rect, app: &App) {
    let items = app
        .input
        .animations()
        .iter()
        .map(animation_item)
        .collect::<Vec<_>>();
    let mut state = ListState::default();
    state.select(Some(app.animation_index));
    let title = format!(
        " {} ",
        count_label(app.input.animations().len(), "animation")
    );
    let list = List::new(items)
        .block(panel(&title, matches!(app.focus, Focus::Animations)))
        .highlight_style(
            Style::default()
                .bg(PRIMARY)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_themes(frame: &mut Frame, area: Rect, app: &App) {
    let themes = app.input.themes();
    let title = format!(" {} ", count_label(themes.len(), "theme"));
    if themes.is_empty() {
        let block = panel(&title, false);
        let content = block.inner(area);
        let empty_state_area = Rect::new(
            content.x,
            content.y + content.height.saturating_sub(1) / 2,
            content.width,
            1,
        );
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new("No themes available")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray))
                .wrap(Wrap { trim: true }),
            empty_state_area,
        );
        return;
    }

    let items = std::iter::once(ListItem::new(Line::from("Default")))
        .chain(themes.iter().map(theme_item))
        .collect::<Vec<_>>();
    let mut state = ListState::default();
    state.select(Some(app.theme_index.map_or(0, |index| index + 1)));
    let list = List::new(items)
        .block(panel(&title, matches!(app.focus, Focus::Themes)))
        .highlight_style(
            Style::default()
                .bg(PRIMARY)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_preview(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    is_rendering: bool,
    playback_error: Option<&str>,
) {
    let animation = app.input.selected_animation(app.animation_index);
    let block = panel(" Preview ", true).title_bottom(
        Line::from(metadata_values(animation))
            .centered()
            .style(Style::default().fg(Color::Gray)),
    );
    let content = block.inner(area);
    let message_height = content.height.min(4);
    let message_area = Rect::new(
        content.x,
        content.y + (content.height - message_height) / 2,
        content.width,
        message_height,
    );
    frame.render_widget(block, area);
    if !is_rendering {
        let detail = playback_error
            .unwrap_or("This terminal does not expose a supported graphics protocol.");
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("Rendering unavailable"),
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
        .title(title)
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

fn count_label(count: usize, singular: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{count} {singular}{suffix}")
}

fn controls_text(input: &LoadedInput) -> &'static str {
    if !input.is_dotlottie() {
        return "q / Esc Quit";
    }

    "↑/↓ Choose  ·  Tab Switch panel  ·  q / Esc Quit"
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

fn preview_area(area: Rect, input: &LoadedInput) -> Rect {
    let (inner, _) = app_layout(area);
    if input.is_dotlottie() {
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
        area.height.saturating_sub(2),
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
    use super::{is_quit_key, preview_target};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::layout::Rect;

    #[test]
    fn recognises_the_documented_quit_keys() {
        assert!(is_quit_key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        assert!(is_quit_key(KeyEvent::new(
            KeyCode::Char('Q'),
            KeyModifiers::SHIFT,
        )));
        assert!(is_quit_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(is_quit_key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert!(!is_quit_key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )));
    }

    #[test]
    fn placement_is_centered_and_matches_the_rendered_pixel_size() {
        let target = preview_target(Rect::new(20, 5, 100, 50), 512, 512).unwrap();

        assert_eq!(target.column, 49);
        assert_eq!(target.row, 20);
        assert_eq!(target.columns, 43);
        assert_eq!(target.rows, 22);
    }
}
