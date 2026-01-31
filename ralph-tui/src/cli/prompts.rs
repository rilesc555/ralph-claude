//! User prompt functions for interactive CLI input.

use std::io::{self, Write};
use std::path::PathBuf;

/// Find active tasks (directories with prd.json, excluding archived)
pub fn find_active_tasks() -> Vec<PathBuf> {
    let tasks_dir = PathBuf::from("tasks");
    if !tasks_dir.exists() {
        return Vec::new();
    }

    let mut tasks = Vec::new();

    // Look for prd.json files in tasks/ subdirectories
    if let Ok(entries) = std::fs::read_dir(&tasks_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            // Skip archived directory
            if path.file_name().map_or(false, |n| n == "archived") {
                continue;
            }
            if path.is_dir() {
                let prd_path = path.join("prd.json");
                if prd_path.exists() {
                    tasks.push(path);
                }
            }
        }
    }

    tasks.sort();
    tasks
}

/// Get task info for display
pub fn get_task_info(task_dir: &PathBuf) -> (String, usize, usize, String) {
    let prd_path = task_dir.join("prd.json");
    let content = std::fs::read_to_string(&prd_path).unwrap_or_default();

    // Parse JSON to get info
    if let Ok(prd) = serde_json::from_str::<serde_json::Value>(&content) {
        let description = prd.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("No description")
            .chars()
            .take(50)
            .collect::<String>();

        let stories = prd.get("userStories")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0);

        let completed = prd.get("userStories")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter(|s| {
                s.get("passes").and_then(|v| v.as_bool()).unwrap_or(false)
            }).count())
            .unwrap_or(0);

        let prd_type = prd.get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("feature")
            .to_string();

        (description, completed, stories, prd_type)
    } else {
        ("Unable to parse prd.json".to_string(), 0, 0, "unknown".to_string())
    }
}

/// Display task selection prompt and return selected task
pub fn prompt_task_selection(tasks: &[PathBuf]) -> io::Result<PathBuf> {
    println!();
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║  Ralph TUI - Select a Task                                    ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    println!("Active tasks:");
    println!();

    for (i, task) in tasks.iter().enumerate() {
        let (desc, completed, total, prd_type) = get_task_info(task);
        let task_name = task.display().to_string();
        println!(
            "  {}) {:35} [{}/{}] ({})",
            i + 1,
            task_name,
            completed,
            total,
            prd_type
        );
        if !desc.is_empty() {
            println!("     {}", desc);
        }
    }

    println!();
    print!("Select task [1-{}]: ", tasks.len());
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let selection: usize = input.trim().parse().map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "Invalid selection")
    })?;

    if selection < 1 || selection > tasks.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Selection out of range",
        ));
    }

    println!();
    println!("Selected: {}", tasks[selection - 1].display());
    println!();

    Ok(tasks[selection - 1].clone())
}

/// Prompt for iterations if not provided
pub fn prompt_iterations() -> io::Result<u32> {
    print!("Max iterations [10]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim();
    if input.is_empty() {
        return Ok(10);
    }

    input.parse().map_err(|_| {
        eprintln!("Invalid number. Using default of 10.");
        io::Error::new(io::ErrorKind::InvalidInput, "Invalid number")
    }).or(Ok(10))
}

/// Prompt for rotation threshold
pub fn prompt_rotation_threshold(current: u32, progress_lines: usize) -> io::Result<u32> {
    println!();
    println!("Progress file has {} lines (rotation threshold: {})", progress_lines, current);
    print!("Rotation threshold [{}]: ", current);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim();
    if input.is_empty() {
        return Ok(current);
    }

    input.parse().map_err(|_| {
        eprintln!("Invalid number. Using default of {}.", current);
        io::Error::new(io::ErrorKind::InvalidInput, "Invalid number")
    }).or(Ok(current))
}
