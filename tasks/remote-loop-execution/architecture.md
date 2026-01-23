# Remote Loop Execution Architecture

## Overview

This document describes how ralph loops can be dispatched to remote machines and monitored/controlled via ralph-tui or ralph-uv attach over an OpenZiti overlay network. The design extends the existing Unix socket RPC with a network transport layer while preserving the local execution path unchanged.

---

## Component Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           LOCAL MACHINE (Client)                             │
│                                                                             │
│  ┌──────────────┐     ┌──────────────────┐     ┌────────────────────────┐  │
│  │  ralph-tui   │────▶│  Local SQLite DB │◀────│  ralph-uv start-remote │  │
│  │  (Rust TUI)  │     │  sessions.db     │     │  (CLI command)         │  │
│  └──────┬───────┘     └──────────────────┘     └────────────┬───────────┘  │
│         │                                                    │              │
│         │ if remote: connect via Ziti SDK                    │ git push     │
│         │                                                    │ + RPC start  │
│  ┌──────┴──────────────────────────────────────────────────┐ │              │
│  │                   OpenZiti Client SDK                    │ │              │
│  │  Identity: client-{hostname}.json                       │ │              │
│  └──────┬──────────────────────────────────────────────────┘ │              │
│         │                                                    │              │
└─────────┼────────────────────────────────────────────────────┼──────────────┘
          │ Ziti overlay (encrypted, zero-trust)               │
          │                                                    │
┌─────────┼────────────────────────────────────────────────────┼──────────────┐
│         │              REMOTE MACHINE (Server)               │              │
│  ┌──────┴──────────────────────────────────────────────────┐ │              │
│  │                   OpenZiti Server SDK                    │ │              │
│  │  Identity: server-{hostname}.json                       │ │              │
│  └──────┬──────────────────────────────────────────────────┘ │              │
│         │                                                    │              │
│  ┌──────┴───────────────────────────────────────────────────────────────┐   │
│  │                      ralph-uv daemon (ralphd)                        │   │
│  │                                                                      │   │
│  │  Responsibilities:                                                   │   │
│  │  - Listen for start-loop requests via Ziti service "ralph-control"   │   │
│  │  - Manage loop lifecycles (start, stop, checkpoint)                  │   │
│  │  - Git operations (receive pushes, checkout branches)                │   │
│  │  - Agent CLI installation checks                                     │   │
│  │  - Per-loop Ziti service registration                                │   │
│  │  - Health/status reporting                                           │   │
│  │                                                                      │   │
│  │  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐         │   │
│  │  │  Loop Runner 1 │  │  Loop Runner 2 │  │  Loop Runner N │         │   │
│  │  │  (US-003)      │  │  (US-004)      │  │  (US-007)      │         │   │
│  │  │                │  │                │  │                │         │   │
│  │  │ Ziti Service:  │  │ Ziti Service:  │  │ Ziti Service:  │         │   │
│  │  │ ralph-loop-    │  │ ralph-loop-    │  │ ralph-loop-    │         │   │
│  │  │ {task}-{uuid}  │  │ {task}-{uuid}  │  │ {task}-{uuid}  │         │   │
│  │  │                │  │                │  │                │         │   │
│  │  │ ┌────────────┐│  │ ┌────────────┐│  │ ┌────────────┐│         │   │
│  │  │ │  RPC Server ││  │ │  RPC Server ││  │ │  RPC Server ││         │   │
│  │  │ │(JSON-RPC2.0)││  │ │(JSON-RPC2.0)││  │ │(JSON-RPC2.0)││         │   │
│  │  │ └────────────┘│  │ └────────────┘│  │ └────────────┘│         │   │
│  │  │ ┌────────────┐│  │ ┌────────────┐│  │ ┌────────────┐│         │   │
│  │  │ │  PTY Agent  ││  │ │  PTY Agent  ││  │ │  PTY Agent  ││         │   │
│  │  │ │(claude/ocode)│  │ │(claude/ocode)│  │ │(claude/ocode)│         │   │
│  │  │ └────────────┘│  │ └────────────┘│  │ └────────────┘│         │   │
│  │  └────────────────┘  └────────────────┘  └────────────────┘         │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                    Working Directories                                │   │
│  │  ~/ralph-workspaces/                                                  │   │
│  │  ├── {project-name}/                                                  │   │
│  │  │   ├── bare.git/              (bare repo, receives pushes)          │   │
│  │  │   └── checkouts/                                                   │   │
│  │  │       ├── {task-1}-{uuid}/   (isolated working tree)               │   │
│  │  │       └── {task-2}-{uuid}/   (isolated working tree)               │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Component Responsibilities

