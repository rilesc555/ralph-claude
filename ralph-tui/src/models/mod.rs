//! Data models for Ralph TUI
//!
//! This module contains the core data structures:
//! - PRD and user story types for loading prd.json
//! - Activity tracking for monitoring Claude output
//! - Enums for state management

pub mod activity;
pub mod enums;
pub mod prd;

// Re-exports for convenient access
pub use activity::{parse_activities, Activity, MAX_ACTIVITIES};
pub use enums::{IterationState, Mode, RalphViewMode, StorySortMode, StoryState};
pub use prd::Prd;
