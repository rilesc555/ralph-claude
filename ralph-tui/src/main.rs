use std::io::{self, stdout, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use serde::Deserialize;

/// PRD user story
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserStory {
    id: String,
    title: String,
    #[allow(dead_code)]
    description: String,
    #[allow(dead_code)]
    acceptance_criteria: Vec<String>,
    priority: u32,
    passes: bool,
    #[allow(dead_code)]
    notes: String,
}

/// PRD document structure
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Prd {
    #[allow(dead_code)]
    project: String,
    #[allow(dead_code)]
    task_dir: String,
    branch_name: String,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    prd_type: String,
    description: String,
    user_stories: Vec<UserStory>,
}

impl Prd {
    /// Load PRD from a JSON file
    fn load(path: &PathBuf) -> io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Count completed stories
    fn completed_count(&self) -> usize {
        self.user_stories.iter().filter(|s| s.passes).count()
    }

    /// Get current story (first with passes: false, sorted by priority)
    fn current_story(&self) -> Option<&UserStory> {
        self.user_stories
            .iter()
            .filter(|s| !s.passes)
            .min_by_key(|s| s.priority)
    }
}

/// Mode for modal input system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Ralph,  // Default mode - focus on left panel
    Claude, // Claude mode - focus on right panel, forward input to PTY
}

/// Shared state for PTY with VT100 parser
struct PtyState {
    parser: vt100::Parser,
    child_exited: bool,
    /// Recent raw output for detecting completion signal
    recent_output: String,
}

impl PtyState {
    fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, 1000), // 1000 lines of scrollback
            child_exited: false,
            recent_output: String::new(),
        }
    }

    /// Append output and trim to last 10KB to prevent memory issues
    fn append_output(&mut self, data: &[u8]) {
        if let Ok(s) = std::str::from_utf8(data) {
            self.recent_output.push_str(s);
            // Keep only last 10KB to limit memory
            if self.recent_output.len() > 10 * 1024 {
                let start = self.recent_output.len() - 8 * 1024;
                self.recent_output = self.recent_output[start..].to_string();
            }
        }
    }

    /// Check if completion signal is present in recent output
    fn has_completion_signal(&self) -> bool {
        self.recent_output.contains("<promise>COMPLETE</promise>")
    }

    /// Clear recent output (called when starting new iteration)
    fn clear_recent_output(&mut self) {
        self.recent_output.clear();
    }
}

/// Iteration state for tracking progress across Claude restarts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IterationState {
    Running,       // Claude is currently running
    Completed,     // All stories complete (<promise>COMPLETE</promise> found)
    NeedsRestart,  // Iteration finished but more work remains
    WaitingDelay,  // Waiting before starting next iteration
}

/// Application state
struct App {
    pty_state: Arc<Mutex<PtyState>>,
    master_pty: Option<Box<dyn portable_pty::MasterPty + Send>>,
    pty_writer: Option<Box<dyn Write + Send>>,
    mode: Mode,
    task_dir: PathBuf,
    prd_path: PathBuf,
    prd: Option<Prd>,
    prd_needs_reload: Arc<Mutex<bool>>,
    // Iteration loop state
    current_iteration: u32,
    max_iterations: u32,
    iteration_state: IterationState,
    delay_start: Option<std::time::Instant>,
}

impl App {
    fn new(rows: u16, cols: u16, task_dir: PathBuf, max_iterations: u32) -> Self {
        let prd_path = task_dir.join("prd.json");
        let prd = Prd::load(&prd_path).ok();

        Self {
            pty_state: Arc::new(Mutex::new(PtyState::new(rows, cols))),
            master_pty: None,
            pty_writer: None,
            mode: Mode::Ralph, // Default to Ralph mode
            task_dir,
            prd_path,
            prd,
            prd_needs_reload: Arc::new(Mutex::new(false)),
            current_iteration: 1,
            max_iterations,
            iteration_state: IterationState::Running,
            delay_start: None,
        }
    }

