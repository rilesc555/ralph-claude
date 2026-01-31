//! CLI argument parsing and configuration.

use std::io;
use std::path::PathBuf;

use super::prompts::{find_active_tasks, prompt_iterations, prompt_rotation_threshold, prompt_task_selection};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Configuration from CLI arguments
pub struct CliConfig {
    pub task_dir: PathBuf,
    pub max_iterations: u32,
    pub rotate_threshold: u32,
    pub skip_prompts: bool,
}

/// Print usage information
pub fn print_usage() {
    eprintln!("Ralph TUI - Interactive terminal interface for Ralph agent");
    eprintln!();
    eprintln!("Usage: ralph-tui [task-directory] [OPTIONS]");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  [task-directory]  Path to the task directory containing prd.json");
    eprintln!("                    If omitted, prompts for task selection");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -i, --iterations <N>   Maximum iterations to run (default: 10)");
    eprintln!("  --rotate-at <N>        Rotate progress file at N lines (default: 300)");
    eprintln!("  -y, --yes              Skip confirmation prompts");
    eprintln!("  -h, --help             Show this help message");
    eprintln!("  -V, --version          Show version");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  ralph-tui                          # Interactive task selection");
    eprintln!("  ralph-tui tasks/my-feature         # Run specific task");
    eprintln!("  ralph-tui tasks/my-feature -i 5    # Run with 5 iterations");
}

/// Parse CLI arguments and return configuration
pub fn parse_args() -> io::Result<CliConfig> {
    let args: Vec<String> = std::env::args().collect();
    let mut task_dir: Option<PathBuf> = None;
    let mut max_iterations: Option<u32> = None;
    let mut rotate_threshold: u32 = 300;
    let mut skip_prompts = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-h" || arg == "--help" {
            print_usage();
            std::process::exit(0);
        } else if arg == "-V" || arg == "--version" {
            println!("ralph-tui {}", VERSION);
            std::process::exit(0);
        } else if arg == "-y" || arg == "--yes" {
            skip_prompts = true;
            i += 1;
        } else if arg == "-i" || arg == "--iterations" {
            i += 1;
            if i >= args.len() {
                print_usage();
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Missing value for --iterations",
                ));
            }
            max_iterations = Some(args[i].parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Invalid iterations value: {}", args[i]),
                )
            })?);
            i += 1;
        } else if arg == "--rotate-at" {
            i += 1;
            if i >= args.len() {
                print_usage();
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Missing value for --rotate-at",
                ));
            }
            rotate_threshold = args[i].parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Invalid rotate-at value: {}", args[i]),
                )
            })?;
            i += 1;
        } else if !arg.starts_with('-') {
            task_dir = Some(PathBuf::from(arg));
            i += 1;
        } else {
            print_usage();
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Unknown argument: {}", arg),
            ));
        }
    }

    // If no task directory provided, find and prompt
    let task_dir = if let Some(dir) = task_dir {
        dir
    } else {
        let tasks = find_active_tasks();
        if tasks.is_empty() {
            println!("No active tasks found.");
            println!();
            println!("To create a new task:");
            println!("  1. Use /prd to create a PRD in tasks/{{effort-name}}/");
            println!("  2. Use /ralph to convert it to prd.json");
            println!("  3. Run: ralph-tui tasks/{{effort-name}}");
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "No active tasks found",
            ));
        } else if tasks.len() == 1 {
            println!("Found one active task: {}", tasks[0].display());
            println!();
            tasks[0].clone()
        } else {
            prompt_task_selection(&tasks)?
        }
    };

    // Prompt for iterations if not provided and not skipping prompts
    let max_iterations = if let Some(iters) = max_iterations {
        iters
    } else if skip_prompts {
        10
    } else {
        prompt_iterations().unwrap_or(10)
    };

    // Check progress file for rotation threshold prompt
    let progress_path = task_dir.join("progress.txt");
    if progress_path.exists() && !skip_prompts {
        if let Ok(content) = std::fs::read_to_string(&progress_path) {
            let lines = content.lines().count();
            // Prompt if within 50 lines of threshold or has prior rotations
            let has_prior_rotation = task_dir.join("progress-1.txt").exists();
            if lines > rotate_threshold.saturating_sub(50) as usize || has_prior_rotation {
                rotate_threshold = prompt_rotation_threshold(rotate_threshold, lines)
                    .unwrap_or(rotate_threshold);
            }
        }
    }

    Ok(CliConfig {
        task_dir,
        max_iterations,
        rotate_threshold,
        skip_prompts,
    })
}
