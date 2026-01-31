//! PRD (Product Requirements Document) data structures
//!
//! This module contains the core data structures for loading and working with
//! ralph prd.json files.

use serde::Deserialize;
use std::io;
use std::path::PathBuf;

/// Acceptance criterion - supports both v1.0 (string) and v2.0 (object) schemas
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptanceCriterion {
    pub description: String,
    pub passes: bool,
}

// Custom deserializer to handle both string (v1.0) and object (v2.0) formats
impl<'de> serde::Deserialize<'de> for AcceptanceCriterion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};

        struct AcceptanceCriterionVisitor;

        impl<'de> Visitor<'de> for AcceptanceCriterionVisitor {
            type Value = AcceptanceCriterion;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or an object with description and passes fields")
            }

            // v1.0 schema: plain string (treated as passes: false)
            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(AcceptanceCriterion {
                    description: value.to_string(),
                    passes: false,
                })
            }

            // v2.0 schema: object with description and passes
            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut description: Option<String> = None;
                let mut passes: Option<bool> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "description" => {
                            description = Some(map.next_value()?);
                        }
                        "passes" => {
                            passes = Some(map.next_value()?);
                        }
                        _ => {
                            let _: serde::de::IgnoredAny = map.next_value()?;
                        }
                    }
                }

                Ok(AcceptanceCriterion {
                    description: description.unwrap_or_default(),
                    passes: passes.unwrap_or(false),
                })
            }
        }

        deserializer.deserialize_any(AcceptanceCriterionVisitor)
    }
}

/// PRD user story
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserStory {
    pub id: String,
    pub title: String,
    #[allow(dead_code)]
    pub description: String,
    #[allow(dead_code)]
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub priority: u32,
    pub passes: bool,
    #[allow(dead_code)]
    pub notes: String,
}

/// Default schema version for backwards compatibility
fn default_schema_version() -> String {
    "1.0".to_string()
}

/// PRD document structure
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prd {
    /// Schema version for format compatibility (default: "1.0")
    #[allow(dead_code)]
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    #[allow(dead_code)]
    pub project: String,
    #[allow(dead_code)]
    pub task_dir: String,
    /// Branch name for this effort (null = don't create branch, work in existing repos)
    #[serde(default)]
    pub branch_name: Option<String>,
    /// Target branch to merge into when complete (null = no merge)
    #[allow(dead_code)]
    #[serde(default)]
    pub merge_target: Option<String>,
    /// Whether to auto-merge on completion (default: false)
    #[allow(dead_code)]
    #[serde(default)]
    pub auto_merge: bool,
    /// Whether to pause for user confirmation between stories/iterations (default: false)
    #[serde(default)]
    pub pause_between_stories: bool,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    pub prd_type: String,
    pub description: String,
    pub user_stories: Vec<UserStory>,
}