    /// Reload PRD from disk if flagged
    fn reload_prd_if_needed(&mut self) {
        let needs_reload = {
            let mut flag = self.prd_needs_reload.lock().unwrap();
            if *flag {
                *flag = false;
                true
            } else {
                false
            }
        };

        if needs_reload {
            if let Ok(prd) = Prd::load(&self.prd_path) {
                self.prd = Some(prd);
            }
        }
    }

    /// Write bytes to the PTY stdin
    fn write_to_pty(&mut self, data: &[u8]) {
        if let Some(ref mut writer) = self.pty_writer {
            let _ = writer.write_all(data);
            let _ = writer.flush();
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
        // Also resize the VT100 parser's screen
        let mut state = self.pty_state.lock().unwrap();
        state.parser.screen_mut().set_size(rows, cols);
    }
}

/// Simple text wrapping helper
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.is_empty() {
            current_line = word.to_string();
        } else if current_line.len() + 1 + word.len() <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// Build the Ralph prompt from task directory and prompt.md
/// Returns the full prompt string to be piped to Claude Code stdin
fn build_ralph_prompt(task_dir: &PathBuf) -> io::Result<String> {
    // Find the project root (where prompt.md lives)
    // Walk up from task_dir until we find prompt.md or hit filesystem root
    let mut prompt_path = task_dir.clone();
    loop {
        let candidate = prompt_path.join("prompt.md");
        if candidate.exists() {
            break;
        }
        if !prompt_path.pop() {
            // Reached filesystem root without finding prompt.md
            // Try relative to current directory
            let cwd_prompt = PathBuf::from("prompt.md");
            if cwd_prompt.exists() {
                prompt_path = PathBuf::from(".");
                break;
            }
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Could not find prompt.md in task directory ancestors or current directory",
            ));
        }
    }

    let prompt_file = prompt_path.join("prompt.md");
    let prompt_content = std::fs::read_to_string(&prompt_file)?;

    // Build the full prompt matching ralph.sh format
    let prompt = format!(
        "# Ralph Agent Instructions\n\n\
         Task Directory: {task_dir}\n\
         PRD File: {task_dir}/prd.json\n\
         Progress File: {task_dir}/progress.txt\n\n\
         {prompt_content}",
        task_dir = task_dir.display(),
        prompt_content = prompt_content,
    );

    Ok(prompt)
}

