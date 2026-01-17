use std::io::{self, stdout, Read};
use std::sync::{Arc, Mutex};
use std::thread;

use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// Shared state for PTY output buffer
struct PtyState {
    output_buffer: String,
    child_exited: bool,
}

impl PtyState {
    fn new() -> Self {
        Self {
            output_buffer: String::new(),
            child_exited: false,
        }
    }
}

/// Application state
struct App {
    pty_state: Arc<Mutex<PtyState>>,
    master_pty: Option<Box<dyn portable_pty::MasterPty + Send>>,
}

impl App {
    fn new() -> Self {
        Self {
            pty_state: Arc::new(Mutex::new(PtyState::new())),
            master_pty: None,
        }
    }

    /// Resize the PTY to match the given dimensions
    fn resize_pty(&self, cols: u16, rows: u16) {
        if let Some(ref master) = self.master_pty {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }
}

fn main() -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Get initial terminal size for PTY
    let initial_size = terminal.size()?;
    // Calculate right panel size (70% of width, minus borders)
    let pty_cols = (initial_size.width as f32 * 0.70) as u16 - 2;
    let pty_rows = initial_size.height - 3; // Account for bottom bar and borders

    // Create PTY
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: pty_rows,
            cols: pty_cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    // Spawn bash as proof of concept
    let cmd = CommandBuilder::new("bash");
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    // Drop slave after spawning (important for proper cleanup)
    drop(pair.slave);

    // Create app state
    let mut app = App::new();
    app.master_pty = Some(pair.master);

    // Clone reader for background thread
    let mut reader = app
        .master_pty
        .as_ref()
        .unwrap()
        .try_clone_reader()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    // Spawn thread to read PTY output
    let pty_state = Arc::clone(&app.pty_state);
    let reader_thread = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    // EOF - child process has exited
                    let mut state = pty_state.lock().unwrap();
                    state.child_exited = true;
                    break;
                }
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]);
                    let mut state = pty_state.lock().unwrap();
                    state.output_buffer.push_str(&text);
                    // Limit buffer size to prevent memory issues
                    if state.output_buffer.len() > 100_000 {
                        let drain_to = state.output_buffer.len() - 50_000;
                        state.output_buffer.drain(..drain_to);
                    }
                }
                Err(_) => {
                    let mut state = pty_state.lock().unwrap();
                    state.child_exited = true;
                    break;
                }
            }
        }
    });

    // Track last known size for resize detection
    let mut last_cols = pty_cols;
    let mut last_rows = pty_rows;

    // Run the app
    let result = run(&mut terminal, &mut app, &mut last_cols, &mut last_rows);

    // Wait for child process to exit
    let _ = child.wait();

    // Drop master PTY to signal EOF to reader thread
    drop(app.master_pty.take());

    // Wait for reader thread to finish
    let _ = reader_thread.join();

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    last_cols: &mut u16,
    last_rows: &mut u16,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| {
            let area = frame.area();

            // Check for terminal resize
            let new_pty_cols = (area.width as f32 * 0.70) as u16 - 2;
            let new_pty_rows = area.height.saturating_sub(3);

            if new_pty_cols != *last_cols || new_pty_rows != *last_rows {
                *last_cols = new_pty_cols;
                *last_rows = new_pty_rows;
                app.resize_pty(new_pty_cols, new_pty_rows);
            }

            // Create main layout: content area + bottom bar
            let main_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),    // Main content area
                    Constraint::Length(1), // Bottom bar (single line)
                ])
                .split(area);

            let content_area = main_layout[0];
            let bottom_bar_area = main_layout[1];

            // Create horizontal split: 30% left panel, 70% right panel
            let panels = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(30), // Ralph Status panel
                    Constraint::Percentage(70), // Claude Code panel
                ])
                .split(content_area);

            let left_panel_area = panels[0];
            let right_panel_area = panels[1];

            // Left panel: Ralph Status
            let left_block = Block::default()
                .title(" Ralph Status ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            // Get PTY state for display
            let state = app.pty_state.lock().unwrap();
            let status_text = if state.child_exited {
                "PTY: Child process exited"
            } else {
                "PTY: Running bash (proof of concept)"
            };

            let left_content = Paragraph::new(status_text)
                .block(left_block)
                .style(Style::default().fg(Color::White));

            frame.render_widget(left_content, left_panel_area);

            // Right panel: Claude Code (PTY output)
            let right_block = Block::default()
                .title(" Claude Code ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            // Display PTY output (last portion that fits)
            let output = &state.output_buffer;
            let right_content = Paragraph::new(output.as_str())
                .block(right_block)
                .style(Style::default().fg(Color::White))
                .wrap(ratatui::widgets::Wrap { trim: false });

            frame.render_widget(right_content, right_panel_area);

            // Bottom bar with keybinding hints
            let keybindings = Paragraph::new(" q: Quit | i/Tab: Claude Mode | Esc: Ralph Mode ")
                .style(Style::default().fg(Color::Black).bg(Color::Cyan));

            frame.render_widget(keybindings, bottom_bar_area);
        })?;

        // Check if child exited
        {
            let state = app.pty_state.lock().unwrap();
            if state.child_exited {
                // Wait a moment before exiting so user can see final output
                drop(state);
                std::thread::sleep(std::time::Duration::from_millis(500));
                break;
            }
        }

        // Handle input
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    Ok(())
}
