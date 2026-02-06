# Background OpenCode Loops

## Problem Statement

Currently, `ralph run tasks/<x> -a opencode` runs the Python loop in the foreground. When the user closes their terminal, the loop dies—even though `opencode serve` is spawned in its own process group. The only reliable way to stop a loop should be `ralph stop <task>`.

Users also want to `opencode attach` to a running ralph session, pause it, send their own prompts, and have ralph continue afterward. This interactive capability must be preserved.

## Current Architecture

### How iterations work today (OpenCode server mode)

1. CLI (`src/ralph/cli.py`) calls `_spawn_opencode_server()` which:
   - Starts `opencode serve` as a subprocess with `preexec_fn=os.setsid`
   - Registers the session in SQLite with `session_type="opencode-server"`
   - Runs `LoopRunner.run()` directly in the same process

2. Loop (`src/ralph/loop.py`) advances iterations via `_run_agent_via_server()`:
   - Creates a new OpenCode session each iteration
   - Calls `OpencodeServer.send_prompt()` which is **synchronous** (blocks until response)
   - When that HTTP call returns, the iteration is considered complete

3. The `OpencodeServer.wait_for_idle()` method exists but is **not used** by the loop

### Problems with current approach

1. **Terminal dependency**: The loop runner process dies when terminal closes
2. **No interruptibility mid-iteration**: Synchronous `send_prompt()` blocks; can't cleanly stop while OpenCode is "thinking"
3. **Stdout/stderr pipes can deadlock**: `opencode serve` is spawned with `stdout=PIPE, stderr=PIPE` but nothing drains them
4. **PID tracking is wrong**: `SessionInfo.pid` stores the server PID, not the loop PID; `ralph stop` kills the wrong process

## OpenCode Event System

### Completion signals

OpenCode publishes events via SSE at `GET /event`. The relevant events are:

| Event | Status | Description |
|-------|--------|-------------|
| `session.status` | Current | Carries `{sessionID, status}` where status is `idle`, `busy`, or `retry` |
| `session.idle` | Deprecated | Compatibility event, fires when session becomes idle |

The canonical completion signal is `session.status` with `status.type === "idle"`.

Source: `context/opencode/packages/opencode/src/session/status.ts`

```typescript
export function set(sessionID: string, status: Info) {
  Bus.publish(Event.Status, { sessionID, status })
  if (status.type === "idle") {
    // deprecated
    Bus.publish(Event.Idle, { sessionID })
    delete state()[sessionID]
    return
  }
  state()[sessionID] = status
}
```

### API endpoints

| Endpoint | Method | Behavior |
|----------|--------|----------|
| `/session/{id}/message` | POST | Synchronous—blocks until agent responds |
| `/session/{id}/prompt_async` | POST | Async—returns 204 immediately |
| `/session/{id}/abort` | POST | Stops processing |
| `/session/status` | GET | Returns status of all sessions |
| `/event` | GET | SSE stream of all events |

## Proposed Design

### 1. Background worker process

When `ralph run tasks/<x> -a opencode` is invoked:

1. Parent process spawns a **detached worker** (`start_new_session=True`)
2. Worker process runs the actual loop (starts `opencode serve`, iterates stories)
3. Parent prints status info and exits immediately:
   ```
   Started background loop for 'my-feature'
     ralph status          # Check progress
     ralph attach my-feature   # Watch/interact
     ralph stop my-feature     # Stop the loop
   ```

The worker is immune to terminal close (SIGHUP).

### 2. Async prompt + event-driven completion

Switch from synchronous to async iteration:

```
Current:
  response = server.send_prompt(session_id, prompt)  # blocks
  # iteration complete

Proposed:
  server.send_prompt_async(session_id, prompt)  # returns immediately
  while True:
      if check_stop_signal():
          server.abort_session(session_id)
          break
      if poll_session_status(session_id) == "idle":
          break
      sleep(0.5)
  # iteration complete
```

Benefits:
- `ralph stop` is responsive even mid-iteration
- Can abort cleanly via `/session/{id}/abort`

### 3. User interaction compatibility

When user runs `opencode attach` and sends prompts:

- OpenCode creates new messages in the session
- Session status goes `busy` -> `idle` for each user turn
- Ralph must not treat user-initiated idle transitions as iteration completion

**Correlation strategy**:
- Ralph tracks the `messageID` it sent via `prompt_async`
- Ralph only considers iteration complete when assistant response to *that* message is done
- OR: Ralph uses a "pause" state—when user attaches and prompts, ralph enters paused state and doesn't advance

### 4. Proper PID tracking

Update `SessionInfo` schema:

```python
@dataclass
class SessionInfo:
    # ... existing fields ...
    loop_pid: int          # Worker process PID (for stop)
    server_pid: int | None # opencode serve PID (for cleanup)
```

Update `stop_session()`:
1. Write stop signal file
2. Send SIGTERM to `loop_pid`
3. If loop doesn't exit, kill `server_pid` as backup

### 5. Fix stdout/stderr handling

Change `opencode serve` subprocess creation:

```python
# Current (can deadlock)
subprocess.Popen(cmd, stdout=PIPE, stderr=PIPE, ...)

# Proposed (safe)
log_file = log_dir / f"{task_name}-server.log"
subprocess.Popen(cmd, stdout=log_file, stderr=STDOUT, ...)
```

## Implementation Tasks

### Phase 1: Background worker

1. Add `--foreground` flag to CLI (for debugging)
2. Modify `_spawn_opencode_server()` to:
   - By default, spawn detached worker process
   - Worker sets `RALPH_WORKER=1` env var
   - When `RALPH_WORKER=1`, run loop directly
3. Update session DB schema for `loop_pid` vs `server_pid`
4. Update `stop_session()` to target `loop_pid`

### Phase 2: Async iteration

1. Add `send_prompt_async()` wrapper if not present
2. Add `poll_session_status()` method (GET `/session/status`)
3. Modify `_run_agent_via_server()`:
   - Use async prompt
   - Poll for idle in loop with stop signal checks
   - Call `abort_session()` on stop
4. Fix stdout/stderr redirect in `OpencodeServer.start()`

### Phase 3: User interaction support

1. Track sent `messageID` in loop state
2. Add logic to correlate idle events with ralph's messages
3. Consider "pause on attach" feature for explicit handoff

## Testing

- Start loop, close terminal, verify loop continues (`ralph status`)
- `ralph stop` during active iteration, verify clean abort
- `opencode attach`, send manual prompt, verify ralph doesn't advance prematurely
- Verify logs capture server output after redirect

## Open Questions

1. Should we use SSE streaming or polling for status? Polling is simpler but adds latency.
2. How to detect when user has attached and taken over? Options:
   - Explicit `/pause` command in ralph
   - Heuristic: if non-ralph messages appear, pause
   - Do nothing—let user manage via `ralph checkpoint`
3. Should `--foreground` be the default during development/testing?
