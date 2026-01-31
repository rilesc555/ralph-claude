//! File watching and prompt building utilities.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

/// Embedded default prompt.md as fallback
const EMBEDDED_PROMPT: &str = include_str!("../../prompt.md");

/// Find prompt.md in order of priority:
/// 1. ./ralph/prompt.md (local project customization)
/// 2. ~/.config/ralph/prompt.md (global user config)
/// 3. Embedded fallback (with warning)
pub fn find_prompt_content() -> (String, Option<String>) {
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

/// Build the Ralph prompt from task directory and prompt.md
/// Returns the full prompt string to be piped to Claude Code stdin
pub fn build_ralph_prompt(task_dir: &PathBuf) -> io::Result<String> {
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

/// Set up a file watcher for prd.json changes
pub fn setup_prd_watcher(
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
