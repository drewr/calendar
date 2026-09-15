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
use ratatui::prelude::Stylize;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;

const ACCOUNTS_DIR: &str = ".config/gcalcli/accounts";

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
    /// gcalcli config dir for this account, or None for the default account.
    config_dir: Option<String>,
}

#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Accounts,
    Calendars,
    Events,
    EventDetail,
}

struct App {
    accounts: Vec<Account>,
    account_state: ListState,
    /// config dir of the currently selected account (None = default).
    account_config: Option<String>,
    calendars: Vec<Calendar>,
    cal_state: ListState,
    events: Vec<CalEvent>,
    events_state: ListState,
    screen: Screen,
    selected_calendar: Option<String>,
    searching: bool,
    query: String,
    loading: bool,
    error: Option<String>,
    detail_index: Option<usize>,
    detail_scroll: u16,
    detail_lines: Vec<String>,
    /// Height of the currently-visible list page (used for paging).
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
            selected_calendar: None,
            searching: false,
            query: String::new(),
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
                        _ => &mut app.events_state,
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

fn handle_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    if key.code == KeyCode::Char('q') && !app.searching && app.screen != Screen::EventDetail {
        return Err("quit".into());
    }
    match app.screen {
        Screen::Accounts => handle_accounts_key(key, app),
        Screen::Calendars => handle_calendars_key(key, app),
        Screen::Events => handle_events_key(key, app),
        Screen::EventDetail => handle_detail_key(key, app),
    }
}

fn handle_accounts_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let n = app.accounts.len();
    match key.code {
        KeyCode::Esc => return Err("quit".into()),
        KeyCode::Enter => {
            if let Some(i) = app.account_state.selected() {
                app.account_config = app.accounts[i].config_dir.clone();
                app.screen = Screen::Calendars;
                app.calendars.clear();
                app.cal_state.select(Some(0));
                app.loading = true;
            }
        }
        KeyCode::Down | KeyCode::Char('j') if !ctrl => step_list(n, 1, &mut app.account_state),
        KeyCode::Up | KeyCode::Char('k') if !ctrl => step_list(n, -1, &mut app.account_state),
        KeyCode::Char('n') if ctrl => step_list(n, 1, &mut app.account_state),
        KeyCode::Char('p') if ctrl => step_list(n, -1, &mut app.account_state),
        KeyCode::Char('f') if ctrl => step_list(n, 1, &mut app.account_state),
        KeyCode::Char('b') if ctrl => step_list(n, -1, &mut app.account_state),
        KeyCode::PageDown | KeyCode::Char(' ') => page_list(n, 1, app.page_hint as isize, &mut app.account_state),
        KeyCode::PageUp => page_list(n, -1, app.page_hint as isize, &mut app.account_state),
        KeyCode::Home | KeyCode::Char('g') if ctrl => set_list(0, &mut app.account_state),
        KeyCode::Char('g') if !ctrl => {
            set_list(n.saturating_sub(1), &mut app.account_state);
        }
        _ => {}
    }
    Ok(())
}

fn handle_calendars_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let n = app.calendars.len();
    match key.code {
        KeyCode::Esc => return Err("quit".into()),
        KeyCode::Enter => {
            if let Some(i) = app.cal_state.selected() {
                app.selected_calendar = Some(app.calendars[i].title.clone());
                app.screen = Screen::Events;
                app.loading = true;
            }
        }
        KeyCode::Char('a') if !ctrl => {
            app.screen = Screen::Accounts;
        }
        KeyCode::Down | KeyCode::Char('j') if !ctrl => step_list(n, 1, &mut app.cal_state),
        KeyCode::Up | KeyCode::Char('k') if !ctrl => step_list(n, -1, &mut app.cal_state),
        KeyCode::Char('n') if ctrl => step_list(n, 1, &mut app.cal_state),
        KeyCode::Char('p') if ctrl => step_list(n, -1, &mut app.cal_state),
        KeyCode::Char('f') if ctrl => step_list(n, 1, &mut app.cal_state),
        KeyCode::Char('b') if ctrl => step_list(n, -1, &mut app.cal_state),
        KeyCode::PageDown | KeyCode::Char(' ') => page_list(n, 1, app.page_hint as isize, &mut app.cal_state),
        KeyCode::PageUp => page_list(n, -1, app.page_hint as isize, &mut app.cal_state),
        KeyCode::Home | KeyCode::Char('g') if ctrl => set_list(0, &mut app.cal_state),
        KeyCode::Char('g') if !ctrl => set_list(n.saturating_sub(1), &mut app.cal_state),
        _ => {}
    }
    Ok(())
}

