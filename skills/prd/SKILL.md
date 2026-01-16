---
name: prd
description: "Generate a structured requirements document for features OR bug investigations. Use when planning a feature, troubleshooting a bug, or any multi-step effort. Triggers on: create a prd, write prd for, plan this feature, investigate this bug, troubleshoot, debug."
---

# PRD Generator

Create detailed requirements documents that are clear, actionable, and suitable for autonomous execution via Ralph.

**Supports both feature development AND bug investigations.**

---

## The Job

1. Determine if this is a **feature** or **bug/investigation**
2. Ask 3-5 essential clarifying questions (with lettered options)
3. Generate a structured PRD based on answers
4. Create directory: `tasks/{effort-name}/`
5. Save PRD to `tasks/{effort-name}/prd.md`
6. Initialize empty `tasks/{effort-name}/progress.txt`

**Important:** Do NOT start implementing. Just create the PRD and directory structure.

---

## Directory Structure

Each effort gets its own subdirectory:

```
tasks/
├── device-system-refactor/
│   ├── prd.md           # The requirements document
│   ├── prd.json         # Created by /ralph skill
│   └── progress.txt     # Ralph's iteration logs
├── fix-auth-timeout/
│   ├── prd.md
│   ├── prd.json
│   └── progress.txt
└── archived/            # Completed efforts (via /archive skill)
    └── ...
```

This keeps each effort self-contained and prevents file conflicts.

---

## Step 1: Determine Type

First, identify what kind of effort this is:

### Feature PRD
For new functionality, enhancements, refactors, or improvements.

### Bug Investigation PRD
For troubleshooting, debugging, or fixing issues. These follow a structured investigation flow:
1. Reproduce the issue
2. Add instrumentation/logging
3. Identify root cause
4. Evaluate solutions
5. Implement fix
6. Validate fix

---

## Step 2: Clarifying Questions

Ask only critical questions where the initial prompt is ambiguous. Focus on:

- **Problem/Goal:** What problem does this solve?
- **Core Functionality:** What are the key actions?
- **Scope/Boundaries:** What should it NOT do?
- **Success Criteria:** How do we know it's done?

### Format Questions Like This:

```
1. What is the primary goal of this feature?
   A. Improve user onboarding experience
   B. Increase user retention
   C. Reduce support burden
   D. Other: [please specify]

2. Who is the target user?
   A. New users only
   B. Existing users only
   C. All users
   D. Admin users only

3. What is the scope?
   A. Minimal viable version
   B. Full-featured implementation
   C. Just the backend/API
   D. Just the UI
```

This lets users respond with "1A, 2C, 3B" for quick iteration.

### For Bug Investigations, also ask:

```
1. How reliably can you reproduce this?
   A. 100% reproducible with known steps
   B. Intermittent but frequent
   C. Rare / hard to reproduce
   D. Only seen in production

2. What's the impact severity?
   A. Critical - blocking users
   B. High - major functionality broken
   C. Medium - workaround exists
   D. Low - minor inconvenience

3. What debugging has been done so far?
   A. None yet
   B. Basic investigation (logs checked)
   C. Significant debugging already
   D. Root cause identified, need fix
```

---

## Step 3: PRD Structure

Generate the PRD with these sections:

### 1. Introduction/Overview
Brief description of the feature/issue and the problem it solves.

### 2. Type
Either `Feature` or `Bug Investigation`.

### 3. Goals
Specific, measurable objectives (bullet list).

### 4. User Stories
Each story needs:
- **Title:** Short descriptive name
- **Description:** "As a [user], I want [feature] so that [benefit]"
- **Acceptance Criteria:** Verifiable checklist of what "done" means

Each story should be small enough to implement in one focused session (one Ralph iteration).

**Format:**
```markdown
### US-001: [Title]
**Description:** As a [user], I want [feature] so that [benefit].

**Acceptance Criteria:**
- [ ] Specific verifiable criterion
- [ ] Another criterion
- [ ] Typecheck/lint passes
- [ ] **[UI stories only]** Verify in browser
```

