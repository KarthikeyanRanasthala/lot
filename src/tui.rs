use crate::input::{AnimationInfo, LoadedInput, ThemeInfo};
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind,
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
use std::{io, time::Duration};

const PRIMARY: Color = Color::Rgb(1, 157, 145);

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
    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Down => app.next(),
                KeyCode::Up => app.previous(),
                KeyCode::Tab => app.toggle_focus(),
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => app.next(),
                MouseEventKind::ScrollUp => app.previous(),
                _ => {}
            },
            _ => {}
        }
    }
}

#[derive(Clone, Copy)]
enum Focus {
    Animations,
    Themes,
}

struct App {
    input: LoadedInput,
    animation_index: usize,
    theme_index: usize,
    focus: Focus,
}

impl App {
    fn new(input: LoadedInput) -> Self {
        Self {
            input,
            animation_index: 0,
            theme_index: 0,
            focus: Focus::Animations,
        }
    }

    fn next(&mut self) {
        let len = self.focused_len();
        if len > 0 {
            let index = self.focused_index_mut();
            *index = (*index + 1) % len;
        }
    }

    fn previous(&mut self) {
        let len = self.focused_len();
        if len > 0 {
            let index = self.focused_index_mut();
            *index = (*index + len - 1) % len;
        }
    }

    fn focused_len(&self) -> usize {
        match self.focus {
            Focus::Animations => self.input.animations().len(),
            Focus::Themes => self.input.themes().len(),
        }
    }

    fn focused_index_mut(&mut self) -> &mut usize {
        match self.focus {
            Focus::Animations => &mut self.animation_index,
            Focus::Themes => &mut self.theme_index,
        }
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

fn draw(frame: &mut Frame, app: &App) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY))
        .title(" lot ");
    let area = outer.inner(frame.area());
    frame.render_widget(outer, frame.area());

    if app.input.is_dotlottie() {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(30), Constraint::Min(28)])
            .split(area);
        draw_sidebar(frame, columns[0], app);
        draw_preview(frame, columns[1], app);
    } else {
        draw_preview(frame, area, app);
    }
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

    let items = themes.iter().map(theme_item).collect::<Vec<_>>();
    let mut state = ListState::default();
    state.select(Some(app.theme_index));
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

fn draw_preview(frame: &mut Frame, area: Rect, app: &App) {
    let animation = app.input.selected_animation(app.animation_index);
    let block = panel(" Preview · renderer unavailable ", true).title_bottom(
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
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Rendering is not enabled yet"),
            Line::from(""),
            Line::from("This build has loaded and validated the animation."),
            Line::from("A terminal renderer will appear here in a later release."),
        ])
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::White)),
        message_area,
    );
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
