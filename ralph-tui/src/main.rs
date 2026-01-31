mod cli;
mod models;
mod pty;
mod theme;

use models::{IterationState, Mode, Prd, RalphViewMode, StorySortMode, StoryState};
use pty::{forward_key_to_pty, spawn_claude, strip_ansi_codes, PtyState};
use theme::{
    get_pulse_color, get_spinner_frame, BG_PRIMARY, BG_SECONDARY, BG_TERTIARY, BORDER_SUBTLE, CYAN_DIM, CYAN_PRIMARY,
    GREEN_ACTIVE, GREEN_SUCCESS, AMBER_WARNING, RED_ERROR, ROUNDED_BORDERS, TEXT_MUTED, TEXT_PRIMARY,
    TEXT_SECONDARY,
};
use cli::{parse_args, CliConfig, VERSION};

use std::io::{self, stdout, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use portable_pty::PtySize;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge, Paragraph},
};

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
    delay_start: Option<Instant>,
    // Elapsed time tracking
    session_start: Instant,
    iteration_start: Instant,
    // Progress rotation (reserved for future progress file rotation feature)
    #[allow(dead_code)]
    rotate_threshold: u32,
    #[allow(dead_code)]
    skip_prompts: bool,
    // Animation state
    animation_tick: u64,
    last_animation_update: Instant,
    // Session identification
    session_id: String,
    // Story list scroll offset (for arrow key navigation)
    story_scroll_offset: usize,
    // Currently selected story index (for detail views)
    selected_story_index: usize,
    // Sort mode for story list (priority vs ID)
    story_sort_mode: StorySortMode,
    // Ralph terminal view mode (what content to show)
    ralph_view_mode: RalphViewMode,
    // Whether Ralph terminal is expanded (true = 5-6 lines, false = 2-3 lines)
    ralph_expanded: bool,
    // Scroll offset for Ralph terminal content (when viewing details)
    ralph_scroll_offset: usize,
    // Scroll offset for Claude terminal (0 = at bottom, >0 = scrolled up into history)
    claude_scroll_offset: usize,
}

impl App {
    fn new(rows: u16, cols: u16, config: CliConfig) -> Self {
        let prd_path = config.task_dir.join("prd.json");
        let prd = Prd::load(&prd_path).ok();
        let now = Instant::now();
        // Generate session ID from process ID (format: RL-XXXXX)
        let session_id = format!("RL-{:05}", std::process::id() % 100000);
        // Find first incomplete story before moving prd
        let selected_story_index = Self::find_first_incomplete_story(&prd);

        Self {
            pty_state: Arc::new(Mutex::new(PtyState::new(rows, cols))),
            master_pty: None,
            pty_writer: None,
            mode: Mode::Ralph, // Default to Ralph mode
            task_dir: config.task_dir,
            prd_path,
            prd,
            prd_needs_reload: Arc::new(Mutex::new(false)),
            current_iteration: 1,
            max_iterations: config.max_iterations,
            iteration_state: IterationState::Running,
            delay_start: None,
            session_start: now,
            iteration_start: now,
            rotate_threshold: config.rotate_threshold,
            skip_prompts: config.skip_prompts,
            animation_tick: 0,
            last_animation_update: now,
            session_id,
            story_scroll_offset: 0,
            selected_story_index,
            story_sort_mode: StorySortMode::default(),
            ralph_view_mode: RalphViewMode::Normal,
            ralph_expanded: false,
            ralph_scroll_offset: 0,
            claude_scroll_offset: 0,
        }
    }

    /// Find the index of the first incomplete story (or 0 if all complete)
    fn find_first_incomplete_story(prd: &Option<Prd>) -> usize {
        if let Some(prd) = prd {
            prd.user_stories
                .iter()
                .position(|s| !s.passes)
                .unwrap_or(0)
        } else {
            0
        }
    }

    /// Reload PRD from disk if flagged
    fn reload_prd_if_needed(&mut self) {
        let needs_reload = {
            let Ok(mut flag) = self.prd_needs_reload.lock() else {
                return;
            };
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
        if let Ok(mut state) = self.pty_state.lock() {
            state.parser.screen_mut().set_size(rows, cols);
        }
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

/// Format duration as MM:SS
fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}

/// Render iteration and completion stat cards in a given area
/// Returns the widgets to be rendered: (left_card, right_card)
fn render_stat_cards(
    area: Rect,
    current_iteration: u32,
    max_iterations: u32,
    completed: usize,
    total: usize,
    frame: &mut Frame,
) {
    // Split area horizontally for two cards with a small gap
    let card_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    // Left card: Iterations
    let iter_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ROUNDED_BORDERS)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(BG_SECONDARY));

    let iter_content = vec![
        Line::from(vec![
            Span::styled("⏱ ", Style::default().fg(CYAN_PRIMARY)),
            Span::styled(
                format!("{}/{}", current_iteration, max_iterations),
                Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("ITERATIONS", Style::default().fg(TEXT_MUTED)),
        ]),
    ];

    let iter_paragraph = Paragraph::new(iter_content)
        .block(iter_block)
        .alignment(Alignment::Center);

    frame.render_widget(iter_paragraph, card_layout[0]);

    // Right card: Completed
    let comp_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ROUNDED_BORDERS)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(BG_SECONDARY));

    let comp_content = vec![
        Line::from(vec![
            Span::styled("◎ ", Style::default().fg(CYAN_PRIMARY)),
            Span::styled(
                format!("{}/{}", completed, total),
                Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("COMPLETED", Style::default().fg(TEXT_MUTED)),
        ]),
    ];

    let comp_paragraph = Paragraph::new(comp_content)
        .block(comp_block)
        .alignment(Alignment::Center);

    frame.render_widget(comp_paragraph, card_layout[1]);
}

/// Render a single user story card
/// Returns the height of the card:
/// - Completed/Pending: 3 lines (border + content + border)
/// - Active: 5 lines (border + title + progress bar + percentage + border)
fn render_story_card(
    area: Rect,
    story_id: &str,
    story_title: &str,
    state: StoryState,
    tick: u64,
    progress_percent: u16,
    criteria_passed: usize,
    criteria_total: usize,
    selected: bool,
    frame: &mut Frame,
) {
    // Determine colors based on state
    // For active state, use pulsing indicator color
    let (indicator, indicator_color, text_color, bg_color) = match state {
        StoryState::Completed => ("●", GREEN_SUCCESS, CYAN_PRIMARY, BG_SECONDARY),
        StoryState::Active => {
            let pulse_color = get_pulse_color(tick, GREEN_ACTIVE, CYAN_DIM);
            ("●", pulse_color, CYAN_PRIMARY, BG_TERTIARY)
        }
        StoryState::Pending => ("○", TEXT_MUTED, TEXT_SECONDARY, BG_SECONDARY),
    };

    // Use highlight border for selected card, normal for others
    let border_color = if selected { CYAN_PRIMARY } else { BORDER_SUBTLE };

    // Create card block with rounded borders
    let card_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ROUNDED_BORDERS)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    // Format story ID as #XX (extract numeric part)
    let story_num = story_id.trim_start_matches(|c: char| !c.is_ascii_digit());
    let formatted_id = format!("#{}", story_num);

    // Build card content - single line with indicator, ID, and truncated title
    let inner_width = area.width.saturating_sub(4) as usize; // Account for borders and padding
    let prefix = format!("{} {} ", indicator, formatted_id);
    let prefix_len = prefix.chars().count();
    let available_title_width = inner_width.saturating_sub(prefix_len);

    let title_char_count = story_title.chars().count();
    let truncated_title = if title_char_count > available_title_width {
        // Safely truncate using character boundaries
        let take_chars = available_title_width.saturating_sub(3);
        let truncated: String = story_title.chars().take(take_chars).collect();
        format!("{}...", truncated)
    } else {
        story_title.to_string()
    };

    let title_line = Line::from(vec![
        Span::styled(format!("{} ", indicator), Style::default().fg(indicator_color)),
        Span::styled(format!("{} ", formatted_id), Style::default().fg(text_color).add_modifier(Modifier::BOLD)),
        Span::styled(truncated_title, Style::default().fg(text_color)),
    ]);

    // For active state, show progress bar and percentage
    if state == StoryState::Active {
        // Render block first to get inner area
        let inner_area = card_block.inner(area);
        frame.render_widget(card_block, area);

        // Split inner area: title (1 line), progress bar (1 line), percentage (1 line)
        let inner_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Title line
                Constraint::Length(1), // Progress bar
                Constraint::Length(1), // Percentage
            ])
            .split(inner_area);

        // Render title
        let title_paragraph = Paragraph::new(vec![title_line]);
        frame.render_widget(title_paragraph, inner_layout[0]);

        // Render progress bar (Gauge widget)
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(CYAN_PRIMARY).bg(BG_SECONDARY))
            .percent(progress_percent)
            .label(""); // No label on the gauge itself
        frame.render_widget(gauge, inner_layout[1]);

        // Render criteria count below the progress bar (e.g., "2/5 criteria")
        let criteria_text = format!("{}/{} criteria ({}%)", criteria_passed, criteria_total, progress_percent);
        let percent_line = Line::from(Span::styled(
            criteria_text,
            Style::default().fg(TEXT_MUTED),
        ));
        let percent_paragraph = Paragraph::new(vec![percent_line]);
        frame.render_widget(percent_paragraph, inner_layout[2]);
    } else {
        // Completed and Pending states - simple single line card
        let paragraph = Paragraph::new(vec![title_line])
            .block(card_block);
        frame.render_widget(paragraph, area);
    }
}

