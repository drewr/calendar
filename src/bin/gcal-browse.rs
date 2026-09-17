use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;

const ACCOUNTS_DIR: &str = ".config/gcalcli/accounts";

// ── k9s "dracula" skin palette ──────────────────────────────────────────────
const FG: Color = Color::Rgb(0xf8, 0xf8, 0xf2);
const BG: Color = Color::Rgb(0x28, 0x2a, 0x36);
const SEL: Color = Color::Rgb(0x44, 0x47, 0x5a); // current line / selection
const COMM: Color = Color::Rgb(0x62, 0x72, 0xa4); // comment / muted
const CYAN: Color = Color::Rgb(0x8b, 0xe9, 0xfd);
const GREEN: Color = Color::Rgb(0x50, 0xfa, 0x7b);
const PINK: Color = Color::Rgb(0xff, 0x79, 0xc6);
const PURPLE: Color = Color::Rgb(0xbd, 0x93, 0xf9);
const RED: Color = Color::Rgb(0xff, 0x55, 0x55);
const YELLOW: Color = Color::Rgb(0xf1, 0xfa, 0x8c);

struct Calendar {
    title: String,
    access: String,
}

struct CalEvent {
    date: String,
    time: String,
    end_date: String,
    end_time: String,
    title: String,
    desc: String,
    location: String,
    url: String,
    calendar: String,
    email: String,
}

struct Account {
    name: String,
    config_dir: Option<String>,
}

#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Accounts,
    Calendars,
    Events,
    EventDetail,
    Help,
}

struct App {
    accounts: Vec<Account>,
    account_state: ListState,
    account_config: Option<String>,
    calendars: Vec<Calendar>,
    cal_state: ListState,
    events: Vec<CalEvent>,
    events_state: ListState,
    screen: Screen,
    prev_screen: Screen,
    selected_calendar: Option<String>,
    searching: bool,
    query: String,
    commanding: bool,
    command: String,
    loading: bool,
    error: Option<String>,
    detail_index: Option<usize>,
    detail_scroll: u16,
    detail_lines: Vec<String>,
    page_hint: u16,
}

impl App {
    fn new(accounts: Vec<Account>) -> Self {
        let mut account_state = ListState::default();
        if !accounts.is_empty() {
            account_state.select(Some(0));
        }
        let mut cal_state = ListState::default();
        cal_state.select(Some(0));
        App {
            accounts,
            account_state,
            account_config: None,
            calendars: Vec::new(),
            cal_state,
            events: Vec::new(),
            events_state: ListState::default(),
            screen: Screen::Accounts,
            prev_screen: Screen::Accounts,
            selected_calendar: None,
            searching: false,
            query: String::new(),
            commanding: false,
            command: String::new(),
            loading: false,
            error: None,
            detail_index: None,
            detail_scroll: 0,
            detail_lines: Vec::new(),
            page_hint: 0,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let accounts = match list_accounts() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to discover accounts: {e}");
            std::process::exit(1);
        }
    };
    if accounts.is_empty() {
        eprintln!("No gcalcli accounts found. Run `gcalcli list` to verify your account.");
        std::process::exit(1);
    }

    let mut app = App::new(accounts);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    match result {
        Ok(()) => {}
        Err(e) if e.to_string() == "quit" => {}
        Err(e) => eprintln!("{e}"),
    }
    Ok(())
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        if app.loading {
            app.loading = false;
            let result = match app.screen {
                Screen::Calendars => load_calendars(app),
                Screen::Events => load_events(app),
                _ => Ok(()),
            };
            match result {
                Ok(()) => {
                    app.error = None;
                    let select = match app.screen {
                        Screen::Calendars => &mut app.cal_state,
                        Screen::Events => &mut app.events_state,
                        _ => return Ok(()),
                    };
                    select.select(Some(0));
                }
                Err(e) => app.error = Some(e.to_string()),
            }
        }
        terminal.draw(|f| draw(f, app))?;
        if event::poll(Duration::from_millis(50))? {
            let Event::Key(key) = event::read()? else {
                continue;
            };
            handle_key(key, app)?;
        }
    }
}

// ── Key handling ────────────────────────────────────────────────────────────