/// Convert vt100::Color to ratatui::Color
fn vt100_to_ratatui_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(idx) => Color::Indexed(idx),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Render the VT100 screen to a Vec of ratatui Lines (styled text)
/// This function renders the visible content of the terminal emulator
fn render_vt100_screen(screen: &vt100::Screen) -> Vec<Line<'static>> {
    let (rows, cols) = screen.size();
    let mut lines = Vec::with_capacity(rows as usize);

    // Render each visible row
    for row in 0..rows {
        let mut spans = Vec::new();
        let mut col = 0u16;

        while col < cols {
            if let Some(cell) = screen.cell(row, col) {
                let contents = cell.contents();

                // Skip wide character continuations
                if cell.is_wide_continuation() {
                    col += 1;
                    continue;
                }

                let display_str = if contents.is_empty() {
                    " ".to_string()
                } else {
                    contents.to_string()
                };

                let mut style = Style::default();
                style = style.fg(vt100_to_ratatui_color(cell.fgcolor()));
                style = style.bg(vt100_to_ratatui_color(cell.bgcolor()));

                if cell.bold() {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.italic() {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.underline() {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell.inverse() {
                    style = style.add_modifier(Modifier::REVERSED);
                }

                spans.push(Span::styled(display_str, style));

                // Wide characters take 2 columns
                if cell.is_wide() {
                    col += 2;
                } else {
                    col += 1;
                }
            } else {
                spans.push(Span::raw(" "));
                col += 1;
            }
        }
        lines.push(Line::from(spans));
    }

    lines
}

/// Forward a key event to the PTY
/// Converts crossterm key events to the appropriate byte sequences for the terminal
fn forward_key_to_pty(app: &mut App, key_code: KeyCode, modifiers: KeyModifiers) {
    let bytes: Vec<u8> = match key_code {
        // Printable characters
        KeyCode::Char(c) => {
            if modifiers.contains(KeyModifiers::CONTROL) {
                // Handle Ctrl+key combinations
                // Ctrl+A = 0x01, Ctrl+B = 0x02, ..., Ctrl+Z = 0x1A
                // Ctrl+C = 0x03 (interrupt)
                if c.is_ascii_alphabetic() {
                    let ctrl_char = (c.to_ascii_lowercase() as u8) - b'a' + 1;
                    vec![ctrl_char]
                } else if c == '[' {
                    vec![0x1b] // Escape
                } else if c == '\\' {
                    vec![0x1c] // File separator (Ctrl+\)
                } else if c == ']' {
                    vec![0x1d] // Group separator (Ctrl+])
                } else if c == '^' {
                    vec![0x1e] // Record separator (Ctrl+^)
                } else if c == '_' {
                    vec![0x1f] // Unit separator (Ctrl+_)
                } else {
                    // Just send the character for other Ctrl combinations
                    c.to_string().into_bytes()
                }
            } else if modifiers.contains(KeyModifiers::ALT) {
                // Alt+key sends ESC followed by the character
                let mut bytes = vec![0x1b]; // ESC
                bytes.extend(c.to_string().into_bytes());
                bytes
            } else {
                // Regular character
                c.to_string().into_bytes()
            }
        }

        // Special keys
        KeyCode::Enter => vec![b'\r'],     // Carriage return
        KeyCode::Backspace => vec![0x7f],  // DEL character (most terminals)
        KeyCode::Delete => vec![0x1b, b'[', b'3', b'~'], // ANSI escape sequence
        KeyCode::Tab => vec![b'\t'],       // Tab character

        // Arrow keys (ANSI escape sequences)
        KeyCode::Up => vec![0x1b, b'[', b'A'],
        KeyCode::Down => vec![0x1b, b'[', b'B'],
        KeyCode::Right => vec![0x1b, b'[', b'C'],
        KeyCode::Left => vec![0x1b, b'[', b'D'],

        // Home/End keys
        KeyCode::Home => vec![0x1b, b'[', b'H'],
        KeyCode::End => vec![0x1b, b'[', b'F'],

        // Page Up/Down
        KeyCode::PageUp => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1b, b'[', b'6', b'~'],

        // Insert key
        KeyCode::Insert => vec![0x1b, b'[', b'2', b'~'],

        // Function keys
        KeyCode::F(1) => vec![0x1b, b'O', b'P'],
        KeyCode::F(2) => vec![0x1b, b'O', b'Q'],
        KeyCode::F(3) => vec![0x1b, b'O', b'R'],
        KeyCode::F(4) => vec![0x1b, b'O', b'S'],
        KeyCode::F(5) => vec![0x1b, b'[', b'1', b'5', b'~'],
        KeyCode::F(6) => vec![0x1b, b'[', b'1', b'7', b'~'],
        KeyCode::F(7) => vec![0x1b, b'[', b'1', b'8', b'~'],
        KeyCode::F(8) => vec![0x1b, b'[', b'1', b'9', b'~'],
        KeyCode::F(9) => vec![0x1b, b'[', b'2', b'0', b'~'],
        KeyCode::F(10) => vec![0x1b, b'[', b'2', b'1', b'~'],
        KeyCode::F(11) => vec![0x1b, b'[', b'2', b'3', b'~'],
        KeyCode::F(12) => vec![0x1b, b'[', b'2', b'4', b'~'],
        KeyCode::F(_) => return, // Unsupported function keys

        // Other keys we don't handle
        _ => return,
    };

    app.write_to_pty(&bytes);
}

fn print_usage() {
    eprintln!("Usage: ralph-tui <task-directory> [OPTIONS]");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  <task-directory>  Path to the task directory containing prd.json");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -i, --iterations <N>  Maximum iterations to run (default: 10)");
    eprintln!("  -h, --help            Show this help message");
    eprintln!();
    eprintln!("Example:");
    eprintln!("  ralph-tui tasks/my-feature");
    eprintln!("  ralph-tui tasks/my-feature -i 5");
}

