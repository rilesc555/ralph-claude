# PRD: Remote Loop Execution Feasibility Investigation

## Type
Bug Investigation (Investigation/Feasibility Study)

## Problem Statement

Currently, ralph loops can only be monitored and controlled from the same machine where they run. The RPC layer uses Unix domain sockets (`AF_UNIX`), which are inherently local-only. There is no mechanism to:

- Start a ralph loop on a remote machine and monitor it from a local client
- Connect ralph-tui or `ralph-uv attach` to a loop running on a different host
- Use interactive mode (keystroke forwarding) across a network boundary

**Actual behavior:** All ralph-uv communication is local-only via Unix sockets at `~/.local/share/ralph/sockets/<task>.sock`

**Expected behavior:** Users should be able to run loops on remote machines (cloud VPS, LAN servers) and monitor/interact with them from local ralph-tui or ralph-uv attach clients with full interactive mode support.

## Environment

- Ralph-UV: Python, asyncio, PTY-based agent management
- Ralph-TUI: Rust, ratatui, currently uses tmux pipe-pane (not RPC)
- RPC Protocol: JSON-RPC 2.0 over Unix sockets (NDJSON framing)
- Transports: Unix domain sockets only (no TCP/HTTP/WebSocket)
- Both cloud (public IP) and LAN environments need support

## Goals

- Assess feasibility of remote loop monitoring and control
- Identify the minimal changes needed to enable remote connectivity
- Determine security requirements (auth, encryption)
- Evaluate approach options (TCP sockets, SSH tunneling, WebSocket, etc.)
- Understand impact on interactive mode (keystroke forwarding over network)
- Document a recommended architecture for remote execution

## Investigation Stories

### US-001: Full architecture sketch
**Description:** As a developer, I need a complete architectural diagram/document showing all components, their roles, communication paths, and how they compose into the remote execution system.

**Acceptance Criteria:**
- [ ] Diagram showing: ralph-uv daemon, loop runners, RPC layer, OpenZiti overlay, client(s)
- [ ] Define the ralph-uv daemon's responsibilities (listen for start requests, manage loop lifecycles, expose per-loop RPC)
- [ ] Define how loop runners register with the daemon and expose their RPC
- [ ] Define client connection flow: client reads SQLite → if remote, connect via OpenZiti → remote daemon
- [ ] Define the "start loop" request/response contract (repo URL? task dir? branch? iterations?)
- [ ] Define how multiple concurrent loops are isolated (separate checkouts, separate sockets, separate Ziti services or multiplexed?)
- [ ] Document which existing code is reused vs. what's new
- [ ] Address: what happens on disconnect/reconnect, daemon crash, loop crash
- [ ] Define loop completion flow: daemon pushes event → local SQLite marked completed/failed
- [ ] Address stale state: how does local SQLite reconcile if no client was connected when loop finished?
- [ ] Save architecture document to `tasks/remote-loop-execution/architecture.md`

### US-002: Audit current RPC transport layer
**Description:** As a developer, I need to understand exactly how the current Unix socket RPC is implemented so I can identify extension points for OpenZiti transport.

**Acceptance Criteria:**
- [ ] Document all socket creation/binding code paths in `rpc.py`
- [ ] Document all client connection code in `attach.py`
- [ ] Identify where `AF_UNIX` is hardcoded vs. abstracted
- [ ] Map the full lifecycle: server start → client connect → subscribe → events → disconnect
- [ ] Note any assumptions that would break over a network (latency, ordering, disconnects)
- [ ] Update notes in prd.json with findings

### US-003: Evaluate OpenZiti Python SDK capabilities
**Description:** As a developer, I need to understand the OpenZiti Python SDK's capabilities for both server (daemon) and client (attach/TUI proxy) use.