### 1. ralph-uv daemon (`ralphd`)

A long-running process on the remote machine that:

1. **Listens for start requests** - Binds to Ziti service `ralph-control-{hostname}` for incoming job requests
2. **Manages loop lifecycles** - Starts, monitors, stops, and checkpoints loop runners
3. **Git operations** - Maintains bare repos, creates working tree checkouts per job
4. **Agent CLI management** - Checks for and installs agent CLIs (claude, opencode) on first use
5. **Per-loop service registration** - Registers a unique Ziti service per active loop so clients can connect directly to any loop
6. **Health reporting** - Responds to health checks and status queries from clients
7. **Cleanup** - Removes finished loop services, optionally cleans old checkouts

**Process model**: Single Python process using asyncio. Loop runners are child processes managed via PTY (reuses existing `PtyAgent`).

**State**: In-memory registry of active loops + optional local SQLite for persistence across daemon restarts.

### 2. Loop Runners (per-loop processes)

Each loop runner is essentially the existing `LoopRunner` class running as a child process of the daemon, with one modification:

- **RPC transport**: Instead of binding a Unix socket, the RPC server binds to a Ziti service (or the daemon proxies Ziti connections to the Unix socket)

**Two transport options** (see "Transport Strategy" below):
- **Option A (Proxy)**: Loop runner uses Unix socket as today; daemon proxies Ziti connections to it
- **Option B (Native)**: Loop runner's RPC server directly binds a Ziti service

**Recommended: Option A (Proxy)** - Keeps loop runner code unchanged; daemon handles all Ziti concerns.

### 3. RPC Layer

The existing JSON-RPC 2.0 protocol (`docs/protocol.md`) is preserved exactly. The only change is the transport layer beneath it:

| Scenario | Transport |
|----------|-----------|
| Local loop, local client | Unix domain socket (unchanged) |
| Remote loop, remote client (daemon-local) | Unix domain socket (unchanged) |
| Remote loop, remote client (over network) | Ziti service → daemon proxy → Unix socket |

**Protocol wire format**: NDJSON (newline-delimited JSON) - works identically over any stream-oriented transport.

### 4. OpenZiti Overlay

**Services exposed per remote machine**:

| Service Name | Purpose | Bound by |
|--------------|---------|----------|
| `ralph-control-{hostname}` | Daemon control (start/stop/list loops) | Daemon |
| `ralph-loop-{task}-{uuid}` | Per-loop RPC (attach, interactive mode) | Daemon (proxy) |

**Identities**:
- **Server identity** (`server-{hostname}.json`): Enrolled on remote machine, can bind services
- **Client identity** (`client-{hostname}.json`): Enrolled on local machine, can dial services

**Authentication**: Zero-trust (Ziti handles mutual TLS, no open ports, no VPN needed).

### 5. Clients (ralph-tui / ralph-uv attach)

Clients read the local SQLite database to discover loops. For remote loops, the DB entry includes:
- `transport = "ziti"` (vs `"local"`)
- `ziti_service` - The service name to dial
- `ziti_identity` - Path to the client identity file
- `remote_host` - Human-readable remote label

**Connection flow**:
1. Client reads `sessions.db` → finds loop entry
2. If `transport == "local"`: Connect to Unix socket (existing path)
3. If `transport == "ziti"`: Load identity → Dial Ziti service → Get stream → Use NDJSON JSON-RPC

---

## Client Connection Flow (Detailed)