fn handle_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // Command mode (`:`)
    if app.commanding {
        return handle_command_key(key, app);
    }
    // Filter mode (`/`)
    if app.searching {
        return handle_filter_key(key, app);
    }

    match key.code {
        KeyCode::Char('q') => return Err("quit".into()),
        KeyCode::Char('c') if ctrl => return Err("quit".into()),
        KeyCode::F(1) => return Err("quit".into()),
        _ => {}
    }

    // Help is toggled from every screen.
    if key.code == KeyCode::Char('?') {
        if app.screen == Screen::Help {
            app.screen = app.prev_screen;
        } else {
            app.prev_screen = app.screen;
            app.screen = Screen::Help;
        }
        return Ok(());
    }

    match app.screen {
        Screen::Accounts => handle_accounts_key(key, app),
        Screen::Calendars => handle_calendars_key(key, app),
        Screen::Events => handle_events_key(key, app),
        Screen::EventDetail => handle_detail_key(key, app),
        Screen::Help => Ok(()),
    }
}

fn handle_accounts_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let n = app.accounts.len();
    let page = app.page_hint as isize;
    match key.code {
        KeyCode::Esc => go_back(app),
        KeyCode::Enter => {
            let Some(i) = app.account_state.selected() else {
                return Ok(());
            };
            if i >= app.accounts.len() {
                return Ok(());
            }
            app.account_config = app.accounts[i].config_dir.clone();
            app.prev_screen = app.screen;
            app.screen = Screen::Calendars;
            app.calendars.clear();
            app.cal_state.select(Some(0));
            app.loading = true;
        }
        KeyCode::Char('/') => start_filter(app),
        KeyCode::Char(':') => start_command(app),
        KeyCode::Char('r') if ctrl => app.loading = true,
        KeyCode::Down | KeyCode::Char('j') if !ctrl => step_list(n, 1, &mut app.account_state),
        KeyCode::Up | KeyCode::Char('k') if !ctrl => step_list(n, -1, &mut app.account_state),
        KeyCode::Char('n') if ctrl => step_list(n, 1, &mut app.account_state),
        KeyCode::Char('p') if ctrl => step_list(n, -1, &mut app.account_state),
        KeyCode::PageDown | KeyCode::Char(' ') => page_list(n, 1, page, &mut app.account_state),
        KeyCode::PageUp => page_list(n, -1, page, &mut app.account_state),
        KeyCode::Char('g') => set_list(0, &mut app.account_state),
        KeyCode::Char('G') => set_list(n.saturating_sub(1), &mut app.account_state),
        _ => {}
    }
    Ok(())
}

fn handle_calendars_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let n = app.calendars.len();
    let page = app.page_hint as isize;
    match key.code {
        KeyCode::Esc => go_back(app),
        KeyCode::Enter => {
            let Some(i) = app.cal_state.selected() else {
                return Ok(());
            };
            if i >= app.calendars.len() {
                return Ok(());
            }
            app.selected_calendar = Some(app.calendars[i].title.clone());
            app.prev_screen = app.screen;
            app.screen = Screen::Events;
            app.loading = true;
        }
        KeyCode::Char('/') => start_filter(app),
        KeyCode::Char(':') => start_command(app),
        KeyCode::Char('r') if ctrl => app.loading = true,
        KeyCode::Down | KeyCode::Char('j') if !ctrl => step_list(n, 1, &mut app.cal_state),
        KeyCode::Up | KeyCode::Char('k') if !ctrl => step_list(n, -1, &mut app.cal_state),
        KeyCode::Char('n') if ctrl => step_list(n, 1, &mut app.cal_state),
        KeyCode::Char('p') if ctrl => step_list(n, -1, &mut app.cal_state),
        KeyCode::PageDown | KeyCode::Char(' ') => page_list(n, 1, page, &mut app.cal_state),
        KeyCode::PageUp => page_list(n, -1, page, &mut app.cal_state),
        KeyCode::Char('g') => set_list(0, &mut app.cal_state),
        KeyCode::Char('G') => set_list(n.saturating_sub(1), &mut app.cal_state),
        _ => {}
    }
    Ok(())
}

