# Ralph OpenCode Server Management Plan

## Current Problems

### 1. Confusing Status After Loop Completes
- `ralph status` shows `server_port=14096` for stopped/completed tasks
- Users think the server is still running when it's not
- `ralph attach` marks session as `failed` even when loop completed normally

### 2. `ralph attach` Marks Wrong Status
- When status is `stopped` or `completed`, attach shouldn't mark `failed`
- Only mark `failed` when status is `running` but healthcheck fails
- Current behavior corrupts the historical status

### 3. Task Name Resolution Fails
- `ralph run use-dev-backend-locally` looks for literal path, not task name
- User has to type `ralph run tasks/use-dev-backend-locally`
- Should resolve task names automatically from project root

### 4. Port Display is Misleading
- Ports are reused heavily (range 14096-14196)
- Multiple tasks showing same port doesn't mean shared server
- Dead servers still show port numbers

### 5. Server Lifecycle Causes Confusion
- Server dies when worker exits (by design)
- Can't attach after loop completes
- No way to keep server alive for inspection

---

## Key Discovery: OpenCode Server Architecture

### The Server is Directory-Agnostic

An `opencode serve` instance does NOT have a fixed working directory. Instead:

1. **Per-request directory resolution** - Every HTTP request can specify a directory:
   - Query param: `?directory=/path/to/project`
   - Header: `x-opencode-directory: /path/to/project`
   - Fallback: `process.cwd()` where server was started

2. **Instance caching** - The server caches "instances" (project metadata, git info) per directory. First request for a directory initializes it; subsequent requests reuse.

3. **Multi-project capable** - A single server can handle requests for multiple projects simultaneously. Client A can work on `/project-a` while Client B works on `/project-b`.

4. **Sessions are project-scoped** - Each session belongs to a project (identified by git root commit). Sessions are stored globally but filtered by project ID.

### Current Ralph Behavior

Ralph starts `opencode serve` with `cwd=project_root`, so the default directory works. But Ralph's HTTP client doesn't send `?directory=` explicitly - it relies on the cwd fallback.

### Implication

Ralph could run ONE long-lived opencode server and pass `?directory=<project>` per request. Multiple tasks from different projects could share one server. This would massively simplify port management.

---

## Proposed Fixes

### Fix 1: Status Display (Small, High Impact)
In `ralph status`:
- Only show port when status is `running`
- Show `-` for stopped/completed/failed sessions
- Or show `last:14096` to indicate it's historical, not live

### Fix 2: Attach Status Logic (Small, High Impact)
In `ralph attach`:
- Check DB status first
- If `stopped` or `completed`: print "Session not running. Restart with: ralph run ..."
- If `running` but healthcheck fails: mark `failed`
- Never change status from `stopped`/`completed` to `failed`

### Fix 3: Task Name Resolution (Medium)
In `ralph run`:
- Accept bare task name (e.g., `use-dev-backend-locally`)
- Resolve to `tasks/<name>` relative to git root
- Keep full path support for backwards compatibility

### Fix 4: Server Lifecycle (Design Decision Required)

#### Option A: Single Global Server (Recommended - based on new understanding)
- Run ONE opencode server (e.g., on port 14096)
- Pass `?directory=<project_root>` with every HTTP request
- Server handles multiple projects automatically via instance caching
- Server lifecycle independent of task lifecycle
- Start on first `ralph run`, keep alive until explicit `ralph server stop`
- Benefits:
  - Attach always works (server survives task completion)
  - No port churn
  - Simpler DB schema (just track "is server running?")
  - Multiple concurrent tasks share one server

#### Option B: Persistent Server Per Project
- One opencode server per project root
- Multiple tasks in same project share the server
- Server survives loop completion
- Explicit cleanup via `ralph stop-server` or `ralph clean`
- DB schema change: add "servers" table keyed by project root

#### Option C: Keep Server Per Run, Add Keep/TTL (Simplest)
- Keep current "server per worker" model
- Add `--keep-server` flag (or default to keeping on stop)
- Add TTL/cleanup so ports don't leak
- Doesn't leverage multi-project capability

### Fix 5: Attach Session Targeting (Optional)
- Default `ralph attach` to attach to server without `--session`
- Add `--latest` to attach to most recent session
- Current behavior attaches to potentially stale session ID

---

## Implementation Order

1. **Fix 2** - Attach status logic (prevents data corruption)
2. **Fix 1** - Status display (reduces confusion)
3. **Fix 3** - Task name resolution (UX improvement)
4. **Fix 4** - Server lifecycle (requires design decision)
5. **Fix 5** - Attach targeting (optional enhancement)

---

## Questions to Resolve

1. **Server lifecycle**: Global singleton (Option A), per-project (Option B), or per-run (Option C)?
2. **Directory parameter**: Should Ralph explicitly pass `?directory=` or continue relying on cwd?
3. **Backwards compatibility**: Should old DB entries be migrated?
4. **Multi-project**: If using global server, how to handle `ralph attach <task>` when tasks span projects?

---

## Observed Behavior (From Logs)

### What Happened When You Ran `ralph run tasks/use-dev-backend-locally`

1. Background worker spawned
2. Worker started `opencode serve` on port 14096
3. Healthcheck passed
4. Ran 10 iterations (each creates new opencode session)
5. Hit max iterations
6. Worker stopped opencode server (by design)
7. Worker exited

### Why `ralph attach` Failed

- Server on 14096 was already stopped
- Attach checked healthcheck, failed
- Marked session as `failed` (incorrect - should stay `stopped`)

### The Port 44957 Server

- Unrelated to ralph (no DB entry)
- Probably started manually via `opencode serve`
- Not tracked by ralph session management
