# Workspace Support for Ralph Loop

## Summary

Add a `--workspace` option to `ralph run` that creates an isolated git worktree via OpenCode's worktree API. This allows ralph to run in a completely isolated copy of the codebase, preventing conflicts with the user's working directory.

---

## 1. Feature Overview

### New CLI Options

```bash
ralph run tasks/my-feature --workspace       # Create new isolated worktree
ralph run tasks/my-feature --workspace my-sandbox  # Named worktree
ralph run tasks/my-feature --workspace --workspace-reset  # Reset worktree to main before running
ralph run tasks/my-feature --workspace --workspace-keep   # Keep worktree after completion
```

### Key Benefits

- **Isolated working directory**: Ralph won't conflict with user's uncommitted changes
- **Clean git state**: Each ralph session starts with a pristine codebase
- **Resettable**: Ability to reset worktree to main branch between runs
- **Separate branch**: Each worktree gets its own branch (`opencode/<name>`)

---

## 2. Architecture Changes

### 2.1 New Module: `src/ralph/workspace.py`

```python
"""Workspace management for ralph using OpenCode worktrees.

Provides isolated git worktrees for ralph loops via OpenCode's HTTP API.
Worktrees are stored at ~/.local/share/opencode/worktree/<project-id>/<name>/.
"""

from __future__ import annotations

import time
from dataclasses import dataclass
from pathlib import Path

from ralph.opencode_server import OpencodeClient, OpencodeServerError


@dataclass
class WorkspaceInfo:
    """Information about an OpenCode worktree workspace."""
    
    name: str           # Worktree name (e.g., "brave-forest")
    branch: str         # Git branch (e.g., "opencode/brave-forest")
    directory: Path     # Full path to worktree directory


class WorkspaceError(Exception):
    """Raised when workspace operations fail."""


class WorkspaceManager:
    """Manages OpenCode worktrees for ralph sessions.
    
    Creates and manages isolated git worktrees via the OpenCode HTTP API.
    """
    
    def __init__(self, client: OpencodeClient) -> None:
        self.client = client
    
    def create(self, name: str | None = None) -> WorkspaceInfo:
        """Create a new worktree workspace.
        
        Args:
            name: Optional name (auto-generated if not provided)
        
        Returns:
            WorkspaceInfo with directory path for the new worktree
        """
        # POST /experimental/worktree
        ...
    
    def list_workspaces(self) -> list[str]:
        """List all sandbox worktree directories."""
        # GET /experimental/worktree
        ...
    
    def reset(self, directory: Path) -> None:
        """Reset a worktree to the default branch (main/master)."""
        # POST /experimental/worktree/reset
        ...
    
    def remove(self, directory: Path) -> None:
        """Remove a worktree and its branch."""
        # DELETE /experimental/worktree
        ...
    
    def wait_for_ready(
        self, 
        workspace: WorkspaceInfo, 
        timeout: float = 60.0
    ) -> bool:
        """Wait for worktree to be fully initialized.
        
        The worktree creation API returns immediately, but git reset --hard
        and startup scripts run asynchronously. Poll until ready.
        """
        # Poll for .git existence or check directory is populated
        ...
```

### 2.2 Updates to `opencode_server.py`

Add methods for worktree API endpoints:

```python
class OpencodeClient:
    # ... existing methods ...
    
    def create_worktree(
        self, 
        name: str | None = None,
        start_command: str | None = None,
    ) -> dict[str, str]:
        """Create a new worktree.
        
        Returns: {"name": "...", "branch": "...", "directory": "..."}
        """
        url = self._url_with_directory("/experimental/worktree")
        payload = {}
        if name:
            payload["name"] = name
        if start_command:
            payload["startCommand"] = start_command
        return self._http_post(url, payload)
    
    def list_worktrees(self) -> list[str]:
        """List worktree directories for current project."""
        url = self._url_with_directory("/experimental/worktree")
        return self._http_get(url)
    
    def reset_worktree(self, directory: str) -> bool:
        """Reset worktree to default branch."""
        url = self._url_with_directory("/experimental/worktree/reset")
        self._http_post(url, {"directory": directory})
        return True
    
    def remove_worktree(self, directory: str) -> bool:
        """Remove worktree and delete its branch."""
        url = self._url_with_directory("/experimental/worktree")
        return self._http_delete(url, {"directory": directory})
    
    def _http_get(self, url: str, timeout: float | None = HTTP_TIMEOUT) -> Any:
        """Make an HTTP GET request.
        
        Returns the parsed JSON response.
        """
        req = self._build_request(url, method="GET")
        try:
            with urlopen(req, timeout=timeout) as resp:
                resp_body = resp.read().decode("utf-8")
                if resp_body:
                    return json.loads(resp_body)
                return None
        except (URLError, OSError, TimeoutError) as e:
            raise OpencodeServerError(f"HTTP GET {url} failed: {e}") from e
    
    def _http_delete(
        self, url: str, data: dict[str, Any], timeout: float | None = HTTP_TIMEOUT
    ) -> Any:
        """Make an HTTP DELETE request with JSON body.
        
        Returns the parsed JSON response.
        """
        body = json.dumps(data).encode("utf-8")
        req = self._build_request(url, method="DELETE")
        req.add_header("Content-Type", "application/json")
        req.data = body
        
        try:
            with urlopen(req, timeout=timeout) as resp:
                resp_body = resp.read().decode("utf-8")
                if resp_body:
                    return json.loads(resp_body)
                return True
        except (URLError, OSError, TimeoutError) as e:
            raise OpencodeServerError(f"HTTP DELETE {url} failed: {e}") from e
```

