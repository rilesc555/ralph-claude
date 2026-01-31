//! Activity tracking from Claude Code output
//!
//! This module parses tool call patterns from Claude output to show
//! recent activities in the UI.

/// Maximum number of activities to track
pub const MAX_ACTIVITIES: usize = 10;

/// Recent activity from Claude Code (tool calls, actions)
#[derive(Debug, Clone)]
pub struct Activity {
    pub action_type: String,
    pub target: String,
}

impl Activity {
    pub fn new(action_type: &str, target: &str) -> Self {
        Self {
            action_type: action_type.to_string(),
            target: target.to_string(),
        }
    }

    /// Format for display (truncate target if too long)
    pub fn format(&self, max_width: usize) -> String {
        let prefix = format!("{}: ", self.action_type);
        let available = max_width.saturating_sub(prefix.len());
        let char_count = self.target.chars().count();
        let target = if char_count > available {
            // Safely truncate from the end using character boundaries
            let skip_chars = char_count.saturating_sub(available.saturating_sub(3));
            let truncated: String = self.target.chars().skip(skip_chars).collect();
            format!("...{}", truncated)
        } else {
            self.target.clone()
        };
        format!("{}{}", prefix, target)
    }
}

/// Parse activities from Claude output
/// Looks for tool call patterns in the output
pub fn parse_activities(text: &str) -> Vec<Activity> {
    let mut activities = Vec::new();

    // Patterns to look for (case-insensitive matching in output)
    // Claude Code typically shows tool usage in various formats
    let patterns: &[(&str, &[&str])] = &[
        ("Read", &["reading ", "read file", "read("]),
        ("Edit", &["editing ", "edit file", "edit("]),
        ("Write", &["writing ", "write file", "write("]),
        ("Bash", &["running ", "$ ", "bash(", "executing "]),
        ("Grep", &["searching ", "grep(", "grep for"]),
        ("Glob", &["finding files", "glob(", "globbing"]),
        ("TodoWrite", &["updating todos", "todowrite(", "adding todo"]),
    ];

    for line in text.lines() {
        let line_lower = line.to_lowercase();

        for (action_type, prefixes) in patterns {
            for prefix in *prefixes {
                if let Some(pos) = line_lower.find(prefix) {
                    // Extract target (rest of line after pattern, cleaned up)
                    // Use get() to safely handle potential UTF-8 boundary issues
                    let start_idx = pos + prefix.len();
                    let after = match line.get(start_idx..) {
                        Some(s) => s,
                        None => continue, // Skip if index is invalid
                    };
                    let target = after
                        .trim()
                        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`')
                        .split(|c: char| c == '\n' || c == '\r')
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(100)  // Limit target length
                        .collect::<String>();

                    if !target.is_empty() {
                        let activity = Activity::new(action_type, &target);
                        // Avoid duplicates
                        if !activities.iter().any(|a: &Activity|
                            a.action_type == activity.action_type && a.target == activity.target
                        ) {
                            activities.push(activity);
                        }
                    }
                    break;  // Only match first pattern per line
                }
            }
        }
    }

    activities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_activities_read() {
        let text = "Reading src/main.rs\nEditing src/lib.rs";
        let activities = parse_activities(text);
        assert_eq!(activities.len(), 2);
        assert_eq!(activities[0].action_type, "Read");
        assert_eq!(activities[0].target, "src/main.rs");
        assert_eq!(activities[1].action_type, "Edit");
        assert_eq!(activities[1].target, "src/lib.rs");
    }

    #[test]
    fn test_activity_format_truncation() {
        let activity = Activity::new("Read", "very/long/path/to/some/file.rs");
        let formatted = activity.format(20);
        assert!(formatted.len() <= 23); // "..." adds 3 chars
        assert!(formatted.starts_with("Read: "));
    }

    #[test]
    fn test_no_duplicates() {
        let text = "Reading file.rs\nReading file.rs";
        let activities = parse_activities(text);
        assert_eq!(activities.len(), 1);
    }
}
