# Ralph Agent Instructions

Ralph is an autonomous AI agent loop that runs coding agents repeatedly until all PRD items are complete. Each iteration spawns a fresh agent instance with clean context.

## Build/Test/Lint Commands

### Python (src/ralph/)

```bash
# Install (requires uv - https://docs.astral.sh/uv/)
uv tool install -e .         # Install as CLI tool
uv sync                      # Sync dependencies for development

# Run the CLI
ralph run tasks/my-feature -i 10 -a claude

# Type checking (ty - extremely fast, from Astral)
uvx ty check src/ralph

# Linting and formatting
uv run ruff check src/           # Check for lint errors
uv run ruff check --fix src/     # Auto-fix lint errors
uv run ruff format src/          # Format code (run before committing)

# Single file checks
uvx ty check src/ralph/loop.py   # Type check one file
uv run ruff check src/ralph/loop.py
```

### TypeScript (plugins/opencode-ralph-hook/)

```bash
cd plugins/opencode-ralph-hook
bun install
bun run build             # Compile TypeScript
bun run typecheck         # Type check without emitting
```

## Code Style Guidelines

### Python

**Imports** - Use this order with blank lines between groups:
```python
from __future__ import annotations

import json                    # Standard library (alphabetical)
import os
from pathlib import Path

import click                   # Third-party

from ralph.agents import VALID_AGENTS   # Local imports

if TYPE_CHECKING:              # Type-only imports at end
    from click import Context
```

**Naming Conventions**:
- Classes: `PascalCase` (e.g., `LoopRunner`, `SessionDB`, `AgentResult`)
- Functions/methods: `snake_case` (e.g., `build_prompt`, `run_agent`)
- Private functions: `_prefix` (e.g., `_find_active_tasks`, `_run_git`)
- Constants: `UPPER_SNAKE_CASE` (e.g., `VALID_AGENTS`, `DEFAULT_MAX_ITERATIONS`)

**Type Annotations**:
- Full type hints on all functions - no exceptions
- Use `| None` instead of `Optional` (Python 3.10+ style)
- Always specify return types, even for `-> None`
- Use dataclasses with type hints for structured data

```python
@dataclass
class AgentConfig:
    """Configuration for agent execution."""
    prompt: str
    working_dir: Path
    yolo_mode: bool = False

def resolve_agent(
    cli_agent: str | None,
    task_dir: Path,
    skip_prompts: bool,
) -> str:
    """Resolve which agent to use."""
```

**Docstrings**: Triple-quoted at module, class, and public method level.

**Error Handling**:
- Define custom exception classes for domain errors
- Use explicit exception types, avoid bare `except:`
- Log to file for background processes

```python
class BranchError(Exception):
    """Git branch operation failed."""

def setup_branch(config: BranchConfig) -> None:
    try:
        _run_git(["checkout", config.branch_name])
    except subprocess.CalledProcessError as e:
        raise BranchError(f"Failed to checkout: {e}") from e
```

### TypeScript

**Imports**:
```typescript
import * as fs from "fs";
import * as path from "path";
import * as os from "os";
```

**Naming**:
- Interfaces: `PascalCase` (e.g., `IdleSignal`, `PluginConfig`)
- Functions: `camelCase` (e.g., `getConfig`, `writeSignal`)
- Constants: `camelCase` or `UPPER_SNAKE_CASE`

**Types**: Use interfaces for data structures, JSDoc for function documentation.

```typescript
/** Signal payload written to the signal file on session idle. */
interface IdleSignal {
  event: "idle";
  timestamp: string;
  session_id: string;
}
```

## Project Structure

```
ralph-claude/
├── src/ralph/                    # Python implementation
│   ├── __init__.py               # Package version
│   ├── cli.py                    # Click CLI entrypoint
│   ├── loop.py                   # Core iteration logic (LoopRunner)
│   ├── agents.py                 # Agent ABC + Claude/OpenCode impls
│   ├── session.py                # Session management (tmux, SQLite)
│   ├── prompt.py                 # Prompt building
│   ├── branch.py                 # Git branch management
│   ├── opencode_server.py        # OpenCode HTTP server mode
│   └── attach.py                 # Session attach command
├── plugins/opencode-ralph-hook/  # OpenCode completion detection
├── skills/                       # Claude Code skills for PRD
├── prompt.md                     # Instructions for each iteration
├── tasks/                        # Task directories with prd.json
└── context/opencode              # opencode source code (for reference--DO NOT EDIT)
```

## Key Patterns

**Agent Abstraction**: `Agent` ABC with `ClaudeAgent` and `OpencodeAgent` implementations. Factory function `create_agent(name)` returns the appropriate agent. Failover via `FailureTracker` after consecutive failures.

**Session Management**: SQLite registry at `~/.local/share/ralph/sessions.db`. tmux sessions for Claude agent. HTTP API for OpenCode (`opencode serve`). Signal files for stop/checkpoint.

**Completion Detection**: OpenCode plugin writes signal file on `session.idle`. Claude parses `stream-json` output. Completion signal: `<promise>COMPLETE</promise>`.

## CLI Usage

```bash
ralph run tasks/my-feature -i 10 -a opencode  # Run a task
ralph status              # List sessions
ralph stop my-feature     # Stop a session
ralph checkpoint my-feature  # Pause after current iteration
ralph attach my-feature   # Attach to running session
ralph clean               # Remove stale sessions
```

## Versioning

All versions are centralized in `src/ralph/version.py`:

| Component | Version | Description |
|-----------|---------|-------------|
| `TOOL_VERSION` | 0.2.0 | Ralph CLI (semver) |
| `SCHEMA_VERSION` | 2.3 | prd.json format |
| `PROMPT_VERSION` | 2.3 | prompt.md format |

**When updating versions**: Edit `src/ralph/version.py` - all other files derive from it.

## Configuration

### prd.json Schema (v2.3)
```json
{
  "schemaVersion": "2.3",
  "project": "ProjectName",
  "taskDir": "tasks/effort-name",
  "branchName": "ralph/effort-name",
  "agent": "opencode",
  "userStories": [...]
}
```

### Environment Variables
- `RALPH_AGENT`: Default agent (claude/opencode)
- `YOLO_MODE`: Skip permission prompts
- `RALPH_VERBOSE`: Enable verbose output
- `RALPH_SIGNAL_FILE`: Signal file path (set by ralph for plugins)

## Debugging

- OpenCode logs: `~/.local/share/opencode/log/`
- Ralph agent logs: `~/.local/state/ralph/agent.log`
- Plugin logs: `~/.local/state/ralph/plugin.log`
- Use `--log-level DEBUG` with opencode
- Use `--verbose` with ralph for agent output

## Ruff Configuration

Enabled rule sets in `pyproject.toml`:
- `E`: pycodestyle errors
- `F`: Pyflakes
- `I`: isort (import sorting)
- `UP`: pyupgrade (Python version upgrades)

**Line length**: 88 characters (ruff default). Run `uv run ruff format src/` to auto-format.
