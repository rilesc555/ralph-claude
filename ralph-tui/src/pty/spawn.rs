//! Claude process spawning with PTY setup.

use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use super::PtyState;

/// Result of spawning a Claude process
pub struct SpawnResult {
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    pub reader_thread: thread::JoinHandle<()>,
    pub master_pty: Box<dyn portable_pty::MasterPty + Send>,
    pub pty_writer: Box<dyn std::io::Write + Send>,
}

/// Spawn Claude Code process and return spawn result
///
/// Arguments:
/// - `ralph_prompt`: The fully built Ralph agent prompt
/// - `iteration`: Current iteration number (for temp file naming)
/// - `pty_state`: Shared PTY state for output tracking
/// - `pty_rows`: Terminal height
/// - `pty_cols`: Terminal width
pub fn spawn_claude(
    ralph_prompt: &str,
    iteration: u32,
    pty_state: Arc<Mutex<PtyState>>,
    pty_rows: u16,
    pty_cols: u16,
) -> io::Result<SpawnResult> {
    // Write prompt to a temp file for safe handling of special characters
    let prompt_temp_file = std::env::temp_dir().join(format!(
        "ralph_prompt_{}_{}.txt",
        std::process::id(),
        iteration
    ));
    std::fs::write(&prompt_temp_file, ralph_prompt)?;

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

    // Spawn Claude Code interactively with the prompt as a positional argument
    // This runs Claude in full interactive mode with the Ralph prompt
    let mut cmd = CommandBuilder::new("claude");

    // Set working directory to current directory (where ralph-tui was invoked)
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(&cwd);
    }

    // Set TERM environment variable for proper terminal handling
    cmd.env("TERM", "xterm-256color");
    // Force color output (NO_COLOR should NOT be set - any value disables colors per the standard)
    cmd.env("FORCE_COLOR", "1");
    cmd.env("COLORTERM", "truecolor");
    // Explicitly remove NO_COLOR if it's set in the parent environment
    cmd.env_remove("NO_COLOR");

    cmd.arg("--dangerously-skip-permissions");

    // Use ralph settings file for stop hook (enables iteration detection)
    // Settings are installed to ~/.config/ralph/settings.json by install.sh (Unix)
    // or %USERPROFILE%\.config\ralph\settings.json by install.ps1 (Windows)
    let home_dir = if cfg!(windows) {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    };
    if let Some(home) = home_dir {
        let settings_path = PathBuf::from(home).join(".config").join("ralph").join("settings.json");
        if settings_path.exists() {
            cmd.arg("--settings");
            cmd.arg(settings_path.to_string_lossy().to_string());
        }
    }

    // Prompt is passed as the last positional argument
    let prompt_content = std::fs::read_to_string(&prompt_temp_file)?;
    cmd.arg(&prompt_content);

    // Clean up temp file
    let _ = std::fs::remove_file(&prompt_temp_file);

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

    // Reset PTY state for new iteration
    {
        let mut state = pty_state.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "Failed to lock PTY state")
        })?;
        state.child_exited = false;
        state.clear_recent_output();
        // Re-initialize parser to clear screen
        state.parser = vt100::Parser::new(pty_rows, pty_cols, 1000);
    }

    // Spawn thread to read PTY output and feed to VT100 parser
    let pty_state_clone = Arc::clone(&pty_state);
    let reader_thread = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    // EOF - child process has exited
                    if let Ok(mut state) = pty_state_clone.lock() {
                        state.child_exited = true;
                    }
                    break;
                }
                Ok(n) => {
                    // Feed raw bytes to VT100 parser and track for completion detection
                    if let Ok(mut state) = pty_state_clone.lock() {
                        state.parser.process(&buf[..n]);
                        state.append_output(&buf[..n]);
                    }
                }
                Err(_) => {
                    if let Ok(mut state) = pty_state_clone.lock() {
                        state.child_exited = true;
                    }
                    break;
                }
            }
        }
    });

    Ok(SpawnResult {
        child,
        reader_thread,
        master_pty: pair.master,
        pty_writer,
    })
}