**Important:**
- Acceptance criteria must be verifiable, not vague. "Works correctly" is bad. "Button shows confirmation dialog before deleting" is good.
- **For any story with UI changes:** Always include "Verify in browser" as acceptance criteria.

### 5. Functional Requirements
Numbered list of specific functionalities:
- "FR-1: The system must allow users to..."
- "FR-2: When a user clicks X, the system must..."

Be explicit and unambiguous.

### 6. Non-Goals (Out of Scope)
What this feature will NOT include. Critical for managing scope.

### 7. Design Considerations (Optional)
- UI/UX requirements
- Link to mockups if available
- Relevant existing components to reuse

### 8. Technical Considerations (Optional)
- Known constraints or dependencies
- Integration points with existing systems
- Performance requirements

### 9. Success Metrics
How will success be measured?
- "Reduce time to complete X by 50%"
- "Increase conversion rate by 10%"

### 10. Open Questions
Remaining questions or areas needing clarification.

---

## Writing for Junior Developers

The PRD reader may be a junior developer or AI agent. Therefore:

- Be explicit and unambiguous
- Avoid jargon or explain it
- Provide enough detail to understand purpose and core logic
- Number requirements for easy reference
- Use concrete examples where helpful

---

## Bug Investigation PRD Structure

For bug investigations, use this adapted structure:

```markdown
# PRD: [Bug/Issue Name]

## Type
Bug Investigation

## Problem Statement
Clear description of the bug:
- What is happening (actual behavior)
- What should happen (expected behavior)
- When it started (if known)
- Impact on users

## Reproduction Steps
1. Step to reproduce
2. Another step
3. Expected vs actual result

## Environment
- Browser/OS/Device (if relevant)
- Environment (dev/staging/prod)
- Relevant versions

## Goals
- Identify root cause
- Implement fix
- Prevent regression

## Investigation Stories

### US-001: Reproduce the issue reliably
**Description:** As a developer, I need to reliably reproduce the bug so I can debug it.

**Acceptance Criteria:**
- [ ] Document exact reproduction steps
- [ ] Identify minimum conditions to trigger bug
- [ ] Create test case or script if possible
- [ ] Typecheck passes

### US-002: Add instrumentation and logging
**Description:** As a developer, I need visibility into what's happening when the bug occurs.

**Acceptance Criteria:**
- [ ] Add relevant logging to suspected areas
- [ ] Capture state at key points
- [ ] Log errors with full context
- [ ] Typecheck passes

### US-003: Identify root cause
**Description:** As a developer, I need to understand why the bug is happening.

**Acceptance Criteria:**
- [ ] Document the root cause clearly
- [ ] Identify the specific code/logic causing the issue
- [ ] Understand the conditions that trigger it
- [ ] Update notes in prd.json with findings

### US-004: Evaluate solution options
**Description:** As a developer, I need to consider different fix approaches.

**Acceptance Criteria:**
- [ ] Document at least 2 potential solutions
- [ ] List pros/cons of each approach
- [ ] Recommend best solution with rationale
- [ ] Update notes in prd.json with decision

### US-005: Implement the fix
**Description:** As a developer, I need to fix the bug.

**Acceptance Criteria:**
- [ ] Implement chosen solution
- [ ] Ensure fix doesn't introduce regressions
- [ ] Typecheck passes
- [ ] Tests pass (if applicable)

### US-006: Validate the fix
**Description:** As a developer, I need to confirm the bug is fixed.

**Acceptance Criteria:**
- [ ] Original reproduction steps no longer trigger bug
- [ ] Test edge cases related to the fix
- [ ] Remove or reduce debug logging if added
- [ ] Typecheck passes
- [ ] Verify in browser (if UI-related)

## Hypotheses
Initial theories about what might be causing the issue:
1. Hypothesis A: [theory]
2. Hypothesis B: [theory]

## Related Code
Files/areas likely involved:
- `path/to/file.ts` - reason
- `path/to/other.ts` - reason

## Non-Goals
- What we're NOT trying to fix in this investigation
- Related issues to address separately

## Open Questions
Unknowns that might affect the investigation.
```