```
Client (ralph-tui or ralph-uv attach)
    │
    ├─ 1. Query sessions.db: SELECT * FROM sessions WHERE task_name = ?
    │
    ├─ 2. Check transport field
    │      ├─ "local" → connect to ~/.local/share/ralph/sockets/{task}.sock
    │      └─ "ziti"  → continue below
    │
    ├─ 3. Load Ziti identity from ziti_identity path
    │
    ├─ 4. Dial Ziti service (ziti_service field)
    │      └─ Returns a stream (socket-like object)
    │
    ├─ 5. Use existing NDJSON JSON-RPC protocol over the stream
    │      ├─ subscribe(events: ["*"])
    │      ├─ get_status()
    │      └─ receive event notifications
    │
    └─ 6. Interactive mode works identically:
           ├─ set_interactive_mode(enabled: true)
           ├─ write_pty(data: "keystrokes")
           └─ receive output events
```

---

## Start Loop Request/Response Contract

### Request: `start_loop`

Sent by local client to daemon's control service (`ralph-control-{hostname}`).

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "start_loop",
  "params": {
    "origin_url": "git@github.com:user/project.git",
    "branch": "ralph/my-feature",
    "task_dir": "tasks/my-feature",
    "max_iterations": 50,
    "agent": "claude",
    "prd_path": "tasks/my-feature/prd.json",
    "push_refspec": "refs/heads/ralph/my-feature"
  }
}
```

### Response: Success

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "status": "started",
    "loop_id": "my-feature-a1b2c3",
    "ziti_service": "ralph-loop-my-feature-a1b2c3",
    "task_name": "my-feature",
    "checkout_dir": "/home/user/ralph-workspaces/project/checkouts/my-feature-a1b2c3"
  }
}
```

