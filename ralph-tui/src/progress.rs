//! Progress file rotation module.
//!
//! Handles automatic rotation of progress.txt files when they exceed a
//! configured line threshold. Preserves the header and "Codebase Patterns"
//! sections in the new file.

use std::fs;
use std::io;
use std::path::Path;

/// Configuration for progress file rotation.
#[derive(Debug, Clone)]
pub struct RotationConfig {
    /// Line threshold that triggers rotation.
    pub threshold: u32,
    /// Maximum number of archive files to keep (progress-1.txt through progress-N.txt).
    pub max_archives: u32,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            threshold: 300,
            max_archives: 5,
        }
    }
}

/// Result of a rotation check.
#[derive(Debug, PartialEq)]
pub enum RotationResult {
    /// File doesn't exist or is below threshold - no rotation needed.
    NotNeeded,
    /// File was successfully rotated.
    Rotated {
        /// Number of lines in the original file.
        original_lines: usize,
        /// Path to the new archive file (e.g., progress-1.txt).
        archive_path: String,
    },
    /// An error occurred during rotation.
    Error(String),
}

/// Check if progress.txt needs rotation and perform it if necessary.
///
/// This is the main entry point for the rotation logic.
///
/// # Arguments
/// * `task_dir` - Path to the task directory containing progress.txt
/// * `config` - Rotation configuration (threshold and max archives)
///
/// # Returns
/// A `RotationResult` indicating what happened.
pub fn check_and_rotate_progress(task_dir: &Path, config: &RotationConfig) -> RotationResult {
    let progress_path = task_dir.join("progress.txt");

    // Check if file exists
    if !progress_path.exists() {
        return RotationResult::NotNeeded;
    }

    // Read the file content
    let content = match fs::read_to_string(&progress_path) {
        Ok(c) => c,
        Err(e) => return RotationResult::Error(format!("Failed to read progress.txt: {}", e)),
    };

    // Count lines
    let line_count = content.lines().count();

    // Check if rotation is needed
    if line_count < config.threshold as usize {
        return RotationResult::NotNeeded;
    }

    // Perform rotation
    match rotate_progress_files(task_dir, config.max_archives) {
        Ok(archive_path) => {
            // Extract preserved content and write new file
            let preserved = extract_preserved_content(&content);
            match fs::write(&progress_path, preserved) {
                Ok(_) => RotationResult::Rotated {
                    original_lines: line_count,
                    archive_path,
                },
                Err(e) => RotationResult::Error(format!("Failed to write new progress.txt: {}", e)),
            }
        }
        Err(e) => RotationResult::Error(format!("Failed to rotate files: {}", e)),
    }
}

/// Rotate existing archive files and move progress.txt to progress-1.txt.
///
/// Algorithm:
/// 1. Delete progress-N.txt if it exists (where N = max_archives)
/// 2. Shift existing archives up: progress-4 -> progress-5, etc.
/// 3. Rename progress.txt -> progress-1.txt
///
/// # Returns
/// The path to the new archive file (progress-1.txt) on success.
fn rotate_progress_files(task_dir: &Path, max_archives: u32) -> io::Result<String> {
    // Delete the oldest archive if it exists
    let oldest_path = task_dir.join(format!("progress-{}.txt", max_archives));
    if oldest_path.exists() {
        fs::remove_file(&oldest_path)?;
    }

    // Shift existing archives up (from max-1 down to 1)
    for i in (1..max_archives).rev() {
        let current = task_dir.join(format!("progress-{}.txt", i));
        let next = task_dir.join(format!("progress-{}.txt", i + 1));
        if current.exists() {
            fs::rename(&current, &next)?;
        }
    }

    // Move current progress.txt to progress-1.txt
    let progress_path = task_dir.join("progress.txt");
    let archive_path = task_dir.join("progress-1.txt");
    fs::rename(&progress_path, &archive_path)?;

    Ok(archive_path.to_string_lossy().to_string())
}

