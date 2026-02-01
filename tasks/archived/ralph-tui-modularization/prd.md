# PRD: Ralph-TUI Modularization

## Type
Feature

## Introduction

Refactor the monolithic `ralph-tui/src/main.rs` (~3,400 lines, 38K tokens) into a well-organized module structure. The current single-file architecture causes:
- Context window limitations for AI-assisted development (can't read entire file at once)
- Difficulty navigating and understanding code relationships
- Harder testing of individual components
- Poor separation of concerns

This refactor will split the code into logical modules while preserving all existing functionality and allowing minor improvements where beneficial.

## Goals

- Split main.rs into focused, single-responsibility modules
- Keep each module under 500 lines (fits comfortably in AI context)
- Enable unit testing of extracted logic
- Improve code organization following Rust idioms
- Preserve all existing functionality
- Allow minor cleanups and pattern improvements during refactor

## Target Module Structure

```
src/
├── main.rs           # Entry point only (~50 lines)
├── app.rs            # App struct and core state (~200 lines)
├── models/
│   ├── mod.rs        # Re-exports
│   ├── prd.rs        # Prd, UserStory, AcceptanceCriterion
│   ├── activity.rs   # Activity struct and parsing
│   └── enums.rs      # Mode, RalphViewMode, IterationState, StorySortMode, StoryState
├── pty/
│   ├── mod.rs        # Re-exports
│   ├── state.rs      # PtyState struct and impl
│   ├── spawn.rs      # spawn_claude function
│   └── input.rs      # forward_key_to_pty
├── ui/
│   ├── mod.rs        # Re-exports
│   ├── render.rs     # Main render logic, vt100_to_ratatui_color
│   ├── stats.rs      # render_stat_cards
│   ├── stories.rs    # render_story_card, render_progress_cards
│   └── helpers.rs    # wrap_text
├── cli/
│   ├── mod.rs        # Re-exports
│   ├── args.rs       # parse_args, CliConfig, print_usage
│   └── prompts.rs    # prompt_task_selection, prompt_iterations, prompt_rotation_threshold
├── watcher.rs        # setup_prd_watcher, find_prompt_content, build_ralph_prompt
├── utils.rs          # format_duration, strip_ansi_codes, find_active_tasks, get_task_info
├── run.rs            # run() and run_delay() functions (main event loop)
└── theme.rs          # (already exists)
```

## User Stories

### US-001: Extract models module
**Description:** As a developer, I want PRD-related structs in their own module so I can understand and modify the data model independently.

**Acceptance Criteria:**
- [ ] Create `src/models/mod.rs` with re-exports
- [ ] Create `src/models/prd.rs` with `Prd`, `UserStory`, `AcceptanceCriterion` structs and their impls
- [ ] Create `src/models/activity.rs` with `Activity` struct and `parse_activities` function
- [ ] Create `src/models/enums.rs` with `Mode`, `RalphViewMode`, `IterationState`, `StorySortMode`, `StoryState`
- [ ] Update main.rs to `use models::*` or specific imports
- [ ] All existing functionality preserved
- [ ] Typecheck passes (`cargo check`)
- [ ] Application runs correctly

### US-002: Extract CLI module
**Description:** As a developer, I want CLI parsing and user prompts in their own module so command-line handling is isolated.

**Acceptance Criteria:**
- [ ] Create `src/cli/mod.rs` with re-exports
- [ ] Create `src/cli/args.rs` with `CliConfig`, `parse_args`, `print_usage`
- [ ] Create `src/cli/prompts.rs` with `prompt_task_selection`, `prompt_iterations`, `prompt_rotation_threshold`
- [ ] Update main.rs to use cli module
- [ ] All CLI functionality preserved
- [ ] Typecheck passes
- [ ] Application runs correctly

### US-003: Extract PTY module
**Description:** As a developer, I want PTY handling in its own module so terminal/process management is encapsulated.

**Acceptance Criteria:**
- [ ] Create `src/pty/mod.rs` with re-exports
- [ ] Create `src/pty/state.rs` with `PtyState` struct and its impl
- [ ] Create `src/pty/spawn.rs` with `spawn_claude` function
- [ ] Create `src/pty/input.rs` with `forward_key_to_pty` function
- [ ] Update main.rs to use pty module
- [ ] All PTY functionality preserved
- [ ] Typecheck passes
- [ ] Application runs correctly

### US-004: Extract UI module
**Description:** As a developer, I want UI rendering functions in their own module so display logic is separated from business logic.

**Acceptance Criteria:**
- [ ] Create `src/ui/mod.rs` with re-exports
- [ ] Create `src/ui/render.rs` with `render_vt100_screen`, `vt100_to_ratatui_color`
- [ ] Create `src/ui/stats.rs` with `render_stat_cards`
- [ ] Create `src/ui/stories.rs` with `render_story_card`, `render_progress_cards`
- [ ] Create `src/ui/helpers.rs` with `wrap_text`
- [ ] Update main.rs to use ui module
- [ ] All UI rendering preserved
- [ ] Typecheck passes
- [ ] Application runs correctly

### US-005: Extract utils and watcher modules
**Description:** As a developer, I want utility functions and file watching in their own modules.

**Acceptance Criteria:**
- [ ] Create `src/utils.rs` with `format_duration`, `strip_ansi_codes`, `find_active_tasks`, `get_task_info`
- [ ] Create `src/watcher.rs` with `setup_prd_watcher`, `find_prompt_content`, `build_ralph_prompt`
- [ ] Update main.rs to use these modules
- [ ] All functionality preserved
- [ ] Typecheck passes
- [ ] Application runs correctly

### US-006: Extract App struct and run loop
**Description:** As a developer, I want the App struct and main run loop in focused modules so core logic is easy to follow.

**Acceptance Criteria:**
- [ ] Create `src/app.rs` with `App` struct and its impl
- [ ] Create `src/run.rs` with `run()` and `run_delay()` functions
- [ ] main.rs should only contain the `main()` function and module declarations
- [ ] main.rs is under 100 lines
- [ ] All functionality preserved
- [ ] Typecheck passes
- [ ] Application runs correctly

### US-007: Add unit tests for extracted modules
**Description:** As a developer, I want basic unit tests for the extracted modules so I can verify logic independently.

**Acceptance Criteria:**
- [ ] Add tests for `models::activity::parse_activities`
- [ ] Add tests for `models::prd::Prd::load` (happy path and error cases)
- [ ] Add tests for `utils::format_duration`
- [ ] Add tests for `utils::strip_ansi_codes`
- [ ] Add tests for `ui::helpers::wrap_text`
- [ ] Add tests for `models::enums::StorySortMode` methods
- [ ] All tests pass (`cargo test`)
- [ ] Typecheck passes

### US-008: Final cleanup and documentation
**Description:** As a developer, I want the refactored codebase cleaned up with any minor improvements applied.

**Acceptance Criteria:**
- [ ] Review all modules for consistency (naming, visibility, imports)
- [ ] Remove any dead code discovered during refactor
- [ ] Ensure public API is minimal (only expose what's needed)
- [ ] Add module-level doc comments for each module
- [ ] Typecheck passes
- [ ] All tests pass
- [ ] Application runs correctly end-to-end

## Functional Requirements

- FR-1: All existing ralph-tui functionality must be preserved
- FR-2: Each module must have a clear single responsibility
- FR-3: No module should exceed 500 lines
- FR-4: Public API should be minimal - prefer `pub(crate)` over `pub` where possible
- FR-5: Circular dependencies between modules must be avoided
- FR-6: The application must compile and run after each story is completed

## Non-Goals

- Major architectural changes beyond modularization
- Adding new features
- Changing the UI or behavior
- Performance optimization (unless trivially obvious)
- Changing external dependencies

## Technical Considerations

- The existing `theme.rs` module should remain unchanged
- Use `pub(crate)` visibility by default, only `pub` for items used by main.rs
- Consider using `#[cfg(test)]` modules within each file for unit tests
- The `App` struct has many dependencies - may need careful ordering of module extractions

## Success Metrics

- main.rs reduced from ~3,400 lines to under 100 lines
- Each module is under 500 lines
- No functionality regressions
- Unit tests provide basic coverage of pure functions
- AI tools can read entire modules in single context

## Open Questions

- Should `App` own certain modules or just use them? (Decide during implementation)
- Are there any circular dependency risks between modules? (Verify during extraction)

## Merge Target

`main` - Merge to main branch when complete.
Auto-merge: Yes (merge automatically when all stories pass)
