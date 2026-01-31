//! Enums used throughout the Ralph TUI
//!
//! This module contains the various enum types used for state management
//! and UI rendering.

/// Mode for modal input system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ralph,  // Default mode - focus on left panel
    Claude, // Claude mode - focus on right panel, forward input to PTY
}

/// View mode for Ralph terminal panel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RalphViewMode {
    #[default]
    Normal,       // Default: show minimal ralph output or ASCII logo
    StoryDetails, // Show selected story details from prd.json
    Progress,     // Show progress.txt entries for selected story
    Requirements, // Show requirements from prd.md for selected story
}

/// Iteration state for tracking progress across Claude restarts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IterationState {
    Running,            // Claude is currently running
    Completed,          // All stories complete (<promise>COMPLETE</promise> found)
    NeedsRestart,       // Iteration finished but more work remains
    WaitingDelay,       // Waiting before starting next iteration
    WaitingUserConfirm, // Waiting for user to press Enter to continue (pause mode)
}

/// Sort mode for user stories in the sidebar
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StorySortMode {
    #[default]
    Priority, // Sort by priority field (original behavior)
    Id,       // Sort by numeric ID (e.g., US-018 -> 18)
}

impl StorySortMode {
    pub fn toggle(&self) -> Self {
        match self {
            StorySortMode::Priority => StorySortMode::Id,
            StorySortMode::Id => StorySortMode::Priority,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            StorySortMode::Priority => "Priority",
            StorySortMode::Id => "ID",
        }
    }
}

/// Story state for rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryState {
    Completed,
    Active,
    Pending,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_story_sort_mode_toggle() {
        assert_eq!(StorySortMode::Priority.toggle(), StorySortMode::Id);
        assert_eq!(StorySortMode::Id.toggle(), StorySortMode::Priority);
    }

    #[test]
    fn test_story_sort_mode_label() {
        assert_eq!(StorySortMode::Priority.label(), "Priority");
        assert_eq!(StorySortMode::Id.label(), "ID");
    }

    #[test]
    fn test_story_sort_mode_default() {
        assert_eq!(StorySortMode::default(), StorySortMode::Priority);
    }

    #[test]
    fn test_ralph_view_mode_default() {
        assert_eq!(RalphViewMode::default(), RalphViewMode::Normal);
    }
}