/// Extract content to preserve in the new progress.txt.
///
/// Preserves:
/// - Header section (everything up to and including first `---`)
/// - "Codebase Patterns" section (from `## Codebase Patterns` to next `---`)
///
/// # Returns
/// A string containing the preserved content with proper formatting.
fn extract_preserved_content(content: &str) -> String {
    let mut result = String::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    // Phase 1: Copy header (everything up to and including first ---)
    while i < lines.len() {
        let line = lines[i];
        result.push_str(line);
        result.push('\n');
        if line == "---" {
            i += 1;
            break;
        }
        i += 1;
    }

    // Phase 2: Find and copy Codebase Patterns section
    let mut found_patterns = false;
    for j in i..lines.len() {
        let line = lines[j];
        if line.starts_with("## Codebase Patterns") {
            found_patterns = true;
            result.push('\n'); // Blank line before section
            // Copy until next ---
            let mut k = j;
            while k < lines.len() {
                let pattern_line = lines[k];
                result.push_str(pattern_line);
                result.push('\n');
                if pattern_line == "---" {
                    break;
                }
                k += 1;
            }
            break;
        }
    }

    // If no patterns section found, add placeholder
    if !found_patterns {
        result.push_str("\n## Codebase Patterns\n\n---\n");
    }

    // Add trailing newline for new entries
    result.push('\n');

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_rotation_config_default() {
        let config = RotationConfig::default();
        assert_eq!(config.threshold, 300);
        assert_eq!(config.max_archives, 5);
    }

    #[test]
    fn test_extract_preserved_content_basic() {
        let content = r#"# Ralph Progress Log
Effort: test-effort
Type: Feature
Started: 2026-01-31
---

## Codebase Patterns
- Pattern 1
- Pattern 2

---

## 2026-01-31 - US-001
- Did some work
"#;

        let preserved = extract_preserved_content(content);
        assert!(preserved.contains("# Ralph Progress Log"));
        assert!(preserved.contains("Effort: test-effort"));
        assert!(preserved.contains("## Codebase Patterns"));
        assert!(preserved.contains("- Pattern 1"));
        assert!(!preserved.contains("## 2026-01-31 - US-001"));
        assert!(!preserved.contains("- Did some work"));
    }

    #[test]
    fn test_extract_preserved_content_no_patterns() {
        let content = r#"# Ralph Progress Log
Effort: test-effort
Type: Feature
Started: 2026-01-31
---

## 2026-01-31 - US-001
- Did some work
"#;

        let preserved = extract_preserved_content(content);
        assert!(preserved.contains("# Ralph Progress Log"));
        // Should add placeholder Codebase Patterns section
        assert!(preserved.contains("## Codebase Patterns"));
    }

    #[test]
    fn test_check_and_rotate_no_file() {
        let dir = tempdir().unwrap();
        let config = RotationConfig::default();

        let result = check_and_rotate_progress(dir.path(), &config);
        assert_eq!(result, RotationResult::NotNeeded);
    }

    #[test]
    fn test_check_and_rotate_below_threshold() {
        let dir = tempdir().unwrap();
        let config = RotationConfig {
            threshold: 100,
            max_archives: 5,
        };

        // Create a small progress file
        let content = "# Header\n---\n## Entry\n- Line 1\n";
        fs::write(dir.path().join("progress.txt"), content).unwrap();

        let result = check_and_rotate_progress(dir.path(), &config);
        assert_eq!(result, RotationResult::NotNeeded);
    }

    #[test]
    fn test_check_and_rotate_above_threshold() {
        let dir = tempdir().unwrap();
        let config = RotationConfig {
            threshold: 10,
            max_archives: 5,
        };

        // Create a progress file with more than 10 lines
        let mut content = String::from("# Ralph Progress Log\nEffort: test\nType: Feature\nStarted: 2026-01-31\n---\n\n## Codebase Patterns\n- Pattern\n\n---\n\n");
        for i in 0..20 {
            content.push_str(&format!("Line {}\n", i));
        }
        fs::write(dir.path().join("progress.txt"), &content).unwrap();

        let result = check_and_rotate_progress(dir.path(), &config);

        match result {
            RotationResult::Rotated { original_lines, archive_path } => {
                assert!(original_lines > 10);
                assert!(archive_path.contains("progress-1.txt"));
                // Verify archive was created
                assert!(dir.path().join("progress-1.txt").exists());
                // Verify new progress.txt exists and has preserved content
                let new_content = fs::read_to_string(dir.path().join("progress.txt")).unwrap();
                assert!(new_content.contains("# Ralph Progress Log"));
                assert!(new_content.contains("## Codebase Patterns"));
            }
            _ => panic!("Expected Rotated result, got {:?}", result),
        }
    }

    #[test]
    fn test_rotate_shifts_existing_archives() {
        let dir = tempdir().unwrap();
        let config = RotationConfig {
            threshold: 5,
            max_archives: 3,
        };

        // Create existing archives
        fs::write(dir.path().join("progress-1.txt"), "Archive 1").unwrap();
        fs::write(dir.path().join("progress-2.txt"), "Archive 2").unwrap();

        // Create progress file above threshold
        let content = "# Header\n---\n## Codebase Patterns\n---\nLine1\nLine2\nLine3\nLine4\nLine5\nLine6\n";
        fs::write(dir.path().join("progress.txt"), content).unwrap();

        let result = check_and_rotate_progress(dir.path(), &config);
        assert!(matches!(result, RotationResult::Rotated { .. }));

        // Verify shifts
        assert!(dir.path().join("progress-1.txt").exists());
        assert!(dir.path().join("progress-2.txt").exists());
        assert!(dir.path().join("progress-3.txt").exists());

        // Verify content shifted correctly
        let archive2 = fs::read_to_string(dir.path().join("progress-2.txt")).unwrap();
        assert_eq!(archive2, "Archive 1");
        let archive3 = fs::read_to_string(dir.path().join("progress-3.txt")).unwrap();
        assert_eq!(archive3, "Archive 2");
    }

    #[test]
    fn test_rotate_deletes_oldest_archive() {
        let dir = tempdir().unwrap();
        let config = RotationConfig {
            threshold: 5,
            max_archives: 2,
        };

        // Create archives at max capacity
        fs::write(dir.path().join("progress-1.txt"), "Archive 1").unwrap();
        fs::write(dir.path().join("progress-2.txt"), "Archive 2 - oldest").unwrap();

        // Create progress file above threshold
        let content = "# Header\n---\n## Codebase Patterns\n---\nLine1\nLine2\nLine3\nLine4\nLine5\nLine6\n";
        fs::write(dir.path().join("progress.txt"), content).unwrap();

        check_and_rotate_progress(dir.path(), &config);

        // Archive 2 should now contain what was Archive 1
        let archive2 = fs::read_to_string(dir.path().join("progress-2.txt")).unwrap();
        assert_eq!(archive2, "Archive 1");
        // Original Archive 2 content should be gone (deleted as oldest)
    }
}