### Response: Error (agent not installed)

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32001,
    "message": "Agent 'claude' not found. Install it or use --agent opencode.",
    "data": {
      "available_agents": ["opencode"],
      "install_instructions": "See https://docs.anthropic.com/..."
    }
  }
}
```

### Additional Daemon Control Methods

| Method | Purpose |
|--------|---------|
| `start_loop` | Start a new loop on this remote |
| `stop_loop` | Stop a running loop by ID |
| `list_loops` | List all active loops on this remote |
| `get_health` | Daemon health check (uptime, resource usage) |
| `get_agents` | List available agent CLIs |

---

## Remote Bootstrapping Flow

### Full Sequence: `ralph-uv start-remote`

```
User runs: ralph-uv start-remote tasks/my-feature --remote my-server

  1. LOCAL: Validate prd.json exists and has branchName
  2. LOCAL: Ensure local branch exists and is committed
  3. LOCAL: Push branch to origin (git push origin ralph/my-feature)
  4. LOCAL: Load client Ziti identity
  5. LOCAL: Dial ralph-control-{my-server} service

  6. REMOTE (daemon receives start_loop request):
     a. Clone/fetch from origin_url into bare repo:
        ~/ralph-workspaces/{project}/bare.git
        (if bare repo doesn't exist, create it)
     b. Fetch the specified branch into bare repo
     c. Create isolated working tree checkout:
        git worktree add ~/ralph-workspaces/{project}/checkouts/{task}-{uuid} {branch}
     d. Check agent CLI is available:
        - `which claude` or `which opencode`
        - If missing: attempt auto-install (see Agent CLI Install)
        - If auto-install fails: return error to client
     e. Register Ziti service: ralph-loop-{task}-{uuid}
     f. Start loop runner process (iteration 0 = dep install, then normal loop)
     g. Return success response with ziti_service name

  7. LOCAL: Receive success response
  8. LOCAL: Register in local sessions.db:
     INSERT INTO sessions (task_name, ..., transport, ziti_service, ziti_identity, remote_host)
     VALUES ('my-feature', ..., 'ziti', 'ralph-loop-my-feature-a1b2c3', '~/.ziti/client.json', 'my-server')
  9. LOCAL: Print "Loop started on my-server. Use 'ralph-uv attach my-feature' to monitor."
```

### Agent CLI Auto-Install

The daemon attempts installation if the agent binary is not found:

| Agent | Install Method |
|-------|---------------|
| `claude` | `npm install -g @anthropic-ai/claude-code` (requires Node.js) |
| `opencode` | `curl -fsSL https://opencode.ai/install \| sh` (or Go binary download) |

If installation fails, the daemon returns an error. The user must install manually.

### Task 0 (Dependency Installation)

The first iteration of any remote loop is a "bootstrap iteration":

1. The agent prompt includes a special prefix: `"Before starting on stories, install project dependencies..."`
2. The agent discovers lockfiles and installs deps:
   - `package-lock.json` / `yarn.lock` / `pnpm-lock.yaml` → npm/yarn/pnpm install
   - `requirements.txt` / `pyproject.toml` → pip install / uv sync
   - `go.mod` → go mod download
   - `Cargo.lock` → cargo fetch
3. If lockfiles are not in the repo root, the agent searches common locations
4. After deps are installed, the agent proceeds to the first story normally

**Note**: Task 0 is handled naturally by the agent prompt, not special daemon logic. The prompt.md template gains a conditional section:

```markdown
<!-- agent:* -->
## First-Run Setup (Remote Execution)
If this is iteration 1 and the project has uninstalled dependencies (e.g., missing node_modules/),
install them before proceeding to stories. Look for lockfiles (package-lock.json, yarn.lock, etc.)
and run the appropriate install command.
<!-- /agent:* -->
```

### Git Push to Remote

The daemon maintains bare repos that accept pushes:

```bash
# First time: daemon creates bare repo
git init --bare ~/ralph-workspaces/{project}/bare.git

# Client pushes via Ziti (git-over-ziti, or daemon exposes git-receive-pack)
# Alternative: client pushes to origin, daemon fetches from origin
```

**Recommended approach**: Client pushes to origin (GitHub/GitLab), daemon fetches from origin. This avoids implementing git-over-Ziti and leverages existing git remotes.

### Subsequent Jobs to Same Remote

- **Same project, different branch**: Daemon fetches new branch, creates new worktree checkout
- **Same project, same branch**: Daemon cleans existing checkout (`git reset --hard && git pull`), or creates fresh worktree
- **Different project**: Daemon creates new bare repo + checkout

---

## Concurrent Loop Isolation

### Strategy: Separate Checkouts + Separate Services

Each concurrent loop gets:

| Resource | Isolation Method |
|----------|-----------------|
| Working directory | Separate `git worktree` under `~/ralph-workspaces/{project}/checkouts/{task}-{uuid}/` |
| Ziti service | Unique service name: `ralph-loop-{task}-{uuid}` |
| RPC socket | Separate Unix socket: `/tmp/ralph-{task}-{uuid}.sock` |
| PTY | Separate PTY pair per loop runner process |
| Agent process | Separate child process per loop |
| Git operations | Worktree isolation (each has own HEAD, index) |

### Why Not Multiplex?

A single Ziti service with multiplexed loop channels was considered but rejected:
- Adds complexity to the protocol (channel IDs, routing)
- Clients must understand multiplexing
- Single service failure affects all loops
- Per-service approach matches the existing per-socket architecture

### Resource Limits

The daemon should enforce:
- Maximum concurrent loops per remote (configurable, default: 4)
- Maximum total disk usage for checkouts
- Per-loop timeout (configurable, default: 24h)

---

## Existing Code Reuse vs. New Code

### Reused (Unchanged)

| Component | File | Notes |
|-----------|------|-------|
| JSON-RPC 2.0 protocol | `rpc.py` | Wire format identical over any stream |
| Session state model | `rpc.py` (`SessionState`) | Same state fields |
| RPC method handlers | `rpc.py` | All methods work as-is |
| Loop iteration logic | `loop.py` (`LoopRunner`) | Core loop unchanged |
| Agent abstraction | `agents.py` | Claude/OpenCode agents unchanged |
| PTY management | `interactive.py` | `PtyAgent`, `InteractiveController` |
| Prompt building | `prompt.py` | Template system unchanged |
| Branch management | `branch.py` | Git branch operations |
| Protocol spec | `docs/protocol.md` | NDJSON JSON-RPC unchanged |

### Modified (Extended)

| Component | File | Changes |
|-----------|------|---------|
| Session DB schema | `session.py` | Add `transport`, `ziti_service`, `ziti_identity`, `remote_host` columns |
| Attach client | `attach.py` | Add Ziti transport option alongside Unix socket |
| CLI | `cli.py` | Add `start-remote`, `list-remotes` commands |
| RPC server start | `rpc.py` | Optional: accept connections from daemon proxy (minor) |

### New Code

| Component | Purpose |
|-----------|---------|
| `daemon.py` | ralph-uv daemon process (`ralphd`) |
| `daemon_rpc.py` | Daemon control RPC (start_loop, stop_loop, list_loops, health) |
| `transport.py` | Transport abstraction: `UnixTransport`, `ZitiTransport` |
| `ziti.py` | OpenZiti SDK wrapper (identity loading, service binding/dialing) |
| `remote.py` | Remote bootstrapping logic (git fetch, worktree, agent install) |
| `cli.py` extensions | `start-remote`, `list-remotes`, `register-remote` commands |

---

## Failure Handling

### Disconnect / Reconnect

| Scenario | Behavior |
|----------|----------|
| Client disconnects during attach | Loop continues autonomously (existing behavior). Client can re-attach anytime. |
| Client loses Ziti connectivity | Same as disconnect. Ziti handles reconnection transparently when possible. |
| Client reconnects after gap | `get_status` returns current state. `subscribe` resumes events from now (missed events are lost, but `recent_output` buffer provides last 200 lines). |

### Daemon Crash

| Scenario | Behavior |
|----------|----------|
| Daemon crashes while loops running | Loop runner processes are children of daemon → they receive SIGHUP and terminate. |
| Daemon restarts after crash | Scans `~/ralph-workspaces/` for active loop PID files. If PIDs still alive, re-registers their Ziti services. If dead, marks as failed. |
| Daemon crash during git operations | Worktree may be in inconsistent state. Daemon cleans up on restart (delete incomplete checkouts). |

**Mitigation**: Daemon uses a PID file + lock for each active loop. On restart:
1. Read all `*.pid` files in workspaces
2. Check if PIDs are alive (`kill -0`)
3. Re-adopt living processes, clean up dead ones

### Loop Runner Crash

| Scenario | Behavior |
|----------|----------|
| Agent process crashes | Loop runner catches, increments failure counter, starts next iteration (existing failover logic) |
| Loop runner process crashes | Daemon detects via waitpid/SIGCHLD. Marks loop as failed. Emits `state_change` event to any connected clients. |
| Loop runner hangs | Daemon enforces per-loop timeout. Sends SIGTERM, waits, then SIGKILL. |

---

## Loop Completion Flow

```
Loop Runner detects all stories passes: true
    │
    ├─ 1. Loop runner sets status = "completed"
    │
    ├─ 2. RPC server emits state_change event: {status: "completed"}
    │      └─ Connected clients receive this immediately
    │
    ├─ 3. Loop runner exits (process terminates)
    │
    ├─ 4. Daemon detects child exit (waitpid/SIGCHLD)
    │      a. Deregisters Ziti service for this loop
    │      b. Updates daemon's internal registry
    │      c. Pushes completion event to daemon control service:
    │         {"method": "event", "params": {"type": "loop_completed", "data": {...}}}
    │
    ├─ 5. If client is connected to daemon control service:
    │      └─ Receives loop_completed event in real-time
    │
    └─ 6. Client updates local sessions.db:
           UPDATE sessions SET status = 'completed' WHERE task_name = ?
```

### Push-Based Completion Notification

The daemon pushes a `loop_completed` event on the control service. Clients subscribed to the control service receive it.

**Event payload**:
```json
{
  "type": "loop_completed",
  "data": {
    "loop_id": "my-feature-a1b2c3",
    "task_name": "my-feature",
    "status": "completed",
    "iterations_used": 12,
    "final_story": "US-007",
    "branch": "ralph/my-feature",
    "completed_at": "2026-01-23T15:30:00Z"
  }
}
```

---

## Stale State Reconciliation

### Problem

If no client is connected when a loop finishes on the remote, the local SQLite still shows `status = "running"`.

### Solution: Multi-Layer Reconciliation

1. **Active polling (foreground)**:
   When `ralph-uv status` or `ralph-tui` queries sessions, for each remote session with `status = "running"`:
   - Dial the loop's Ziti service
   - If connection fails (service deregistered): Mark as `"unknown"`
   - If connects: Call `get_status` → update local DB with actual status

2. **Daemon heartbeat (background, optional)**:
   The daemon periodically pushes status updates on the control service:
   ```json
   {"method": "event", "params": {"type": "heartbeat", "data": {"active_loops": [...]}}}
   ```
   A background process or ralph-tui can subscribe to these.

3. **On-attach reconciliation**:
   When a client tries to attach to a remote loop:
   - If Ziti service is gone: Mark session as `"completed"` or `"failed"` (ambiguous without more info)
   - If Ziti service exists: Proceed normally

4. **Explicit sync command**:
   `ralph-uv sync-remote my-server` — Connects to daemon, lists all loops, reconciles local DB.

### Staleness Window

| Strategy | Max stale time |
|----------|---------------|
| Active polling on status query | Stale until next `ralph-uv status` |
| Daemon heartbeat (30s interval) | ~30 seconds |
| On-attach | Stale until user tries to attach |
| Explicit sync | Stale until user runs sync |

**Recommended default**: Active polling on status query + on-attach. Heartbeat is optional for near-real-time updates.

---

## Transport Strategy Detail

### Proxy Architecture (Recommended)

```
Client (Ziti SDK)
    │
    │ Dial "ralph-loop-{task}-{uuid}"
    │
    ▼
Daemon (Ziti server, bound to service)
    │
    │ Accept connection → spawn proxy task
    │ Connect to local Unix socket: /tmp/ralph-{task}-{uuid}.sock
    │ Bidirectional byte forwarding (asyncio streams)
    │
    ▼
Loop Runner RPC Server (Unix socket, unchanged)
```

**Advantages**:
- Loop runner code is completely unchanged
- Daemon controls service lifecycle (register/deregister on loop start/stop)
- Single Ziti identity for the daemon (no per-loop identities)
- Existing Unix socket permissions still apply locally

**Implementation**: ~50 lines of asyncio bidirectional stream copy:
```python
async def proxy_connection(ziti_reader, ziti_writer, unix_reader, unix_writer):
    async def forward(src, dst):
        while True:
            data = await src.read(4096)
            if not data:
                break
            dst.write(data)
            await dst.drain()
    await asyncio.gather(
        forward(ziti_reader, unix_writer),
        forward(unix_reader, ziti_writer),
    )
```

---

## Configuration

### Remote Machine Setup (One-Time Manual)

The user must perform these steps once per remote machine:

1. **Install prerequisites**: `git`, `python3.11+`, `jq`, `node` (for Claude CLI)
2. **Install ralph-uv**: `pip install ralph-uv` or `uv tool install ralph-uv`
3. **Enroll Ziti identity**: Download and enroll server identity file
4. **Configure API keys**: Set `ANTHROPIC_API_KEY` (for Claude) or relevant keys in environment
5. **Authenticate agent CLIs**: `claude auth login` or equivalent
6. **Start daemon**: `ralphd --identity ~/.ziti/server.json` (can be a systemd service)

### Local Machine Setup (One-Time)

1. **Enroll Ziti identity**: Download and enroll client identity file
2. **Register remote**: `ralph-uv register-remote my-server --identity ~/.ziti/client.json`
   - This stores the remote's Ziti service name and identity path in local config

### Config File: `~/.config/ralph/remotes.toml`

```toml
[remotes.my-server]
ziti_identity = "~/.ziti/client-home.json"
control_service = "ralph-control-my-server"
label = "Home Server (AMD Ryzen 9)"

[remotes.cloud-1]
ziti_identity = "~/.ziti/client-cloud.json"
control_service = "ralph-control-cloud-1"
label = "Cloud GPU (A100)"
```

---

## Security Considerations

1. **Zero-trust networking**: OpenZiti provides mutual TLS, no open ports needed on either side
2. **Identity-based access**: Each machine has its own enrolled identity; revocable via Ziti controller
3. **No shared secrets over wire**: API keys stay on the remote machine, never transmitted
4. **Socket permissions**: Local Unix sockets remain 0600 (owner-only)
5. **Code execution**: Only code from git repos is executed; daemon validates origin URLs against allowlist (optional)

---

## Future Considerations

- **Multi-tenant**: Multiple users sharing a remote machine (separate daemon instances or user isolation)
- **Resource monitoring**: CPU/memory/disk metrics per loop exposed via `get_health`
- **Log streaming**: Full log persistence on remote with on-demand retrieval
- **Git-over-Ziti**: Direct `git push` to remote bare repo over Ziti (eliminates origin dependency)
- **Auto-scaling**: Daemon requesting cloud instances and self-configuring Ziti enrollment