### 2.3 Updates to `cli.py`

Add workspace options to `run` command:

```python
@cli.command()
@click.argument("task_dir", ...)
@click.option(
    "--workspace",
    "workspace_name",
    default=None,
    is_flag=False,
    flag_value="",  # Empty string = auto-generate name
    help="Run in isolated worktree. Optional name, otherwise auto-generated.",
)
@click.option(
    "--workspace-reset",
    is_flag=True,
    help="Reset workspace to main branch before running.",
)
@click.option(
    "--workspace-keep",
    is_flag=True,
    help="Don't remove workspace after completion (for debugging).",
)
def run(
    task_dir: str | None,
    workspace_name: str | None,  # None = no workspace, "" = auto, "foo" = named
    workspace_reset: bool,
    workspace_keep: bool,
    # ... existing options ...
) -> None:
    # ... existing logic ...
    
    if workspace_name is not None:
        # Create or reuse workspace via OpenCode API
        workspace = setup_workspace(
            client=opencode_client,
            name=workspace_name or None,
            reset=workspace_reset,
        )
        # Use workspace.directory as working dir instead of project root
        working_dir = workspace.directory
```

### 2.4 Updates to `LoopConfig` and `LoopRunner`

```python
@dataclass
class LoopConfig:
    task_dir: Path
    # ... existing fields ...
    workspace_dir: Path | None = None  # If set, run in this worktree
    workspace_keep: bool = False       # Don't clean up workspace on completion


class LoopRunner:
    def __init__(self, config: LoopConfig, ...):
        # Use workspace_dir as working directory if set
        self._working_dir = config.workspace_dir or config.task_dir.parent.parent
    
    def _run_agent(self, agent_name: str, story: dict[str, Any]) -> AgentResult:
        working_dir = self._working_dir  # Use workspace if configured
        # ... rest of method ...
```

### 2.5 Updates to `SessionInfo` and Database

Add workspace tracking to session info:

```python
@dataclass
class SessionInfo:
    # ... existing fields ...
    workspace_dir: str = ""   # Worktree directory (empty if not using workspace)
    workspace_name: str = ""  # Worktree name for cleanup
```

Database migration (add columns):

```sql
ALTER TABLE sessions ADD COLUMN workspace_dir TEXT NOT NULL DEFAULT '';
ALTER TABLE sessions ADD COLUMN workspace_name TEXT NOT NULL DEFAULT '';
```

---

## 3. Implementation Tasks

| Task | File(s) | Complexity | Notes |
|------|---------|------------|-------|
| 1. Add HTTP GET/DELETE helpers | `opencode_server.py` | Low | Add `_http_get`, `_http_delete` methods |
| 2. Add worktree API methods | `opencode_server.py` | Medium | `create_worktree`, `list_worktrees`, `reset_worktree`, `remove_worktree` |
| 3. Create workspace module | `workspace.py` (new) | Medium | `WorkspaceManager` class with create/reset/remove |
| 4. Add CLI options | `cli.py` | Medium | `--workspace`, `--workspace-reset`, `--workspace-keep` |
| 5. Update LoopConfig | `loop.py` | Low | Add `workspace_dir` field |
| 6. Update LoopRunner | `loop.py` | Medium | Use `workspace_dir` for working directory |
| 7. Update SessionInfo | `session.py` | Low | Add workspace tracking fields |
| 8. Add DB migration | `session.py` | Low | Handle schema upgrade for new columns |
| 9. Cleanup on completion | `loop.py`, `cli.py` | Medium | Remove workspace unless `--workspace-keep` |
| 10. Wait for ready logic | `workspace.py` | Medium | Poll until worktree is initialized |

---

## 4. Key Design Decisions

### 4.1 Worktree Lifecycle

1. **Creation**: Ralph creates worktree at start of `ralph run --workspace`
2. **Initialization**: Wait for OpenCode's async initialization (git reset, startup scripts)
3. **Usage**: Loop runs agents with `working_dir` set to worktree directory
4. **Cleanup**: Remove worktree on completion (unless `--workspace-keep`)

### 4.2 Branch Strategy

OpenCode creates branches as `opencode/<name>`. This aligns well with ralph's existing branch strategy (`ralph/<task-name>`). The worktree branch is independent from ralph's task branch - ralph will still checkout/create its task branch inside the worktree.