/// Parse CLI arguments and return (task_dir, max_iterations)
fn parse_args() -> io::Result<(PathBuf, u32)> {
    let args: Vec<String> = std::env::args().collect();
    let mut task_dir: Option<PathBuf> = None;
    let mut max_iterations: u32 = 10; // Default

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-h" || arg == "--help" {
            print_usage();
            std::process::exit(0);
        } else if arg == "-i" || arg == "--iterations" {
            i += 1;
            if i >= args.len() {
                print_usage();
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Missing value for --iterations",
                ));
            }
            max_iterations = args[i].parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Invalid iterations value: {}", args[i]),
                )
            })?;
        } else if !arg.starts_with('-') {
            task_dir = Some(PathBuf::from(arg));
        } else {
            print_usage();
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Unknown argument: {}", arg),
            ));
        }
        i += 1;
    }

    let task_dir = task_dir.ok_or_else(|| {
        print_usage();
        io::Error::new(io::ErrorKind::InvalidInput, "Task directory argument required")
    })?;

    Ok((task_dir, max_iterations))
}

/// Spawn Claude Code process and return (child, reader_thread)
/// Returns None if spawning fails
fn spawn_claude(
    app: &mut App,
    pty_rows: u16,
    pty_cols: u16,
) -> io::Result<(Box<dyn portable_pty::Child + Send + Sync>, thread::JoinHandle<()>)> {
    // Build the Ralph prompt
    let ralph_prompt = build_ralph_prompt(&app.task_dir)?;

    // Write prompt to a temp file (safer than passing via command line due to length/quoting)
    let prompt_temp_file = std::env::temp_dir().join(format!(
        "ralph_prompt_{}_{}.txt",
        std::process::id(),
        app.current_iteration
    ));
    std::fs::write(&prompt_temp_file, &ralph_prompt)?;

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

    // Spawn Claude Code with the Ralph prompt piped to stdin
    let mut cmd = CommandBuilder::new("bash");
    cmd.arg("-c");
    cmd.arg(format!(
        "cat '{}' | claude --dangerously-skip-permissions --print; rm -f '{}'",
        prompt_temp_file.display(),
        prompt_temp_file.display()
    ));

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    // Drop slave after spawning (important for proper cleanup)
    drop(pair.slave);

    // Clone reader for background thread (must be done before take_writer)
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    // Get writer for sending input to PTY
    let pty_writer = pair
        .master
        .take_writer()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    // Update app state
    app.master_pty = Some(pair.master);
    app.pty_writer = Some(pty_writer);

    // Reset PTY state for new iteration
    {
        let mut state = app.pty_state.lock().unwrap();
        state.child_exited = false;
        state.clear_recent_output();
        // Re-initialize parser to clear screen
        state.parser = vt100::Parser::new(pty_rows, pty_cols, 1000);
    }

    // Spawn thread to read PTY output and feed to VT100 parser
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
                    // Feed raw bytes to VT100 parser and track for completion detection
                    let mut state = pty_state.lock().unwrap();
                    state.parser.process(&buf[..n]);
                    state.append_output(&buf[..n]);
                }
                Err(_) => {
                    let mut state = pty_state.lock().unwrap();
                    state.child_exited = true;
                    break;
                }
            }
        }
    });

    app.iteration_state = IterationState::Running;

    Ok((child, reader_thread))
}