**Acceptance Criteria:**
- [ ] Document how to create a Ziti socket server (bind to a Ziti service) using `openziti` Python SDK
- [ ] Document how to connect as a Ziti client to a service using `openziti` Python SDK
- [ ] Determine if SDK supports asyncio (needed for RPC server integration)
- [ ] Determine how identity files (.json/.jwt) are loaded and used
- [ ] Test or document: can one SDK process host multiple services (one per loop)?
- [ ] Identify SDK limitations, maturity, and maintenance status
- [ ] Update notes in prd.json with findings

### US-004: Assess interactive mode over network
**Description:** As a developer, I need to determine if real-time keystroke forwarding is feasible over OpenZiti with acceptable latency.

**Acceptance Criteria:**
- [ ] Analyze current interactive mode flow (Esc sending, keystroke forwarding via `write_pty`)
- [ ] Identify latency requirements for interactive mode to feel responsive
- [ ] Determine if JSON-RPC overhead per keystroke is acceptable or if batching/raw mode is needed
- [ ] Test or estimate round-trip times for keystroke → agent response over OpenZiti
- [ ] Document any protocol changes needed for responsive interactive mode
- [ ] Update notes in prd.json with findings

### US-005: Design unified session DB schema for remote loops
**Description:** As a developer, I need to extend the SQLite session schema so remote loops appear alongside local loops, with enough info for clients to connect via OpenZiti.

**Acceptance Criteria:**
- [ ] Design schema changes: add fields for remote vs local, Ziti service name, identity file path, remote host label
- [ ] Define how a remote loop gets registered in local SQLite (on "start remote loop" command? on first attach?)
- [ ] Ensure `ralph-uv status` and ralph-tui can list remote loops without changes to their query logic
- [ ] Define how stale remote entries are cleaned up (daemon unreachable, loop finished)
- [ ] Document how ralph-tui connects to remote loops (Python SDK bridge? Ziti tunneler? embedded?)
- [ ] Update notes in prd.json with findings

### US-006: Prototype feasibility test
**Description:** As a developer, I need to validate the architecture with a minimal proof-of-concept over OpenZiti.

**Acceptance Criteria:**
- [ ] Set up OpenZiti network (controller + edge router, or CloudZiti)
- [ ] Enroll a ralph-uv server identity and a client identity
- [ ] Create a Ziti service for ralph RPC
- [ ] Connect `ralph-uv attach` to a loop via OpenZiti (not Unix socket)
- [ ] Verify event streaming works over Ziti (output events, state changes)
- [ ] Test interactive mode keystroke forwarding over Ziti
- [ ] Measure latency overhead vs Unix socket baseline
- [ ] Document any issues discovered during prototype
- [ ] Update notes in prd.json with findings and measurements

## Hypotheses

1. **Python SDK is sufficient for both sides:** The `openziti` Python SDK can handle both the server (daemon binding to Ziti service) and client (attach/proxy connecting to Ziti service) without needing the tunneler.

2. **Interactive mode latency is acceptable:** Keystroke-by-keystroke forwarding over JSON-RPC + OpenZiti should be fast enough (<10ms overhead on LAN, acceptable on WAN) since Ziti uses persistent connections.

3. **Unified SQLite eliminates proxy need:** By registering remote loops in the same local SQLite, clients (attach, TUI) just check connection info and dial accordingly — no separate proxy process needed.

4. **Ralph-TUI needs RPC migration first:** The TUI currently uses tmux pipe-pane, so it would need to switch to RPC-based communication before remote support is useful.

5. **OpenZiti eliminates custom auth:** Zero-trust identity model means no need for API keys, TLS cert management, or custom auth — Ziti handles it all at the network layer.

6. **Daemon is a thin supervisor:** The daemon's main job is accepting "start loop" JSON-RPC calls and spawning loop runner subprocesses. Each loop runner manages its own per-loop RPC. Daemon exposes them as Ziti services (or sub-routes of one service).

## Related Code