impl Prd {
    /// Load PRD from a JSON file
    pub fn load(path: &PathBuf) -> io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Check if schema needs migration and perform it interactively
    /// Returns Ok(true) if migration was performed, Ok(false) if no migration needed
    pub fn check_and_migrate_schema(path: &PathBuf) -> io::Result<bool> {
        use std::io::{BufRead, Write as IoWrite};

        let content = std::fs::read_to_string(path)?;
        let mut json: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let obj = json.as_object_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "prd.json is not a JSON object")
        })?;

        // Check current schema version
        let current_version = obj
            .get("schemaVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0");

        // Parse version numbers for comparison
        let needs_migration = match current_version {
            "2.2" => false,
            "2.1" | "2.0" | "1.0" | _ => true,
        };

        if !needs_migration {
            return Ok(false);
        }

        // Schema needs migration - prompt user
        println!();
        println!("╔═══════════════════════════════════════════════════════════════╗");
        println!("║  PRD Schema Migration Available                               ║");
        println!("╚═══════════════════════════════════════════════════════════════╝");
        println!();
        println!("  Your prd.json uses schema version: {}", current_version);
        println!("  Latest schema version: 2.2");
        println!();
        println!("  New in 2.2:");
        println!("    • pauseBetweenStories - Pause for confirmation between stories");
        println!();

        // Ask if user wants to migrate
        print!("  Would you like to upgrade to schema 2.2? [Y/n]: ");
        std::io::stdout().flush()?;

        let stdin = std::io::stdin();
        let mut migrate_input = String::new();
        stdin.lock().read_line(&mut migrate_input)?;
        let migrate_answer = migrate_input.trim().to_lowercase();

        if migrate_answer == "n" || migrate_answer == "no" {
            println!("  Skipping migration. Using existing schema.");
            println!();
            return Ok(false);
        }

        // Ask about pauseBetweenStories preference
        println!();
        println!("  Configure pause between stories:");
        println!("    When enabled, Ralph will keep the Claude session open after");
        println!("    each story completes so you can continue chatting.");
        println!("    Type 'exit' in the Claude session to proceed to the next story.");
        println!();
        print!("  Pause between stories? [y/N]: ");
        std::io::stdout().flush()?;

        let mut pause_input = String::new();
        stdin.lock().read_line(&mut pause_input)?;
        let pause_answer = pause_input.trim().to_lowercase();
        let pause_between_stories = pause_answer == "y" || pause_answer == "yes";

        // Update the JSON
        obj.insert("schemaVersion".to_string(), serde_json::Value::String("2.2".to_string()));
        obj.insert("pauseBetweenStories".to_string(), serde_json::Value::Bool(pause_between_stories));

        // Write back with pretty formatting
        let updated_content = serde_json::to_string_pretty(&json)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        std::fs::write(path, updated_content)?;

        println!();
        println!("  ✓ Migrated to schema 2.2");
        println!("  ✓ pauseBetweenStories: {}", pause_between_stories);
        println!();

        Ok(true)
    }

    /// Count completed stories
    pub fn completed_count(&self) -> usize {
        self.user_stories.iter().filter(|s| s.passes).count()
    }

    /// Check if all stories pass (project complete)
    pub fn all_stories_pass(&self) -> bool {
        !self.user_stories.is_empty() && self.user_stories.iter().all(|s| s.passes)
    }

    /// Get current story (first with passes: false, sorted by priority)
    pub fn current_story(&self) -> Option<&UserStory> {
        self.user_stories
            .iter()
            .filter(|s| !s.passes)
            .min_by_key(|s| s.priority)
    }

    /// Calculate progress as percentage based on per-criteria completion
    /// This gives more granular progress than story-level tracking
    #[allow(dead_code)]
    pub fn criteria_progress(&self) -> f64 {
        let total: usize = self.user_stories.iter()
            .map(|s| s.acceptance_criteria.len())
            .sum();
        if total == 0 {
            return 0.0;
        }
        let passed: usize = self.user_stories.iter()
            .flat_map(|s| &s.acceptance_criteria)
            .filter(|c| c.passes)
            .count();
        (passed as f64 / total as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_acceptance_criterion_from_string() {
        let json = r#""Some criterion""#;
        let criterion: AcceptanceCriterion = serde_json::from_str(json).unwrap();
        assert_eq!(criterion.description, "Some criterion");
        assert!(!criterion.passes);
    }

    #[test]
    fn test_acceptance_criterion_from_object() {
        let json = r#"{"description": "Some criterion", "passes": true}"#;
        let criterion: AcceptanceCriterion = serde_json::from_str(json).unwrap();
        assert_eq!(criterion.description, "Some criterion");
        assert!(criterion.passes);
    }

    fn create_temp_prd_file(content: &str) -> (tempfile::NamedTempFile, PathBuf) {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        let path = file.path().to_path_buf();
        (file, path)
    }

    #[test]
    fn test_prd_load_success() {
        let json = r#"{
            "project": "test-project",
            "taskDir": "tasks/test",
            "branchName": "test-branch",
            "type": "feature",
            "description": "Test description",
            "userStories": [
                {
                    "id": "US-001",
                    "title": "Test Story",
                    "description": "Story description",
                    "acceptanceCriteria": [
                        {"description": "Criterion 1", "passes": true}
                    ],
                    "priority": 1,
                    "passes": false,
                    "notes": ""
                }
            ]
        }"#;
        let (_file, path) = create_temp_prd_file(json);

        let prd = Prd::load(&path).unwrap();
        assert_eq!(prd.project, "test-project");
        assert_eq!(prd.task_dir, "tasks/test");
        assert_eq!(prd.branch_name, Some("test-branch".to_string()));
        assert_eq!(prd.description, "Test description");
        assert_eq!(prd.user_stories.len(), 1);
        assert_eq!(prd.user_stories[0].id, "US-001");
        assert_eq!(prd.user_stories[0].title, "Test Story");
    }

    #[test]
    fn test_prd_load_with_v1_schema() {
        // v1.0 schema: acceptanceCriteria as strings
        let json = r#"{
            "project": "test-project",
            "taskDir": "tasks/test",
            "type": "feature",
            "description": "Test description",
            "userStories": [
                {
                    "id": "US-001",
                    "title": "Test Story",
                    "description": "Story description",
                    "acceptanceCriteria": ["Criterion 1", "Criterion 2"],
                    "priority": 1,
                    "passes": false,
                    "notes": ""
                }
            ]
        }"#;
        let (_file, path) = create_temp_prd_file(json);

        let prd = Prd::load(&path).unwrap();
        assert_eq!(prd.user_stories[0].acceptance_criteria.len(), 2);
        assert_eq!(prd.user_stories[0].acceptance_criteria[0].description, "Criterion 1");
        assert!(!prd.user_stories[0].acceptance_criteria[0].passes);
    }

    #[test]
    fn test_prd_load_file_not_found() {
        let path = PathBuf::from("/nonexistent/path/prd.json");
        let result = Prd::load(&path);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn test_prd_load_invalid_json() {
        let (_file, path) = create_temp_prd_file("{ invalid json }");

        let result = Prd::load(&path);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_prd_load_missing_required_field() {
        // Missing 'project' field
        let json = r#"{
            "taskDir": "tasks/test",
            "type": "feature",
            "description": "Test description",
            "userStories": []
        }"#;
        let (_file, path) = create_temp_prd_file(json);

        let result = Prd::load(&path);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_prd_completed_count() {
        let json = r#"{
            "project": "test",
            "taskDir": "tasks/test",
            "type": "feature",
            "description": "Test",
            "userStories": [
                {"id": "US-001", "title": "Story 1", "description": "", "acceptanceCriteria": [], "priority": 1, "passes": true, "notes": ""},
                {"id": "US-002", "title": "Story 2", "description": "", "acceptanceCriteria": [], "priority": 2, "passes": false, "notes": ""},
                {"id": "US-003", "title": "Story 3", "description": "", "acceptanceCriteria": [], "priority": 3, "passes": true, "notes": ""}
            ]
        }"#;
        let (_file, path) = create_temp_prd_file(json);

        let prd = Prd::load(&path).unwrap();
        assert_eq!(prd.completed_count(), 2);
    }

    #[test]
    fn test_prd_all_stories_pass() {
        // All passing
        let json = r#"{
            "project": "test",
            "taskDir": "tasks/test",
            "type": "feature",
            "description": "Test",
            "userStories": [
                {"id": "US-001", "title": "Story 1", "description": "", "acceptanceCriteria": [], "priority": 1, "passes": true, "notes": ""}
            ]
        }"#;
        let (_file, path) = create_temp_prd_file(json);
        let prd = Prd::load(&path).unwrap();
        assert!(prd.all_stories_pass());

        // Not all passing
        let json2 = r#"{
            "project": "test",
            "taskDir": "tasks/test",
            "type": "feature",
            "description": "Test",
            "userStories": [
                {"id": "US-001", "title": "Story 1", "description": "", "acceptanceCriteria": [], "priority": 1, "passes": true, "notes": ""},
                {"id": "US-002", "title": "Story 2", "description": "", "acceptanceCriteria": [], "priority": 2, "passes": false, "notes": ""}
            ]
        }"#;
        let (_file2, path2) = create_temp_prd_file(json2);
        let prd2 = Prd::load(&path2).unwrap();
        assert!(!prd2.all_stories_pass());
    }

    #[test]
    fn test_prd_current_story() {
        let json = r#"{
            "project": "test",
            "taskDir": "tasks/test",
            "type": "feature",
            "description": "Test",
            "userStories": [
                {"id": "US-001", "title": "Story 1", "description": "", "acceptanceCriteria": [], "priority": 3, "passes": false, "notes": ""},
                {"id": "US-002", "title": "Story 2", "description": "", "acceptanceCriteria": [], "priority": 1, "passes": false, "notes": ""},
                {"id": "US-003", "title": "Story 3", "description": "", "acceptanceCriteria": [], "priority": 2, "passes": true, "notes": ""}
            ]
        }"#;
        let (_file, path) = create_temp_prd_file(json);

        let prd = Prd::load(&path).unwrap();
        let current = prd.current_story().unwrap();
        // Should pick US-002 (lowest priority number that hasn't passed)
        assert_eq!(current.id, "US-002");
    }

    #[test]
    fn test_prd_criteria_progress() {
        let json = r#"{
            "project": "test",
            "taskDir": "tasks/test",
            "type": "feature",
            "description": "Test",
            "userStories": [
                {
                    "id": "US-001",
                    "title": "Story 1",
                    "description": "",
                    "acceptanceCriteria": [
                        {"description": "C1", "passes": true},
                        {"description": "C2", "passes": true}
                    ],
                    "priority": 1,
                    "passes": false,
                    "notes": ""
                },
                {
                    "id": "US-002",
                    "title": "Story 2",
                    "description": "",
                    "acceptanceCriteria": [
                        {"description": "C3", "passes": false},
                        {"description": "C4", "passes": false}
                    ],
                    "priority": 2,
                    "passes": false,
                    "notes": ""
                }
            ]
        }"#;
        let (_file, path) = create_temp_prd_file(json);

        let prd = Prd::load(&path).unwrap();
        // 2 out of 4 criteria pass = 50%
        assert!((prd.criteria_progress() - 50.0).abs() < 0.001);
    }
}
