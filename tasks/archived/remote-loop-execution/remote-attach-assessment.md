# Remote Attach/Monitoring Assessment

## Overview

This document assesses the feasibility of remote session attach over OpenZiti for both agent types (opencode-server and tmux/claude), including latency analysis and architecture recommendations.

---

## 1. Remote OpenCode Attach Flow

### Architecture: HTTP Proxy over Ziti

OpenCode's server mode is **inherently network-ready**. The `opencode serve` command exposes a standard HTTP API that `opencode attach` connects to. Remote attach simply requires proxying this HTTP port over Ziti.

```
┌─────────────────────────────────────────────────────────────┐
│ LOCAL MACHINE                                                │
│                                                              │
│  opencode attach http://<ziti-intercept>:<port>              │
│       │                                                      │
│       ▼                                                      │
│  ┌─────────────────────────────────────────┐                 │
│  │ Ziti Tunneler (intercept mode)          │                 │
│  │ Intercepts traffic to ziti-intercept IP │                 │
│  │ Routes through Ziti overlay             │                 │
│  └─────────────────────────────────────────┘                 │
└──────────────────────┬───────────────────────────────────────┘
                       │ Ziti overlay (mTLS, zero-trust)
┌──────────────────────┼───────────────────────────────────────┐
│ REMOTE MACHINE       │                                        │
│                      ▼                                        │
│  ┌─────────────────────────────────────────┐                 │
│  │ Ziti Tunneler (host mode)               │                 │
│  │ Terminates Ziti → forwards to localhost │                 │
│  └─────────────────────┬───────────────────┘                 │
│                        │ localhost:<port>                      │
│                        ▼                                      │
│  ┌─────────────────────────────────────────┐                 │
│  │ opencode serve --port <port>            │                 │
│  │ Full HTTP API (health, sessions, SSE)   │                 │
│  └─────────────────────────────────────────┘                 │
└──────────────────────────────────────────────────────────────┘
```

### Why This Works Trivially

1. **`opencode attach` is already an HTTP client** — it connects to a URL, not a local socket. The URL can point anywhere reachable.
2. **Ziti tunnelers provide transparent intercept** — the local machine sees a normal TCP socket; the remote sees localhost traffic.
3. **All opencode APIs are stateless HTTP** — no session affinity, no upgrade protocols, no raw TCP framing.
4. **SSE (Server-Sent Events) works over standard HTTP** — long-lived GET with chunked transfer encoding. Ziti tunnelers handle this like any other TCP stream.

### Connection Flow

1. Local client runs: `ralph-uv attach <task>`
2. Ralph-uv reads sessions.db → finds `session_type="opencode-server"`, `transport="ziti"`
3. Instead of `http://localhost:<port>`, constructs `http://<ziti-service-intercept>:<port>`
4. Runs: `opencode attach http://<ziti-service-intercept>:<port>`
5. OpenCode TUI renders, receives SSE events, sends HTTP requests — all transparently proxied

### Alternative: Python SDK Direct (No Tunneler)

Instead of using Ziti tunnelers, ralph-uv could use the Python SDK directly to create a local proxy:

```python
# In ralph-uv attach command:
import openziti
import threading
import socket

def start_local_proxy(ziti_service: str, identity_path: str) -> int:
    """Start a local TCP→Ziti proxy, return the local port."""
    ztx, _ = openziti.load(identity_path)
    
    local_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    local_sock.bind(('127.0.0.1', 0))
    local_port = local_sock.getsockname()[1]
    local_sock.listen(5)
    
    def proxy_loop():
        while True:
            client, _ = local_sock.accept()
            ziti_conn = ztx.connect(ziti_service)
            # Bidirectional forward between client and ziti_conn
            threading.Thread(target=forward, args=(client, ziti_conn)).start()
            threading.Thread(target=forward, args=(ziti_conn, client)).start()
    
    threading.Thread(target=proxy_loop, daemon=True).start()
    return local_port

# Then:
local_port = start_local_proxy("ralph-loop-task-uuid", "~/.ziti/client.json")
subprocess.run(["opencode", "attach", f"http://localhost:{local_port}"])
```

**Pros**: No external tunneler dependency; ralph-uv is self-contained.
**Cons**: Thread-based proxy adds complexity; Python SDK has no native asyncio.

### Recommendation for OpenCode Remote Attach

**Use Ziti tunnelers** for the initial implementation. They're battle-tested, handle all edge cases (timeouts, reconnects, buffering), and require zero changes to `opencode attach`.

If tunneler deployment is undesirable later, the Python SDK proxy approach is a viable alternative with ~50 lines of code.