- `src/ralph_uv/rpc.py` - RPC server implementation (Unix socket, asyncio)
- `src/ralph_uv/attach.py` - Attach client (connects to Unix socket, hotkey handling)
- `src/ralph_uv/interactive.py` - Interactive mode controller, PTY management
- `src/ralph_uv/loop.py` - Loop runner (starts RPC server, manages agent)
- `src/ralph_uv/session.py` - Session database and tmux management
- `ralph-tui/src/main.rs` - TUI entry point, tmux attachment
- `ralph-tui/src/rpc.rs` - TUI RPC client (if exists)

## Non-Goals

- Actually implementing production-ready remote execution (this is feasibility only)
- Building a web UI for remote monitoring
- Multi-user concurrent access to the same loop
- Cross-platform remote access (Windows → Linux, etc.)
- Auto-discovery of remote ralph instances (mDNS, etc.)

## Functional Requirements

- FR-1: Produce a complete architecture document covering all components and their interactions
- FR-2: Document the current RPC transport layer and its extension points
- FR-3: Evaluate OpenZiti Python SDK for both server and client use cases
- FR-4: Provide latency measurements or estimates for interactive mode over OpenZiti
- FR-5: Design unified SQLite schema for remote loop registration
- FR-6: Create a working proof-of-concept demonstrating OpenZiti-based RPC connection
- FR-7: Define the daemon's "start loop" API contract

## Technical Considerations

- The RPC server already uses asyncio, making transport extension relatively straightforward
- NDJSON framing is transport-agnostic (works over any stream socket)
- Interactive mode sends individual keystrokes — high-frequency small messages
- PTY output can be high-bandwidth (agent streaming large file contents)
- Ralph-TUI is Rust — need to evaluate OpenZiti Rust SDK maturity
- Network disconnects need graceful handling (reconnection, state recovery)
- OpenZiti adds a dependency (tunneler binary or SDK library)
- OpenZiti Python SDK: `openziti` package on PyPI
- OpenZiti Rust SDK: `ziti` crate (check maturity/maintenance status)
- Daemon process needs to manage lifecycle of multiple loop runners
- Consider systemd unit file for the ralph-uv daemon on remote machines

## Success Metrics

- Clear go/no-go recommendation for remote execution via OpenZiti
- If go: estimated implementation effort in developer-days
- Proof-of-concept successfully connects over OpenZiti and streams events
- Interactive mode latency characterized over Ziti (LAN and WAN)
- OpenZiti SDK maturity confirmed for both Python (server) and Rust (TUI client)
- Daemon architecture documented for remote loop starting

## Resolved Questions

- **Protocol:** JSON-RPC (keep the existing protocol, transport-agnostic)
- **Relay/proxy:** No relay service. Ralph-uv IS the server; ralph-tui and ralph-uv attach are clients.
- **Client architecture:** Ralph-tui connects through a local ralph-uv proxy (ralph-uv as server model)
- **Credentials:** Config file and environment variables
- **Remote starting:** Yes — remote clients can start NEW loops on the remote machine. A small always-running service on the remote box listens for requests to start loops.
- **Networking:** OpenZiti for transport (zero-trust overlay network with built-in identity, encryption, and mutual auth)

## Architectural Direction

Ralph-uv should be thought of as a **server**. Ralph-tui and `ralph-uv attach` are **clients**.

### Core Insight: Unified Session DB

The key architectural decision is that **remote loops are registered in the same local SQLite database as local loops**. This means:

- `ralph-uv status` shows both local and remote loops
- `ralph-tui` lists all loops (local + remote) from SQLite
- `ralph-uv attach <task>` looks up the loop in SQLite, sees connection info, and connects appropriately (Unix socket for local, OpenZiti for remote)
- **No separate proxy process needed** — the Ziti transport is an implementation detail inside the attach/TUI client code

### Components

1. **Remote daemon** (ralph-uv daemon, runs on remote machine):
   - Persistent process, listens for requests over OpenZiti
   - Accepts "start loop" requests, spawns loop runner subprocesses
   - Exposes per-loop RPC over OpenZiti (same JSON-RPC protocol)
   - Manages loop lifecycles on the remote machine

