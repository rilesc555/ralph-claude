# Ralph Agent Instructions

Ralph is an autonomous AI agent loop that runs coding agents repeatedly until all PRD items are complete. Each iteration spawns a fresh agent instance with clean context.

## Build/Test/Lint Commands

### Python (src/ralph/)

```bash
# Install (requires uv)
uv tool install -e .         # Install as CLI tool
uv sync                      # Sync dependencies for development

# Run the CLI
ralph run tasks/my-feature -i 10 -a claude

# Type checking (strict mode)
uv run mypy --strict src/ralph

# Linting and formatting
uv run ruff check src/
uv run ruff format src/

# Run a single Python file check
uv run mypy src/ralph/loop.py
uv run ruff check src/ralph/loop.py
```

### TypeScript (plugins/opencode-ralph-hook/)

```bash
cd plugins/opencode-ralph-hook
npm install
npm run build             # Compile TypeScript
npm run typecheck         # Type check without emitting
```

## Code Style Guidelines

### Python

**Imports** - Use this order with blank lines between groups:
```python
from __future__ import annotations

import json                    # Standard library
import os

import click                   # Third-party

from ralph.agents import VALID_AGENTS   # Local

if TYPE_CHECKING:              # Type-only imports
    from click import Context
```

**Naming**:
- Classes: `PascalCase` (e.g., `LoopRunner`, `SessionDB`)
- Functions/methods: `snake_case` (e.g., `build_prompt`, `run_agent`)
- Private functions: `_prefix` (e.g., `_find_active_tasks`)
- Constants: `UPPER_SNAKE_CASE` (e.g., `VALID_AGENTS`, `DEFAULT_MAX_ITERATIONS`)

**Type Annotations**:
- Full type hints on all functions (strict mypy mode)
- Use `| None` instead of `Optional` (Python 3.10+ style)
- Always specify return types
- Use dataclasses with type hints for structured data

```python
def resolve_agent(
    cli_agent: str | None,
    task_dir: Path,
    skip_prompts: bool,
) -> str:
    """Resolve which agent to use."""
```

**Docstrings**: Triple-quoted at module, class, and complex method level.

**Error Handling**: Use explicit exception types, log to file for background processes.

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

**Types**: Use interfaces for data structures, JSDoc for function docs.

## Project Structure

```
ralph-claude/
├── src/ralph/              # Python implementation (main)
│   ├── cli.py              # Click-based CLI entrypoint
│   ├── loop.py             # Core iteration logic
│   ├── agents.py           # Agent abstraction (Claude, OpenCode)
│   ├── session.py          # Session management (tmux, SQLite)
│   ├── prompt.py           # Prompt building
│   └── branch.py           # Git branch management
├── ralph-tui/              # Rust TUI implementation (Deprecated)
├── plugins/opencode-ralph-hook/  # OpenCode completion detection plugin
├── skills/                 # Claude Code skills for PRD generation
├── prompt.md               # Instructions given to each agent iteration
└── tasks/                  # Task directories with prd.json files
```

## Key Patterns

### Agent Abstraction
- `Agent` ABC with `ClaudeAgent` and `OpencodeAgent` implementations
- Factory: `create_agent(name)` returns the appropriate agent
- Failover via `FailureTracker` after consecutive failures

### Session Management
- SQLite registry: `~/.local/share/ralph/sessions.db`
- tmux sessions for Claude agent
- HTTP API mode for OpenCode (`opencode serve`)
- Signal files for stop/checkpoint communication

### Completion Detection
- OpenCode: Plugin writes signal file on `session.idle` event
- Claude: Parses `stream-json` output for result
- Completion signal: `<promise>COMPLETE</promise>`

## CLI Usage

```bash
# Run a task
ralph run tasks/my-feature -i 10 -a opencode

# Session management
ralph status              # List sessions
ralph stop my-feature     # Stop a session
ralph checkpoint my-feature  # Pause after current iteration
ralph attach my-feature   # Attach to running session
ralph clean               # Remove stale sessions
```

## Configuration

### prd.json Schema (v2.0)
```json
{
  "schemaVersion": "2.0",
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
- Use `--log-level DEBUG` with opencode for detailed logs
- Use `--verbose` with ralph for agent output visibility
