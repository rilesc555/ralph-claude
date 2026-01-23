"""JSON-RPC server for ralph-uv TUI communication.

Provides a JSON-RPC 2.0 server over Unix domain sockets for communication
between ralph-uv (Python loop runner) and ralph-tui (Rust TUI). Each session
exposes a socket at ~/.local/share/ralph/sockets/<task>.sock.

Protocol supports:
- State queries: get_status
- Control commands: start, stop, checkpoint, inject_prompt, set_interactive_mode
- Event subscriptions: subscribe/unsubscribe for real-time output and state changes
"""

from __future__ import annotations

import asyncio
import json
import os
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

from ralph_uv.session import DATA_DIR

# Socket directory
SOCKET_DIR = DATA_DIR / "sockets"

# Maximum recent output lines to keep in buffer
MAX_OUTPUT_BUFFER = 200

# JSON-RPC 2.0 error codes
PARSE_ERROR = -32700
INVALID_REQUEST = -32600
METHOD_NOT_FOUND = -32601
INVALID_PARAMS = -32602
INTERNAL_ERROR = -32603


@dataclass
class SessionState:
    """Mutable state exposed via JSON-RPC to TUI clients."""

    task_name: str
    task_dir: str
    iteration: int = 0
    max_iterations: int = 50
    current_story: str = ""
    agent: str = "claude"
    status: str = "running"  # running, stopped, completed, failed, checkpointed
    interactive_mode: bool = False
    started_at: str = ""
    updated_at: str = ""
    recent_output: deque[str] = field(
        default_factory=lambda: deque(maxlen=MAX_OUTPUT_BUFFER)
    )
    injected_prompt: str = ""

    def to_dict(self) -> dict[str, Any]:
        """Serialize state for JSON-RPC responses."""
        return {
            "task_name": self.task_name,
            "task_dir": self.task_dir,
            "iteration": self.iteration,
            "max_iterations": self.max_iterations,
            "current_story": self.current_story,
            "agent": self.agent,
            "status": self.status,
            "interactive_mode": self.interactive_mode,
            "mode_indicator": "interactive" if self.interactive_mode else "autonomous",
            "started_at": self.started_at,
            "updated_at": self.updated_at,
            "recent_output": list(self.recent_output),
        }

    def update_timestamp(self) -> None:
        """Update the updated_at timestamp."""
        self.updated_at = datetime.now().isoformat()


