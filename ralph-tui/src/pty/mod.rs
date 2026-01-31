//! PTY (pseudo-terminal) handling for Claude Code subprocess management.
//!
//! This module encapsulates all PTY-related functionality:
//! - `state`: VT100 terminal state tracking and output parsing
//! - `spawn`: Claude process spawning with PTY setup
//! - `input`: Keyboard input forwarding to PTY

mod input;
mod spawn;
mod state;

pub use input::forward_key_to_pty;
pub use spawn::spawn_claude;
pub use state::{strip_ansi_codes, PtyState};
