# Ralph

![Ralph](ralph.webp)

Ralph is an autonomous AI agent loop that runs coding agents ([Claude Code](https://docs.anthropic.com/en/docs/claude-code) or [OpenCode](https://opencode.ai)) repeatedly until all PRD items are complete. Each iteration is a fresh agent instance with clean context. Memory persists via git history, `progress.txt`, and `prd.json`.

**Supports both feature development AND bug investigations.**

Based on [Geoffrey Huntley's Ralph pattern](https://ghuntley.com/ralph/).

## Prerequisites

- Python 3.12+ with [uv](https://github.com/astral-sh/uv) for installation
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) or [OpenCode](https://opencode.ai) installed and authenticated
- A git repository for your project
- (Optional) `tmux` for session management

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

Install Ralph as a CLI tool using uv:

```bash
# Install from the repository
uv tool install git+https://github.com/apino/ralph-claude.git

# Or clone and install locally
git clone https://github.com/apino/ralph-claude.git
cd ralph-claude
uv tool install -e .
```

**Verify installation:**

```bash
ralph --version
ralph --help
```

**Uninstall:**

```bash
uv tool uninstall ralph
```

### Installing skills globally

The `/prd` and `/ralph` Claude Code skills help create and convert PRDs. Install manually:

```bash
mkdir -p ~/.claude/skills
cp -r skills/prd ~/.claude/skills/
cp -r skills/ralph ~/.claude/skills/
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
# Interactive - select task from list
ralph run

# Run specific task
ralph run tasks/device-system-refactor

# With options
ralph run tasks/fix-auth-timeout -i 20 -a opencode
```

**CLI Commands:**

| Command | Description |
|---------|-------------|
| `ralph run [TASK_DIR]` | Start or resume a task |
| `ralph status` | List running Ralph sessions |
| `ralph stop <TASK>` | Stop a running session |
| `ralph checkpoint <TASK>` | Gracefully stop with state summary |
| `ralph attach <TASK>` | Watch running session output |
| `ralph clean` | Remove stale sessions |

**Run Options:**

| Flag | Description |
|------|-------------|
| `-i, --iterations N` | Set max iterations (default: 10) |
| `-a, --agent NAME` | Agent to use: `claude` or `opencode` |
| `-y, --yes` | Skip confirmation prompts |
| `--yolo` | Enable permissive mode (skip agent permission prompts) |
| `--model MODEL` | Model override (e.g., `anthropic/claude-sonnet-4`) |

Examples:
```bash
# Basic mode - prompts for task and iterations
ralph run

# Run specific task with iteration count
ralph run tasks/fix-auth-timeout -i 20

# Use OpenCode agent with specific model
ralph run tasks/big-refactor -a opencode --model anthropic/claude-sonnet-4

# Skip all prompts with permissive mode
ralph run tasks/quick-fix -y --yolo
```

Ralph will:
1. Create a feature branch (from PRD `branchName`)
2. Pick the highest priority story where `passes: false`
3. Implement that single story
4. Run quality checks (typecheck, tests)
5. Commit if checks pass
6. Update `prd.json` to mark story as `passes: true`
7. Append learnings to `progress.txt`
8. Rotate `progress.txt` if it exceeds threshold
9. Repeat until all stories pass or max iterations reached

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
| `src/ralph/` | Python implementation of the Ralph loop |
| `prompt.md` | Instructions given to each agent iteration |
| `skills/prd/` | Skill for generating PRDs (features and bugs) |
| `skills/ralph/` | Skill for converting PRDs to JSON |
| `prd.json.example` | Example PRD format |

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

Each iteration spawns a **new agent instance** with clean context. The only memory between iterations is:
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

## Progress Rotation

For long-running efforts, `progress.txt` can grow very large, consuming excessive context tokens. Ralph automatically rotates the file when it exceeds a threshold (default: 500 lines).

**How it works:**

1. Before each iteration, Ralph checks `progress.txt` line count
2. When threshold exceeded:
   - Renames `progress.txt` → `progress-N.txt`
   - Creates new `progress.txt` with:
     - Codebase Patterns section (preserved)
     - Brief summary referencing prior file
     - Ready for new iteration logs

**File structure after rotation:**
```
tasks/big-refactor/
├── prd.json
├── progress.txt       # Current (references progress-1.txt)
├── progress-1.txt     # Previous
└── progress-2.txt     # Older
```

## Agent Failover

Ralph supports automatic failover between agents when one fails repeatedly:

```bash
# Default: claude with opencode as fallback
ralph run tasks/my-feature -a claude

# Or prefer opencode with claude as fallback
ralph run tasks/my-feature -a opencode
```

When the primary agent fails 3 consecutive times (API errors, rate limits, etc.), Ralph automatically switches to the other agent and continues.

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

# Check Ralph session status
ralph status
```

## Customizing prompt.md

Ralph uses `prompt.md` to instruct the agent on how to work. Edit it to customize behavior for your project:
- Add project-specific quality check commands
- Include codebase conventions
- Add common gotchas for your stack

**Prompt locations (checked in order):**

1. `./ralph/prompt.md` - Project-specific customization
2. `~/.config/ralph/prompt.md` - Global user default
3. Built-in fallback

To customize per-project, create `ralph/prompt.md` in your project root:

```bash
mkdir -p ralph
cp ~/.config/ralph/prompt.md ralph/prompt.md
# Edit ralph/prompt.md with project-specific instructions
```

## OpenCode Stop-Hook Plugin

When using `opencode` as the agent, Ralph uses a TypeScript plugin to detect when OpenCode finishes processing a request. This provides reliable completion detection without polling.

### How it works

1. Before spawning OpenCode, Ralph copies the plugin to `.opencode/plugins/ralph-hook/` in the working directory
2. Ralph sets the `RALPH_SIGNAL_FILE` environment variable pointing to a temporary signal file
3. When OpenCode finishes processing (fires `session.idle`), the plugin writes a JSON signal
4. Ralph detects the signal file and proceeds to the next iteration

### Plugin installation

The plugin is deployed automatically per-project. For global installation:

```bash
# Build the plugin (requires Node.js)
cd plugins/opencode-ralph-hook
npm install && npm run build

# Copy to global plugins directory
mkdir -p ~/.config/opencode/plugins/ralph-hook
cp dist/* package.json ~/.config/opencode/plugins/ralph-hook/
```

## OpenCode Systemd Setup (Linux)

When using OpenCode as the agent, Ralph connects to a long-running server instead of spawning one per task. This provides faster startup, enables `ralph attach` after task completion, and survives shell/session restarts.

### Create the systemd user service

Create the unit file at `~/.config/systemd/user/opencode.service`:

```ini
[Unit]
Description=OpenCode Server
After=default.target

[Service]
Type=simple
ExecStart=%h/.local/bin/opencode serve --port 14096
Restart=on-failure
RestartSec=5
Environment=HOME=%h

[Install]
WantedBy=default.target
```

> **Note:** Adjust `ExecStart` if your `opencode` binary is in a different location. Use `which opencode` to find it.

### Enable and start the service

```bash
# Reload systemd to pick up the new unit file
systemctl --user daemon-reload

# Start the server now
systemctl --user start opencode

# Check it's running
systemctl --user status opencode

# Enable auto-start on login
systemctl --user enable opencode
```

### Management commands

| Command | Description |
|---------|-------------|
| `systemctl --user start opencode` | Start the server |
| `systemctl --user stop opencode` | Stop the server |
| `systemctl --user restart opencode` | Restart after config changes |
| `systemctl --user status opencode` | Check if running |
| `systemctl --user enable opencode` | Auto-start on login |
| `systemctl --user disable opencode` | Disable auto-start |

### Persistence across reboots (optional)

By default, user services only run when you're logged in. To keep the server running even after logout (useful for remote/headless servers):

```bash
# Enable lingering for your user
loginctl enable-linger $USER
```

This allows your user services to start at boot and continue running after logout.

### Verifying the setup

After starting the service, verify it's working:

```bash
# Check health endpoint
curl http://127.0.0.1:14096/global/health
# Should return: {"healthy":true,"version":"..."}

# View logs if something's wrong
journalctl --user -u opencode -f
```

Ralph will automatically connect to this server when running tasks with `-a opencode`.

## References

- [Geoffrey Huntley's Ralph article](https://ghuntley.com/ralph/)
- [Claude Code documentation](https://docs.anthropic.com/en/docs/claude-code)
- [OpenCode documentation](https://opencode.ai/docs)
