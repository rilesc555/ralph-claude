//! Application state and core logic for the Ralph TUI.
//!
//! This module contains the `App` struct which holds all state for the
//! interactive terminal UI, including PTY state, PRD tracking, and
//! navigation/view state.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use portable_pty::PtySize;

use crate::cli::CliConfig;
use crate::models::{IterationState, Mode, Prd, RalphViewMode, StorySortMode};
use crate::pty::PtyState;

/// Application state
pub struct App {
    pub pty_state: Arc<Mutex<PtyState>>,
    pub master_pty: Option<Box<dyn portable_pty::MasterPty + Send>>,
    pub pty_writer: Option<Box<dyn Write + Send>>,
    pub mode: Mode,
    pub task_dir: PathBuf,
    pub prd_path: PathBuf,
    pub prd: Option<Prd>,
    pub prd_needs_reload: Arc<Mutex<bool>>,
    // Iteration loop state
    pub current_iteration: u32,
    pub max_iterations: u32,
    pub iteration_state: IterationState,
    pub delay_start: Option<Instant>,
    // Elapsed time tracking
    pub session_start: Instant,
    pub iteration_start: Instant,
    // Progress rotation configuration
    pub rotate_threshold: u32,
    pub max_archives: u32,
    pub skip_prompts: bool,
    // Animation state
    pub animation_tick: u64,
    pub last_animation_update: Instant,
    // Session identification
    pub session_id: String,
    // Story list scroll offset (for arrow key navigation)
    pub story_scroll_offset: usize,
    // Currently selected story index (for detail views)
    pub selected_story_index: usize,
    // Sort mode for story list (priority vs ID)
    pub story_sort_mode: StorySortMode,
    // Ralph terminal view mode (what content to show)
    pub ralph_view_mode: RalphViewMode,
    // Whether Ralph terminal is expanded (true = 5-6 lines, false = 2-3 lines)
    pub ralph_expanded: bool,
    // Scroll offset for Ralph terminal content (when viewing details)
    pub ralph_scroll_offset: usize,
    // Scroll offset for Claude terminal (0 = at bottom, >0 = scrolled up into history)
    pub claude_scroll_offset: usize,
}

impl App {
    pub fn new(rows: u16, cols: u16, config: CliConfig) -> Self {
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
            max_archives: config.max_archives,
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
    pub fn find_first_incomplete_story(prd: &Option<Prd>) -> usize {
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
    pub fn reload_prd_if_needed(&mut self) {
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
    pub fn resize_pty(&self, cols: u16, rows: u16) {
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