fn handle_events_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let n = app.events.len();

    if app.searching {
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
        return Ok(());
    }

    match key.code {
        KeyCode::Esc => return Err("quit".into()),
        KeyCode::Char('c') if !ctrl => {
            app.screen = Screen::Calendars;
            app.events.clear();
            app.events_state.select(None);
            app.selected_calendar = None;
            app.query.clear();
        }
        KeyCode::Enter => {
            if let Some(i) = app.events_state.selected() {
                if i < app.events.len() {
                    app.detail_index = Some(i);
                    app.detail_scroll = 0;
                    app.detail_lines = detail_lines(app, i);
                    app.screen = Screen::EventDetail;
                }
            }
        }
        KeyCode::Char('/') => start_search(app),
        KeyCode::Char('s') if ctrl => start_search(app),
        KeyCode::Down | KeyCode::Char('j') if !ctrl => step_list(n, 1, &mut app.events_state),
        KeyCode::Up | KeyCode::Char('k') if !ctrl => step_list(n, -1, &mut app.events_state),
        KeyCode::Char('n') if ctrl => step_list(n, 1, &mut app.events_state),
        KeyCode::Char('p') if ctrl => step_list(n, -1, &mut app.events_state),
        KeyCode::Char('f') if ctrl => step_list(n, 1, &mut app.events_state),
        KeyCode::Char('b') if ctrl => step_list(n, -1, &mut app.events_state),
        KeyCode::PageDown | KeyCode::Char(' ') => page_list(n, 1, app.page_hint as isize, &mut app.events_state),
        KeyCode::PageUp => page_list(n, -1, app.page_hint as isize, &mut app.events_state),
        KeyCode::Home | KeyCode::Char('g') if ctrl => set_list(0, &mut app.events_state),
        KeyCode::Char('g') if !ctrl => set_list(n.saturating_sub(1), &mut app.events_state),
        _ => {}
    }
    Ok(())
}

fn handle_detail_key(key: KeyEvent, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace | KeyCode::Char('q')
        | KeyCode::Char('c') => {
            app.screen = Screen::Events;
            app.detail_index = None;
            app.detail_scroll = 0;
        }
        // Next / previous event detail
        KeyCode::Char('n') => detail_next(app, 1),
        KeyCode::Char('p') => detail_next(app, -1),
        KeyCode::Down | KeyCode::Char('j') => detail_scroll(app, 1),
        KeyCode::Up | KeyCode::Char('k') => detail_scroll(app, -1),
        KeyCode::PageDown | KeyCode::Char(' ') => detail_scroll(app, 10),
        KeyCode::PageUp => detail_scroll(app, -10),
        _ => {}
    }
    Ok(())
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
    // Keep the list selection in sync so returning shows the right event.
    app.events_state.select(Some(next));
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

/// Jump one full visible page in `dir` (1 = down, -1 = up).
fn page_list(n: usize, dir: isize, page: isize, state: &mut ListState) {
    if n == 0 {
        return;
    }
    let page = page.max(1);
    let i = state.selected().unwrap_or(0) as isize;
    let next = (i + dir * page).clamp(0, n as isize - 1) as usize;
    state.select(Some(next));
}

fn start_search(app: &mut App) {
    app.searching = true;
    app.query.clear();
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    match app.screen {
        Screen::Accounts => draw_accounts(f, app),
        Screen::Calendars => draw_calendars(f, app),
        Screen::Events => draw_events(f, app),
        Screen::EventDetail => draw_event_detail(f, app),
    }
}

fn header_block(title: String) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::default().bold(),
        )))
}

fn footer_help(spans: Vec<Span<'static>>) -> Paragraph<'static> {
    Paragraph::new(Line::from(spans))
}

fn move_help() -> Vec<Span<'static>> {
    vec![
        Span::styled("↑/↓ j/k", Style::default().fg(Color::DarkGray)),
        Span::raw(" move   "),
        Span::styled("Enter", Style::default().fg(Color::DarkGray)),
        Span::raw(" select   "),
        Span::styled("q/esc", Style::default().fg(Color::DarkGray)),
        Span::raw(" quit   "),
        Span::styled("Emacs: C-n/C-p/C-f/C-b/C-g", Style::default().fg(Color::DarkGray)),
    ]
}

fn current_account_name(app: &App) -> &str {
    app.accounts
        .iter()
        .find(|a| a.config_dir == app.account_config)
        .map(|a| a.name.as_str())
        .unwrap_or("")
}

/// Local timezone abbreviation (e.g. "CDT", "CST", "EST").
fn tz_abbr() -> String {
    // Resolve the system IANA timezone from /etc/localtime so we can produce a
    // real abbreviation (chrono::Local only gives the raw UTC offset).
    match local_tz_name().and_then(|name| name.parse::<chrono_tz::Tz>().ok()) {
        Some(tz) => chrono::Local::now().with_timezone(&tz).format("%Z").to_string(),
        None => chrono::Local::now().format("%Z").to_string(),
    }
}

