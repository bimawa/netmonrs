use std::{
    collections::HashSet,
    io::{self, Stdout},
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
    net::IpAddr,
};

use chrono::Local;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState},
};


#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    target: String,
}


#[derive(PartialEq)]
enum Focus {
    ActiveList,
    HistoryList,
}


enum BackgroundEvent {
    DataUpdate {
        active: Vec<String>,
        new_history_entries: Vec<String>,
        pid_msg: String,
    },
    Error(String),
}


struct App {
    target_name: String,

    active_connections: Vec<String>,
    history_log: Vec<String>,
    seen_ips: HashSet<String>,
    last_status_msg: String,

    focus: Focus,
    active_state: ListState,
    history_state: ListState,
}

impl App {
    fn new(target: String) -> Self {
        Self {
            target_name: target,
            active_connections: Vec::new(),
            history_log: Vec::new(),
            seen_ips: HashSet::new(),
            last_status_msg: String::from("Initializing..."),

            focus: Focus::ActiveList,
            active_state: ListState::default(),
            history_state: ListState::default(),
        }
    }

    fn next(&mut self) {
        let (state, len) = match self.focus {
            Focus::ActiveList => (&mut self.active_state, self.active_connections.len()),
            Focus::HistoryList => (&mut self.history_state, self.history_log.len()),
        };
        if len == 0 { return; }

        let i = match state.selected() {
            Some(i) => if i >= len - 1 { 0 } else { i + 1 },
            None => 0,
        };
        state.select(Some(i));
    }

    fn previous(&mut self) {
        let (state, len) = match self.focus {
            Focus::ActiveList => (&mut self.active_state, self.active_connections.len()),
            Focus::HistoryList => (&mut self.history_state, self.history_log.len()),
        };
        if len == 0 { return; }

        let i = match state.selected() {
            Some(i) => if i == 0 { len - 1 } else { i - 1 },
            None => 0,
        };
        state.select(Some(i));
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::ActiveList => Focus::HistoryList,
            Focus::HistoryList => Focus::ActiveList,
        };
    }

    fn update_seen_ips(&mut self) {
        let mut seen_ips = HashSet::new();
        let history_to_check = if self.history_log.len() > 1000 {
            &self.history_log[self.history_log.len() - 1000..]
        } else {
            &self.history_log
        };

        for h in history_to_check {
             if let Some(ip) = h.split_whitespace().last() {
                 if !ip.is_empty() {
                     seen_ips.insert(ip.to_string());
                 }
             }
        }
        self.seen_ips = seen_ips;
    }
}


fn main() -> io::Result<()> {
    let args = Args::parse();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let res = run_app(&mut stdout, args.target);

    disable_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen)?;

    if let Err(err) = res {
        println!("App Error: {:?}", err);
    }

    Ok(())
}