---

## Output

1. **Create directory:** `tasks/{effort-name}/` (kebab-case)
2. **Save PRD:** `tasks/{effort-name}/prd.md`
3. **Initialize progress file:** `tasks/{effort-name}/progress.txt` with header:

```
# Ralph Progress Log
Effort: {effort-name}
Type: {Feature|Bug Investigation}
Started: {date}
---
```

---

## Example Feature PRD

```markdown
# PRD: Task Priority System

## Type
Feature

## Introduction

Add priority levels to tasks so users can focus on what matters most. Tasks can be marked as high, medium, or low priority, with visual indicators and filtering to help users manage their workload effectively.

## Goals

- Allow assigning priority (high/medium/low) to any task
- Provide clear visual differentiation between priority levels
- Enable filtering and sorting by priority
- Default new tasks to medium priority

## User Stories

### US-001: Add priority field to database
**Description:** As a developer, I need to store task priority so it persists across sessions.

**Acceptance Criteria:**
- [ ] Add priority column to tasks table: 'high' | 'medium' | 'low' (default 'medium')
- [ ] Generate and run migration successfully
- [ ] Typecheck passes

### US-002: Display priority indicator on task cards
**Description:** As a user, I want to see task priority at a glance so I know what needs attention first.

**Acceptance Criteria:**
- [ ] Each task card shows colored priority badge (red=high, yellow=medium, gray=low)
- [ ] Priority visible without hovering or clicking
- [ ] Typecheck passes
- [ ] Verify in browser

### US-003: Add priority selector to task edit
**Description:** As a user, I want to change a task's priority when editing it.

**Acceptance Criteria:**
- [ ] Priority dropdown in task edit modal
- [ ] Shows current priority as selected
- [ ] Saves immediately on selection change
- [ ] Typecheck passes
- [ ] Verify in browser

### US-004: Filter tasks by priority
**Description:** As a user, I want to filter the task list to see only high-priority items when I'm focused.

**Acceptance Criteria:**
- [ ] Filter dropdown with options: All | High | Medium | Low
- [ ] Filter persists in URL params
- [ ] Empty state message when no tasks match filter
- [ ] Typecheck passes
- [ ] Verify in browser

## Functional Requirements

- FR-1: Add `priority` field to tasks table ('high' | 'medium' | 'low', default 'medium')
- FR-2: Display colored priority badge on each task card
- FR-3: Include priority selector in task edit modal
- FR-4: Add priority filter dropdown to task list header
- FR-5: Sort by priority within each status column (high to medium to low)

## Non-Goals

- No priority-based notifications or reminders
- No automatic priority assignment based on due date
- No priority inheritance for subtasks

## Technical Considerations

- Reuse existing badge component with color variants
- Filter state managed via URL search params
- Priority stored in database, not computed

## Success Metrics

- Users can change priority in under 2 clicks
- High-priority tasks immediately visible at top of lists
- No regression in task list performance

## Open Questions

- Should priority affect task ordering within a column?
- Should we add keyboard shortcuts for priority changes?
```

---

## Checklist

Before saving the PRD:

- [ ] Determined type (Feature or Bug Investigation)
- [ ] Asked clarifying questions with lettered options
- [ ] Incorporated user's answers
- [ ] Created `tasks/{effort-name}/` directory
- [ ] User stories are small and specific (completable in one iteration)
- [ ] Acceptance criteria are verifiable (not vague)
- [ ] Functional requirements are numbered and unambiguous
- [ ] Non-goals section defines clear boundaries
- [ ] Saved PRD to `tasks/{effort-name}/prd.md`
- [ ] Initialized `tasks/{effort-name}/progress.txt`
