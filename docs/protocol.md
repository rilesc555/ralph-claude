# Ralph TUI Communication Protocol

## Overview

Ralph-uv communicates with ralph-tui (and other clients) via a JSON-RPC 2.0 protocol over Unix domain sockets. Each running session exposes a socket file that clients connect to for real-time state queries, control commands, and event subscriptions.

## Transport

- **Socket path**: `~/.local/share/ralph/sockets/<task-name>.sock`
- **Protocol**: Newline-delimited JSON (NDJSON) over Unix domain socket
- **Encoding**: UTF-8
- **Permissions**: Socket file is `0600` (owner read/write only)

Each message is a single line of JSON terminated by `\n`. Clients connect, send requests, and receive responses and event notifications over the same connection.

## JSON-RPC 2.0 Basics

All messages follow the [JSON-RPC 2.0 specification](https://www.jsonrpc.org/specification).

### Request

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "get_status",
  "params": {}
}
```

### Response (Success)

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": { ... }
}
```

### Response (Error)

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32601,
    "message": "Method not found: foo"
  }
}
```

### Notification (No `id` field)

Sent by the server for subscribed events. No response expected.

```json
{
  "jsonrpc": "2.0",
  "method": "event",
  "params": {
    "type": "state_change",
    "timestamp": "2026-01-23T10:30:00.000000",
    "data": { "iteration": 5 }
  }
}
```

## Methods

### `get_status`

Returns the current session state.

**Params**: None

**Result**:
```json
{
  "task_name": "my-feature",
  "task_dir": "/path/to/tasks/my-feature",
  "iteration": 3,
  "max_iterations": 50,
  "current_story": "US-003",
  "agent": "claude",
  "status": "running",
  "interactive_mode": false,
  "started_at": "2026-01-23T10:00:00.000000",
  "updated_at": "2026-01-23T10:15:30.000000",
  "recent_output": ["line 1", "line 2", "..."]
}
```

### `stop`

Request graceful shutdown of the loop. The loop will finish the current operation and exit.

**Params**: None

**Result**:
```json
{
  "status": "stop_requested"
}
```

### `checkpoint`

Request the loop to save state and pause after the current iteration.

**Params**: None

**Result**:
```json
{
  "status": "checkpoint_requested"
}
```

### `inject_prompt`

Inject additional instructions that will be prepended to the next agent prompt.

**Params**:
```json
{
  "prompt": "Focus on fixing the type errors first before adding new features."
}
```

**Result**:
```json
{
  "status": "prompt_injected",
  "prompt": "Focus on fixing the type errors first before adding new features."
}
```

### `set_interactive_mode`

Toggle interactive mode on/off. When interactive mode is on, completion detection is suppressed (the agent won't auto-advance to the next iteration).

**Params**:
```json
{
  "enabled": true
}
```

**Result**:
```json
{
  "interactive_mode": true
}
```

### `write_pty`

Forward raw keystroke data to the agent PTY. Only effective when interactive mode is enabled. Used by the attach command to forward user input directly to the running agent.

**Params**:
```json
{
  "data": "ls -la\n"
}
```

**Result** (when interactive mode is on):
```json
{
  "status": "forwarded"
}
```

**Result** (when interactive mode is off):
```json
{
  "status": "ignored",
  "reason": "not in interactive mode"
}
```

### `subscribe`

Subscribe to event types for real-time notifications.

**Params**:
```json
{
  "events": ["output", "state_change"]
}
```

**Valid event types**:
- `output` - New output lines from the agent
- `state_change` - Session state field changes
- `*` - All event types

**Result**:
```json
{
  "subscribed": ["output", "state_change"]
}
```

### `unsubscribe`

Remove event subscriptions.

**Params**:
```json
{
  "events": ["output"]
}
```

**Result**:
```json
{
  "subscribed": ["state_change"]
}
```

## Events

Events are sent as JSON-RPC notifications (no `id` field) to subscribed clients.

### `output`

Emitted when new agent output is available.

```json
{
  "jsonrpc": "2.0",
  "method": "event",
  "params": {
    "type": "output",
    "timestamp": "2026-01-23T10:15:30.123456",
    "data": {
      "line": "Implementing the database migration..."
    }
  }
}
```

### `state_change`

Emitted when session state fields change. Only changed fields are included.

```json
{
  "jsonrpc": "2.0",
  "method": "event",
  "params": {
    "type": "state_change",
    "timestamp": "2026-01-23T10:15:30.123456",
    "data": {
      "iteration": 4,
      "current_story": "US-004"
    }
  }
}
```

## Error Codes

Standard JSON-RPC 2.0 error codes:

| Code | Message | Description |
|------|---------|-------------|
| -32700 | Parse error | Invalid JSON |
| -32600 | Invalid request | Missing required JSON-RPC fields |
| -32601 | Method not found | Unknown method name |
| -32602 | Invalid params | Missing or invalid parameters |
| -32603 | Internal error | Server-side error |

## Supplemental Data (Direct File Access)

The TUI can also read these files directly for supplemental status information:

- **`prd.json`** - Full PRD with story acceptance criteria and pass/fail states
- **`progress.txt`** - Iteration history and codebase patterns

These files are updated by the agent during each iteration and provide a persistent record that survives process restarts. The JSON-RPC protocol provides real-time state; the files provide persistence and detail.

## Connection Lifecycle

1. **Client connects** to the Unix socket
2. **Client subscribes** to desired event types (optional)
3. **Client sends queries/commands** as needed
4. **Server pushes events** for subscribed types
5. **Client disconnects** by closing the connection (server cleans up subscription)

## Example Client Session (Python)

```python
import asyncio
import json

async def connect_to_ralph(task_name: str):
    socket_path = f"~/.local/share/ralph/sockets/{task_name}.sock"
    reader, writer = await asyncio.open_unix_connection(
        os.path.expanduser(socket_path)
    )

    # Subscribe to all events
    request = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "subscribe",
        "params": {"events": ["*"]}
    }
    writer.write((json.dumps(request) + "\n").encode())
    await writer.drain()

    # Read responses and events
    while True:
        line = await reader.readline()
        if not line:
            break
        msg = json.loads(line.decode())
        print(msg)
```

## Example Client Session (Rust)

```rust
use tokio::net::UnixStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn connect_to_ralph(task_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = format!(
        "{}/.local/share/ralph/sockets/{}.sock",
        std::env::var("HOME")?,
        task_name
    );

    let stream = UnixStream::connect(&socket_path).await?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Send get_status request
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_status",
        "params": {}
    });
    writer.write_all(format!("{}\n", request).as_bytes()).await?;

    // Read response
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let response: serde_json::Value = serde_json::from_str(&line)?;
    println!("{:#}", response);

    Ok(())
}
```
