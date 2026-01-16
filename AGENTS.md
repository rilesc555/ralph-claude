# Ralph Agent Instructions

## Overview

Ralph is an autonomous AI agent loop that runs Claude Code repeatedly until all PRD items are complete. Each iteration is a fresh Claude Code instance with clean context.

Supports both **feature development** and **bug investigations**.

## Directory Structure

Each effort gets its own subdirectory under `tasks/`:

```
tasks/
├── device-system-refactor/
│   ├── prd.md           # The requirements document
│   ├── prd.json         # Ralph-format JSON
│   └── progress.txt     # Iteration logs
├── fix-auth-timeout/
│   ├── prd.md
│   ├── prd.json
│   └── progress.txt
└── ...
```

## Commands

```bash
# Run Ralph for a specific task
./ralph.sh tasks/device-system-refactor

# Run with more iterations
./ralph.sh tasks/fix-auth-timeout 20

# Run the flowchart dev server
cd flowchart && npm run dev
```

## Key Files

- `ralph.sh` - The bash loop that spawns fresh Claude Code instances
- `prompt.md` - Instructions given to each Claude Code instance
- `skills/prd/` - Skill for generating PRDs (features and bugs)
- `skills/ralph/` - Skill for converting PRDs to JSON
- `prd.json.example` - Example PRD format
- `flowchart/` - Interactive React Flow diagram explaining how Ralph works

## PRD Types

### Feature
Standard feature development with dependency-ordered stories.

### Bug Investigation
Follows: Reproduce → Instrument → Analyze → Evaluate → Implement → Validate

## Patterns

- Each iteration spawns a fresh Claude Code instance with clean context
- Memory persists via git history, `progress.txt`, and `prd.json`
- Stories should be small enough to complete in one context window
- Use the `notes` field in stories to pass context between iterations
- Always update AGENTS.md with discovered patterns for future iterations