---

## 2. Remote Claude Attach Options

### The Challenge

Claude sessions run in tmux. `tmux attach-session` requires **direct access to the tmux server socket** (`/tmp/tmux-{uid}/default`). This is a Unix domain socket — not network-accessible.

### Option A: SSH-over-Ziti

```
LOCAL                                    REMOTE
ssh -o ProxyCommand="ziti-tunnel dial"   sshd (listening on Ziti service)
    │                                         │
    └── tmux attach -t ralph-<task> ──────────┘
```

**How it works**:
1. Remote machine runs sshd as a Ziti service (host-side tunneler)
2. Local client uses `ssh` with a `ProxyCommand` that dials via Ziti
3. Once SSH is established, run `tmux attach -t ralph-<task>` on remote
4. Full interactive terminal session — native tmux experience

**Pros**:
- Full terminal capabilities (resize, copy mode, mouse, etc.)
- Proven technology (SSH + tmux is the standard remote dev pattern)
- No custom protocol needed
- tmux features like synchronized panes, scrollback, etc. all work

**Cons**:
- Requires sshd on the remote (usually already there)
- Requires SSH keys or auth setup (one-time)
- Adding SSH as a dependency increases attack surface (mitigated by Ziti's zero-trust)
- Extra process (ssh client) between user and tmux

### Option B: Tmux Socket Proxy over Ziti

```
LOCAL                                    REMOTE
tmux -S /tmp/ziti-proxy.sock attach      Ziti → tmux server socket
    │                                         │
    └── Ziti tunneler: forward local socket ──┘
         to remote tmux server socket
```

**How it works**:
1. Expose the remote tmux server socket as a Ziti service
2. Local Ziti tunneler creates a Unix socket that proxies to the remote
3. Local `tmux -S <proxy-socket> attach -t ralph-<task>`

**Assessment: NOT FEASIBLE**

Tmux's Unix socket protocol is not designed for network use:
- **Client-server protocol uses shared memory** for some operations (window content)
- **Low-level ioctl calls** for terminal size negotiation
- **Tight latency requirements** — tmux redraws on every keystroke
- **Socket path encoding** — tmux server identity is tied to socket path
- **Session ownership** — tmux verifies UID via socket credentials (`SO_PEERCRED`)

While raw byte forwarding of the Unix socket might partially work, it would break on:
- Terminal resize (SIGWINCH propagation)
- Large scrollback buffer retrieval
- Any operation using `SCM_RIGHTS` (fd passing)

**Verdict: Rejected** — too fragile and incompatible with tmux internals.

### Option C: Web-Based Terminal (xterm.js) over Ziti

```
LOCAL                                    REMOTE
Browser → http://<ziti>:<port>           Web server → tmux send-keys/capture-pane
                                         (custom ralph-web-terminal)
```

**How it works**:
1. Remote daemon runs a small web server per tmux session
2. Web server uses `tmux capture-pane -p` to get screen content
3. Web server uses `tmux send-keys` to forward keystrokes
4. Client uses a browser or terminal-based HTTP client

**Assessment**: High effort, poor experience. Adds web server dependency, doesn't provide native terminal feel. Rejected for initial implementation.

### Option D: tmux -CC (Control Mode) over Ziti

```
LOCAL                                    REMOTE
tmux -CC (control client)                tmux server
    │                                         │
    └── Ziti TCP proxy (raw stream) ──────────┘
```

**How it works**:
1. tmux supports a "control mode" (`-CC`) where the client communicates via text commands/responses over stdin/stdout
2. This is a well-defined text protocol (used by iTerm2's tmux integration)
3. Could potentially be proxied over any stream transport

**Assessment**: Interesting but complex:
- Control mode requires a custom client to render the output
- Not a standard user experience (no native tmux keybindings)
- Would need a terminal renderer on the local side
- Potential future option if a TUI is built for this

**Verdict: Future consideration** — too complex for initial implementation.

### Recommendation for Claude Remote Attach

**Use SSH-over-Ziti (Option A)**. It provides:
- Native tmux experience (identical to local attach)
- Proven reliability
- Minimal implementation effort (~10 lines of subprocess code)
- Works with existing SSH infrastructure

Implementation in ralph-uv:
```python
def _attach_remote_claude(session: SessionInfo) -> int:
    """Attach to a remote claude/tmux session via SSH-over-Ziti."""
    # Build SSH command with Ziti proxy
    ssh_cmd = [
        "ssh",
        "-o", f"ProxyCommand=ziti-edge-tunnel dial {session.ziti_service} --identity {session.ziti_identity}",
        f"ralph@{session.remote_host}",
        "--", "tmux", "attach-session", "-t", session.tmux_session,
    ]
    result = subprocess.run(ssh_cmd)
    return result.returncode
```

---

## 3. Latency Requirements for Remote Terminal/TUI

### Human Perception Thresholds

| Latency Range | User Experience |
|---------------|-----------------|
| < 50ms | Imperceptible — feels local |
| 50-100ms | Noticeable but acceptable — "snappy remote" |
| 100-200ms | Clearly remote, still usable for coding |
| 200-500ms | Uncomfortable for interactive typing |
| > 500ms | Unusable for interactive sessions |

### OpenZiti Overlay Latency Characteristics

OpenZiti adds latency at several points:

| Component | Added Latency (typical) |
|-----------|------------------------|
| Ziti SDK identity verification | 0ms (cached after first use) |
| Ziti edge router traversal | 1-5ms (local network) |
| Ziti fabric routing | 5-20ms (depends on topology) |
| TLS overhead per connection | 0ms (persistent connections) |
| Per-packet encryption | < 1ms |
| **Total Ziti overhead** | **5-25ms typical** |

The total latency = Ziti overhead + underlying network latency. For:
- **Same LAN**: 5-25ms Ziti + 0-1ms network = **5-26ms** (imperceptible)
- **Same region cloud**: 5-25ms Ziti + 5-20ms network = **10-45ms** (excellent)
- **Cross-region**: 5-25ms Ziti + 50-150ms network = **55-175ms** (acceptable)
- **Cross-continent**: 5-25ms Ziti + 150-300ms network = **155-325ms** (uncomfortable)

### Impact on Each Agent Type

#### OpenCode (HTTP-based)

- **Attach (initial load)**: Single HTTP request → ~1-2 RTTs. Even at 200ms RTT, this is < 400ms — acceptable.
- **SSE events**: Streaming, no per-event RTT. Latency = propagation delay only.
- **Keystroke echo**: User types → HTTP POST → server processes → SSE event back. This is 2 RTTs for feedback.
  - At 50ms: 100ms round-trip — good
  - At 100ms: 200ms round-trip — noticeable but acceptable
  - At 200ms: 400ms round-trip — uncomfortable for rapid typing
- **Key insight**: OpenCode's `opencode attach` renders the TUI client-side. The server only sends semantic events (token completions, tool results), not raw terminal bytes. This means:
  - **Typing latency is client-local** — no server round-trip for character echo
  - **Only prompt submission** requires a server round-trip
  - **Agent output** streams via SSE with no user-perceptible latency concern

**Verdict**: OpenCode is **highly suitable** for remote attach. The HTTP/SSE model decouples interactive typing from network latency.

#### Claude (tmux/SSH)

- **Every keystroke** goes through: local terminal → SSH → Ziti → remote sshd → tmux → agent PTY, then the response travels back.
- At 50ms RTT: 100ms per keystroke echo — noticeable
- At 100ms RTT: 200ms per keystroke echo — uncomfortable
- At 200ms RTT: 400ms per keystroke echo — unusable for typing

However, **for ralph-uv's use case**, users don't typically type in the tmux session. They attach to **observe** the agent working. The agent reads prompts from the filesystem, not from the user. So:
- **Observation**: Low latency requirement — rendering updates arrive via SSH naturally
- **Occasional interaction**: Scrollback, copy mode — tolerable at 100-200ms
- **Stop/checkpoint**: Can use ralph-uv CLI (separate Ziti RPC call) instead of tmux keystrokes

**Verdict**: Claude remote attach via SSH is **acceptable** for the observation-heavy use pattern. Interactive typing through tmux would be uncomfortable at > 100ms RTT, but that's not the primary use case.

---

## 4. SSE Event Latency over Ziti for OpenCode

### SSE Transport Characteristics

Server-Sent Events use a long-lived HTTP connection with chunked transfer encoding:
- Connection is established once (1 RTT for TCP + 1 RTT for HTTP)
- Events are pushed server→client with no per-event handshake
- Each event is a text block terminated by `\n\n`
- Events are typically small (< 1KB for token updates, < 10KB for tool results)

### Latency Analysis

```
Event lifecycle:
1. Agent produces output (token/tool result)
2. opencode serve detects state change → writes SSE event to all clients
3. SSE event traverses: server → Ziti host tunneler → Ziti fabric → Ziti client tunneler → opencode attach
4. opencode attach renders the event in TUI

Latency = step 3 only (steps 1, 2, 4 are local)
       = network propagation + Ziti overhead
       ≈ network RTT / 2 + ~5-10ms Ziti processing (one-way)
```

| Scenario | One-way Latency | User Experience |
|----------|----------------|-----------------|
| Same LAN | 3-15ms | Indistinguishable from local |
| Same region | 8-35ms | Token-by-token streaming appears smooth |
| Cross-region | 30-100ms | Slight delay in token appearance, still readable |
| Cross-continent | 80-170ms | Noticeable delay, tokens arrive in small bursts |

### Buffering Considerations

1. **TCP Nagle's algorithm**: Can introduce up to 200ms delay for small packets. However, SSE events are flushed immediately by the server (no Nagle concern at application level).

2. **Ziti tunneler buffering**: The tunneler forwards packets as they arrive — no application-level buffering for stream data.

3. **HTTP chunked encoding**: Each SSE event is sent as a chunk. Modern HTTP stacks flush chunks immediately.

4. **opencode serve flush behavior**: The server uses `Transfer-Encoding: chunked` and flushes after each event. Verified by the SSE spec requirement.

### Estimated Real-World Performance

For a typical agent coding session with token-by-token streaming:
- **Tokens per second**: ~50-80 (typical LLM output rate)
- **Events per second**: ~10-20 (opencode batches tokens into SSE events)
- **Event size**: ~100-500 bytes per event
- **Bandwidth requirement**: < 10 KB/s (trivial)

At 50ms one-way latency (cross-region):
- Each SSE event arrives 50ms after it's emitted
- At 20 events/second, this means ~1 event of lag
- User sees a 50ms-delayed stream — **completely acceptable**

### Reconnection Behavior

If the SSE connection drops:
1. `opencode attach` detects the connection loss
2. Reconnects to the SSE endpoint (GET /event)
3. Missed events are lost, but the TUI re-fetches current state via GET /session/:id
4. **Total recovery time**: 1-3 seconds (reconnect + state refresh)

Over Ziti, connection drops are less likely than raw internet (Ziti handles fabric-level routing), but can occur on:
- Ziti edge router restart
- Network interface change (laptop sleep/wake)
- Ziti identity re-enrollment

---

## 5. Architecture for Remote Attach (Both Agent Types)

### Unified Dispatch in ralph-uv attach

```python
def attach(task_name: str) -> int:
    """Attach to a running ralph-uv session (local or remote)."""
    db = SessionDB()
    session = db.get(task_name)
    
    if session is None:
        print(f"Error: No session found for '{task_name}'", file=sys.stderr)
        return 1
    
    # Dispatch based on transport and session type
    if session.transport == "ziti":
        # Remote session
        if session.session_type == "opencode-server":
            return _attach_remote_opencode(session)
        else:
            return _attach_remote_claude(session)
    else:
        # Local session (existing code)
        if session.session_type == "opencode-server":
            return _attach_opencode_server(task_name, session.server_port, db)
        else:
            return _attach_tmux(task_name, db)
```

### Remote OpenCode Attach Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ LOCAL MACHINE                                                    │
│                                                                  │
│  ralph-uv attach <task>                                          │
│       │                                                          │
│       ├── Read sessions.db: transport=ziti, type=opencode-server │
│       │                                                          │
│       ├── Option A: Use Ziti tunneler intercept                  │
│       │   └── opencode attach http://<intercept-ip>:<port>       │
│       │                                                          │
│       └── Option B: Python SDK local proxy (fallback)            │
│           ├── Start local TCP→Ziti proxy on random port          │
│           └── opencode attach http://localhost:<local-port>       │
│                                                                  │
└──────────────────────────────┬───────────────────────────────────┘
                               │ Ziti service: ralph-loop-<task>-<uuid>
┌──────────────────────────────┼───────────────────────────────────┐
│ REMOTE MACHINE               │                                    │
│                              ▼                                    │
│  Ziti host tunneler → localhost:<port>                            │
│                              │                                    │
│  opencode serve --port <port>                                     │
│       │                                                          │
│       ├── GET /global/health                                     │
│       ├── POST /session/:id/message                              │
│       ├── GET /event (SSE stream)                                │
│       └── POST /session/:id/abort                                │
└──────────────────────────────────────────────────────────────────┘
```

**Key insight**: The `opencode attach` command doesn't know or care that it's talking to a remote server. The Ziti transport is invisible at the application layer.

### Remote Claude Attach Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ LOCAL MACHINE                                                    │
│                                                                  │
│  ralph-uv attach <task>                                          │
│       │                                                          │
│       ├── Read sessions.db: transport=ziti, type=tmux            │
│       │                                                          │
│       └── SSH with Ziti ProxyCommand                             │
│           ssh -o "ProxyCommand=..." ralph@remote                 │
│               -- tmux attach -t ralph-<task>                     │
│                                                                  │
└──────────────────────────────┬───────────────────────────────────┘
                               │ Ziti service: ralph-ssh-<hostname>
┌──────────────────────────────┼───────────────────────────────────┐
│ REMOTE MACHINE               │                                    │
│                              ▼                                    │
│  Ziti host tunneler → sshd (port 22)                             │
│                              │                                    │
│  SSH session established                                         │
│       │                                                          │
│       └── tmux attach-session -t ralph-<task>                    │
│           (full native tmux experience)                          │
└──────────────────────────────────────────────────────────────────┘
```

**Key considerations**:
- One SSH Ziti service per remote machine (not per loop)
- SSH keys pre-configured during remote setup
- tmux session names are predictable (`ralph-<task>`)
- User gets the exact same experience as `ssh remote && tmux attach`

### Session DB Schema Extension

```sql
ALTER TABLE sessions ADD COLUMN transport TEXT NOT NULL DEFAULT 'local';
-- 'local' or 'ziti'

ALTER TABLE sessions ADD COLUMN ziti_service TEXT;
-- e.g., 'ralph-loop-my-task-a1b2c3'

ALTER TABLE sessions ADD COLUMN ziti_identity TEXT;
-- e.g., '~/.ziti/client.json'

ALTER TABLE sessions ADD COLUMN remote_host TEXT;
-- Human-readable label, e.g., 'home-server'

ALTER TABLE sessions ADD COLUMN server_url TEXT;
-- For remote opencode: the Ziti intercept URL
-- For remote tmux: null (uses SSH)
```

### Stop/Checkpoint for Remote Sessions

| Operation | OpenCode Remote | Claude Remote |
|-----------|----------------|---------------|
| Stop | POST /session/:id/abort (via Ziti proxy) | ssh remote "ralph-uv stop <task>" or daemon RPC |
| Checkpoint | Daemon RPC: checkpoint_loop (via control service) | ssh remote "ralph-uv checkpoint <task>" or daemon RPC |
| Status | GET /global/health + GET /session/:id (via Ziti) | Daemon RPC: get_status |

For both agent types, the **daemon control service** (`ralph-control-<hostname>`) can handle stop/checkpoint uniformly, regardless of the underlying session type. This is the preferred approach.

---

## 6. Feasibility Summary

| Criterion | OpenCode | Claude |
|-----------|----------|--------|
| Remote attach feasibility | **Trivial** — HTTP proxy over Ziti | **Straightforward** — SSH-over-Ziti |
| Implementation effort | ~20 lines (URL rewrite) | ~30 lines (SSH ProxyCommand) |
| Latency sensitivity | Low (client-side rendering) | Medium (terminal byte stream) |
| Same-LAN experience | Indistinguishable from local | Indistinguishable from local |
| Cross-region experience | Excellent | Good (observation mode) |
| Cross-continent experience | Good | Acceptable (observation only) |
| Dependencies | Ziti tunneler OR Python SDK | Ziti tunneler + sshd + SSH keys |
| SSE real-time updates | Works perfectly over Ziti | N/A (tmux handles display) |
| Reconnection handling | Automatic (opencode attach) | Automatic (SSH keepalive) |
| Multi-client | Yes (SSE broadcast) | No (tmux single-attach or multi-attach) |

### Overall Verdict

**Remote attach is feasible for both agent types.** OpenCode is the superior remote experience due to its HTTP-based architecture that naturally separates UI rendering from network transport. Claude/tmux works well for observation but is less suitable for interactive use at high latencies.

### Recommendation

1. **Prioritize opencode agent for remote loops** — the remote experience is naturally better
2. **Implement remote opencode attach first** — it's the simplest and most impactful
3. **Add SSH-over-Ziti for claude as a secondary option** — for users who prefer claude
4. **Use daemon RPC for stop/checkpoint** — don't rely on transport-specific mechanisms

---

## 7. Security Considerations for Remote Attach

- **OpenCode password auth**: `OPENCODE_SERVER_PASSWORD` should be set on remote servers to prevent unauthorized access to the HTTP API. Ziti provides network-level auth, but defense-in-depth is good practice.
- **SSH key management**: Claude remote attach requires SSH keys on the remote. This is a one-time setup per remote machine.
- **Ziti identity rotation**: Identities can be revoked centrally if a machine is compromised.
- **No exposed ports**: Neither the opencode HTTP port nor sshd needs to be exposed to the internet. Ziti handles all routing internally.