/// Render progress stat cards (stories left + completion %) in a given area
fn render_progress_cards(
    area: Rect,
    completed: usize,
    total: usize,
    frame: &mut Frame,
) {
    // Split area horizontally for two cards
    let card_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    // Left card: Stories Left
    let stories_left = total.saturating_sub(completed);
    let left_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ROUNDED_BORDERS)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(BG_SECONDARY));

    let left_content = vec![
        Line::from(vec![
            Span::styled("◇ ", Style::default().fg(CYAN_PRIMARY)),
            Span::styled(
                format!("{}", stories_left),
                Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("STORIES LEFT", Style::default().fg(TEXT_MUTED)),
        ]),
    ];

    let left_paragraph = Paragraph::new(left_content)
        .block(left_block)
        .alignment(Alignment::Center);

    frame.render_widget(left_paragraph, card_layout[0]);

    // Right card: Progress percentage
    let progress_pct = if total > 0 {
        (completed as f32 / total as f32 * 100.0) as u8
    } else {
        0
    };

    let right_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ROUNDED_BORDERS)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(BG_SECONDARY));

    let progress_color = if progress_pct == 100 {
        GREEN_SUCCESS
    } else {
        CYAN_PRIMARY
    };

    let right_content = vec![
        Line::from(vec![
            Span::styled("⟠ ", Style::default().fg(progress_color)),
            Span::styled(
                format!("{}%", progress_pct),
                Style::default().fg(progress_color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("PROGRESS", Style::default().fg(TEXT_MUTED)),
        ]),
    ];

    let right_paragraph = Paragraph::new(right_content)
        .block(right_block)
        .alignment(Alignment::Center);

    frame.render_widget(right_paragraph, card_layout[1]);
}

/// Build the Ralph prompt from task directory and prompt.md
/// Returns the full prompt string to be piped to Claude Code stdin
/// Embedded default prompt.md as fallback
const EMBEDDED_PROMPT: &str = include_str!("../../prompt.md");

/// Find prompt.md in order of priority:
/// 1. ./ralph/prompt.md (local project customization)
/// 2. ~/.config/ralph/prompt.md (global user config)
/// 3. Embedded fallback (with warning)
fn find_prompt_content() -> (String, Option<String>) {
    // 1. Check local ./ralph/prompt.md
    let local_path = PathBuf::from("ralph/prompt.md");
    if local_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&local_path) {
            return (content, Some(local_path.display().to_string()));
        }
    }

    // 2. Check global ~/.config/ralph/prompt.md (Unix) or %USERPROFILE%\.config\ralph\prompt.md (Windows)
    let home_dir = if cfg!(windows) {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    };
    if let Some(home) = home_dir {
        let global_path = PathBuf::from(home).join(".config").join("ralph").join("prompt.md");
        if global_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&global_path) {
                return (content, Some(global_path.display().to_string()));
            }
        }
    }

    // 3. Fall back to embedded prompt
    eprintln!("Warning: No prompt.md found in ./ralph/ or ~/.config/ralph/, using embedded default");
    (EMBEDDED_PROMPT.to_string(), None)
}

