# Ralph for Claude Code

![Ralph](ralph.webp)

Ralph is an autonomous AI agent loop that runs [Claude Code](https://docs.anthropic.com/en/docs/claude-code) repeatedly until all PRD items are complete. Each iteration is a fresh Claude Code instance with clean context. Memory persists via git history, `progress.txt`, and `prd.json`.

**Watch your agent work in real-time** with `ralph attach` - see every token, tool call, and decision as it happens.

**Supports both feature development AND bug investigations.**

Based on [Geoffrey Huntley's Ralph pattern](https://ghuntley.com/ralph/).

## Prerequisites

- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- `jq` installed (`brew install jq` on macOS)
- `tmux` installed (`brew install tmux` on macOS, `sudo apt install tmux` on Ubuntu)
- A git repository for your project

### Optional: Playwright MCP (for browser testing)

For UI stories that require browser verification, install the Playwright MCP server:

```bash
claude mcp add playwright npx '@playwright/mcp@latest'
```

**Omarchy users:** The default Playwright config won't find Chromium. Edit your Claude config to add the executable path:

```bash
nano ~/.claude.json
```

Update the playwright MCP entry to include `--executable-path`:

```json
{
  "mcpServers": {
    "playwright": {
      "command": "npx",
      "args": [
        "@playwright/mcp@latest",
        "--executable-path",
        "/usr/bin/chromium"
      ]
    }
  }
}
```

## Installation

### Option 1: Global install (recommended)

Install Ralph globally so you can use it from any project:

```bash
# Clone the repo
git clone https://github.com/anomalyco/ralph-claude.git
cd ralph-claude

# Run the installer
./install.sh
```

This installs:
- `ralph` command to `~/.local/bin/`
- `prompt.md` to `~/.local/share/ralph/`
- Skills to `~/.claude/skills/`

**Note:** Make sure `~/.local/bin` is in your PATH. The installer will warn you if it's not.

To uninstall: `./install.sh --uninstall`

### Option 2: Copy to your project

If you prefer to keep Ralph files in your project:

```bash
# From your project root
cp /path/to/ralph-claude/ralph.sh ./
cp /path/to/ralph-claude/prompt.md ./
chmod +x ralph.sh

# Optionally copy skills
mkdir -p ~/.claude/skills
cp -r /path/to/ralph-claude/skills/prd ~/.claude/skills/
cp -r /path/to/ralph-claude/skills/ralph ~/.claude/skills/
```

## Directory Structure

Each effort (feature or bug investigation) gets its own subdirectory:

```
tasks/
├── device-system-refactor/
│   ├── prd.md           # The requirements document
│   ├── prd.json         # Ralph-format JSON (created by /ralph skill)
│   └── progress.txt     # Ralph's iteration logs
├── fix-auth-timeout/
│   ├── prd.md
│   ├── prd.json
│   └── progress.txt
└── archived/            # Completed efforts (moved here when done)
    └── ...
```

This keeps each effort self-contained and allows multiple Ralph loops to run on different efforts without conflicts.

## Workflow

### 1. Create a PRD

Use the PRD skill to generate a detailed requirements document:

```
/prd create a PRD for [your feature or bug description]
```

The skill will:
- Ask clarifying questions (with lettered options for quick responses like "1A, 2C, 3B")
- Determine if this is a feature or bug investigation
- Create `tasks/{effort-name}/prd.md`
- Initialize `tasks/{effort-name}/progress.txt`

**For features:** Describe the new functionality you want.

**For bugs:** Describe the issue, symptoms, and any reproduction steps you know.

### 2. Convert PRD to Ralph format

Use the Ralph skill to convert the markdown PRD to JSON:

```
/ralph convert tasks/{effort-name}/prd.md
```

This creates `tasks/{effort-name}/prd.json` with user stories structured for autonomous execution.

### 3. Run Ralph

```bash
ralph [task-directory] [-i iterations]
```

Examples:
```bash
# Interactive mode - prompts for task and iterations
ralph

# Run specific task (prompts for iterations)
ralph tasks/device-system-refactor

# Run with explicit iteration count (no prompts)
ralph tasks/fix-auth-timeout -i 20

# Initialize tasks directory in a new project
ralph --init
```

**Interactive prompts:**

1. **Task selection** (if no task directory specified):
   - If **one active task**: Runs it automatically
   - If **multiple active tasks**: Shows numbered list to choose from
   - If **no active tasks**: Shows instructions for creating one

2. **Iteration count** (if `-i` not specified):
   ```
   Max iterations [10]:
   ```
   Press Enter for default (10) or enter a number.

Ralph runs in a **tmux session** in the background. After starting, you'll see:
```
  To watch output:     ralph attach
  To checkpoint:       ralph checkpoint
  To stop:             ralph stop
```

Ralph will:
1. Create a feature branch (from PRD `branchName`)
2. Pick the highest priority story where `passes: false`
3. Implement that single story
4. Run quality checks (typecheck, tests)
5. Commit if checks pass
6. Update `prd.json` to mark story as `passes: true`
7. Append learnings to `progress.txt`
8. Repeat until all stories pass or max iterations reached

## Watching Progress

Ralph runs in a tmux session so you can watch the agent work in real-time. When you attach, you'll see the **full agent output** - every token, tool call, and response streamed live.

```bash
# Attach to the running session (read-only)
ralph attach

# If multiple tasks are running, specify which one
ralph attach my-feature
```

You'll see the complete agent experience:
- All reasoning and planning text
- Tool invocations (file reads, edits, bash commands)
- Real-time streaming as the agent works

While attached:
- **Ctrl+B d** - Detach (Ralph keeps running in background)
- To checkpoint, run `ralph checkpoint` from another terminal

**Tip:** Open two terminals - one attached to watch, one to run `ralph checkpoint` or `ralph stop` when needed.

## Checkpoints

You can pause Ralph at any time and resume later:

```bash
# Request a checkpoint (Ralph saves state and exits after current operation)
ralph checkpoint

# Resume where you left off (auto-detects checkpoint)
ralph tasks/my-feature

# Resume with a different agent
ralph tasks/my-feature -a codex
```

Checkpoint saves:
- Current iteration number
- Summary written to `progress.txt`
- State flags in `prd.json`

## Session Management

```bash
# See all running Ralph sessions
ralph status

# Force stop a session (no checkpoint)
ralph stop

# Stop a specific task
ralph stop my-feature
```

### 4. Archive completed efforts

When Ralph completes (or you're done with an effort), archive it:

```bash
mkdir -p tasks/archived
mv tasks/fix-auth-timeout tasks/archived/
```

This keeps the active `tasks/` directory clean while preserving completed work.

## Key Files

| File | Purpose |
|------|---------|
| `ralph.sh` | The bash loop that spawns fresh Claude Code instances |
| `prompt.md` | Instructions given to each Claude Code instance |
| `install.sh` | Installer script for global installation |
| `skills/prd/` | Skill for generating PRDs (features and bugs) |
| `skills/ralph/` | Skill for converting PRDs to JSON |
| `prd.json.example` | Example PRD format |

## CLI Options

### Commands

| Command | Description |
|---------|-------------|
| `ralph [task]` | Start or resume a task (runs in tmux background) |
| `ralph attach [task]` | Watch running session output (read-only) |
| `ralph checkpoint [task]` | Gracefully stop with state summary |
| `ralph stop [task]` | Force stop running session |
| `ralph status` | List running Ralph sessions |

### Flags

| Flag | Description |
|------|-------------|
| `-i, --iterations N` | Maximum iterations (default: 10) |
| `-a, --agent NAME` | Agent to use (claude, codex, opencode, aider, amp) |
| `-m, --model MODEL` | Model to use (e.g. "opus", "anthropic/claude-sonnet-4-5") |
| `-y, --yes` | Skip confirmation prompts |
| `-p, --prompt FILE` | Use custom prompt file |
| `--init` | Initialize tasks/ directory in current project |
| `--version` | Show version |
| `-h, --help` | Show help |

## Prompt File Resolution

Ralph looks for `prompt.md` in this order:
1. `--prompt` flag (explicit path)
2. `$RALPH_PROMPT` environment variable
3. `./prompt.md` (project-local override)
4. `~/.local/share/ralph/prompt.md` (global default)

This allows you to customize the prompt per-project while having a global default.

## PRD Types

### Feature PRD

For new functionality, enhancements, or refactors. Stories follow dependency order:
1. Schema/database changes
2. Backend logic
3. UI components
4. Integration/polish

### Bug Investigation PRD

For troubleshooting and fixing issues. Stories follow investigation flow:
1. **Reproduce** - Document exact reproduction steps
2. **Instrument** - Add logging to understand the issue
3. **Analyze** - Identify root cause (document in `notes` field)
4. **Evaluate** - Consider solution options
5. **Implement** - Fix the bug
6. **Validate** - Confirm the fix works

The `notes` field in each story passes context between iterations.

## Critical Concepts

### Each Iteration = Fresh Context

Each iteration spawns a **new Claude Code instance** with clean context. The only memory between iterations is:
- Git history (commits from previous iterations)
- `progress.txt` (learnings and context)
- `prd.json` (which stories are done, plus notes)

### Small Tasks

Each PRD item should be small enough to complete in one context window. If a task is too big, the LLM runs out of context before finishing and produces poor code.

**Right-sized stories:**
- Add a database column and migration
- Add logging to a specific code area
- Implement a focused bug fix
- Validate a fix with tests

**Too big (split these):**
- "Build the entire dashboard" - Split into: schema, queries, UI components, filters
- "Add authentication" - Split into: schema, middleware, login UI, session handling
- "Fix all the bugs" - Focus on one specific issue

**Rule of thumb:** If you cannot describe the change in 2-3 sentences, it is too big.

### AGENTS.md Updates

After each iteration, Ralph updates relevant `AGENTS.md` files with learnings. Claude Code automatically reads these files, so future iterations benefit from discovered patterns and gotchas.

### Feedback Loops

Ralph only works if there are feedback loops:
- Typecheck catches type errors
- Tests verify behavior
- CI must stay green (broken code compounds across iterations)

### Stop Condition

When all stories have `passes: true`, Ralph outputs `<promise>COMPLETE</promise>` and the loop exits.

## Debugging

Check current state:

```bash
# See which stories are done
cat tasks/{effort-name}/prd.json | jq '.userStories[] | {id, title, passes, notes}'

# See learnings from previous iterations
cat tasks/{effort-name}/progress.txt

# Check git history
git log --oneline -10

# List available task directories
ls -la tasks/
```

## Customizing prompt.md

To customize Ralph's behavior for a specific project, create a local `prompt.md` in your project root. Ralph will use it instead of the global default.

```bash
# Copy the global prompt as a starting point
cp ~/.local/share/ralph/prompt.md ./prompt.md

# Edit to add project-specific instructions
```

Things to customize:
- Project-specific quality check commands
- Codebase conventions
- Common gotchas for your stack
- Testing requirements

## References

- [Geoffrey Huntley's Ralph article](https://ghuntley.com/ralph/)
- [Claude Code documentation](https://docs.anthropic.com/en/docs/claude-code)