fn main() -> io::Result<()> {
    // Parse CLI arguments
    let (task_dir, max_iterations) = parse_args()?;

    // Validate task directory exists
    if !task_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Task directory not found: {}", task_dir.display()),
        ));
    }

    // Validate prd.json exists
    let prd_path = task_dir.join("prd.json");
    if !prd_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("prd.json not found in: {}", task_dir.display()),
        ));
    }

    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Get initial terminal size for PTY
    let initial_size = terminal.size()?;
    // Calculate right panel size (70% of width, minus borders)
    let pty_cols = (initial_size.width as f32 * 0.70) as u16 - 2;
    let pty_rows = initial_size.height - 3; // Account for bottom bar and borders

    // Create app state with VT100 parser sized to PTY dimensions
    let mut app = App::new(pty_rows, pty_cols, task_dir, max_iterations);

    // Set up file watcher for prd.json
    let prd_needs_reload = Arc::clone(&app.prd_needs_reload);
    let prd_path_for_watcher = app.prd_path.clone();
    let _watcher = setup_prd_watcher(prd_path_for_watcher, prd_needs_reload);

    // Track last known size for resize detection
    let mut last_cols = pty_cols;
    let mut last_rows = pty_rows;

    // Spawn initial Claude process
    let (mut child, mut reader_thread) = spawn_claude(&mut app, pty_rows, pty_cols)?;

    // Run the main loop
    let result = loop {
        // Run the UI loop for current iteration
        let run_result = run(&mut terminal, &mut app, &mut last_cols, &mut last_rows);

        // Clean up current iteration
        let _ = child.wait();
        drop(app.master_pty.take());
        drop(app.pty_writer.take());
        let _ = reader_thread.join();

        // Check iteration state
        match app.iteration_state {
            IterationState::Completed => {
                // All done!
                break run_result;
            }
            IterationState::NeedsRestart => {
                // Check if we have more iterations
                if app.current_iteration >= app.max_iterations {
                    break run_result;
                }

                // Start delay period
                app.iteration_state = IterationState::WaitingDelay;
                app.delay_start = Some(std::time::Instant::now());

                // Wait for 2 seconds (with UI updates)
                let delay_result = run_delay(&mut terminal, &mut app, &mut last_cols, &mut last_rows);
                if let Err(e) = delay_result {
                    break Err(e);
                }

                // Check if user quit during delay
                if matches!(app.iteration_state, IterationState::Completed) {
                    break Ok(());
                }

                // Start next iteration
                app.current_iteration += 1;
                app.delay_start = None;

                // Reload PRD to get latest state
                if let Ok(prd) = Prd::load(&app.prd_path) {
                    app.prd = Some(prd);
                }

                // Spawn new Claude process
                match spawn_claude(&mut app, last_rows, last_cols) {
                    Ok((new_child, new_thread)) => {
                        child = new_child;
                        reader_thread = new_thread;
                    }
                    Err(e) => {
                        break Err(e);
                    }
                }
            }
            _ => {
                // Running or WaitingDelay - shouldn't reach here normally
                break run_result;
            }
        }
    };

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

