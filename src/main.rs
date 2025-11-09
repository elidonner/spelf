use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::io::{self, Write};

use tui::{
    backend::CrosstermBackend,
    Terminal,
    widgets::{Block, Borders, List, ListItem, Paragraph},
    layout::{Layout, Constraint, Direction},
    style::{Modifier, Style},
    text::{Span, Spans},
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    terminal::{enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use strsim::levenshtein;
use signal_hook::consts::SIGINT;
use signal_hook::flag;

fn load_dictionary() -> Vec<String> {
    fs::read_to_string("/usr/share/dict/words")
        .map(|content| content.lines().map(String::from).collect())
        .unwrap_or_else(|_| {
            eprintln!("Error: Could not find or read the dictionary file at /usr/share/dict/words.");
            std::process::exit(1);
        })
}

fn setup_signal_handling() -> Arc<AtomicBool> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    flag::register(SIGINT, r).expect("Error setting up signal handler");
    running
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>, Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn cleanup_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<(), Box<dyn std::error::Error>> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn draw_ui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    query: &str,
    filtered_matches: &[String],
    selected_index: usize,
    list_state: &mut tui::widgets::ListState,
) -> Result<(), Box<dyn std::error::Error>> {
    terminal.draw(|f| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)].as_ref())
            .split(f.size());

        // Query block
        let query_block = Paragraph::new(Spans::from(vec![
            Span::raw("Query: "),
            Span::styled(query, Style::default().add_modifier(Modifier::BOLD)),
        ]))
        .block(Block::default().borders(Borders::ALL));
        f.render_widget(query_block, chunks[0]);

        // Matches block
        let items: Vec<ListItem> = filtered_matches
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let content = if i == selected_index {
                    Spans::from(vec![
                        Span::styled(">", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(format!(" {}", item), Style::default().add_modifier(Modifier::BOLD)),
                    ])
                } else {
                    Spans::from(vec![
                        Span::raw("  "), // Add padding for alignment
                        Span::raw(item),
                    ])
                };
                ListItem::new(content)
            })
            .collect();

        let matches_block = List::new(items).block(Block::default().borders(Borders::ALL).title("Matches"));
        list_state.select(Some(selected_index));
        f.render_stateful_widget(matches_block, chunks[1], list_state);
    })?;
    Ok(())
}

fn handle_input(
    query: &mut String,
    selected_index: &mut usize,
    filtered_matches: &[String],
    running: &Arc<AtomicBool>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if event::poll(std::time::Duration::from_millis(100))? {
        match event::read()? {
            Event::Key(KeyEvent { code, modifiers, .. }) => match code {
                KeyCode::Esc => {
                    running.store(false, Ordering::Relaxed);
                    return Ok(None);
                }
                KeyCode::Char('c') | KeyCode::Char('d') | KeyCode::Char('z')
                    if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                    running.store(false, Ordering::Relaxed);
                    return Ok(None);
                }
                KeyCode::Up => {
                    if *selected_index > 0 {
                        *selected_index -= 1;
                    }
                }
                KeyCode::Down => {
                    if *selected_index < filtered_matches.len().saturating_sub(1) {
                        *selected_index += 1;
                    }
                }
                KeyCode::Char('p') if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                    if *selected_index > 0 {
                        *selected_index -= 1;
                    }
                }
                KeyCode::Char('n') if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                    if *selected_index < filtered_matches.len().saturating_sub(1) {
                        *selected_index += 1;
                    }
                }
                KeyCode::Enter => {
                    if !filtered_matches.is_empty() {
                        return Ok(Some(filtered_matches[*selected_index].clone()));
                    }
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    *selected_index = (*selected_index).min(filtered_matches.len().saturating_sub(1));
                }
                KeyCode::Backspace => {
                    query.pop();
                    *selected_index = (*selected_index).min(filtered_matches.len().saturating_sub(1));
                }
                _ => {}
            },
            Event::Resize(_, _) => {} // ignore resize
            _ => {} // handle other events
        }
    }
    Ok(None)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dict = load_dictionary();
    let running = setup_signal_handling();
    let mut terminal = setup_terminal()?;

    let mut query = String::new();
    let mut selected_index = 0;
    let mut list_state = tui::widgets::ListState::default();

    loop {
        if !running.load(Ordering::Relaxed) {
            break;
        }

        let mut matches = dict.iter()
            .map(|w| (w, levenshtein(&query, w)))
            .collect::<Vec<_>>();
        matches.sort_by_key(|(_, d)| *d);

        let filtered_matches: Vec<String> = matches.iter()
            .map(|(w, _)| (*w).clone())
            .collect();

        draw_ui(&mut terminal, &query, &filtered_matches, selected_index, &mut list_state)?;
        if let Some(selected_word) = handle_input(&mut query, &mut selected_index, &filtered_matches, &running)? {
            cleanup_terminal(&mut terminal)?;
            println!("{}", selected_word);
            io::stdout().flush().unwrap();
            return Ok(());
        }
    }

    cleanup_terminal(&mut terminal)?;
    Ok(())
}