fn run_app(terminal: &mut Stdout, target: String) -> io::Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(terminal))?;
    let mut app = App::new(target.clone());

    let (tx, rx) = mpsc::channel::<BackgroundEvent>();

    thread::spawn(move || {
        let mut seen_ips_thread_copy = HashSet::new();
        let lsof_line_pattern = "->";

        loop {
            let start_time = Instant::now();

            let pgrep = Command::new("pgrep").arg("-f").arg(&target).output();

            match pgrep {
                Ok(out) => {
                    let pid_str = String::from_utf8_lossy(&out.stdout);
                    if let Some(pid_line) = pid_str.lines().next() {
                        let pid = pid_line.trim();

                        let lsof = Command::new("sudo")
                            .arg("lsof").arg("-i").arg("-P").arg("-n").arg("-p").arg(pid)
                            .output();

                        match lsof {
                            Ok(lsof_out) => {
                                let output_str = String::from_utf8_lossy(&lsof_out.stdout);
                                let mut active = HashSet::new();
                                let mut new_entries = Vec::new();

                                for line in output_str.lines().skip(1) {
                                    if let Some(pos) = line.find(lsof_line_pattern) {
                                        let ip_start = pos + 2;
                                        if ip_start < line.len() {
                                            let ip_part = &line[ip_start..];
                                            let ip_end = ip_part.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(ip_part.len());
                                            let ip = &ip_part[..ip_end];

                                            if !ip.is_empty() && (ip.contains('.') || ip.contains(':')) {
                                                let final_ip = if ip.starts_with('[') && ip.ends_with(']') {
                                                    &ip[1..ip.len()-1]
                                                } else {
                                                    ip
                                                };

                                                let s = final_ip.to_string();
                                                active.insert(s.clone());

                                                if !seen_ips_thread_copy.contains(&s) {
                                                    seen_ips_thread_copy.insert(s.clone());
                                                    let ts = Local::now().format("%H:%M:%S");
                                                    new_entries.push(format!("[{}] {}", ts, s));
                                                }
                                            }
                                        }
                                    }
                                }


                                let mut sorted_connections: Vec<String> = active.iter().cloned().collect();
                                sorted_connections.sort_by(|a, b| {
                                    match (a.parse::<IpAddr>(), b.parse::<IpAddr>()) {
                                        (Ok(ip_a), Ok(ip_b)) => ip_a.cmp(&ip_b),
                                        _ => a.cmp(b),
                                    }
                                });

                                let _ = tx.send(BackgroundEvent::DataUpdate {
                                    active: sorted_connections,
                                    new_history_entries: new_entries,
                                    pid_msg: format!("Monitoring PID: {}", pid),
                                });
                            }
                            Err(e) => { let _ = tx.send(BackgroundEvent::Error(format!("LSOF Error: {}", e))); }
                        }
                    } else {
                        let _ = tx.send(BackgroundEvent::Error(format!("Waiting for process '{}'...", target)));
                    }
                }
                Err(e) => { let _ = tx.send(BackgroundEvent::Error(format!("PGREP Error: {}", e))); }
            }

            let elapsed = start_time.elapsed();
            if elapsed < Duration::from_secs(1) {
                thread::sleep(Duration::from_secs(1) - elapsed);
            }
        }
    });

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Tab | KeyCode::Left | KeyCode::Right => app.toggle_focus(),
                        KeyCode::Down | KeyCode::Char('j') => app.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.previous(),
                        KeyCode::PageDown => { for _ in 0..10 { app.next(); } },
                        KeyCode::PageUp => { for _ in 0..10 { app.previous(); } },
                        _ => {}
                    }
                }
            }
        }

        while let Ok(msg) = rx.try_recv() {
            match msg {
                BackgroundEvent::DataUpdate { active, new_history_entries, pid_msg } => {
                    app.active_connections = active;
                    app.last_status_msg = pid_msg;

                    for entry in new_history_entries {
                        app.history_log.push(entry);
                    }
                    app.update_seen_ips();
                }
                BackgroundEvent::Error(msg) => {
                    app.last_status_msg = msg;
                    app.active_connections.clear();
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.size());

    let list_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[0]);

    let active_style = if app.focus == Focus::ActiveList {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let active_items: Vec<ListItem> = app.active_connections.iter()
        .map(|i| ListItem::new(format!("🚀 {}", i)))
        .collect();

    let list_active = List::new(active_items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(format!(" Active Connections [{}] ", app.target_name))
            .border_style(active_style))
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");

    f.render_stateful_widget(list_active, list_chunks[0], &mut app.active_state);


    let history_style = if app.focus == Focus::HistoryList {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let history_items: Vec<ListItem> = app.history_log.iter().rev()
        .map(|i| ListItem::new(i.clone()))
        .collect();

    let list_history = List::new(history_items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(" Connection History ")
            .border_style(history_style))
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");

    f.render_stateful_widget(list_history, list_chunks[1], &mut app.history_state);


    let status_style = if app.last_status_msg.contains("Error") || app.last_status_msg.contains("Wait") {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Green)
    };

    let status_bar = ratatui::widgets::Paragraph::new(app.last_status_msg.as_str())
        .style(status_style);

    f.render_widget(status_bar, main_chunks[1]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_seen_ips_empty_history() {
        let mut app = App::new(String::from("test"));
        app.update_seen_ips();
        assert!(app.seen_ips.is_empty());
    }

    #[test]
    fn test_update_seen_ips_single_entry() {
        let mut app = App::new(String::from("test"));
        app.history_log.push("[12:00:00] 192.168.1.1".to_string());
        app.update_seen_ips();
        assert!(app.seen_ips.contains("192.168.1.1"));
        assert_eq!(app.seen_ips.len(), 1);
    }

    #[test]
    fn test_update_seen_ips_multiple_entries() {
        let mut app = App::new(String::from("test"));
        app.history_log.push("[12:00:00] 192.168.1.1".to_string());
        app.history_log.push("[12:00:01] 10.0.0.1".to_string());
        app.history_log.push("[12:00:02] 172.16.0.1".to_string());
        app.update_seen_ips();
        assert!(app.seen_ips.contains("192.168.1.1"));
        assert!(app.seen_ips.contains("10.0.0.1"));
        assert!(app.seen_ips.contains("172.16.0.1"));
        assert_eq!(app.seen_ips.len(), 3);
    }

    #[test]
    fn test_update_seen_ips_duplicate_ips() {
        let mut app = App::new(String::from("test"));
        app.history_log.push("[12:00:00] 192.168.1.1".to_string());
        app.history_log.push("[12:00:01] 192.168.1.1".to_string());
        app.history_log.push("[12:00:02] 10.0.0.1".to_string());
        app.update_seen_ips();
        assert!(app.seen_ips.contains("192.168.1.1"));
        assert!(app.seen_ips.contains("10.0.0.1"));
        assert_eq!(app.seen_ips.len(), 2);
    }

    #[test]
    fn test_update_seen_ips_ipv6() {
        let mut app = App::new(String::from("test"));
        app.history_log.push("[12:00:00] 2001:db8::1".to_string());
        app.history_log.push("[12:00:01] [::1]".to_string());
        app.update_seen_ips();
        assert!(app.seen_ips.contains("2001:db8::1"));
        assert!(app.seen_ips.contains("[::1]"));
        assert_eq!(app.seen_ips.len(), 2);
    }

    #[test]
    fn test_update_seen_ips_limited_history() {
        let mut app = App::new(String::from("test"));
        for i in 0..1001 {
            app.history_log.push(format!("[12:00:{:02}] 192.168.1.{}", i, i));
        }
        app.update_seen_ips();
        assert_eq!(app.seen_ips.len(), 1000);
        assert!(!app.seen_ips.contains("192.168.1.0"));
        assert!(app.seen_ips.contains("192.168.1.1000"));
    }



    #[test]
    fn test_update_seen_ips_complex_format() {
        let mut app = App::new(String::from("test"));
        app.history_log.push("[12:00:00] 192.168.1.1:80".to_string());
        app.history_log.push("[12:00:01] 10.0.0.1:443".to_string());
        app.update_seen_ips();
        assert!(app.seen_ips.contains("192.168.1.1:80"));
        assert!(app.seen_ips.contains("10.0.0.1:443"));
        assert_eq!(app.seen_ips.len(), 2);
    }
}
