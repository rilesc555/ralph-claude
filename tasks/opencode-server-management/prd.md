# PRD: OpenCode Server Management Overhaul

## Type
Feature

## Introduction

Ralph's current OpenCode server management has several usability issues: confusing status displays after tasks complete, incorrect status transitions when attaching to dead servers, cumbersome task name resolution, and a server-per-task model that doesn't leverage OpenCode's multi-project capabilities.

This overhaul implements a **systemd-managed global OpenCode server**. The server runs as a user service (`opencode.service`), always available on port 14096. Ralph connects to it by passing `?directory=<project>` per request, eliminating port churn and enabling attach after task completion. Server lifecycle is fully delegated to systemd.

## Goals

- Eliminate misleading port displays for stopped/completed sessions
- Prevent `ralph attach` from corrupting historical status of completed sessions
- Enable simple task name resolution (e.g., `ralph run my-feature` instead of `ralph run tasks/my-feature`)
- Use systemd user service for server lifecycle (auto-restart, boot persistence)
- Allow attaching to view session history after loop completes
- Migrate existing database entries to new schema

## User Stories

### US-001: Fix attach status corruption
**Description:** As a user, I want `ralph attach` to not mark completed sessions as failed so that my session history remains accurate.

**Acceptance Criteria:**
- [ ] When session status is `stopped` or `completed`, attach prints "Session not running" message without changing status
- [ ] Only mark `failed` when status is `running` but server healthcheck fails
- [ ] Add test or manual verification that status transitions are correct
- [ ] Typecheck passes (`uvx ty check src/ralph`)
- [ ] Lint passes (`uv run ruff check src/`)

### US-002: Fix status display for dead servers
**Description:** As a user, I want `ralph status` to clearly indicate when a server port is historical vs. live so I don't think dead servers are running.

**Acceptance Criteria:**
- [ ] Sessions with status `running` show port normally (e.g., `14096`)
- [ ] Sessions with status `stopped`/`completed`/`failed` show `—` or no port
- [ ] Consider adding a "last port" column or note for historical reference
- [ ] Typecheck passes
- [ ] Lint passes

### US-003: Add task name resolution
**Description:** As a user, I want to run `ralph run my-feature` instead of `ralph run tasks/my-feature` so that common operations are faster.

**Acceptance Criteria:**
- [ ] `ralph run <name>` resolves to `tasks/<name>/` relative to git root
- [ ] Full paths still work (backwards compatible)
- [ ] Clear error message if task name not found in `tasks/` directory
- [ ] Works from any subdirectory of the git repo
- [ ] Typecheck passes
- [ ] Lint passes

### US-004: Remove server lifecycle from Ralph
**Description:** As a developer, I need to remove server start/stop logic from Ralph since systemd now manages the server.

**Acceptance Criteria:**
- [ ] Remove `OpencodeServer.start()` calls from `ralph run` worker
- [ ] Remove `server.stop()` from worker `finally` block
- [ ] Remove `server_pid` tracking from sessions table (systemd owns the process)
- [ ] Keep `server_port` in sessions for historical reference (always 14096 now)
- [ ] Remove or simplify `opencode_server.py` - only keep client/health-check logic
- [ ] Typecheck passes
- [ ] Lint passes

### US-005: Connect to systemd-managed server
**Description:** As a user, I want `ralph run` to connect to the systemd-managed OpenCode server instead of spawning its own.

**Acceptance Criteria:**
- [ ] `ralph run` checks if `opencode.service` is running via health check on port 14096
- [ ] If server not running, print helpful message: "Start with: systemctl --user start opencode"
- [ ] All HTTP requests include `?directory=<project_root>` query parameter
- [ ] Project root determined from git root of task directory
- [ ] Typecheck passes
- [ ] Lint passes

### US-006: Pass directory parameter to server
**Description:** As a developer, I need Ralph to pass `?directory=<project>` to the OpenCode server so one server can handle multiple projects.

**Acceptance Criteria:**
- [ ] All HTTP requests to OpenCode server include `?directory=<project_root>` query parameter
- [ ] Project root determined from git root of task directory
- [ ] Verify server correctly scopes sessions to the project
- [ ] Typecheck passes
- [ ] Lint passes

### US-007: Update attach for global server
**Description:** As a user, I want `ralph attach` to work after task completion by connecting to the systemd server.

**Acceptance Criteria:**
- [ ] Attach connects to port 14096 (systemd server)
- [ ] Still passes `--session <id>` to attach to specific session
- [ ] If systemd server not running, print: "Start with: systemctl --user start opencode"
- [ ] If session doesn't exist on server, show helpful error
- [ ] Typecheck passes
- [ ] Lint passes