class RpcError(Exception):
    """JSON-RPC error with code and message."""

    def __init__(self, code: int, message: str, data: Any = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.data = data


class EventSubscriber:
    """Represents a TUI client subscribed to events."""

    def __init__(self, writer: asyncio.StreamWriter) -> None:
        self.writer = writer
        self.subscriptions: set[str] = set()

    def is_subscribed(self, event_type: str) -> bool:
        """Check if this subscriber is interested in an event type."""
        return event_type in self.subscriptions or "*" in self.subscriptions


class RpcServer:
    """JSON-RPC 2.0 server over Unix domain socket.

    Manages client connections, dispatches RPC methods, and broadcasts
    events to subscribed clients.
    """

    def __init__(self, state: SessionState) -> None:
        self.state = state
        self._server: asyncio.Server | None = None
        self._socket_path: Path | None = None
        self._subscribers: list[EventSubscriber] = []
        self._loop: asyncio.AbstractEventLoop | None = None
        self._methods: dict[str, Any] = {
            "get_status": self._handle_get_status,
            "stop": self._handle_stop,
            "checkpoint": self._handle_checkpoint,
            "inject_prompt": self._handle_inject_prompt,
            "set_interactive_mode": self._handle_set_interactive_mode,
            "write_pty": self._handle_write_pty,
            "subscribe": self._handle_subscribe,
            "unsubscribe": self._handle_unsubscribe,
        }
        # Callbacks set by the LoopRunner
        self._on_stop: Any = None
        self._on_checkpoint: Any = None
        self._on_set_interactive: Any = None
        self._on_write_pty: Any = None

    @property
    def socket_path(self) -> Path | None:
        """The path to the Unix socket, or None if not started."""
        return self._socket_path

    def get_socket_path(self, task_name: str) -> Path:
        """Get the socket path for a task."""
        SOCKET_DIR.mkdir(parents=True, exist_ok=True)
        return SOCKET_DIR / f"{task_name}.sock"

    async def start(self) -> None:
        """Start the JSON-RPC server on a Unix domain socket."""
        self._socket_path = self.get_socket_path(self.state.task_name)

        # Clean up stale socket file
        if self._socket_path.exists():
            self._socket_path.unlink()

        self._server = await asyncio.start_unix_server(
            self._handle_client,
            path=str(self._socket_path),
        )
        self._loop = asyncio.get_running_loop()

        # Set socket permissions (readable/writable by owner only)
        os.chmod(str(self._socket_path), 0o600)

    async def stop(self) -> None:
        """Stop the JSON-RPC server and clean up the socket."""
        if self._server is not None:
            self._server.close()
            await self._server.wait_closed()
            self._server = None

        # Close all subscriber connections
        for sub in self._subscribers:
            try:
                sub.writer.close()
                await sub.writer.wait_closed()
            except (OSError, ConnectionResetError):
                pass
        self._subscribers.clear()

        # Remove socket file
        if self._socket_path is not None and self._socket_path.exists():
            self._socket_path.unlink()
            self._socket_path = None

    def set_callbacks(
        self,
        on_stop: Any = None,
        on_checkpoint: Any = None,
        on_set_interactive: Any = None,
        on_write_pty: Any = None,
    ) -> None:
        """Set callbacks for control commands.

        These are called when TUI sends stop/checkpoint/interactive/write_pty commands.
        """
        self._on_stop = on_stop
        self._on_checkpoint = on_checkpoint
        self._on_set_interactive = on_set_interactive
        self._on_write_pty = on_write_pty

    def emit_event(self, event_type: str, data: dict[str, Any]) -> None:
        """Emit an event to all subscribed clients (non-async entry point).

        Call this from synchronous code; it schedules the async broadcast
        on the event loop if available.
        """
        if self._loop is not None and self._loop.is_running():
            self._loop.call_soon_threadsafe(
                asyncio.ensure_future,
                self._broadcast_event(event_type, data),
            )

    def append_output(self, line: str) -> None:
        """Append a line to the output buffer and emit output event."""
        self.state.recent_output.append(line)
        self.emit_event("output", {"line": line})

    def update_state(self, **kwargs: Any) -> None:
        """Update session state fields and emit state_change event."""
        changed: dict[str, Any] = {}
        for key, value in kwargs.items():
            if hasattr(self.state, key):
                old_value = getattr(self.state, key)
                if old_value != value:
                    setattr(self.state, key, value)
                    changed[key] = value
        if changed:
            self.state.update_timestamp()
            self.emit_event("state_change", changed)

    # --- Client handling ---

    async def _handle_client(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        """Handle a new client connection."""
        subscriber = EventSubscriber(writer)
        self._subscribers.append(subscriber)
        try:
            await self._read_requests(reader, writer, subscriber)
        except (asyncio.IncompleteReadError, ConnectionResetError, OSError):
            pass
        finally:
            self._subscribers.remove(subscriber)
            try:
                writer.close()
                await writer.wait_closed()
            except (OSError, ConnectionResetError):
                pass

    async def _read_requests(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        subscriber: EventSubscriber,
    ) -> None:
        """Read and process JSON-RPC requests from a client."""
        while True:
            data = await reader.readline()
            if not data:
                break  # Client disconnected

            line = data.decode("utf-8").strip()
            if not line:
                continue

            response = await self._process_request(line, subscriber)
            if response is not None:
                await self._send_response(writer, response)

    async def _process_request(
        self, raw: str, subscriber: EventSubscriber
    ) -> dict[str, Any] | None:
        """Parse and dispatch a JSON-RPC request."""
        try:
            request: dict[str, Any] = json.loads(raw)
        except json.JSONDecodeError:
            return self._error_response(None, PARSE_ERROR, "Parse error")

        # Validate JSON-RPC 2.0 structure
        if not isinstance(request, dict):
            return self._error_response(None, INVALID_REQUEST, "Invalid request")

        jsonrpc = request.get("jsonrpc")
        if jsonrpc != "2.0":
            return self._error_response(
                request.get("id"), INVALID_REQUEST, "Invalid JSON-RPC version"
            )

        method = request.get("method")
        if not isinstance(method, str):
            return self._error_response(
                request.get("id"), INVALID_REQUEST, "Missing method"
            )

        request_id = request.get("id")
        params = request.get("params", {})
        if not isinstance(params, dict):
            params = {}

        # Notifications (no id) don't get responses
        is_notification = request_id is None

        try:
            result = await self._dispatch(method, params, subscriber)
            if is_notification:
                return None
            return self._success_response(request_id, result)
        except RpcError as e:
            if is_notification:
                return None
            return self._error_response(request_id, e.code, e.message, e.data)

    async def _dispatch(
        self, method: str, params: dict[str, Any], subscriber: EventSubscriber
    ) -> Any:
        """Dispatch a method call to the appropriate handler."""
        handler = self._methods.get(method)
        if handler is None:
            raise RpcError(METHOD_NOT_FOUND, f"Method not found: {method}")

        return await handler(params, subscriber)

    # --- RPC Method Handlers ---

    async def _handle_get_status(
        self, params: dict[str, Any], subscriber: EventSubscriber
    ) -> dict[str, Any]:
        """Handle get_status request.

        Returns current iteration, story, agent, interactive_mode flag,
        and recent output lines.
        """
        return self.state.to_dict()

    async def _handle_stop(
        self, params: dict[str, Any], subscriber: EventSubscriber
    ) -> dict[str, str]:
        """Handle stop command from TUI."""
        if self._on_stop is not None:
            self._on_stop()
        return {"status": "stop_requested"}

    async def _handle_checkpoint(
        self, params: dict[str, Any], subscriber: EventSubscriber
    ) -> dict[str, str]:
        """Handle checkpoint command from TUI."""
        if self._on_checkpoint is not None:
            self._on_checkpoint()
        return {"status": "checkpoint_requested"}

    async def _handle_inject_prompt(
        self, params: dict[str, Any], subscriber: EventSubscriber
    ) -> dict[str, str]:
        """Handle inject_prompt command.

        The injected prompt will be prepended to the next agent prompt.
        """
        prompt = params.get("prompt", "")
        if not isinstance(prompt, str) or not prompt.strip():
            raise RpcError(INVALID_PARAMS, "Missing or empty 'prompt' parameter")
        self.state.injected_prompt = prompt.strip()
        self.emit_event("state_change", {"injected_prompt": self.state.injected_prompt})
        return {"status": "prompt_injected", "prompt": self.state.injected_prompt}

    async def _handle_set_interactive_mode(
        self, params: dict[str, Any], subscriber: EventSubscriber
    ) -> dict[str, bool]:
        """Handle set_interactive_mode command.

        Toggles interactive mode on/off. When interactive mode is on,
        completion detection is suppressed and user input is forwarded
        to the agent PTY.
        """
        enabled = params.get("enabled")
        if not isinstance(enabled, bool):
            raise RpcError(
                INVALID_PARAMS, "Missing or invalid 'enabled' parameter (must be bool)"
            )
        self.state.interactive_mode = enabled
        self.state.update_timestamp()
        self.emit_event("state_change", {"interactive_mode": enabled})

        # Notify the loop runner to toggle interactive mode
        if self._on_set_interactive is not None:
            self._on_set_interactive(enabled)

        return {"interactive_mode": enabled}

    async def _handle_write_pty(
        self, params: dict[str, Any], subscriber: EventSubscriber
    ) -> dict[str, str]:
        """Handle write_pty command.

        Forwards raw keystroke data to the agent PTY in interactive mode.
        Only effective when interactive mode is enabled.

        Params:
            data: Base64-encoded bytes or plain string to forward to the agent PTY.
        """
        data = params.get("data", "")
        if not isinstance(data, str) or not data:
            raise RpcError(INVALID_PARAMS, "Missing or empty 'data' parameter")

        if not self.state.interactive_mode:
            return {"status": "ignored", "reason": "not in interactive mode"}

        if self._on_write_pty is not None:
            self._on_write_pty(data)

        return {"status": "forwarded"}

    async def _handle_subscribe(
        self, params: dict[str, Any], subscriber: EventSubscriber
    ) -> dict[str, list[str]]:
        """Handle subscribe request for event types.

        Params:
            events: list of event types to subscribe to.
                    Valid: "output", "state_change", "*" (all)
        """
        events = params.get("events", [])
        if not isinstance(events, list):
            raise RpcError(INVALID_PARAMS, "'events' must be a list")

        valid_events = {"output", "state_change", "*"}
        for event in events:
            if event not in valid_events:
                raise RpcError(
                    INVALID_PARAMS,
                    f"Invalid event type: '{event}'. Valid: {', '.join(sorted(valid_events))}",
                )
            subscriber.subscriptions.add(event)

        return {"subscribed": sorted(subscriber.subscriptions)}

    async def _handle_unsubscribe(
        self, params: dict[str, Any], subscriber: EventSubscriber
    ) -> dict[str, list[str]]:
        """Handle unsubscribe request for event types."""
        events = params.get("events", [])
        if not isinstance(events, list):
            raise RpcError(INVALID_PARAMS, "'events' must be a list")

        for event in events:
            subscriber.subscriptions.discard(event)

        return {"subscribed": sorted(subscriber.subscriptions)}

    # --- Event Broadcasting ---

    async def _broadcast_event(self, event_type: str, data: dict[str, Any]) -> None:
        """Broadcast an event notification to all subscribed clients."""
        notification = {
            "jsonrpc": "2.0",
            "method": "event",
            "params": {
                "type": event_type,
                "timestamp": datetime.now().isoformat(),
                "data": data,
            },
        }

        dead_subscribers: list[EventSubscriber] = []
        for sub in self._subscribers:
            if sub.is_subscribed(event_type):
                try:
                    await self._send_response(sub.writer, notification)
                except (OSError, ConnectionResetError):
                    dead_subscribers.append(sub)

        # Clean up disconnected subscribers
        for sub in dead_subscribers:
            self._subscribers.remove(sub)

    # --- Response Helpers ---

    @staticmethod
    def _success_response(request_id: Any, result: Any) -> dict[str, Any]:
        """Build a JSON-RPC 2.0 success response."""
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": result,
        }

    @staticmethod
    def _error_response(
        request_id: Any, code: int, message: str, data: Any = None
    ) -> dict[str, Any]:
        """Build a JSON-RPC 2.0 error response."""
        error: dict[str, Any] = {"code": code, "message": message}
        if data is not None:
            error["data"] = data
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": error,
        }

    @staticmethod
    async def _send_response(
        writer: asyncio.StreamWriter, response: dict[str, Any]
    ) -> None:
        """Send a JSON-RPC response as a newline-delimited JSON message."""
        line = json.dumps(response, separators=(",", ":")) + "\n"
        writer.write(line.encode("utf-8"))
        await writer.drain()


# --- Helper Functions ---


def get_socket_path(task_name: str) -> Path:
    """Get the socket path for a task session."""
    SOCKET_DIR.mkdir(parents=True, exist_ok=True)
    return SOCKET_DIR / f"{task_name}.sock"


def cleanup_socket(task_name: str) -> None:
    """Remove a stale socket file."""
    socket_path = get_socket_path(task_name)
    if socket_path.exists():
        socket_path.unlink()