2. **Local ralph-uv** (existing, enhanced):
   - When a remote loop is started, registers it in local SQLite with Ziti connection info
   - `ralph-uv attach` checks SQLite — if remote, connects via OpenZiti Python SDK
   - `ralph-uv status` shows all loops regardless of location

3. **Ralph-tui** (existing, enhanced):
   - Reads loops from SQLite (already does this)
   - For remote loops, connects via OpenZiti (through Python SDK bridge or direct)
   - No awareness of remote vs local needed in the UI beyond a location indicator

4. **OpenZiti overlay**:
   - Handles encryption, mutual auth, NAT traversal
   - Remote daemon binds to a Ziti service
   - Local clients dial the Ziti service
   - Identity files manually provisioned via Ziti admin console

## Resolved Questions (Round 2)

- **SDK:** Python SDK (`openziti`) for everything — both server-side (ralph-uv daemon) and client-side (ralph-tui will need a Python proxy/bridge, or ralph-uv attach becomes the primary client)
- **Daemon vs loop runner:** Separate process. Daemon listens for requests, spawns loop runner subprocesses.
- **Concurrent loops:** Separate checkouts of the base branch per loop, same model as local concurrent loops.
- **Identity provisioning:** Manual — user creates identities in Ziti admin console, provides identity file to ralph-uv config. SDK just needs to load the identity JSON/JWT.

## Remote Environment Bootstrapping

When a job is sent to a remote machine, the following must happen before the loop can start:

### Code Sync Flow
1. Local machine creates the branch and pushes to origin (so it's backed up)
2. Local machine pushes the branch to a bare repo on the remote (via git push over Ziti or SSH)
3. Remote daemon checks out the branch into a working directory (separate checkout per loop)

### Agent CLI Auto-Install
- Daemon checks if the requested agent CLI (`claude` or `opencode`) is installed
- If missing, installs it automatically (one-time per agent type)
- User's API keys / claude auth are pre-configured manually on the remote

### Project Dependencies ("Task 0")
- First iteration of the loop is effectively "task 0": install project deps
- The agent discovers lockfiles (package.json, Cargo.toml, pyproject.toml, etc.) which may not be in the repo root
- Agent runs appropriate install commands (npm install, cargo build, pip install, etc.)
- This happens naturally as part of the first story's acceptance criteria (e.g., "Typecheck passes" will fail until deps are installed)

### Manual One-Time Setup (per remote machine)
- Install: git, jq, python 3.12+, bash
- Authenticate: `claude` CLI login, or set API keys for opencode
- Install: ralph-uv daemon
- Configure: OpenZiti identity file
- Set up: bare git repo directory for receiving pushes

### What Gets Synced With the Code
- The git repo includes: prd.json, prompt.md, agents/ scripts, AGENTS.md
- These are part of the repo, so they arrive via git push automatically
- No separate file sync needed for ralph infrastructure files

## Lifecycle: Loop Completion

- **Local loops:** Loop runner marks its own session as `completed`/`failed` in SQLite (existing behavior).
- **Remote loops:** Remote daemon pushes a completion event over Ziti. Local SQLite is updated to `completed`/`failed`.
- **Records are never deleted** — just marked with terminal status.
- **Edge case:** If no client is actively connected when a remote loop finishes, the local SQLite may be stale. On next `attach` or `status`, the client should reconcile with the remote daemon.

## Open Questions

- How does ralph-tui (Rust) use OpenZiti? Options: embed Python SDK via subprocess/FFI, use Ziti C SDK bindings, or require ziti-edge-tunnel running locally.
- What does the daemon's "start loop" API look like? (task dir, git repo URL, branch, max iterations, etc.)
- Should each remote loop be its own Ziti service, or one service per daemon with task-name routing?
- Should there be a lightweight background process that stays connected to remote daemons for status updates, or is lazy reconciliation on attach/status sufficient?

## Merge Target

None - this is an investigation/feasibility study. Results inform future implementation work.