### US-008: Migrate existing database entries
**Description:** As a user upgrading Ralph, I want my existing session history preserved with the new schema.

**Acceptance Criteria:**
- [ ] On first run with new schema, migrate existing sessions table
- [ ] Old `server_port` values preserved as historical data
- [ ] Old `server_pid` column dropped (no longer needed)
- [ ] Migration is idempotent (safe to run multiple times)
- [ ] Typecheck passes
- [ ] Lint passes

### US-009: Update stop and checkpoint commands
**Description:** As a developer, I need `ralph stop` and `ralph checkpoint` to work correctly without server management.

**Acceptance Criteria:**
- [ ] `ralph stop <task>` stops only the task worker (signal file)
- [ ] Remove any server stop logic from `ralph stop`
- [ ] `ralph checkpoint` still works (pauses after current iteration)
- [ ] Document that server is managed by systemd, not ralph
- [ ] Typecheck passes
- [ ] Lint passes

### US-010: Handle attach session targeting
**Description:** As a user, I want smarter defaults when attaching so I connect to the right session.

**Acceptance Criteria:**
- [ ] `ralph attach <task>` attaches to the most recent session for that task
- [ ] `ralph attach <task> --session <id>` attaches to specific session
- [ ] `ralph attach` (no args) attaches to most recently active session across all tasks
- [ ] Typecheck passes
- [ ] Lint passes

### US-011: Document systemd setup
**Description:** As a user, I need documentation on setting up the OpenCode systemd service.

**Acceptance Criteria:**
- [ ] Add setup instructions to README or docs
- [ ] Include the systemd unit file content
- [ ] Document commands: `systemctl --user enable/start/stop/status opencode`
- [ ] Explain that server persists across reboots (if `lingering` enabled)
- [ ] Typecheck passes (N/A for docs)
- [ ] Lint passes (N/A for docs)

## Functional Requirements

- FR-1: OpenCode server runs as systemd user service on port 14096
- FR-2: All OpenCode HTTP requests must include `?directory=<git_root>` parameter
- FR-3: Task workers must not start or stop the server (systemd manages it)
- FR-4: `ralph status` must distinguish live vs. historical server ports
- FR-5: `ralph attach` must check session status before marking failed
- FR-6: Task name resolution must search `tasks/` at git root
- FR-7: Schema migration must preserve existing session history

## Non-Goals

- Ralph managing server start/stop (delegated to systemd)
- Multiple simultaneous servers
- `ralph server` subcommands (use `systemctl --user` directly)
- Automatic systemd service installation (user runs setup once)

## Technical Considerations

### Systemd User Service

The OpenCode server runs as a systemd user service:

```ini
# ~/.config/systemd/user/opencode.service
[Unit]
Description=OpenCode Server
After=default.target

[Service]
Type=simple
ExecStart=%h/.opencode/bin/opencode serve --port 14096
Restart=on-failure
RestartSec=5
Environment=HOME=%h

[Install]
WantedBy=default.target
```

Management commands:
- `systemctl --user start opencode` - Start server
- `systemctl --user stop opencode` - Stop server
- `systemctl --user status opencode` - Check status
- `systemctl --user enable opencode` - Start on login
- `loginctl enable-linger $USER` - Keep running after logout (optional)

### OpenCode Server Architecture

OpenCode's `serve` command is **directory-agnostic**:
- Every request can specify directory via `?directory=` query param or `x-opencode-directory` header
- Server caches "instances" (project metadata) per directory on first request
- Sessions are scoped to projects (identified by git root commit)
- One server can handle multiple projects simultaneously

### Health Check

Server health verified via: `GET http://127.0.0.1:14096/global/health`
Returns: `{"healthy":true,"version":"1.1.53"}`

### Database Schema Changes

```sql
-- Sessions table changes
-- server_port kept for historical reference (always 14096 for new sessions)
-- server_pid column dropped (systemd manages process)
ALTER TABLE sessions DROP COLUMN server_pid;
```

## Success Metrics

- `ralph status` output is unambiguous (no misleading "live" ports for dead servers)
- `ralph attach <task>` works after task completes (server always running)
- Session status history is never incorrectly mutated
- Server survives ralph exit, terminal close, and system reboot (with lingering)
- Existing workflows (`ralph run`, `ralph stop`) work without server management

## Open Questions

1. Should Ralph auto-detect if systemd service is not installed and offer setup instructions?
2. Should we support non-systemd environments (macOS launchd, manual daemon)?
3. How to handle port conflicts if user has manual `opencode serve` on 14096?

## Merge Target

None - leave as standalone branch.
