# Ralph Agent Instructions

## Overview

Ralph is an autonomous AI agent loop that runs Claude Code repeatedly until all PRD items are complete. Each iteration is a fresh agent instance with clean context.

Supports both **feature development** and **bug investigations**.

## Installation

```bash
# Clone and install globally
git clone https://github.com/anomalyco/ralph-claude.git
cd ralph-claude
./install.sh
```

This installs:
- `ralph` command to `~/.local/bin/`
- `prompt.md` to `~/.local/share/ralph/`
- Skills to `~/.claude/skills/`

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
# Install Ralph globally
./install.sh

# Run Ralph (starts in tmux background)
ralph tasks/device-system-refactor

# Watch output in real-time
ralph attach

# Checkpoint and exit gracefully
ralph checkpoint

# Resume from checkpoint (auto-detected)
ralph tasks/device-system-refactor

# Resume with different agent
ralph tasks/device-system-refactor -a codex

# Force stop
ralph stop

# List running sessions
ralph status

# Run with specific iterations
ralph tasks/fix-auth-timeout -i 20

# Skip prompts
ralph tasks/fix-auth-timeout -y

# Initialize tasks directory in a new project
ralph --init

# Use custom prompt file
ralph tasks/my-feature -p ./custom-prompt.md

# Run the flowchart dev server
cd flowchart && npm run dev
```

## Session Management

Ralph runs in a tmux session for real-time monitoring:

| Command | Description |
|---------|-------------|
| `ralph attach [task]` | Watch running session (read-only) |
| `ralph checkpoint [task]` | Save state and exit gracefully |
| `ralph stop [task]` | Force stop session |
| `ralph status` | List running sessions |

While attached to a session:
- **Ctrl+B d** - Detach (Ralph keeps running)
- To checkpoint, run `ralph checkpoint` from another terminal

## Command Line Options

| Flag | Description |
|------|-------------|
| `-i, --iterations N` | Maximum iterations (default: 10) |
| `-a, --agent NAME` | Agent to use (claude, codex, opencode, aider, amp) |
| `-m, --model MODEL` | Model to use (e.g. "opus", "anthropic/claude-sonnet-4-5") |
| `-y, --yes` | Skip confirmation prompts |
| `-p, --prompt FILE` | Use custom prompt file |
| `--init` | Initialize tasks/ directory |
| `--version` | Show version |
| `-h, --help` | Show help |

## Installation Paths

When installed globally:
- `~/.local/bin/ralph` - The executable
- `~/.local/share/ralph/prompt.md` - Default prompt template
- `~/.claude/skills/prd/` - PRD generation skill
- `~/.claude/skills/ralph/` - PRD-to-JSON conversion skill

## Prompt Resolution

Ralph looks for prompt.md in this order:
1. `--prompt` flag
2. `$RALPH_PROMPT` environment variable
3. `./prompt.md` (project-local)
4. `~/.local/share/ralph/prompt.md` (global)

## Key Files

- `ralph.sh` - The bash loop that spawns agent instances
- `prompt.md` - Instructions given to each agent instance
- `install.sh` - Global installation script
- `skills/prd/` - Skill for generating PRDs (features and bugs)
- `skills/ralph/` - Skill for converting PRDs to JSON
- `prd.json.example` - Example PRD format
- `flowchart/` - Interactive React Flow diagram explaining how Ralph works

## PRD Types

### Feature
Standard feature development with dependency-ordered stories.

### Bug Investigation
Follows: Reproduce → Instrument → Analyze → Evaluate → Implement → Validate

## prd.json Schema

```json
{
  "project": "MyApp",
  "taskDir": "tasks/my-feature",
  "branchName": "ralph/my-feature",
  "type": "feature",
  "description": "Feature description",
  "agent": "claude",  // Preferred agent (saved on first selection)
  "userStories": [...]
}
```

## Patterns

- Each iteration spawns a fresh agent instance with clean context
- Memory persists via git history, `progress.txt`, and `prd.json`
- Stories should be small enough to complete in one context window
- Use the `notes` field in stories to pass context between iterations
- Agent preference is saved per-task in `prd.json`
- Always update AGENTS.md with discovered patterns for future iterations