/// Read the configured IANA timezone name (e.g. "America/Chicago") from the
/// /etc/localtime symlink, falling back to $TZ. Returns an empty string if
/// neither yields a usable zone name.
fn local_tz_name() -> Option<String> {
    let name = std::fs::canonicalize("/etc/localtime")
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    // Strip the zoneinfo prefix to get the IANA name.
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

/// Format a start time with its timezone abbreviation (all-day events stay bare).
fn fmt_time(t: &str, tz: &str) -> String {
    if t.is_empty() || t == "all-day" {
        t.to_string()
    } else {
        format!("{} {}", t, tz)
    }
}

fn draw_accounts(f: &mut ratatui::Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(2)])
        .split(f.area());

    f.render_widget(
        header_block("gcal-browse  —  select an account".to_string()),
        areas[0],
    );

    let items: Vec<ListItem> = app
        .accounts
        .iter()
        .map(|a| {
            let is_default = a.config_dir.is_none();
            ListItem::new(Line::from(vec![
                if is_default {
                    Span::styled("default", Style::default().fg(Color::DarkGray))
                } else {
                    Span::raw("       ")
                },
                Span::raw("  "),
                Span::styled(&a.name, Style::default()),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Accounts "))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    app.page_hint = areas[1].height.saturating_sub(2);
    f.render_stateful_widget(list, areas[1], &mut app.account_state);

    f.render_widget(
        footer_help(vec![
            Span::styled("Accounts live in ", Style::default().fg(Color::DarkGray)),
            Span::raw("~/.config/gcalcli/accounts/<name>/"),
            Span::raw("   "),
            Span::styled("create with ", Style::default().fg(Color::DarkGray)),
            Span::raw("XDG_DATA_HOME=<name> gcalcli init"),
        ]),
        areas[2],
    );
}

fn draw_calendars(f: &mut ratatui::Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(2)])
        .split(f.area());

    let account = current_account_name(app);
    f.render_widget(
        header_block(format!("gcal-browse [{account}]  —  select a calendar")),
        areas[0],
    );

    if app.loading {
        f.render_widget(
            Paragraph::new("Loading calendars…")
                .style(Style::default().fg(Color::Cyan))
                .alignment(Alignment::Center),
            areas[1],
        );
    } else if let Some(err) = &app.error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(Color::Red),
            )))
            .alignment(Alignment::Center),
            areas[1],
        );
    } else {
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
                    Span::styled(
                        format!("{:<6}", access),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(&c.title),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Calendars "))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");
        app.page_hint = areas[1].height.saturating_sub(2);
        f.render_stateful_widget(list, areas[1], &mut app.cal_state);
    }

    let mut help = vec![
        Span::styled("a", Style::default().fg(Color::DarkGray)),
        Span::raw(" switch account   "),
    ];
    help.extend(move_help());
    f.render_widget(footer_help(help), areas[2]);
}