fn handle_events_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let n = app.events.len();
    let page = app.page_hint as isize;
    match key.code {
        KeyCode::Esc => go_back(app),
        KeyCode::Enter | KeyCode::Char('d') => {
            if let Some(i) = app.events_state.selected() {
                if i < app.events.len() {
                    app.detail_index = Some(i);
                    app.detail_scroll = 0;
                    app.detail_lines = detail_lines(app, i);
                    app.prev_screen = Screen::Events;
                    app.screen = Screen::EventDetail;
                }
            }
        }
        KeyCode::Char('/') => start_filter(app),
        KeyCode::Char(':') => start_command(app),
        KeyCode::Char('r') if ctrl => app.loading = true,
        KeyCode::Down | KeyCode::Char('j') if !ctrl => step_list(n, 1, &mut app.events_state),
        KeyCode::Up | KeyCode::Char('k') if !ctrl => step_list(n, -1, &mut app.events_state),
        KeyCode::Char('n') if ctrl => step_list(n, 1, &mut app.events_state),
        KeyCode::Char('p') if ctrl => step_list(n, -1, &mut app.events_state),
        KeyCode::PageDown | KeyCode::Char(' ') => page_list(n, 1, page, &mut app.events_state),
        KeyCode::PageUp => page_list(n, -1, page, &mut app.events_state),
        KeyCode::Char('g') => set_list(0, &mut app.events_state),
        KeyCode::Char('G') => set_list(n.saturating_sub(1), &mut app.events_state),
        _ => {}
    }
    Ok(())
}

fn handle_detail_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace | KeyCode::Char('q') => {
            app.screen = Screen::Events;
            app.detail_index = None;
            app.detail_scroll = 0;
        }
        KeyCode::Char('n') => detail_next(app, 1),
        KeyCode::Char('p') => detail_next(app, -1),
        KeyCode::Down | KeyCode::Char('j') => detail_scroll(app, 1),
        KeyCode::Up | KeyCode::Char('k') => detail_scroll(app, -1),
        KeyCode::PageDown | KeyCode::Char(' ') => detail_scroll(app, 10),
        KeyCode::PageUp => detail_scroll(app, -10),
        KeyCode::Char('g') => app.detail_scroll = 0,
        KeyCode::Char('G') => {
            let n = app.detail_lines.len();
            if n > 0 {
                app.detail_scroll = (n as u16).saturating_sub(1);
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_filter_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    match key.code {
        KeyCode::Char(c) => {
            app.query.push(c);
        }
        KeyCode::Backspace => {
            app.query.pop();
        }
        KeyCode::Enter => {
            app.searching = false;
            app.events.clear();
            app.loading = true;
        }
        KeyCode::Esc => {
            app.searching = false;
            app.query.clear();
            app.events.clear();
            app.loading = true;
        }
        _ => {}
    }
    Ok(())
}

fn handle_command_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    match key.code {
        KeyCode::Char(c) => {
            app.command.push(c);
        }
        KeyCode::Backspace => {
            app.command.pop();
        }
        KeyCode::Enter => {
            let cmd = app.command.trim().to_lowercase();
            app.commanding = false;
            app.command.clear();
            match cmd.as_str() {
                "q" | "quit" | "exit" => return Err("quit".into()),
                "refresh" => app.loading = true,
                _ => {}
            }
        }
        KeyCode::Esc => {
            app.commanding = false;
            app.command.clear();
        }
        _ => {}
    }
    Ok(())
}

fn start_filter(app: &mut App) {
    app.searching = true;
    app.query.clear();
}

fn start_command(app: &mut App) {
    app.commanding = true;
    app.command.clear();
}

fn go_back(app: &mut App) {
    app.screen = match app.screen {
        Screen::EventDetail => Screen::Events,
        Screen::Events => Screen::Calendars,
        Screen::Calendars => Screen::Accounts,
        other => other,
    };
    if app.screen == Screen::Calendars || app.screen == Screen::Accounts {
        app.selected_calendar = None;
        app.events.clear();
        app.events_state.select(None);
    }
}

fn step_list(n: usize, delta: isize, state: &mut ListState) {
    if n == 0 {
        return;
    }
    let i = state.selected().unwrap_or(0) as isize;
    let next = (i + delta).clamp(0, n as isize - 1) as usize;
    state.select(Some(next));
}