### 4.3 Session Directory Context

The OpenCode server scopes sessions by the `?directory=` parameter. When using a workspace:
- Create session with `directory=<worktree-path>`
- This ensures file operations are scoped to the worktree

### 4.4 Task Directory Location

The `prd.json` and `progress.txt` stay in the original `tasks/` directory, NOT in the worktree. The worktree is purely for code isolation:

```
tasks/my-feature/prd.json                           # stays in main repo
tasks/my-feature/progress.txt                       # stays in main repo
~/.local/share/opencode/worktree/<id>/<name>/       # isolated code copy
```

---

## 5. OpenCode API Details

### Create Worktree

```http
POST /experimental/worktree?directory=<project-root>
Content-Type: application/json

{"name": "optional-name", "startCommand": "optional-startup-script"}
```

**Response:**
```json
{
  "name": "brave-forest",
  "branch": "opencode/brave-forest", 
  "directory": "/home/user/.local/share/opencode/worktree/abc123/brave-forest"
}
```

> **Note**: Returns immediately. Git reset and startup scripts run async in background.

### List Worktrees

```http
GET /experimental/worktree?directory=<project-root>
```

**Response:**
```json
["/path/to/worktree1", "/path/to/worktree2"]
```

### Reset Worktree

```http
POST /experimental/worktree/reset?directory=<project-root>
Content-Type: application/json

{"directory": "/path/to/worktree"}
```

**Response:**
```json
true
```

### Remove Worktree

```http
DELETE /experimental/worktree?directory=<project-root>
Content-Type: application/json

{"directory": "/path/to/worktree"}
```

**Response:**
```json
true
```

---

## 6. Error Handling

| Error | Handling |
|-------|----------|
| OpenCode server not running | Fail early with helpful message |
| Worktree creation fails | Fail with error, don't start loop |
| Worktree init timeout | Fail after 60s with suggestion to check server logs |
| Cleanup fails | Log warning, don't fail the loop |
| Not a git repo | Fail early (worktrees require git) |

---

## 7. Example Usage

```bash
# Basic: auto-generate workspace name
ralph run tasks/my-feature --workspace -i 10

# Named workspace (reusable)
ralph run tasks/my-feature --workspace my-sandbox -i 10

# Reset workspace to main before running (clean slate)
ralph run tasks/my-feature --workspace my-sandbox --workspace-reset -i 10

# Keep workspace for debugging
ralph run tasks/my-feature --workspace --workspace-keep -i 10

# Combine with existing options
ralph run tasks/my-feature --workspace -a opencode --yolo -i 20
```

---

## 8. Future Enhancements (Out of Scope)

- `ralph workspace list` - List all workspaces
- `ralph workspace remove <name>` - Manually remove workspace
- `ralph workspace reset <name>` - Reset workspace without running
- Automatic workspace reuse by task name
- Workspace pooling for faster startup

---

## 9. Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `src/ralph/workspace.py` | **Create** | WorkspaceManager class |
| `src/ralph/opencode_server.py` | Modify | Add worktree API methods, HTTP helpers |
| `src/ralph/cli.py` | Modify | Add `--workspace` options |
| `src/ralph/loop.py` | Modify | Accept `workspace_dir` in config |
| `src/ralph/session.py` | Modify | Track workspace in SessionInfo |

---

## 10. Testing Strategy

### Unit Tests

- `test_workspace.py`: Test WorkspaceManager methods (mock HTTP)
- `test_opencode_server.py`: Test new HTTP methods

### Integration Tests

- Create worktree, verify directory exists
- Reset worktree, verify clean state
- Remove worktree, verify cleanup

### Manual Testing

```bash
# Start opencode server
systemctl --user start opencode

# Test workspace creation
ralph run tasks/test-feature --workspace --foreground -i 1

# Verify worktree exists
ls ~/.local/share/opencode/worktree/

# Test workspace reset
ralph run tasks/test-feature --workspace test-ws --workspace-reset -i 1

# Test cleanup (workspace should be removed after completion)
ls ~/.local/share/opencode/worktree/
```

---

## 11. Implementation Order

1. **Phase 1: API Layer** (opencode_server.py)
   - Add `_http_get` and `_http_delete` helpers
   - Add worktree API methods

2. **Phase 2: Workspace Module** (workspace.py)
   - Create WorkspaceManager class
   - Implement wait_for_ready logic

3. **Phase 3: CLI Integration** (cli.py)
   - Add `--workspace` options
   - Wire up workspace creation/cleanup

4. **Phase 4: Loop Integration** (loop.py)
   - Update LoopConfig with workspace fields
   - Update LoopRunner to use workspace directory

5. **Phase 5: Session Tracking** (session.py)
   - Add workspace fields to SessionInfo
   - Handle database schema migration

6. **Phase 6: Cleanup & Polish**
   - Error handling improvements
   - Documentation updates
   - AGENTS.md updates
