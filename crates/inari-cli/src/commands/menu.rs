use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io;

const ITEMS: &[(&str, &str)] = &[
    ("Start runtime",   "Start all available services"),
    ("Stop runtime",    "Stop all running services"),
    ("Restart runtime", "Restart all services"),
    ("Status",          "Show service status"),
    ("Open web panel",  "Start API server + open browser"),
    ("Quit",            "Exit Inari"),
];

pub async fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let choice = tui_loop(&mut terminal)?;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    match choice {
        Some(0) => super::start::run().await,
        Some(1) => super::stop::run().await,
        Some(2) => super::restart::run().await,
        Some(3) => super::status::run().await,
        Some(4) => super::panel::run().await,
        _       => Ok(()),
    }
}

fn tui_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<Option<usize>> {
    let mut selected = 0usize;
    let n = ITEMS.len();

    loop {
        let sel = selected;
        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Length(2),
                ])
                .split(area);

            let title = Paragraph::new("Inari v0.1.0  ·  SushiBox")
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(title, chunks[0]);

            let items: Vec<ListItem> = ITEMS
                .iter()
                .map(|(label, _)| ListItem::new(*label))
                .collect();
            let mut state = ListState::default();
            state.select(Some(sel));
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" Menu "))
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("> ");
            f.render_stateful_widget(list, chunks[1], &mut state);

            let hint = ITEMS[sel].1;
            let help = Paragraph::new(
                format!("{hint}  |  \u{2191}\u{2193} navigate   Enter select   q quit"),
            )
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
            f.render_widget(help, chunks[2]);
        })?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = if sel == 0 { n - 1 } else { sel - 1 };
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (sel + 1) % n;
                }
                KeyCode::Enter => return Ok(Some(sel)),
                KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                _ => {}
            }
        }
    }
}