fn build_ralph_prompt(task_dir: &PathBuf) -> io::Result<String> {
    let (prompt_content, _source) = find_prompt_content();

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

fn main() -> io::Result<()> {
    // Set up panic hook to restore terminal state before panicking
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restore terminal state
        let _ = disable_raw_mode();
        let _ = stdout().execute(DisableMouseCapture);
        let _ = stdout().execute(LeaveAlternateScreen);
        // Call the default panic handler
        default_panic(info);
    }));

    // Parse CLI arguments (includes interactive prompts if needed)
    let config = parse_args()?;

    // Validate task directory exists
    if !config.task_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Task directory not found: {}", config.task_dir.display()),
        ));
    }

    // Validate prd.json exists
    let prd_path = config.task_dir.join("prd.json");
    if !prd_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("prd.json not found in: {}", config.task_dir.display()),
        ));
    }

    // Check for schema migration
    Prd::check_and_migrate_schema(&prd_path)?;

    // Show startup banner
    println!();
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║  Ralph TUI - Autonomous Agent Loop                            ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    println!("  Task:       {}", config.task_dir.display());
    println!("  Max iters:  {}", config.max_iterations);
    println!();
    println!("Starting TUI...");
    println!();

    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Get initial terminal size for PTY
    let initial_size = terminal.size()?;
    // Calculate right panel size (70% of width, minus borders)
    // Ensure minimum sizes to prevent issues
    let pty_cols = ((initial_size.width as f32 * 0.70) as u16).saturating_sub(2).max(40);
    let pty_rows = initial_size.height.saturating_sub(3).max(10);

    // Create app state with VT100 parser sized to PTY dimensions
    let mut app = App::new(pty_rows, pty_cols, config);

    // Set up file watcher for prd.json
    let prd_needs_reload = Arc::clone(&app.prd_needs_reload);
    let prd_path_for_watcher = app.prd_path.clone();
    let _watcher = setup_prd_watcher(prd_path_for_watcher, prd_needs_reload);

    // Track last known size for resize detection
    let mut last_cols = pty_cols;
    let mut last_rows = pty_rows;

    // Spawn initial Claude process
    let ralph_prompt = build_ralph_prompt(&app.task_dir)?;
    let spawn_result = spawn_claude(
        &ralph_prompt,
        app.current_iteration,
        Arc::clone(&app.pty_state),
        pty_rows,
        pty_cols,
    )?;
    let mut child = spawn_result.child;
    let mut reader_thread = spawn_result.reader_thread;
    app.master_pty = Some(spawn_result.master_pty);
    app.pty_writer = Some(spawn_result.pty_writer);
    app.iteration_state = IterationState::Running;

    // Run the main loop
    let result = loop {
        // Run the UI loop for current iteration
        let run_result = run(&mut terminal, &mut app, &mut last_cols, &mut last_rows);

        // Clean up current iteration - kill the child process first to avoid blocking
        let _ = child.kill();
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

                // Start delay period before next iteration
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
                app.iteration_start = Instant::now();
                app.delay_start = None;

                // Reload PRD to get latest state
                if let Ok(prd) = Prd::load(&app.prd_path) {
                    // Check if all stories pass - project is complete!
                    if prd.all_stories_pass() {
                        app.prd = Some(prd);
                        app.iteration_state = IterationState::Completed;
                        break Ok(());
                    }
                    app.prd = Some(prd);
                }

                // Spawn new Claude process
                let ralph_prompt = match build_ralph_prompt(&app.task_dir) {
                    Ok(p) => p,
                    Err(e) => break Err(e),
                };
                match spawn_claude(
                    &ralph_prompt,
                    app.current_iteration,
                    Arc::clone(&app.pty_state),
                    last_rows,
                    last_cols,
                ) {
                    Ok(spawn_result) => {
                        child = spawn_result.child;
                        reader_thread = spawn_result.reader_thread;
                        app.master_pty = Some(spawn_result.master_pty);
                        app.pty_writer = Some(spawn_result.pty_writer);
                        app.iteration_state = IterationState::Running;
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

    // Always restore terminal, regardless of any errors
    let _ = disable_raw_mode();
    let _ = stdout().execute(DisableMouseCapture);
    let _ = stdout().execute(LeaveAlternateScreen);

    result
}

/// Set up a file watcher for prd.json changes
fn setup_prd_watcher(
    prd_path: PathBuf,
    needs_reload: Arc<Mutex<bool>>,
) -> Option<RecommendedWatcher> {
    // Use a shorter poll interval for more responsive updates
    let config = Config::default().with_poll_interval(Duration::from_millis(500));

    // Canonicalize the path for reliable comparison
    let canonical_prd = prd_path.canonicalize().unwrap_or_else(|_| prd_path.clone());
    let prd_filename = prd_path.file_name().map(|s| s.to_os_string());

    let watcher_result = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                // Check if any event path matches our prd.json file
                // Compare by filename since paths may differ in representation
                let matches = event.paths.iter().any(|p| {
                    // Try canonical path comparison first
                    if let Ok(canonical) = p.canonicalize() {
                        if canonical == canonical_prd {
                            return true;
                        }
                    }
                    // Fall back to filename comparison
                    if let Some(ref expected_name) = prd_filename {
                        if let Some(event_name) = p.file_name() {
                            return event_name == expected_name;
                        }
                    }
                    false
                });

                if matches {
                    if let Ok(mut flag) = needs_reload.lock() {
                        *flag = true;
                    }
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

        // Update animation tick every 100ms
        if app.last_animation_update.elapsed() >= Duration::from_millis(100) {
            app.animation_tick = app.animation_tick.wrapping_add(1);
            app.last_animation_update = Instant::now();
        }

        terminal.draw(|frame| {
            let area = frame.area();

            // Check for terminal resize
            let new_pty_cols = ((area.width as f32 * 0.70) as u16).saturating_sub(2).max(40);
            let new_pty_rows = area.height.saturating_sub(3).max(10);

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
                    Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD),
                    Style::default().fg(BORDER_SUBTLE),
                ),
                Mode::Claude => (
                    Style::default().fg(BORDER_SUBTLE),
                    Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD),
                ),
            };

            // Left panel: Ralph Status
            let left_title = match app.mode {
                Mode::Ralph => Line::from(vec![
                    Span::raw(" Ralph Status "),
                    Span::styled("[ACTIVE]", Style::default().fg(CYAN_PRIMARY)),
                    Span::raw(" "),
                ]),
                Mode::Claude => Line::from(" Ralph Status "),
            };
            let left_block = Block::default()
                .title(left_title)
                .borders(Borders::ALL)
                .border_style(left_border_style)
                .style(Style::default().bg(BG_PRIMARY));

            // Render the outer block first to get the inner area
            let left_inner = left_block.inner(left_panel_area);
            frame.render_widget(left_block, left_panel_area);

            // Get PRD data for stats
            let (completed, total) = if let Some(ref prd) = app.prd {
                (prd.completed_count(), prd.user_stories.len())
            } else {
                (0, 0)
            };

            // Get PTY state for display (use default values if mutex is poisoned)
            let mut pty_state_guard = app.pty_state.lock().ok();

            // Split inner area: header (3 lines), stat cards (8 lines for 2 rows), rest
            let inner_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Header
                    Constraint::Length(8), // Two stat card rows (4 lines each)
                    Constraint::Min(0),    // Rest of content
                ])
                .split(left_inner);

            let header_area = inner_layout[0];
            let cards_area = inner_layout[1];
            let content_area_inner = inner_layout[2];

            // Header: Ralph branding
            let header_lines = vec![
                Line::from(vec![
                    Span::styled("● ", Style::default().fg(GREEN_ACTIVE)),
                    Span::styled("RALPH LOOP", Style::default().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled(format!("Terminal v{}", VERSION), Style::default().fg(CYAN_PRIMARY)),
                ]),
                Line::from(""), // Gap after header
            ];
            let header = Paragraph::new(header_lines);
            frame.render_widget(header, header_area);

            // Split cards area into two rows
            let cards_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4), // First row: iteration/completed
                    Constraint::Length(4), // Second row: stories left/progress
                ])
                .split(cards_area);

            // Render iteration/completion stat cards (first row)
            render_stat_cards(
                cards_layout[0],
                app.current_iteration,
                app.max_iterations,
                completed,
                total,
                frame,
            );

            // Render progress stat cards (second row)
            render_progress_cards(
                cards_layout[1],
                completed,
                total,
                frame,
            );

            // Build remaining status content
            let mut status_lines: Vec<Line> = Vec::new();
            status_lines.push(Line::from("")); // Gap after cards

            // Active Phase section
            let session_elapsed = app.session_start.elapsed();
            status_lines.push(Line::from(vec![
                Span::styled("✦ ACTIVE PHASE", Style::default().fg(TEXT_MUTED)),
            ]));
            // Determine current phase name based on iteration state
            let phase_name = match app.iteration_state {
                IterationState::Running => "Execute Iteration Cycle",
                IterationState::Completed => "All Stories Complete",
                IterationState::NeedsRestart => "Preparing Next Iteration",
                IterationState::WaitingDelay => "Waiting for Delay",
                IterationState::WaitingUserConfirm => "Paused - Type 'exit' to continue",
            };
            status_lines.push(Line::from(vec![
                Span::styled(
                    phase_name,
                    Style::default().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
                ),
            ]));
            status_lines.push(Line::from(vec![
                Span::styled(
                    format!("⏱ Uptime: {}", format_duration(session_elapsed)),
                    Style::default().fg(TEXT_MUTED),
                ),
            ]));
            status_lines.push(Line::from("")); // Gap after active phase

            // Elapsed time (iteration-specific)
            let iteration_elapsed = app.iteration_start.elapsed();
            status_lines.push(Line::from(vec![
                Span::styled("Session: ", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format_duration(session_elapsed),
                    Style::default().fg(TEXT_PRIMARY),
                ),
                Span::raw("  "),
                Span::styled("Iter: ", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format_duration(iteration_elapsed),
                    Style::default().fg(TEXT_PRIMARY),
                ),
            ]));
            status_lines.push(Line::from(""));

            // Update activities from PTY output
            let activities = if let Some(ref mut guard) = pty_state_guard {
                guard.update_activities();
                guard.get_activities()
            } else {
                Vec::new()
            };

            // Recent activities section
            if !activities.is_empty() {
                status_lines.push(Line::from(vec![
                    Span::styled("Recent Activity:", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                ]));
                let max_activity_width = left_panel_area.width.saturating_sub(6) as usize;
                for activity in activities.iter().take(5) {
                    status_lines.push(Line::from(vec![
                        Span::styled("  • ", Style::default().fg(TEXT_MUTED)),
                        Span::styled(
                            activity.format(max_activity_width),
                            Style::default().fg(TEXT_PRIMARY),
                        ),
                    ]));
                }
                status_lines.push(Line::from(""));
            }

            // PRD information
            if let Some(ref prd) = app.prd {
                // Description
                status_lines.push(Line::from(vec![
                    Span::styled("Task: ", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                ]));
                // Wrap description to fit panel
                for line in wrap_text(&prd.description, left_panel_area.width.saturating_sub(4) as usize) {
                    status_lines.push(Line::from(Span::raw(format!("  {}", line))));
                }
                status_lines.push(Line::from(""));

                // Branch (or working directory note if no branch)
                let branch_display = prd.branch_name.as_deref().unwrap_or("(working in existing repos)");
                status_lines.push(Line::from(vec![
                    Span::styled("Branch: ", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::raw(branch_display),
                ]));
                status_lines.push(Line::from(""));

                // Progress (text display - cards show the numbers)
                let progress_pct = if total > 0 {
                    (completed as f32 / total as f32 * 100.0) as u8
                } else {
                    0
                };
                status_lines.push(Line::from(vec![
                    Span::styled("Progress: ", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!("{}%", progress_pct),
                        if completed == total {
                            Style::default().fg(GREEN_SUCCESS).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(CYAN_PRIMARY)
                        },
                    ),
                ]));

                // Overall progress bar (text-based)
                let bar_width = left_panel_area.width.saturating_sub(6) as usize; // Leave room for borders
                let filled = (bar_width as f32 * progress_pct as f32 / 100.0) as usize;
                let empty = bar_width.saturating_sub(filled);
                let bar_filled: String = "█".repeat(filled);
                let bar_empty: String = "░".repeat(empty);
                let progress_color = if completed == total { GREEN_SUCCESS } else { CYAN_PRIMARY };
                status_lines.push(Line::from(vec![
                    Span::styled(bar_filled, Style::default().fg(progress_color)),
                    Span::styled(bar_empty, Style::default().fg(BORDER_SUBTLE)),
                ]));
                status_lines.push(Line::from(""));

                // User Stories section header
                status_lines.push(Line::from(vec![
                    Span::styled("↳ USER STORIES / PHASES", Style::default().fg(TEXT_MUTED)),
                ]));
            } else {
                status_lines.push(Line::from(vec![
                    Span::styled("Error: ", Style::default().fg(RED_ERROR).add_modifier(Modifier::BOLD)),
                    Span::raw("Failed to load prd.json"),
                ]));
            }

            // Calculate lines for status content
            let status_line_count = status_lines.len() as u16;

            // Split content area: status text at top, story cards in middle, hints at bottom
            let content_split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(status_line_count),
                    Constraint::Min(0), // Story cards area
                    Constraint::Length(4), // Hints area
                ])
                .split(content_area_inner);

            let status_area = content_split[0];
            let stories_area = content_split[1];
            let hints_area = content_split[2];

            let left_content = Paragraph::new(status_lines)
                .style(Style::default().fg(TEXT_PRIMARY));

            frame.render_widget(left_content, status_area);

            // Render keybinding hints at the bottom of left panel
            let sort_label = app.story_sort_mode.label();
            let hints_lines = vec![
                Line::from(Span::styled("─── Navigation ───", Style::default().fg(BORDER_SUBTLE))),
                Line::from(vec![
                    Span::styled("↑↓", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(" or ", Style::default().fg(TEXT_MUTED)),
                    Span::styled("j/k", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(" Select story", Style::default().fg(TEXT_MUTED)),
                ]),
                Line::from(vec![
                    Span::styled("s", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(" Story  ", Style::default().fg(TEXT_MUTED)),
                    Span::styled("p", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(" Progress  ", Style::default().fg(TEXT_MUTED)),
                    Span::styled("r", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(" Reqs  ", Style::default().fg(TEXT_MUTED)),
                    Span::styled("o", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" Sort:{}", sort_label), Style::default().fg(TEXT_MUTED)),
                ]),
            ];
            let hints = Paragraph::new(hints_lines);
            frame.render_widget(hints, hints_area);

            // Render story cards if we have a PRD
            if let Some(ref prd) = app.prd {
                // Calculate progress percent for active story based on per-criteria completion
                let progress_percent = if let Some(story) = prd.current_story() {
                    let total = story.acceptance_criteria.len();
                    if total > 0 {
                        let passed = story.acceptance_criteria.iter().filter(|c| c.passes).count();
                        ((passed as f32 / total as f32) * 100.0) as u16
                    } else {
                        0
                    }
                } else {
                    100 // All stories complete
                };

                // Get stories sorted by current sort mode
                let mut stories: Vec<_> = prd.user_stories.iter().collect();
                match app.story_sort_mode {
                    StorySortMode::Priority => stories.sort_by_key(|s| s.priority),
                    StorySortMode::Id => stories.sort_by(|a, b| {
                        // Extract numeric part from ID (e.g., "US-018" -> 18)
                        let num_a = a.id.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().unwrap_or(0);
                        let num_b = b.id.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().unwrap_or(0);
                        num_a.cmp(&num_b)
                    }),
                }

                // Find current story for state comparison
                let current_story = prd.current_story();

                // Card heights: active = 5 lines, others = 3 lines
                let active_card_height = 5u16;
                let normal_card_height = 3u16;

                // Ensure selected_story_index is valid
                if app.selected_story_index >= stories.len() && !stories.is_empty() {
                    app.selected_story_index = stories.len() - 1;
                }

                // Make scroll follow selection: adjust scroll_offset so selected story is visible
                // If selected < scroll_offset, scroll up
                if app.selected_story_index < app.story_scroll_offset {
                    app.story_scroll_offset = app.selected_story_index;
                }
                // Estimate visible stories (assuming average card height of 3 lines)
                // Subtract 2 lines for potential scroll indicators (above/below)
                let effective_height = stories_area.height.saturating_sub(2);
                let estimated_visible = (effective_height / normal_card_height).max(1) as usize;
                if app.selected_story_index >= app.story_scroll_offset + estimated_visible {
                    // Selected story is below visible area, scroll down
                    // Put selected story at the bottom of visible area
                    app.story_scroll_offset = app.selected_story_index.saturating_sub(estimated_visible.saturating_sub(1));
                }
                let max_scroll = stories.len().saturating_sub(1);
                if app.story_scroll_offset > max_scroll {
                    app.story_scroll_offset = max_scroll;
                }

                // Calculate total height needed and visible stories
                let mut y_offset = 0u16;
                let mut rendered_count = 0usize;

                // Show scroll indicator if content extends above
                if app.story_scroll_offset > 0 {
                    let indicator = Line::from(vec![
                        Span::styled("  ▲ ", Style::default().fg(TEXT_MUTED)),
                        Span::styled(
                            format!("{} more above", app.story_scroll_offset),
                            Style::default().fg(TEXT_MUTED),
                        ),
                    ]);
                    let indicator_para = Paragraph::new(indicator);
                    let indicator_area = Rect {
                        x: stories_area.x,
                        y: stories_area.y,
                        width: stories_area.width,
                        height: 1,
                    };
                    frame.render_widget(indicator_para, indicator_area);
                    y_offset = 1;
                }

                for (idx, story) in stories.iter().enumerate() {
                    // Skip stories before scroll offset
                    if idx < app.story_scroll_offset {
                        continue;
                    }

                    // Determine story state
                    let state = if story.passes {
                        StoryState::Completed
                    } else if Some(*story) == current_story {
                        StoryState::Active
                    } else {
                        StoryState::Pending
                    };

                    let card_height = if state == StoryState::Active {
                        active_card_height
                    } else {
                        normal_card_height
                    };

                    // Check if card fits in available space (reserve 1 line for bottom indicator)
                    let remaining_stories = stories.len() - idx - 1;
                    let reserve_for_indicator = if remaining_stories > 0 { 1 } else { 0 };
                    if y_offset + card_height + reserve_for_indicator > stories_area.height {
                        // Show scroll indicator for remaining stories
                        let remaining = stories.len() - idx;
                        if remaining > 0 && y_offset < stories_area.height {
                            let indicator = Line::from(vec![
                                Span::styled("  ▼ ", Style::default().fg(TEXT_MUTED)),
                                Span::styled(
                                    format!("{} more below", remaining),
                                    Style::default().fg(TEXT_MUTED),
                                ),
                            ]);
                            let indicator_para = Paragraph::new(indicator);
                            let indicator_area = Rect {
                                x: stories_area.x,
                                y: stories_area.y + y_offset,
                                width: stories_area.width,
                                height: 1,
                            };
                            frame.render_widget(indicator_para, indicator_area);
                        }
                        break;
                    }

                    let card_area = Rect {
                        x: stories_area.x,
                        y: stories_area.y + y_offset,
                        width: stories_area.width,
                        height: card_height,
                    };

                    // Check if this story is selected
                    let is_selected = idx == app.selected_story_index;

                    // Calculate criteria progress for this story
                    let criteria_total = story.acceptance_criteria.len();
                    let criteria_passed = story.acceptance_criteria.iter().filter(|c| c.passes).count();

                    render_story_card(
                        card_area,
                        &story.id,
                        &story.title,
                        state,
                        app.animation_tick,
                        progress_percent,
                        criteria_passed,
                        criteria_total,
                        is_selected,
                        frame,
                    );

                    y_offset += card_height;
                    rendered_count += 1;
                }

                // If we rendered all remaining stories, no need for bottom indicator
                let _ = rendered_count;
            }

            // Right panel: Two separate terminals (Ralph on top, Claude on bottom)
            // Each terminal is its own bordered section

            // Determine Ralph terminal height based on expanded state
            let ralph_is_expanded = app.ralph_expanded || app.ralph_view_mode != RalphViewMode::Normal;
            let ralph_terminal_height = if ralph_is_expanded {
                9  // Expanded: 2 border + 5 content + 2 padding
            } else {
                6  // Normal: 2 border + 2 content + 2 padding
            };

            // Split right panel directly into Ralph terminal (top) and Claude terminal (bottom)
            let terminal_split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(ralph_terminal_height),  // Ralph terminal (top)
                    Constraint::Min(0),  // Claude terminal (takes remaining space, bottom)
                ])
                .split(right_panel_area);

            let ralph_terminal_area = terminal_split[0];
            let claude_terminal_area = terminal_split[1];

            // === CLAUDE TERMINAL ===
            // Create bordered block for Claude terminal
            let claude_title = match app.mode {
                Mode::Claude => Line::from(vec![
                    Span::raw(" >_ claude-code - ralph-loop "),
                    Span::styled("[ACTIVE]", Style::default().fg(CYAN_PRIMARY)),
                    Span::raw(" "),
                ]),
                Mode::Ralph => Line::from(" >_ claude-code - ralph-loop "),
            };
            let claude_block = Block::default()
                .title(claude_title)
                .borders(Borders::ALL)
                .border_style(right_border_style)
                .style(Style::default().bg(BG_PRIMARY));

            let claude_content_area = claude_block.inner(claude_terminal_area);
            frame.render_widget(claude_block, claude_terminal_area);

            // Claude terminal content (VT100 rendered) - uses full inner area
            // Set scrollback offset for user-controlled scrolling (mouse wheel)
            let lines = if let Some(ref mut pty_state) = pty_state_guard {
                // Set scrollback position for viewing history
                pty_state.parser.screen_mut().set_scrollback(app.claude_scroll_offset);
                let screen = pty_state.parser.screen();
                let rendered = render_vt100_screen(screen);
                // Reset scrollback to 0 so stop hook detection sees current content
                pty_state.parser.screen_mut().set_scrollback(0);
                rendered
            } else {
                vec![Line::from(Span::styled(
                    "Error: Failed to access PTY state",
                    Style::default().fg(RED_ERROR),
                ))]
            };

            // Scroll to show the bottom of the terminal output (most recent content)
            // When claude_scroll_offset is 0, we're at the bottom (current view)
            // When claude_scroll_offset > 0, we're viewing history
            let content_height = claude_content_area.height as usize;
            let scroll_offset = if app.claude_scroll_offset == 0 && lines.len() > content_height {
                (lines.len() - content_height) as u16
            } else {
                0
            };

            let claude_content = Paragraph::new(lines)
                .scroll((scroll_offset, 0));
            frame.render_widget(claude_content, claude_content_area);

            // === RALPH TERMINAL ===
            // Create bordered block for Ralph terminal
            let ralph_title = match app.mode {
                Mode::Ralph => Line::from(vec![
                    Span::raw(" >_ ralph output "),
                    Span::styled("[ACTIVE]", Style::default().fg(CYAN_PRIMARY)),
                    Span::raw(" "),
                ]),
                Mode::Claude => Line::from(" >_ ralph output "),
            };
            let ralph_border_style = match app.mode {
                Mode::Ralph => Style::default().fg(CYAN_PRIMARY),
                Mode::Claude => Style::default().fg(BORDER_SUBTLE),
            };
            let ralph_block = Block::default()
                .title(ralph_title)
                .borders(Borders::ALL)
                .border_style(ralph_border_style)
                .style(Style::default().bg(BG_PRIMARY));

            let ralph_content_area = ralph_block.inner(ralph_terminal_area);
            frame.render_widget(ralph_block, ralph_terminal_area);

            // Ralph terminal content (based on view mode)
            let ralph_content_lines: Vec<Line> = match app.ralph_view_mode {
                RalphViewMode::Normal => {
                    // Show ASCII logo and status
                    vec![
                        Line::from(vec![
                            Span::styled("  ▶▶ ", Style::default().fg(GREEN_ACTIVE)),
                            Span::styled("RALPH LOOP", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                            Span::styled(" ◀◀", Style::default().fg(GREEN_ACTIVE)),
                        ]),
                        Line::from(Span::styled(
                            format!("     Iteration {}/{}", app.current_iteration, app.max_iterations),
                            Style::default().fg(TEXT_MUTED),
                        )),
                    ]
                }
                RalphViewMode::StoryDetails => {
                    // Show selected story details from prd.json
                    if let Some(ref prd) = app.prd {
                        let mut stories: Vec<_> = prd.user_stories.iter().collect();
                        match app.story_sort_mode {
                            StorySortMode::Priority => stories.sort_by_key(|s| s.priority),
                            StorySortMode::Id => stories.sort_by(|a, b| {
                                let num_a = a.id.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().unwrap_or(0);
                                let num_b = b.id.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().unwrap_or(0);
                                num_a.cmp(&num_b)
                            }),
                        }
                        if let Some(story) = stories.get(app.selected_story_index) {
                            let status_text = if story.passes { "✓ PASSED" } else { "○ PENDING" };
                            let status_color = if story.passes { GREEN_SUCCESS } else { AMBER_WARNING };
                            let mut lines = vec![
                                Line::from(vec![
                                    Span::styled(format!("  {} ", story.id), Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                                    Span::styled(status_text, Style::default().fg(status_color)),
                                ]),
                                Line::from(Span::styled(format!("  {}", story.title), Style::default().fg(TEXT_PRIMARY))),
                            ];
                            // Add all acceptance criteria (scrollable)
                            lines.push(Line::from(Span::styled("  ─── Acceptance Criteria ───", Style::default().fg(BORDER_SUBTLE))));
                            for (i, criterion) in story.acceptance_criteria.iter().enumerate() {
                                let check = if criterion.passes { "✓" } else { "○" };
                                let check_color = if criterion.passes { GREEN_SUCCESS } else { TEXT_MUTED };
                                lines.push(Line::from(vec![
                                    Span::styled(format!("  {} ", check), Style::default().fg(check_color)),
                                    Span::styled(format!("{}. {}", i + 1, criterion.description), Style::default().fg(TEXT_SECONDARY)),
                                ]));
                            }
                            // Add description if present
                            if !story.description.is_empty() {
                                lines.push(Line::from(""));
                                lines.push(Line::from(Span::styled("  ─── Description ───", Style::default().fg(BORDER_SUBTLE))));
                                lines.push(Line::from(Span::styled(format!("  {}", story.description), Style::default().fg(TEXT_MUTED))));
                            }
                            // Add notes if present
                            if !story.notes.is_empty() {
                                lines.push(Line::from(""));
                                lines.push(Line::from(Span::styled("  ─── Notes ───", Style::default().fg(BORDER_SUBTLE))));
                                lines.push(Line::from(Span::styled(format!("  {}", story.notes), Style::default().fg(TEXT_MUTED))));
                            }
                            lines
                        } else {
                            vec![Line::from(Span::styled("  No story selected", Style::default().fg(TEXT_MUTED)))]
                        }
                    } else {
                        vec![Line::from(Span::styled("  No PRD loaded", Style::default().fg(TEXT_MUTED)))]
                    }
                }
                RalphViewMode::Progress => {
                    // Show progress.txt entries for selected story
                    if let Some(ref prd) = app.prd {
                        let mut stories: Vec<_> = prd.user_stories.iter().collect();
                        match app.story_sort_mode {
                            StorySortMode::Priority => stories.sort_by_key(|s| s.priority),
                            StorySortMode::Id => stories.sort_by(|a, b| {
                                let num_a = a.id.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().unwrap_or(0);
                                let num_b = b.id.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().unwrap_or(0);
                                num_a.cmp(&num_b)
                            }),
                        }
                        if let Some(story) = stories.get(app.selected_story_index) {
                            let progress_path = app.task_dir.join("progress.txt");
                            if let Ok(content) = std::fs::read_to_string(&progress_path) {
                                // Find entries containing the story ID
                                let story_id = &story.id;
                                let mut matching_lines: Vec<Line> = vec![
                                    Line::from(vec![
                                        Span::styled("  Progress for ", Style::default().fg(TEXT_MUTED)),
                                        Span::styled(story_id.clone(), Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                                    ]),
                                ];

                                // Find section that mentions this story ID
                                let mut in_matching_section = false;
                                let mut found_any = false;
                                for line in content.lines() {
                                    if line.contains(story_id) && line.starts_with("##") {
                                        in_matching_section = true;
                                        found_any = true;
                                        continue; // Skip the header line itself
                                    } else if line.starts_with("##") || line.starts_with("---") {
                                        in_matching_section = false;
                                    }

                                    if in_matching_section && !line.is_empty() {
                                        // Show full line (scrollable)
                                        matching_lines.push(Line::from(Span::styled(
                                            format!("  {}", line),
                                            Style::default().fg(TEXT_SECONDARY),
                                        )));
                                    }
                                }

                                if !found_any {
                                    matching_lines.push(Line::from(Span::styled(
                                        "  No progress entries found",
                                        Style::default().fg(TEXT_MUTED),
                                    )));
                                }
                                matching_lines
                            } else {
                                vec![Line::from(Span::styled("  progress.txt not found", Style::default().fg(TEXT_MUTED)))]
                            }
                        } else {
                            vec![Line::from(Span::styled("  No story selected", Style::default().fg(TEXT_MUTED)))]
                        }
                    } else {
                        vec![Line::from(Span::styled("  No PRD loaded", Style::default().fg(TEXT_MUTED)))]
                    }
                }
                RalphViewMode::Requirements => {
                    // Show requirements from prd.md for selected story
                    if let Some(ref prd) = app.prd {
                        let mut stories: Vec<_> = prd.user_stories.iter().collect();
                        match app.story_sort_mode {
                            StorySortMode::Priority => stories.sort_by_key(|s| s.priority),
                            StorySortMode::Id => stories.sort_by(|a, b| {
                                let num_a = a.id.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().unwrap_or(0);
                                let num_b = b.id.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().unwrap_or(0);
                                num_a.cmp(&num_b)
                            }),
                        }
                        if let Some(story) = stories.get(app.selected_story_index) {
                            let prd_md_path = app.task_dir.join("prd.md");
                            if let Ok(content) = std::fs::read_to_string(&prd_md_path) {
                                let story_id = &story.id;
                                let story_title = &story.title;
                                let mut matching_lines: Vec<Line> = vec![
                                    Line::from(vec![
                                        Span::styled("  Requirements for ", Style::default().fg(TEXT_MUTED)),
                                        Span::styled(story_id.clone(), Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                                    ]),
                                ];

                                // Find section that mentions this story ID or title
                                let mut in_matching_section = false;
                                let mut found_any = false;
                                for line in content.lines() {
                                    // Look for headers containing story ID or title
                                    if (line.contains(story_id) || line.contains(story_title.as_str()))
                                        && (line.starts_with("#") || line.starts_with("##"))
                                    {
                                        in_matching_section = true;
                                        found_any = true;
                                        continue;
                                    } else if line.starts_with("#") {
                                        in_matching_section = false;
                                    }

                                    if in_matching_section && !line.is_empty() {
                                        // Show full line (scrollable)
                                        matching_lines.push(Line::from(Span::styled(
                                            format!("  {}", line),
                                            Style::default().fg(TEXT_SECONDARY),
                                        )));
                                    }
                                }

                                if !found_any {
                                    matching_lines.push(Line::from(Span::styled(
                                        "  No requirements section found in prd.md",
                                        Style::default().fg(TEXT_MUTED),
                                    )));
                                }
                                matching_lines
                            } else {
                                vec![Line::from(Span::styled("  prd.md not found", Style::default().fg(TEXT_MUTED)))]
                            }
                        } else {
                            vec![Line::from(Span::styled("  No story selected", Style::default().fg(TEXT_MUTED)))]
                        }
                    } else {
                        vec![Line::from(Span::styled("  No PRD loaded", Style::default().fg(TEXT_MUTED)))]
                    }
                }
            };

            // Add scroll hint and apply scroll offset for Ralph terminal content (only when not in Normal mode)
            let mut ralph_content_lines = ralph_content_lines;
            let ralph_scroll = if app.ralph_view_mode != RalphViewMode::Normal {
                // Add scroll hint at the top
                ralph_content_lines.insert(0, Line::from(vec![
                    Span::styled("  PgUp/PgDn", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(" to scroll │ Press key again to close", Style::default().fg(TEXT_MUTED)),
                ]));
                ralph_content_lines.insert(1, Line::from(""));
                // Cap scroll offset to content length
                let max_scroll = ralph_content_lines.len().saturating_sub(ralph_content_area.height as usize);
                app.ralph_scroll_offset.min(max_scroll) as u16
            } else {
                0
            };

            let ralph_content = Paragraph::new(ralph_content_lines)
                .style(Style::default().bg(BG_SECONDARY))
                .scroll((ralph_scroll, 0));
            frame.render_widget(ralph_content, ralph_content_area);

            // Bottom footer bar with session ID, mode indicator, and keybinding hints
            let (mode_text, keybindings_text) = match app.mode {
                Mode::Ralph => ("Ralph Mode", "i: Claude Mode | ^Q: Quit"),
                Mode::Claude => ("Claude Mode", "^O: Ralph Mode | ^Q: Quit"),
            };

            // Create footer line with session ID on left, mode in middle, keybindings on right
            // Calculate total fixed width: " Session ID " (12) + session_id + " │ " (3) + mode_text + remaining + keybindings + " " (1)
            let fixed_width = 12 + app.session_id.len() as u16 + 3 + mode_text.len() as u16 + keybindings_text.len() as u16 + 2;
            let fill_width = bottom_bar_area.width.saturating_sub(fixed_width) as usize;

            let footer_line = Line::from(vec![
                Span::styled(" Session ID ", Style::default().fg(TEXT_MUTED).bg(BG_SECONDARY)),
                Span::styled(&app.session_id, Style::default().fg(CYAN_PRIMARY).bg(BG_SECONDARY)),
                Span::styled(" │ ", Style::default().fg(BORDER_SUBTLE).bg(BG_SECONDARY)),
                Span::styled(mode_text, Style::default().fg(CYAN_PRIMARY).bg(BG_SECONDARY)),
                // Fill remaining space with background color
                Span::styled(
                    " ".repeat(fill_width),
                    Style::default().bg(BG_SECONDARY),
                ),
                Span::styled(keybindings_text, Style::default().fg(TEXT_MUTED).bg(BG_SECONDARY)),
                Span::styled(" ", Style::default().bg(BG_SECONDARY)),
            ]);

            let footer = Paragraph::new(footer_line)
                .style(Style::default().bg(BG_SECONDARY));

            frame.render_widget(footer, bottom_bar_area);
        })?;

        // Check if child exited or stop hook fired
        {
            let state_result = app.pty_state.lock();
            let (child_exited, is_complete, stop_hook_fired, debug_info) = match state_result {
                Ok(mut state) => {
                    // Update activities one final time before checking exit
                    state.update_activities();
                    let stop_signal = state.has_stop_hook_signal();
                    // Debug: capture last 500 chars of recent_output for logging
                    let debug = if stop_signal {
                        format!("STOP HOOK DETECTED! Buffer len: {}", state.recent_output().len())
                    } else {
                        let stripped = strip_ansi_codes(state.recent_output());
                        let lower = stripped.to_lowercase();
                        format!(
                            "No stop hook. Buffer len: {}. Contains 'stop hook': {}, 'iteration complete': {}",
                            state.recent_output().len(),
                            lower.contains("stop hook"),
                            lower.contains("iteration complete")
                        )
                    };
                    (state.child_exited, state.has_completion_signal(), stop_signal, debug)
                }
                Err(_) => (true, false, false, "Mutex poisoned".to_string()),
            };

            // Write debug info periodically (every ~5 seconds based on loop timing)
            static DEBUG_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let count = DEBUG_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if count % 100 == 0 || stop_hook_fired {
                let debug_log_path = std::env::temp_dir().join("ralph-tui-debug.log");
                let _ = std::fs::write(debug_log_path, format!(
                    "Count: {}\nchild_exited: {}\nstop_hook_fired: {}\nis_complete: {}\nDebug: {}\n",
                    count, child_exited, stop_hook_fired, is_complete, debug_info
                ));
            }

            // Stop hook fires when Claude's response completes - triggers new iteration
            // Claude doesn't actually exit, so we detect the hook message in output
            if child_exited || stop_hook_fired {
                // Check if pause mode is enabled
                let should_pause = app.prd.as_ref().map(|p| p.pause_between_stories).unwrap_or(false);

                // In pause mode: if stop hook fired but Claude hasn't exited, let user continue
                // The user can keep chatting with Claude and only exit when ready
                if should_pause && stop_hook_fired && !child_exited && !is_complete {
                    // Mark that we're in paused state (stop hook fired, waiting for user to exit)
                    if app.iteration_state != IterationState::WaitingUserConfirm {
                        app.iteration_state = IterationState::WaitingUserConfirm;
                    }
                    // Don't break - continue running and let user interact with Claude
                    // Loop will exit when child_exited becomes true (user types "exit")
                } else {
                    // Normal flow: either not in pause mode, or child has exited, or all complete
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
        }

        // Handle input based on current mode
        if event::poll(std::time::Duration::from_millis(50))? {
            match event::read()? {
            Event::Mouse(mouse) => {
                // Handle mouse scroll in Claude mode for terminal scrollback
                if app.mode == Mode::Claude {
                    // Max scrollback matches the parser initialization (1000 lines)
                    const MAX_SCROLLBACK: usize = 1000;
                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            // Scroll up (into history)
                            app.claude_scroll_offset = app.claude_scroll_offset.saturating_add(3);
                            app.claude_scroll_offset = app.claude_scroll_offset.min(MAX_SCROLLBACK);
                        }
                        MouseEventKind::ScrollDown => {
                            // Scroll down (towards current)
                            app.claude_scroll_offset = app.claude_scroll_offset.saturating_sub(3);
                        }
                        _ => {}
                    }
                }
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                // Universal quit: Ctrl+Q only (Ctrl+C should go to PTY for interrupt)
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
                    app.iteration_state = IterationState::Completed;
                    break;
                }

                match app.mode {
                    Mode::Ralph => {
                        // In Ralph mode: handle TUI controls
                        let story_count = app.prd.as_ref().map(|p| p.user_stories.len()).unwrap_or(0);

                        match key.code {
                            KeyCode::Char('i') | KeyCode::Tab => {
                                app.mode = Mode::Claude;
                            }
                            // j/k and arrow keys for story navigation
                            KeyCode::Up | KeyCode::Char('k') => {
                                if story_count > 0 {
                                    if app.selected_story_index > 0 {
                                        app.selected_story_index -= 1;
                                    } else {
                                        // Wrap to bottom
                                        app.selected_story_index = story_count - 1;
                                    }
                                    // Reset scroll when changing story
                                    app.ralph_scroll_offset = 0;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if story_count > 0 {
                                    if app.selected_story_index < story_count - 1 {
                                        app.selected_story_index += 1;
                                    } else {
                                        // Wrap to top
                                        app.selected_story_index = 0;
                                    }
                                    // Reset scroll when changing story
                                    app.ralph_scroll_offset = 0;
                                }
                            }
                            // PageUp/PageDown for scrolling Ralph terminal content
                            KeyCode::PageUp | KeyCode::Char('K') => {
                                if app.ralph_view_mode != RalphViewMode::Normal && app.ralph_scroll_offset > 0 {
                                    app.ralph_scroll_offset = app.ralph_scroll_offset.saturating_sub(3);
                                }
                            }
                            KeyCode::PageDown | KeyCode::Char('J') => {
                                if app.ralph_view_mode != RalphViewMode::Normal {
                                    app.ralph_scroll_offset += 3;
                                }
                            }
                            // s: Toggle story details view
                            KeyCode::Char('s') => {
                                app.ralph_view_mode = if app.ralph_view_mode == RalphViewMode::StoryDetails {
                                    RalphViewMode::Normal
                                } else {
                                    RalphViewMode::StoryDetails
                                };
                                app.ralph_scroll_offset = 0; // Reset scroll on view change
                            }
                            // p: Toggle progress view
                            KeyCode::Char('p') => {
                                app.ralph_view_mode = if app.ralph_view_mode == RalphViewMode::Progress {
                                    RalphViewMode::Normal
                                } else {
                                    RalphViewMode::Progress
                                };
                                app.ralph_scroll_offset = 0; // Reset scroll on view change
                            }
                            // r: Toggle requirements view
                            KeyCode::Char('r') => {
                                app.ralph_view_mode = if app.ralph_view_mode == RalphViewMode::Requirements {
                                    RalphViewMode::Normal
                                } else {
                                    RalphViewMode::Requirements
                                };
                                app.ralph_scroll_offset = 0; // Reset scroll on view change
                            }
                            // o: Toggle story sort order (priority vs ID)
                            KeyCode::Char('o') => {
                                app.story_sort_mode = app.story_sort_mode.toggle();
                            }
                            _ => {}
                        }
                    }
                    Mode::Claude => {
                        // In Claude mode: Ctrl+O returns to Ralph mode
                        // All other keys (including ESC) are forwarded to PTY
                        // This allows ESC to work natively in Claude for interrupting
                        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('o') {
                            app.mode = Mode::Ralph;
                        } else {
                            // Forward key to PTY (including ESC)
                            if let Some(ref mut writer) = app.pty_writer {
                                forward_key_to_pty(writer, key.code, key.modifiers);
                            }
                            // Reset scroll offset when user types (auto-scroll to bottom)
                            app.claude_scroll_offset = 0;
                        }
                    }
                }
            }
            _ => {} // Ignore other events (resize, focus, etc.)
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

        // Update animation tick every 100ms (for spinner animation)
        if app.last_animation_update.elapsed() >= Duration::from_millis(100) {
            app.animation_tick = app.animation_tick.wrapping_add(1);
            app.last_animation_update = Instant::now();
        }

        terminal.draw(|frame| {
            let area = frame.area();

            // Check for terminal resize
            let new_pty_cols = ((area.width as f32 * 0.70) as u16).saturating_sub(2).max(40);
            let new_pty_rows = area.height.saturating_sub(3).max(10);

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
            let left_title = Line::from(vec![
                Span::raw(" Ralph Status "),
                Span::styled("[ACTIVE]", Style::default().fg(CYAN_PRIMARY)),
                Span::raw(" "),
            ]);
            let left_block = Block::default()
                .title(left_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD))
                .style(Style::default().bg(BG_PRIMARY));

            // Render the outer block first to get the inner area
            let left_inner = left_block.inner(left_panel_area);
            frame.render_widget(left_block, left_panel_area);

            // Get PRD data for stats
            let (completed, total) = if let Some(ref prd) = app.prd {
                (prd.completed_count(), prd.user_stories.len())
            } else {
                (0, 0)
            };

            // Split inner area: header (3 lines), stat cards (8 lines for 2 rows), rest
            let inner_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Header
                    Constraint::Length(8), // Two stat card rows (4 lines each)
                    Constraint::Min(0),    // Rest of content
                ])
                .split(left_inner);

            let header_area = inner_layout[0];
            let cards_area = inner_layout[1];
            let content_area_inner = inner_layout[2];

            // Header: Ralph branding
            let header_lines = vec![
                Line::from(vec![
                    Span::styled("● ", Style::default().fg(GREEN_ACTIVE)),
                    Span::styled("RALPH LOOP", Style::default().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled(format!("Terminal v{}", VERSION), Style::default().fg(CYAN_PRIMARY)),
                ]),
                Line::from(""), // Gap after header
            ];
            let header = Paragraph::new(header_lines);
            frame.render_widget(header, header_area);

            // Split cards area into two rows
            let cards_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4), // First row: iteration/completed
                    Constraint::Length(4), // Second row: stories left/progress
                ])
                .split(cards_area);

            // Render iteration/completion stat cards (first row)
            render_stat_cards(
                cards_layout[0],
                app.current_iteration,
                app.max_iterations,
                completed,
                total,
                frame,
            );

            // Render progress stat cards (second row)
            render_progress_cards(
                cards_layout[1],
                completed,
                total,
                frame,
            );

            // Build remaining content
            let mut status_lines: Vec<Line> = Vec::new();
            status_lines.push(Line::from("")); // Gap after cards

            // Active Phase section
            let session_elapsed = app.session_start.elapsed();
            status_lines.push(Line::from(vec![
                Span::styled("✦ ACTIVE PHASE", Style::default().fg(TEXT_MUTED)),
            ]));
            // During delay, we're waiting for the next iteration
            let phase_name = "Preparing Next Iteration";
            status_lines.push(Line::from(vec![
                Span::styled(
                    phase_name,
                    Style::default().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
                ),
            ]));
            status_lines.push(Line::from(vec![
                Span::styled(
                    format!("⏱ Uptime: {}", format_duration(session_elapsed)),
                    Style::default().fg(TEXT_MUTED),
                ),
            ]));
            status_lines.push(Line::from("")); // Gap after active phase

            // Elapsed time (iteration-specific)
            let iteration_elapsed = app.iteration_start.elapsed();
            status_lines.push(Line::from(vec![
                Span::styled("Session: ", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format_duration(session_elapsed),
                    Style::default().fg(TEXT_PRIMARY),
                ),
                Span::raw("  "),
                Span::styled("Iter: ", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format_duration(iteration_elapsed),
                    Style::default().fg(TEXT_PRIMARY),
                ),
            ]));
            status_lines.push(Line::from(""));

            // Delay countdown - prominently displayed with spinner
            let remaining = if let Some(start) = app.delay_start {
                DELAY_SECS.saturating_sub(start.elapsed().as_secs())
            } else {
                0
            };
            let spinner = get_spinner_frame(app.animation_tick);
            // Add visual separator for prominence
            status_lines.push(Line::from(vec![
                Span::styled("━━━━━━━━━━━━━━━━━━━━━━━━━", Style::default().fg(AMBER_WARNING)),
            ]));
            status_lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", spinner),
                    Style::default().fg(AMBER_WARNING).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("Starting next iteration in {}s...", remaining),
                    Style::default().fg(AMBER_WARNING).add_modifier(Modifier::BOLD),
                ),
            ]));
            status_lines.push(Line::from(vec![
                Span::styled("━━━━━━━━━━━━━━━━━━━━━━━━━", Style::default().fg(AMBER_WARNING)),
            ]));
            status_lines.push(Line::from(""));

            // PRD info
            if let Some(ref prd) = app.prd {
                status_lines.push(Line::from(vec![
                    Span::styled("Task: ", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                ]));
                for line in wrap_text(&prd.description, left_panel_area.width.saturating_sub(4) as usize) {
                    status_lines.push(Line::from(Span::raw(format!("  {}", line))));
                }
                status_lines.push(Line::from(""));

                let progress_pct = if total > 0 {
                    (completed as f32 / total as f32 * 100.0) as u8
                } else {
                    0
                };
                status_lines.push(Line::from(vec![
                    Span::styled("Progress: ", Style::default().fg(CYAN_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!("{}%", progress_pct),
                        if completed == total {
                            Style::default().fg(GREEN_SUCCESS).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(CYAN_PRIMARY)
                        },
                    ),
                ]));

                // Overall progress bar (text-based)
                let bar_width = left_panel_area.width.saturating_sub(6) as usize;
                let filled = (bar_width as f32 * progress_pct as f32 / 100.0) as usize;
                let empty = bar_width.saturating_sub(filled);
                let bar_filled: String = "█".repeat(filled);
                let bar_empty: String = "░".repeat(empty);
                let progress_color = if completed == total { GREEN_SUCCESS } else { CYAN_PRIMARY };
                status_lines.push(Line::from(vec![
                    Span::styled(bar_filled, Style::default().fg(progress_color)),
                    Span::styled(bar_empty, Style::default().fg(BORDER_SUBTLE)),
                ]));
            }

            let left_content = Paragraph::new(status_lines)
                .style(Style::default().fg(TEXT_PRIMARY));

            frame.render_widget(left_content, content_area_inner);

            // Right panel - dual terminals (same layout as run())
            let right_block = Block::default()
                .title(" Terminals ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_SUBTLE))
                .style(Style::default().bg(BG_PRIMARY));

            // Render the outer block first to get the inner area
            let right_inner = right_block.inner(right_panel_area);
            frame.render_widget(right_block, right_panel_area);

            // Determine Ralph terminal height (always normal during delay)
            let ralph_terminal_height = 4u16;  // Normal: 1 chrome + 2 content + 1 separator

            // Split right inner into Claude terminal (top) and Ralph terminal (bottom)
            let terminal_split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),  // Claude terminal
                    Constraint::Length(ralph_terminal_height),  // Ralph terminal
                ])
                .split(right_inner);

            let claude_terminal_area = terminal_split[0];
            let ralph_terminal_area = terminal_split[1];

            // === CLAUDE TERMINAL ===
            let claude_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Window chrome
                    Constraint::Min(0),    // Content
                    Constraint::Length(1), // Input bar
                ])
                .split(claude_terminal_area);

            let claude_chrome_area = claude_layout[0];
            let claude_content_area = claude_layout[1];
            let claude_input_area = claude_layout[2];

            // Claude window chrome with centered title (no traffic lights)
            let claude_title = ">_ claude-code - ralph-loop";
            let title_width = claude_title.len() as u16;
            let available_width = claude_chrome_area.width;
            let center_offset = (available_width.saturating_sub(title_width)) / 2;
            let right_pad = available_width.saturating_sub(center_offset + title_width);

            let claude_chrome_line = Line::from(vec![
                Span::styled(" ".repeat(center_offset as usize), Style::default().bg(BG_TERTIARY)),
                Span::styled(claude_title, Style::default().fg(TEXT_SECONDARY).bg(BG_TERTIARY)),
                Span::styled(" ".repeat(right_pad as usize), Style::default().bg(BG_TERTIARY)),
            ]);

            let claude_chrome = Paragraph::new(claude_chrome_line)
                .style(Style::default().bg(BG_TERTIARY));
            frame.render_widget(claude_chrome, claude_chrome_area);

            // Render VT100 screen content
            let lines = if let Ok(pty_state) = app.pty_state.lock() {
                let screen = pty_state.parser.screen();
                render_vt100_screen(screen)
            } else {
                vec![Line::from(Span::styled(
                    "Error: Failed to access PTY state",
                    Style::default().fg(RED_ERROR),
                ))]
            };

            let claude_content = Paragraph::new(lines);
            frame.render_widget(claude_content, claude_content_area);

            // Claude input bar (placeholder style during delay)
            let remaining_width = claude_input_area.width.saturating_sub(32);
            let claude_input_content = Line::from(vec![
                Span::styled("│ ", Style::default().fg(BORDER_SUBTLE).bg(BG_SECONDARY)),
                Span::styled("> ", Style::default().fg(CYAN_PRIMARY).bg(BG_SECONDARY)),
                Span::styled("ralph@loop:~$ ", Style::default().fg(TEXT_SECONDARY).bg(BG_SECONDARY)),
                Span::styled("Enter command...", Style::default().fg(TEXT_MUTED).bg(BG_SECONDARY)),
                Span::styled(" ".repeat(remaining_width as usize), Style::default().bg(BG_SECONDARY)),
            ]);

            let claude_input = Paragraph::new(claude_input_content)
                .style(Style::default().bg(BG_SECONDARY));
            frame.render_widget(claude_input, claude_input_area);

            // === RALPH TERMINAL ===
            let ralph_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Window chrome
                    Constraint::Min(0),    // Content
                ])
                .split(ralph_terminal_area);

            let ralph_chrome_area = ralph_layout[0];
            let ralph_content_area = ralph_layout[1];

            // Ralph window chrome
            let ralph_title = ">_ ralph output";
            let ralph_title_width = ralph_title.len() as u16;
            let ralph_available_width = ralph_chrome_area.width;
            let ralph_center_offset = (ralph_available_width.saturating_sub(ralph_title_width)) / 2;
            let ralph_right_pad = ralph_available_width.saturating_sub(ralph_center_offset + ralph_title_width);

            let ralph_chrome_line = Line::from(vec![
                Span::styled(" ".repeat(ralph_center_offset as usize), Style::default().bg(BG_TERTIARY)),
                Span::styled(ralph_title, Style::default().fg(TEXT_SECONDARY).bg(BG_TERTIARY)),
                Span::styled(" ".repeat(ralph_right_pad as usize), Style::default().bg(BG_TERTIARY)),
            ]);

            let ralph_chrome = Paragraph::new(ralph_chrome_line)
                .style(Style::default().bg(BG_TERTIARY));
            frame.render_widget(ralph_chrome, ralph_chrome_area);

            // Ralph content: show waiting message during delay
            let ralph_content_lines = vec![
                Line::from(Span::styled(
                    format!("  Waiting {} seconds before next iteration...", remaining),
                    Style::default().fg(AMBER_WARNING),
                )),
            ];

            let ralph_content = Paragraph::new(ralph_content_lines)
                .style(Style::default().bg(BG_SECONDARY));
            frame.render_widget(ralph_content, ralph_content_area);

            // Bottom footer bar with session ID, mode indicator, and keybinding hints
            let mode_text = "Ralph Mode";
            let keybindings_text = "^Q: Quit | Waiting for next iteration...";

            // Create footer line with session ID on left, mode in middle, keybindings on right
            let fixed_width = 12 + app.session_id.len() as u16 + 3 + mode_text.len() as u16 + keybindings_text.len() as u16 + 2;
            let fill_width = bottom_bar_area.width.saturating_sub(fixed_width) as usize;

            let footer_line = Line::from(vec![
                Span::styled(" Session ID ", Style::default().fg(TEXT_MUTED).bg(BG_SECONDARY)),
                Span::styled(&app.session_id, Style::default().fg(CYAN_PRIMARY).bg(BG_SECONDARY)),
                Span::styled(" │ ", Style::default().fg(BORDER_SUBTLE).bg(BG_SECONDARY)),
                Span::styled(mode_text, Style::default().fg(CYAN_PRIMARY).bg(BG_SECONDARY)),
                // Fill remaining space with background color
                Span::styled(
                    " ".repeat(fill_width),
                    Style::default().bg(BG_SECONDARY),
                ),
                Span::styled(keybindings_text, Style::default().fg(TEXT_MUTED).bg(BG_SECONDARY)),
                Span::styled(" ", Style::default().bg(BG_SECONDARY)),
            ]);

            let footer = Paragraph::new(footer_line)
                .style(Style::default().bg(BG_SECONDARY));

            frame.render_widget(footer, bottom_bar_area);
        })?;

        // Handle input - allow quit during delay
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Only handle key press events (Windows sends both Press and Release)
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                // Ctrl+Q to quit
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
                    app.iteration_state = IterationState::Completed;
                    break;
                }
            }
        }
    }

    Ok(())
}