fn set_list(i: usize, state: &mut ListState) {
    state.select(Some(i));
}

fn page_list(n: usize, dir: isize, page: isize, state: &mut ListState) {
    if n == 0 {
        return;
    }
    let page = page.max(1);
    let i = state.selected().unwrap_or(0) as isize;
    let next = (i + dir * page).clamp(0, n as isize - 1) as usize;
    state.select(Some(next));
}

fn detail_scroll(app: &mut App, delta: isize) {
    let n = app.detail_lines.len();
    if n == 0 {
        return;
    }
    let max = (n as u16).saturating_sub(1);
    let next = (app.detail_scroll as isize + delta).clamp(0, max as isize) as u16;
    app.detail_scroll = next;
}

fn detail_next(app: &mut App, delta: isize) {
    let Some(i) = app.detail_index else { return };
    let n = app.events.len();
    if n == 0 {
        return;
    }
    let next = ((i as isize) + delta).clamp(0, n as isize - 1) as usize;
    app.detail_index = Some(next);
    app.detail_scroll = 0;
    app.detail_lines = detail_lines(app, next);
    app.events_state.select(Some(next));
}

// ── Drawing ─────────────────────────────────────────────────────────────────

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    // Fill the entire screen with the dracula background so nothing shows
    // through as the terminal's default (often light) background.
    f.render_widget(
        Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(BG)),
        f.area(),
    );

    if app.screen == Screen::Help {
        draw_help(f);
        return;
    }
    let (header, main, menu, prompt) = layout(f.area());

    // k9s-style bordered frame around the main content.
    let frame_title: &str = match app.screen {
        Screen::Accounts => "Accounts",
        Screen::Calendars => "Calendars",
        Screen::Events => "Events",
        Screen::EventDetail => "Event Detail",
        _ => "",
    };
    let frame = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COMM))
        .title(Span::styled(
            format!(" {frame_title} "),
            Style::default().fg(FG).bg(BG),
        ));
    let inner = frame.inner(main);
    f.render_widget(frame, main);

    draw_header(f, header, app);
    match app.screen {
        Screen::Accounts => draw_list_view(f, inner, app),
        Screen::Calendars => draw_calendars(f, inner, app),
        Screen::Events => draw_events(f, inner, app),
        Screen::EventDetail => draw_event_detail(f, inner, app),
        _ => {}
    }
    draw_menu(f, menu, app);
    draw_prompt(f, prompt, app);
}

fn layout(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let a = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
    (a[0], a[1], a[2], a[3])
}

fn esc(style: Style) -> Style {
    style.bg(BG)
}

fn st() -> Style {
    esc(Style::default().fg(FG))
}

