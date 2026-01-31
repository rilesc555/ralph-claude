//! PTY state management with VT100 terminal emulation.

use crate::models::{parse_activities, Activity, MAX_ACTIVITIES};

/// Shared state for PTY with VT100 parser
pub struct PtyState {
    pub parser: vt100::Parser,
    pub child_exited: bool,
    /// Recent raw output for detecting completion signal
    recent_output: String,
    /// Recent activities parsed from output
    activities: Vec<Activity>,
    /// Last parsed output position (to avoid re-parsing)
    last_activity_parse_pos: usize,
}

impl PtyState {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, 1000), // 1000 lines of scrollback
            child_exited: false,
            recent_output: String::new(),
            activities: Vec::new(),
            last_activity_parse_pos: 0,
        }
    }

    /// Append output and trim to last 10KB to prevent memory issues
    pub fn append_output(&mut self, data: &[u8]) {
        if let Ok(s) = std::str::from_utf8(data) {
            self.recent_output.push_str(s);
            // Keep only last 10KB to limit memory
            if self.recent_output.len() > 10 * 1024 {
                let target_start = self.recent_output.len() - 8 * 1024;
                // Find a valid UTF-8 character boundary using char_indices
                // char_indices always returns valid byte boundaries
                if let Some((start, _)) = self
                    .recent_output
                    .char_indices()
                    .find(|(i, _)| *i >= target_start)
                {
                    // Use safe get() to avoid any potential panic
                    if let Some(trimmed) = self.recent_output.get(start..) {
                        self.recent_output = trimmed.to_string();
                    }
                }
                // If we can't find a valid boundary, just clear (shouldn't happen)
            }
        }
    }

    /// Check if completion signal is present in recent output
    pub fn has_completion_signal(&self) -> bool {
        self.recent_output.contains("<promise>COMPLETE</promise>")
    }

    /// Check if stop hook fired (iteration complete message in output)
    /// This is used to detect when Claude's Stop hook runs with continue: false
    /// Since Claude doesn't exit, we detect the message instead
    /// We check for multiple possible patterns since ANSI codes may interfere
    pub fn has_stop_hook_signal(&self) -> bool {
        // Check raw output first (with ANSI stripping)
        let stripped = strip_ansi_codes(&self.recent_output);
        let stripped_lower = stripped.to_lowercase();

        if stripped_lower.contains("iteration complete")
            || stripped_lower.contains("ralph-tui will start next iteration")
            || stripped_lower.contains("ran 1 stop hook")
            || stripped_lower.contains("stop hook")
        {
            return true;
        }

        // Also check the VT100 screen content (rendered text)
        let screen = self.parser.screen();
        let (rows, _cols) = screen.size();
        for row in 0..rows {
            let row_text = screen.contents_between(row, 0, row, 200);
            let row_lower = row_text.to_lowercase();
            if row_lower.contains("stop hook") || row_lower.contains("iteration complete") {
                return true;
            }
        }

        false
    }

    /// Clear recent output (called when starting new iteration)
    pub fn clear_recent_output(&mut self) {
        self.recent_output.clear();
        self.activities.clear();
        self.last_activity_parse_pos = 0;
    }

    /// Parse activities from new output since last parse
    pub fn update_activities(&mut self) {
        if self.recent_output.len() <= self.last_activity_parse_pos {
            return;
        }

        // Parse only the new portion of output (safe slice access)
        let new_output = match self.recent_output.get(self.last_activity_parse_pos..) {
            Some(s) => s,
            None => {
                // Position is invalid (maybe string was trimmed), reset
                self.last_activity_parse_pos = 0;
                return;
            }
        };
        let new_activities = parse_activities(new_output);

        // Add new activities, avoiding duplicates
        for activity in new_activities {
            if !self.activities.iter().any(|a|
                a.action_type == activity.action_type && a.target == activity.target
            ) {
                self.activities.push(activity);
            }
        }

        // Keep only the last MAX_ACTIVITIES
        if self.activities.len() > MAX_ACTIVITIES {
            let remove_count = self.activities.len() - MAX_ACTIVITIES;
            self.activities.drain(0..remove_count);
        }

        self.last_activity_parse_pos = self.recent_output.len();
    }

    /// Get recent activities (newest first)
    pub fn get_activities(&self) -> Vec<Activity> {
        self.activities.iter().rev().cloned().collect()
    }

    /// Get recent output buffer for debugging
    pub fn recent_output(&self) -> &str {
        &self.recent_output
    }
}

/// Strip ANSI escape sequences from a string for reliable text matching
pub fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC and the following sequence
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    chars.next(); // consume '['
                    // Skip until we hit a letter (the terminator)
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch.is_ascii_alphabetic() {
                            break;
                        }
                    }
                } else if next == ']' {
                    // OSC sequence - skip until BEL or ST
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch == '\x07' || ch == '\\' {
                            break;
                        }
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_codes_empty() {
        assert_eq!(strip_ansi_codes(""), "");
    }

    #[test]
    fn test_strip_ansi_codes_no_codes() {
        assert_eq!(strip_ansi_codes("hello world"), "hello world");
    }

    #[test]
    fn test_strip_ansi_codes_color() {
        // Red text: ESC[31m ... ESC[0m
        assert_eq!(strip_ansi_codes("\x1b[31mred text\x1b[0m"), "red text");
    }

    #[test]
    fn test_strip_ansi_codes_bold() {
        // Bold: ESC[1m ... ESC[0m
        assert_eq!(strip_ansi_codes("\x1b[1mbold\x1b[0m"), "bold");
    }

    #[test]
    fn test_strip_ansi_codes_multiple() {
        // Multiple codes in sequence
        assert_eq!(
            strip_ansi_codes("\x1b[1m\x1b[32mgreen bold\x1b[0m normal"),
            "green bold normal"
        );
    }

    #[test]
    fn test_strip_ansi_codes_cursor_movement() {
        // Cursor position: ESC[row;colH
        assert_eq!(strip_ansi_codes("\x1b[10;5Htext"), "text");
    }

    #[test]
    fn test_strip_ansi_codes_mixed_content() {
        // Mixed ANSI and regular text
        let input = "prefix\x1b[31m red \x1b[0mmiddle\x1b[34m blue \x1b[0msuffix";
        assert_eq!(strip_ansi_codes(input), "prefix red middle blue suffix");
    }
}
