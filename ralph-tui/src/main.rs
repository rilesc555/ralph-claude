//! Ralph TUI - Autonomous Agent Loop Terminal Interface

mod app;
mod cli;
mod models;
mod pty;
mod run;
mod theme;
mod ui;
mod utils;
mod watcher;

use std::io::{self, stdout};
use std::sync::Arc;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{prelude::*, Terminal};

use app::App;
use cli::parse_args;
use models::Prd;
use run::run_loop;
use watcher::setup_prd_watcher;

fn main() -> io::Result<()> {
    // Set up panic hook to restore terminal state before panicking
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = stdout().execute(DisableMouseCapture);
        let _ = stdout().execute(LeaveAlternateScreen);
        default_panic(info);
    }));

    // Parse CLI arguments (includes interactive prompts if needed)
    let config = parse_args()?;

    // Validate task directory and prd.json exist
    if !config.task_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Task directory not found: {}", config.task_dir.display()),
        ));
    }
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
    let pty_cols = ((initial_size.width as f32 * 0.70) as u16).saturating_sub(2).max(40);
    let pty_rows = initial_size.height.saturating_sub(3).max(10);

    // Create app state and set up file watcher
    let mut app = App::new(pty_rows, pty_cols, config);
    let _watcher = setup_prd_watcher(app.prd_path.clone(), Arc::clone(&app.prd_needs_reload));

    // Run the main iteration loop
    let result = run_loop(&mut terminal, &mut app, pty_rows, pty_cols);

    // Always restore terminal state
    let _ = disable_raw_mode();
    let _ = stdout().execute(DisableMouseCapture);
    let _ = stdout().execute(LeaveAlternateScreen);

    result
}