fn draw_header(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let acc = current_account_name(app);
    let (logo, title) = match app.screen {
        Screen::Accounts => ("accounts", "Select an Account"),
        Screen::Calendars => ("calendars", "Select a Calendar"),
        Screen::Events => {
            let cal = app.selected_calendar.as_deref().unwrap_or("");
            (cal, "Upcoming Events")
        }
        Screen::EventDetail => {
            let cal = app.selected_calendar.as_deref().unwrap_or("");
            (cal, "Event Detail")
        }
        _ => ("", ""),
    };
    let today = chrono::Local::now().format("%a %b %d %Y").to_string();
    let line = Line::from(vec![
        Span::styled("gcal-browse", st().fg(PURPLE).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(if acc.is_empty() { "n/a" } else { acc }, st().fg(GREEN)),
        Span::raw(" ● "),
        Span::styled(logo, st().fg(PINK)),
        Span::raw(" · "),
        Span::styled(title, st()),
        Span::raw("   "),
        Span::styled(today, st().fg(COMM)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_menu(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let items = match app.screen {
        Screen::EventDetail => vec![
            ("j/k", "scroll"),
            ("n/p", "event"),
            ("g/G", "top/bot"),
            ("esc", "back"),
            ("q", "quit"),
        ],
        _ => vec![
            ("j/k", "down/up"),
            ("enter", "select"),
            ("d", "detail"),
            ("/", "filter"),
            (":", "cmd"),
            ("g/G", "top/bot"),
            ("esc", "back"),
            ("ctrl-r", "refresh"),
            ("q", "quit"),
        ],
    };
    let mut spans = Vec::new();
    for (i, (k, desc)) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(format!("<{k}>"), st().fg(PINK)));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(*desc, st()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_prompt(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut spans = Vec::new();

    // Breadcrumbs (k9s-style, SEL background).
    let crumbs: Vec<String> = match app.screen {
        Screen::EventDetail => vec![
            "accounts".to_string(),
            current_account_name(app).to_string(),
            app.selected_calendar.clone().unwrap_or_default(),
            "detail".to_string(),
        ],
        Screen::Events => vec![
            "accounts".to_string(),
            current_account_name(app).to_string(),
            app.selected_calendar.clone().unwrap_or_default(),
        ],
        Screen::Calendars => vec![
            "accounts".to_string(),
            current_account_name(app).to_string(),
        ],
        _ => vec!["accounts".to_string()],
    };
    for (i, c) in crumbs.iter().enumerate() {
        if !c.is_empty() {
            if i > 0 {
                spans.push(Span::raw(" ▸ "));
            }
            spans.push(Span::styled(c.clone(), st().bg(SEL).fg(FG)));
        }
    }

    if app.searching {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("/ ", st().fg(CYAN)));
        if !app.query.is_empty() {
            spans.push(Span::styled(&app.query, st()));
        }
        spans.push(Span::styled("█", st().add_modifier(Modifier::SLOW_BLINK)));
    } else if app.commanding {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(":> ", st().fg(CYAN)));
        spans.push(Span::styled(&app.command, st()));
        spans.push(Span::styled("█", st().add_modifier(Modifier::SLOW_BLINK)));
    } else {
        spans.push(Span::raw("  :>"));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_list_view(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = app
        .accounts
        .iter()
        .map(|a| {
            let is_default = a.config_dir.is_none();
            ListItem::new(Line::from(vec![
                Span::styled(
                    if is_default { "default" } else { "named" },
                    if is_default { st().fg(COMM) } else { st().fg(CYAN) },
                ),
                Span::raw("  "),
                Span::styled(&a.name, st()),
            ]))
        })
        .collect();
    render_table(
        f,
        area,
        &mut app.page_hint,
        &["TYPE", "NAME"],
        items,
        &mut app.account_state,
    );
}

fn draw_calendars(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    if app.loading {
        render_center(f, area, "Loading calendars…", CYAN);
        return;
    }
    if let Some(err) = &app.error {
        render_center(f, area, &format!("Error: {err}"), RED);
        return;
    }
    let items: Vec<ListItem> = app
        .calendars
        .iter()
        .map(|c| {
            let access = match c.access.as_str() {
                "owner" => "owner",
                "writer" => "rw",
                "reader" => "ro",
                other => other,
            };
            ListItem::new(Line::from(vec![
                Span::styled(access, st().fg(COMM)),
                Span::raw("  "),
                Span::styled(&c.title, st()),
            ]))
        })
        .collect();
    render_table(
        f,
        area,
        &mut app.page_hint,
        &["ACCESS", "CALENDAR"],
        items,
        &mut app.cal_state,
    );
}

fn draw_events(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    if app.loading {
        render_center(f, area, "Loading events…", CYAN);
        return;
    }
    if let Some(err) = &app.error {
        render_center(f, area, &format!("Error: {err}"), RED);
        return;
    }
    let tz = tz_abbr();
    let items: Vec<ListItem> = app
        .events
        .iter()
        .map(|e| {
            let date_label = match chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") {
                Ok(d) => format!("{} {}", e.date, d.format("%a").to_string().to_uppercase()),
                Err(_) => e.date.clone(),
            };
            let mut spans = vec![
                Span::styled(date_label, st().fg(YELLOW)),
                Span::raw("  "),
                Span::styled(format!("{:<11}", fmt_time(&e.time, &tz)), st().fg(CYAN)),
                Span::raw("  "),
                Span::styled(&e.title, st()),
            ];
            if !e.desc.is_empty() {
                spans.push(Span::raw("  —  "));
                spans.push(Span::styled(&e.desc, st().fg(COMM)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    if items.is_empty() {
        render_center(f, area, "No upcoming events.", COMM);
        return;
    }
    render_table(
        f,
        area,
        &mut app.page_hint,
        &["DATE", "TIME", "EVENT/TITLE"],
        items,
        &mut app.events_state,
    );
}

/// Render a list as a k9s-style table: header row + highlighted selection.
fn render_table(
    f: &mut ratatui::Frame,
    area: Rect,
    page_hint: &mut u16,
    headers: &[&str],
    items: Vec<ListItem>,
    state: &mut ListState,
) {
    // A plain header row (inverted foreground), no border box.
    let header_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let header_line = Line::from(
        headers
            .iter()
            .map(|h| Span::styled(format!("{h:<15}"), st().fg(COMM)))
            .collect::<Vec<_>>(),
    );
    f.render_widget(Paragraph::new(header_line), header_area);

    let body = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: area.height.saturating_sub(1),
    };
    let list = List::new(items)
        .highlight_style(st().bg(SEL).fg(FG).add_modifier(Modifier::BOLD))
        .highlight_symbol("› ");
    *page_hint = body.height.saturating_sub(1);
    f.render_stateful_widget(list, body, state);
}

fn render_center(f: &mut ratatui::Frame, area: Rect, msg: &str, color: Color) {
    let p = Paragraph::new(Line::from(Span::styled(msg, st().fg(color))))
        .alignment(Alignment::Center);
    f.render_widget(p, area);
}

fn draw_event_detail(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::NONE)
        .title(" Details ");
    let inner = block.inner(area);

    if app.detail_lines.is_empty() {
        f.render_widget(
            Paragraph::new("No event selected.").style(st().fg(COMM)),
            inner,
        );
        return;
    }

    let viewport = inner.height;
    let scroll = app.detail_scroll.min(app.detail_lines.len().saturating_sub(1) as u16);
    let visible: Vec<Line> = app
        .detail_lines
        .iter()
        .skip(scroll as usize)
        .take(viewport as usize)
        .map(|l| {
            // Style the "key:" prefix in cyan/purple, rest in foreground.
            if let Some(idx) = l.find(':') {
                Line::from(vec![
                    Span::styled(l[..=idx].to_string(), st().fg(PURPLE).add_modifier(Modifier::BOLD)),
                    Span::styled(l[idx + 1..].to_string(), st()),
                ])
            } else {
                Line::from(Span::styled(l.clone(), st()))
            }
        })
        .collect();

    f.render_widget(Paragraph::new(visible), inner);
}

fn draw_help(f: &mut ratatui::Frame) {
    let (header, main, _menu, prompt) = layout(f.area());
    let _ = header;
    let _ = prompt;
    let lines = vec![
        Line::from(Span::styled(" Help — Key Bindings", st().fg(PURPLE).add_modifier(Modifier::BOLD))),
        Line::from(" "),
        Line::from(vec![Span::styled("<j/k>", st().fg(PINK)), Span::raw("  move down/up"), Span::raw("   "), Span::styled("<↓/↑>", st().fg(PINK)), Span::raw("  move")]),
        Line::from(vec![Span::styled("<enter>", st().fg(PINK)), Span::raw("  select / drill down")]),
        Line::from(vec![Span::styled("<d>", st().fg(PINK)), Span::raw("  open event detail")]),
        Line::from(vec![Span::styled("<g>/<G>", st().fg(PINK)), Span::raw("  go to top / bottom")]),
        Line::from(vec![Span::styled("<pgup>/<pgdn>", st().fg(PINK)), Span::raw("  page up / page down")]),
        Line::from(vec![Span::styled("</>", st().fg(PINK)), Span::raw("  filter events (prompt)")]),
        Line::from(vec![Span::styled("<:>", st().fg(PINK)), Span::raw("  command mode (:q to quit, :refresh)")]),
        Line::from(vec![Span::styled("<esc>", st().fg(PINK)), Span::raw("  go back one level")]),
        Line::from(vec![Span::styled("<?>", st().fg(PINK)), Span::raw("  toggle this help")]),
        Line::from(vec![Span::styled("<ctrl-r>", st().fg(PINK)), Span::raw("  refresh current view")]),
        Line::from(vec![Span::styled("<q>/<ctrl-c>/<F1>", st().fg(PINK)), Span::raw("  quit")]),
        Line::from(" "),
        Line::from(vec![Span::styled("Emacs: ", st().fg(COMM)), Span::raw("C-n down · C-p up · C-f right · C-b left · C-g top")]),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).border_style(st().fg(COMM)).title(" Help ")),
        main,
    );
}

// ── Utilities ───────────────────────────────────────────────────────────────

fn current_account_name(app: &App) -> &str {
    app.accounts
        .iter()
        .find(|a| a.config_dir == app.account_config)
        .map(|a| a.name.as_str())
        .unwrap_or("")
}

/// Local timezone abbreviation (e.g. "CDT", "CST", "EST").
fn tz_abbr() -> String {
    match local_tz_name().and_then(|name| name.parse::<chrono_tz::Tz>().ok()) {
        Some(tz) => chrono::Local::now().with_timezone(&tz).format("%Z").to_string(),
        None => chrono::Local::now().format("%Z").to_string(),
    }
}

fn local_tz_name() -> Option<String> {
    let name = std::fs::canonicalize("/etc/localtime")
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = name
        .rsplit("/zoneinfo/")
        .next()
        .unwrap_or("")
        .to_string();
    if !name.is_empty() {
        return Some(name);
    }
    std::env::var("TZ").ok().filter(|s| !s.is_empty())
}

fn fmt_time(t: &str, tz: &str) -> String {
    if t.is_empty() || t == "all-day" {
        t.to_string()
    } else {
        format!("{} {}", t, tz)
    }
}

/// Build the human-readable lines shown on the event detail screen.
fn detail_lines(app: &App, idx: usize) -> Vec<String> {
    let Some(e) = app.events.get(idx) else {
        return vec![String::new()];
    };
    let total = app.events.len();
    let mut lines = Vec::new();
    lines.push(format!("{} ({}/{})", e.title, idx + 1, total));
    lines.push(String::new());
    let tz = tz_abbr();
    let range = if e.time == "all-day" {
        if !e.end_date.is_empty() && e.end_date != e.date {
            format!("{}  (all-day)  →  {}", e.date, e.end_date)
        } else {
            format!("{}  (all-day)", e.date)
        }
    } else if !e.end_time.is_empty() {
        format!(
            "{} {}  →  {} {}",
            e.date,
            fmt_time(&e.time, &tz),
            e.end_date,
            fmt_time(&e.end_time, &tz)
        )
    } else {
        format!("{} {}", e.date, fmt_time(&e.time, &tz))
    };
    lines.push(format!("When:  {range}"));
    if !e.location.is_empty() {
        lines.push(format!("Where: {}", e.location));
    }
    if !e.calendar.is_empty() {
        lines.push(format!("Calendar: {}", e.calendar));
    }
    if !e.email.is_empty() {
        lines.push(format!("Organizer: {}", e.email));
    }
    if !e.url.is_empty() {
        lines.push(format!("Link: {}", e.url));
    }
    if !e.desc.is_empty() {
        lines.push(String::new());
        lines.push("Description:".to_string());
        for dline in e.desc.trim_end().split('\n') {
            lines.push(dline.to_string());
        }
    }
    lines
}

// ── gcalcli integrations ────────────────────────────────────────────────────

fn list_accounts() -> Result<Vec<Account>, Box<dyn std::error::Error>> {
    // Only show a "Default" account if gcalcli's platform data dir actually
    // holds an oauth token there; otherwise it would be a stale, unauthenticated
    // placeholder now that accounts live under ~/.config/gcalcli/accounts/*.
    let mut accounts = Vec::new();
    if default_data_dir()?.join("oauth").exists() {
        accounts.push(Account {
            name: "Default".to_string(),
            config_dir: None,
        });
    }

    let dir = home_dir()?.join(ACCOUNTS_DIR);
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut names: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let oauth = entry.path().join("gcalcli").join("oauth");
                if oauth.exists() {
                    if let Some(name) = entry.file_name().to_str() {
                        names.push(name.to_string());
                    }
                }
            }
        }
        names.sort();
        for name in names {
            accounts.push(Account {
                name: name.clone(),
                config_dir: Some(dir.join(&name).display().to_string()),
            });
        }
    }

    Ok(accounts)
}

/// Resolve gcalcli's default platform data dir (where its oauth lives when no
/// XDG_DATA_HOME override is applied), mirroring platformdirs.user_data_path.
fn default_data_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Ok(home) = std::env::var("XDG_DATA_HOME") {
        if !home.is_empty() {
            return Ok(PathBuf::from(home).join("gcalcli"));
        }
    }
    let home = home_dir()?;
    if cfg!(target_os = "macos") {
        Ok(home.join("Library/Application Support/gcalcli"))
    } else {
        Ok(home.join(".local/share/gcalcli"))
    }
}

fn home_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").map(PathBuf::from)?;
    Ok(home)
}

fn run_gcalcli(args: &[&str], account_dir: Option<&str>) -> Result<std::process::Output, io::Error> {
    let mut cmd = Command::new("gcalcli");
    cmd.args(args);
    if let Some(dir) = account_dir {
        cmd.env("XDG_DATA_HOME", dir);
        cmd.env("GCALCLI_CONFIG", dir);
    }
    cmd.output()
}

fn load_calendars(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let output = run_gcalcli(&["list", "--nocolor"], app.account_config.as_deref())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gcalcli list failed: {stderr}").into());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut calendars = Vec::new();
    for line in stdout.lines() {
        let stripped = strip_ansi(line).trim().to_string();
        if stripped.is_empty() {
            continue;
        }
        if !line_has_access(&stripped) {
            continue;
        }
        if let Some(sep) = stripped.find("  ") {
            let access = stripped[..sep].trim().to_string();
            let title = stripped[sep..].trim().to_string();
            if !title.is_empty() {
                calendars.push(Calendar { title, access });
            }
        }
    }
    if calendars.is_empty() {
        return Err("no calendars found for this account".into());
    }
    app.calendars = calendars;
    Ok(())
}

fn line_has_access(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("owner")
        || t.starts_with("writer")
        || t.starts_with("reader")
        || t.starts_with("freebusy")
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphabetic() {
                    chars.next();
                    break;
                }
                chars.next();
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn load_events(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let cal = app.selected_calendar.as_deref().unwrap_or("");
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let end = (chrono::Utc::now() + chrono::Duration::days(3650)).format("%Y-%m-%d").to_string();

    let mut args: Vec<String> = vec!["agenda".into()];
    args.push(today.clone());
    args.push(end);
    args.extend(
        [
            "--calendar",
            cal,
            "--details",
            "all",
            "--tsv",
            "--nocolor",
            "--military",
            "--nostarted",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let output = run_gcalcli(&arg_refs, app.account_config.as_deref())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gcalcli agenda failed: {stderr}").into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let query_lower = app.query.to_lowercase();

    let mut events = Vec::new();
    let mut lines = stdout.lines();
    if let Some(header) = lines.next() {
        let headers: Vec<&str> = header.split('\t').collect();
        for line in lines {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < headers.len() {
                continue;
            }
            let fget = |name: &str| -> &str {
                headers
                    .iter()
                    .position(|h| *h == name)
                    .and_then(|i| fields.get(i).copied())
                    .unwrap_or("")
            };
            let date = fget("start_date");
            if date.is_empty() {
                continue;
            }
            let title = fget("title");
            let desc = fget("description");
            if !query_lower.is_empty()
                && !title.to_lowercase().contains(&query_lower)
                && !desc.to_lowercase().contains(&query_lower)
            {
                continue;
            }
            let time = fget("start_time");
            let time = if time.is_empty() {
                "all-day".to_string()
            } else {
                time.to_string()
            };
            events.push(CalEvent {
                date: date.to_string(),
                time,
                end_date: fget("end_date").to_string(),
                end_time: fget("end_time").to_string(),
                title: title.to_string(),
                desc: desc.to_string(),
                location: fget("location").to_string(),
                url: fget("html_link").to_string(),
                calendar: fget("calendar").to_string(),
                email: fget("email").to_string(),
            });
        }
    }

    events.retain(|e| e.date >= today);
    let day_rank = |time: &str| if time == "all-day" { 0 } else { 1 };
    events.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then(day_rank(&a.time).cmp(&day_rank(&b.time)))
            .then(a.time.cmp(&b.time))
    });

    app.events = events;
    Ok(())
}