/// Set up a file watcher for prd.json changes
fn setup_prd_watcher(
    prd_path: PathBuf,
    needs_reload: Arc<Mutex<bool>>,
) -> Option<RecommendedWatcher> {
    let config = Config::default().with_poll_interval(Duration::from_secs(1));

    let prd_path_clone = prd_path.clone();
    let watcher_result = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                // Check if the event is for our file
                if event.paths.iter().any(|p| p == &prd_path_clone) {
                    let mut flag = needs_reload.lock().unwrap();
                    *flag = true;
                }
            }
        },
        config,
    );

    match watcher_result {
        Ok(mut watcher) => {
            // Watch the parent directory since some editors replace files
            if let Some(parent) = prd_path.parent() {
                let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
            }
            Some(watcher)
        }
        Err(_) => None,
    }
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    last_cols: &mut u16,
    last_rows: &mut u16,
) -> io::Result<()> {
    loop {
        // Check if PRD needs reloading (file changed on disk)
        app.reload_prd_if_needed();

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

            // Determine border styles based on current mode
            let (left_border_style, right_border_style) = match app.mode {
                Mode::Ralph => (
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    Style::default().fg(Color::DarkGray),
                ),
                Mode::Claude => (
                    Style::default().fg(Color::DarkGray),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
            };

            // Left panel: Ralph Status
            let left_title = match app.mode {
                Mode::Ralph => " Ralph Status [ACTIVE] ",
                Mode::Claude => " Ralph Status ",
            };
            let left_block = Block::default()
                .title(left_title)
                .borders(Borders::ALL)
                .border_style(left_border_style);

            // Get PTY state for display
            let pty_state = app.pty_state.lock().unwrap();

            // Build status text with PRD information
            let mut status_lines: Vec<Line> = Vec::new();

            // Iteration info
            status_lines.push(Line::from(vec![
                Span::styled("Iteration: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{}/{}", app.current_iteration, app.max_iterations),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
            status_lines.push(Line::from(""));

            // PRD information
            if let Some(ref prd) = app.prd {
                // Description
                status_lines.push(Line::from(vec![
                    Span::styled("Task: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]));
                // Wrap description to fit panel
                for line in wrap_text(&prd.description, left_panel_area.width.saturating_sub(4) as usize) {
                    status_lines.push(Line::from(Span::raw(format!("  {}", line))));
                }
                status_lines.push(Line::from(""));

                // Branch
                status_lines.push(Line::from(vec![
                    Span::styled("Branch: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(&prd.branch_name),
                ]));
                status_lines.push(Line::from(""));

                // Progress
                let completed = prd.completed_count();
                let total = prd.user_stories.len();
                let progress_pct = if total > 0 {
                    (completed as f32 / total as f32 * 100.0) as u8
                } else {
                    0
                };
                status_lines.push(Line::from(vec![
                    Span::styled("Progress: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!("{}/{} ({}%)", completed, total, progress_pct),
                        if completed == total {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Yellow)
                        },
                    ),
                ]));
                status_lines.push(Line::from(""));

                // Current story
                if let Some(story) = prd.current_story() {
                    status_lines.push(Line::from(vec![
                        Span::styled("Current Story: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    ]));
                    status_lines.push(Line::from(vec![
                        Span::styled(format!("  {} ", story.id), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                    ]));
                    // Wrap story title
                    for line in wrap_text(&story.title, left_panel_area.width.saturating_sub(4) as usize) {
                        status_lines.push(Line::from(Span::raw(format!("  {}", line))));
                    }
                } else {
                    status_lines.push(Line::from(vec![
                        Span::styled("All stories complete!", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    ]));
                }
            } else {
                status_lines.push(Line::from(vec![
                    Span::styled("Error: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    Span::raw("Failed to load prd.json"),
                ]));
            }

            status_lines.push(Line::from(""));
            status_lines.push(Line::from("─".repeat(left_panel_area.width.saturating_sub(4) as usize)));
            status_lines.push(Line::from(""));

            // PTY status
            let pty_status = if pty_state.child_exited {
                Span::styled("PTY: Exited", Style::default().fg(Color::Red))
            } else {
                Span::styled("PTY: Running", Style::default().fg(Color::Green))
            };
            status_lines.push(Line::from(pty_status));

            let left_content = Paragraph::new(status_lines)
                .block(left_block)
                .style(Style::default().fg(Color::White));

            frame.render_widget(left_content, left_panel_area);

            // Right panel: Claude Code (PTY output with VT100 rendering)
            let right_title = match app.mode {
                Mode::Claude => " Claude Code [ACTIVE] ",
                Mode::Ralph => " Claude Code ",
            };
            let right_block = Block::default()
                .title(right_title)
                .borders(Borders::ALL)
                .border_style(right_border_style);

            // Render VT100 screen content with proper ANSI colors
            // The screen already shows the most recent content (auto-scroll behavior
            // is handled by the terminal emulator when new content is written)
            let screen = pty_state.parser.screen();
            let lines = render_vt100_screen(screen);

            let right_content = Paragraph::new(lines).block(right_block);

            frame.render_widget(right_content, right_panel_area);

            // Bottom bar with keybinding hints (mode-specific)
            let keybindings_text = match app.mode {
                Mode::Ralph => " q: Quit | i/Tab: Enter Claude Mode ",
                Mode::Claude => " Press Esc to return to Ralph ",
            };
            let keybindings = Paragraph::new(keybindings_text)
                .style(Style::default().fg(Color::Black).bg(Color::Cyan));

            frame.render_widget(keybindings, bottom_bar_area);
        })?;

        // Check if child exited
        {
            let state = app.pty_state.lock().unwrap();
            if state.child_exited {
                // Check for completion signal
                let is_complete = state.has_completion_signal();
                drop(state);

                // Wait a moment before proceeding so user can see final output
                std::thread::sleep(std::time::Duration::from_millis(500));

                // Set iteration state based on output
                if is_complete {
                    app.iteration_state = IterationState::Completed;
                } else {
                    app.iteration_state = IterationState::NeedsRestart;
                }
                break;
            }
        }

        // Handle input based on current mode
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match app.mode {
                    Mode::Ralph => {
                        // In Ralph mode: handle TUI controls
                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('i') | KeyCode::Tab => {
                                app.mode = Mode::Claude;
                            }
                            _ => {}
                        }
                    }
                    Mode::Claude => {
                        // In Claude mode: Escape returns to Ralph mode
                        // All other keys are forwarded to PTY
                        if key.code == KeyCode::Esc {
                            app.mode = Mode::Ralph;
                        } else {
                            // Forward key to PTY
                            forward_key_to_pty(app, key.code, key.modifiers);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Run the delay loop between iterations (2 seconds)
/// Shows countdown in UI and allows user to quit
fn run_delay(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    last_cols: &mut u16,
    last_rows: &mut u16,
) -> io::Result<()> {
    const DELAY_SECS: u64 = 2;

    loop {
        // Check if delay is complete
        if let Some(start) = app.delay_start {
            if start.elapsed() >= Duration::from_secs(DELAY_SECS) {
                break;
            }
        } else {
            break;
        }

        // Reload PRD if needed
        app.reload_prd_if_needed();

        terminal.draw(|frame| {
            let area = frame.area();

            // Check for terminal resize
            let new_pty_cols = (area.width as f32 * 0.70) as u16 - 2;
            let new_pty_rows = area.height.saturating_sub(3);

            if new_pty_cols != *last_cols || new_pty_rows != *last_rows {
                *last_cols = new_pty_cols;
                *last_rows = new_pty_rows;
            }

            // Create main layout: content area + bottom bar
            let main_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(1),
                ])
                .split(area);

            let content_area = main_layout[0];
            let bottom_bar_area = main_layout[1];

            // Create horizontal split
            let panels = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(30),
                    Constraint::Percentage(70),
                ])
                .split(content_area);

            let left_panel_area = panels[0];
            let right_panel_area = panels[1];

            // Left panel with delay message
            let left_block = Block::default()
                .title(" Ralph Status [ACTIVE] ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));

            let mut status_lines: Vec<Line> = Vec::new();

            // Iteration info
            status_lines.push(Line::from(vec![
                Span::styled("Iteration: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{}/{}", app.current_iteration, app.max_iterations),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
            status_lines.push(Line::from(""));

            // Delay countdown
            let remaining = if let Some(start) = app.delay_start {
                DELAY_SECS.saturating_sub(start.elapsed().as_secs())
            } else {
                0
            };
            status_lines.push(Line::from(vec![
                Span::styled(
                    format!("Starting next iteration in {}s...", remaining),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ]));
            status_lines.push(Line::from(""));

            // PRD info
            if let Some(ref prd) = app.prd {
                status_lines.push(Line::from(vec![
                    Span::styled("Task: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]));
                for line in wrap_text(&prd.description, left_panel_area.width.saturating_sub(4) as usize) {
                    status_lines.push(Line::from(Span::raw(format!("  {}", line))));
                }
                status_lines.push(Line::from(""));

                let completed = prd.completed_count();
                let total = prd.user_stories.len();
                let progress_pct = if total > 0 {
                    (completed as f32 / total as f32 * 100.0) as u8
                } else {
                    0
                };
                status_lines.push(Line::from(vec![
                    Span::styled("Progress: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!("{}/{} ({}%)", completed, total, progress_pct),
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
            }

            let left_content = Paragraph::new(status_lines)
                .block(left_block)
                .style(Style::default().fg(Color::White));

            frame.render_widget(left_content, left_panel_area);

            // Right panel - show last output
            let right_block = Block::default()
                .title(" Claude Code ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));

            let pty_state = app.pty_state.lock().unwrap();
            let screen = pty_state.parser.screen();
            let lines = render_vt100_screen(screen);

            let right_content = Paragraph::new(lines).block(right_block);
            frame.render_widget(right_content, right_panel_area);

            // Bottom bar
            let keybindings = Paragraph::new(" q: Quit | Waiting for next iteration... ")
                .style(Style::default().fg(Color::Black).bg(Color::Cyan));
            frame.render_widget(keybindings, bottom_bar_area);
        })?;

        // Handle input - allow quit during delay
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    app.iteration_state = IterationState::Completed;
                    break;
                }
            }
        }
    }

    Ok(())
}