fn draw_events(f: &mut ratatui::Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(1), Constraint::Length(2)])
        .split(f.area());

    let cal = app.selected_calendar.as_deref().unwrap_or("");
    let acc = current_account_name(app);
    let today = chrono::Local::now().format("%a %b %d %Y").to_string();
    f.render_widget(
        header_block(format!("{cal} [{acc}]  —  upcoming events   ·   {today}")),
        areas[0],
    );

    let inner = Rect {
        x: areas[0].x + 1,
        y: areas[0].y + 1,
        width: areas[0].width.saturating_sub(2),
        height: areas[0].height.saturating_sub(2),
    };

    let mut status = vec![Line::from(format!(
        "Account: {acc}    Calendar: {cal}    ({} events)",
        app.events.len()
    ))];
    if app.searching {
        status.push(Line::from(vec![
            Span::styled("Search: ", Style::default().fg(Color::Cyan).bold()),
            Span::raw(&app.query),
            Span::styled("█", Style::default().add_modifier(Modifier::SLOW_BLINK)),
            Span::raw("   Enter=apply  Esc=cancel"),
        ]));
    } else if !app.query.is_empty() {
        status.push(Line::from(vec![
            Span::styled("Filter: ", Style::default().fg(Color::Cyan).bold()),
            Span::raw(&app.query),
            Span::raw("   / or C-s to change"),
        ]));
    } else {
        status.push(Line::from(vec![
            Span::styled("/ or C-s", Style::default().fg(Color::DarkGray)),
            Span::raw(" to search"),
        ]));
    }
    f.render_widget(
        Paragraph::new(status).wrap(Wrap { trim: false }),
        inner,
    );

    let list_block = Block::default().borders(Borders::ALL).title(" Events ");
    let list_area = list_block.inner(areas[1]);
    f.render_widget(list_block, areas[1]);

    if app.loading {
        f.render_widget(
            Paragraph::new("Loading events…")
                .style(Style::default().fg(Color::Cyan))
                .alignment(Alignment::Center),
            list_area,
        );
        return;
    }
    if let Some(err) = &app.error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(Color::Red),
            )))
            .alignment(Alignment::Center),
            list_area,
        );
        return;
    }

    let tz = tz_abbr();
    let items: Vec<ListItem> = app
        .events
        .iter()
        .map(|e| {
            let mut spans = vec![
                Span::styled(&e.date, Style::default().fg(Color::Yellow)),
                Span::raw("  "),
                Span::styled(
                    format!("{:<11}", fmt_time(&e.time, &tz)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw("  "),
                Span::styled(&e.title, Style::default().add_modifier(Modifier::BOLD)),
            ];
            if !e.desc.is_empty() {
                spans.push(Span::raw("  —  "));
                spans.push(Span::styled(&e.desc, Style::default().fg(Color::DarkGray)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No upcoming events.")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            list_area,
        );
        return;
    }

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    app.page_hint = list_area.height;
    f.render_stateful_widget(list, list_area, &mut app.events_state);

    f.render_widget(
        footer_help(vec![
            Span::styled("↑/↓ j/k", Style::default().fg(Color::DarkGray)),
            Span::raw(" move   "),
            Span::styled("Enter", Style::default().fg(Color::DarkGray)),
            Span::raw(" detail   "),
            Span::styled("/ , C-s", Style::default().fg(Color::DarkGray)),
            Span::raw(" search   "),
            Span::styled("c", Style::default().fg(Color::DarkGray)),
            Span::raw(" switch calendar   "),
            Span::styled("q/esc", Style::default().fg(Color::DarkGray)),
            Span::raw(" quit"),
        ]),
        areas[2],
    );
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

fn draw_event_detail(f: &mut ratatui::Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    let acc = current_account_name(app);
    let cal = app.selected_calendar.as_deref().unwrap_or("");
    let title = Line::from(vec![
        Span::styled(
            format!("{cal} [{acc}]  —  event detail"),
            Style::default().bold(),
        ),
    ]);
    f.render_widget(Paragraph::new(title), areas[0]);

    // Draw the bordered box, then its inner content area.
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Details ");
    let inner = block.inner(areas[1]);
    f.render_widget(block, areas[1]);

    if app.detail_lines.is_empty() {
        f.render_widget(
            Paragraph::new("No event selected.")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
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
        .map(|l| Line::from(l.clone()))
        .collect();

    f.render_widget(Paragraph::new(visible), inner);

    f.render_widget(
        footer_help(vec![
            Span::styled("j/k, ↑/↓", Style::default().fg(Color::DarkGray)),
            Span::raw(" scroll   "),
            Span::styled("n/p", Style::default().fg(Color::DarkGray)),
            Span::raw(" next/prev event   "),
            Span::styled("esc/enter/c", Style::default().fg(Color::DarkGray)),
            Span::raw(" back"),
        ]),
        areas[2],
    );
}

/// Discover gcalcli accounts.
///
/// The first entry is always the default account (gcalcli's normal data dir,
/// i.e. no `XDG_DATA_HOME` override). Named accounts are subdirectories of
/// `~/.config/gcalcli/accounts/*` that contain a `gcalcli/oauth` token file.
fn list_accounts() -> Result<Vec<Account>, Box<dyn std::error::Error>> {
    let mut accounts = vec![Account {
        name: "Default".to_string(),
        config_dir: None,
    }];

    let dir = home_dir()?.join(ACCOUNTS_DIR);
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut names: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                // gcalcli stores its oauth token in the data dir, which is
                // redirected via XDG_DATA_HOME to this account folder.
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

fn home_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").map(PathBuf::from)?;
    Ok(home)
}

fn run_gcalcli(args: &[&str], account_dir: Option<&str>) -> Result<std::process::Output, io::Error> {
    let mut cmd = Command::new("gcalcli");
    cmd.args(args);
    if let Some(dir) = account_dir {
        // gcalcli's oauth token lives in its data dir, which platformdirs
        // resolves from XDG_DATA_HOME (honored even on macOS). Point it at the
        // account folder so each account keeps its own credentials.
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
        // Format: "<access>  <title>" separated by multiple spaces
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
    // Without an explicit end date gcalcli's agenda only covers a small window
    // (roughly to the end of the current month), so request a wide range to
    // capture all future events.
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
    events.sort_by(|a, b| a.date.cmp(&b.date).then(a.time.cmp(&b.time)));

    app.events = events;
    Ok(())
}
