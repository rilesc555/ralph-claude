"""Attach command: single-loop terminal view with interactive mode.

Connects to a running ralph-uv session via JSON-RPC and provides:
- Live status display (iteration, story progress, agent, mode)
- Real-time agent output streaming
- Interactive mode toggle (hotkey 'i')
- Stop/checkpoint commands

When interactive mode is toggled:
- User keystrokes are forwarded to the agent PTY via RPC write_pty
- The visual indicator switches from [AUTONOMOUS] to [INTERACTIVE]
- Completion detection is suppressed in the running loop

Exiting interactive mode:
- Press 'i' again to toggle back to autonomous
- Press Esc to also exit interactive mode and resume autonomous monitoring
"""

from __future__ import annotations

import json
import os
import select
import socket
import sys
import termios
import tty
from pathlib import Path
from typing import Any

from ralph_uv.rpc import get_socket_path

# Hotkeys
HOTKEY_INTERACTIVE = ord("i")  # Toggle interactive mode
HOTKEY_STOP = ord("s")  # Stop the loop
HOTKEY_CHECKPOINT = ord("c")  # Checkpoint the loop
HOTKEY_QUIT = ord("q")  # Quit attach view

# Escape key byte
ESC_BYTE = 0x1B


def attach(task_name: str) -> int:
    """Attach to a running ralph-uv session.

    Opens a terminal view showing the session status and agent output.
    Supports hotkey 'i' to toggle interactive mode.

    Args:
        task_name: The task name to attach to.

    Returns:
        Exit code (0 = normal exit, 1 = error).
    """
    socket_path = get_socket_path(task_name)
    if not socket_path.exists():
        print(
            f"Error: No running session found for task '{task_name}'", file=sys.stderr
        )
        print(f"  Expected socket at: {socket_path}", file=sys.stderr)
        return 1

    client = RpcClient(socket_path)
    try:
        client.connect()
    except ConnectionError as e:
        print(f"Error: Could not connect to session: {e}", file=sys.stderr)
        return 1

    viewer = AttachViewer(client, task_name)
    try:
        return viewer.run()
    finally:
        client.close()


class RpcClient:
    """JSON-RPC client for connecting to ralph-uv sessions."""

    def __init__(self, socket_path: Path) -> None:
        self._socket_path = socket_path
        self._sock: socket.socket | None = None
        self._buffer: bytes = b""
        self._request_id: int = 0

    def connect(self) -> None:
        """Connect to the ralph-uv RPC socket."""
        self._sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            self._sock.connect(str(self._socket_path))
            self._sock.setblocking(False)
        except (ConnectionRefusedError, FileNotFoundError) as e:
            raise ConnectionError(f"Cannot connect to {self._socket_path}: {e}") from e

    def close(self) -> None:
        """Close the connection."""
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None

    @property
    def connected(self) -> bool:
        """Whether the client is connected."""
        return self._sock is not None

    def send_request(self, method: str, params: dict[str, Any] | None = None) -> None:
        """Send a JSON-RPC request (fire-and-forget for notifications)."""
        if self._sock is None:
            return

        self._request_id += 1
        request: dict[str, Any] = {
            "jsonrpc": "2.0",
            "method": method,
            "id": self._request_id,
        }
        if params is not None:
            request["params"] = params

        data = json.dumps(request, separators=(",", ":")) + "\n"
        try:
            self._sock.sendall(data.encode("utf-8"))
        except OSError:
            self._sock = None  # Mark as disconnected

    def call(self, method: str, params: dict[str, Any] | None = None) -> Any:
        """Send a JSON-RPC request and wait for response."""
        if self._sock is None:
            return None

        self._request_id += 1
        request_id = self._request_id
        request: dict[str, Any] = {
            "jsonrpc": "2.0",
            "method": method,
            "id": request_id,
        }
        if params is not None:
            request["params"] = params

        data = json.dumps(request, separators=(",", ":")) + "\n"
        try:
            # Temporarily make socket blocking for this call
            self._sock.setblocking(True)
            self._sock.sendall(data.encode("utf-8"))

            # Read response
            response = self._read_response()
            self._sock.setblocking(False)

            if response and "result" in response:
                return response["result"]
            return None
        except OSError:
            self._sock = None  # Mark as disconnected
            return None

    def subscribe(self, events: list[str]) -> None:
        """Subscribe to event types."""
        self.send_request("subscribe", {"events": events})

    def read_events(self, timeout: float = 0.1) -> list[dict[str, Any]]:
        """Read any pending events from the socket.

        Args:
            timeout: How long to wait for data.

        Returns:
            List of JSON-RPC notifications received.
        """
        if self._sock is None:
            return []

        events: list[dict[str, Any]] = []
        try:
            ready, _, _ = select.select([self._sock], [], [], timeout)
            if ready:
                data = self._sock.recv(8192)
                if not data:
                    # Connection closed by server
                    self._sock = None
                    return events
                self._buffer += data

                # Parse NDJSON messages
                while b"\n" in self._buffer:
                    line, self._buffer = self._buffer.split(b"\n", 1)
                    if line.strip():
                        try:
                            msg: dict[str, Any] = json.loads(line)
                            events.append(msg)
                        except json.JSONDecodeError:
                            pass
        except (OSError, ValueError):
            self._sock = None  # Mark as disconnected

        return events

    def _read_response(self, timeout: float = 5.0) -> dict[str, Any] | None:
        """Read a single JSON-RPC response."""
        if self._sock is None:
            return None

        import time

        start = time.time()
        while time.time() - start < timeout:
            try:
                data = self._sock.recv(8192)
                if not data:
                    return None
                self._buffer += data

                if b"\n" in self._buffer:
                    line, self._buffer = self._buffer.split(b"\n", 1)
                    if line.strip():
                        result: dict[str, Any] = json.loads(line)
                        return result
            except (OSError, json.JSONDecodeError):
                return None

        return None


class AttachViewer:
    """Terminal UI for attached session viewing with interactive mode.

    Shows:
    - Current iteration and max iterations
    - Story progress (which story, criteria passed/total)
    - Live agent output streamed in real-time
    - Mode indicator (AUTONOMOUS vs INTERACTIVE)

    Supports:
    - 'i' hotkey to toggle interactive mode
    - Esc key to exit interactive mode (back to autonomous)
    - 's' to stop the loop
    - 'c' to checkpoint
    - 'q' to quit the attach view
    """

    def __init__(self, client: RpcClient, task_name: str) -> None:
        self._client = client
        self._task_name = task_name
        self._interactive_mode = False
        self._running = True
        self._original_termios: list[Any] | None = None

    def run(self) -> int:
        """Main loop for the attach viewer.

        Returns:
            Exit code (0 = normal exit).
        """
        # Get initial status
        status = self._client.call("get_status")
        if status is None:
            print("Error: Could not get session status", file=sys.stderr)
            return 1

        self._interactive_mode = bool(status.get("interactive_mode", False))

        # Subscribe to events
        self._client.subscribe(["output", "state_change"])

        # Print header with full status
        self._print_header(status)

        # Set terminal to cbreak mode for hotkey capture
        if sys.stdin.isatty():
            self._original_termios = termios.tcgetattr(sys.stdin.fileno())
            tty.setcbreak(sys.stdin.fileno())

        try:
            self._main_loop()
        finally:
            self._restore_terminal()

        return 0

    def _main_loop(self) -> None:
        """Main event loop: read events and handle user input."""
        while self._running:
            # Check if connection is still alive
            if not self._client.connected:
                self._write_status("Connection lost. Session may have ended.")
                self._running = False
                break

            # Check for user input
            if sys.stdin.isatty():
                ready, _, _ = select.select([sys.stdin], [], [], 0.05)
                if ready:
                    self._handle_input()

            # Read and process events from the RPC server
            events = self._client.read_events(timeout=0.05)
            for event in events:
                self._handle_event(event)

            # Re-check connection after reading events (may have been closed)
            if not self._client.connected:
                self._write_status("Connection lost. Session may have ended.")
                self._running = False

    def _handle_input(self) -> None:
        """Handle user keyboard input."""
        data = os.read(
            sys.stdin.fileno(), 32
        )  # Read up to 32 bytes for escape sequences
        if not data:
            return

        key = data[0]

        if self._interactive_mode:
            # In interactive mode: Esc or 'i' exits interactive mode,
            # everything else is forwarded to the agent PTY
            if key == ESC_BYTE:
                # Esc exits interactive mode
                self._toggle_interactive()
            elif key == HOTKEY_INTERACTIVE:
                # 'i' also toggles back to autonomous
                self._toggle_interactive()
            else:
                # Forward all input to agent PTY via RPC
                self._client.send_request(
                    "write_pty",
                    {"data": data.decode("utf-8", errors="replace")},
                )
        else:
            # In autonomous mode: handle hotkeys
            if key == HOTKEY_INTERACTIVE:
                self._toggle_interactive()
            elif key == HOTKEY_STOP:
                self._client.send_request("stop")
                self._write_status("Stop requested")
            elif key == HOTKEY_CHECKPOINT:
                self._client.send_request("checkpoint")
                self._write_status("Checkpoint requested")
            elif key == HOTKEY_QUIT:
                self._running = False

    def _toggle_interactive(self) -> None:
        """Toggle interactive mode via RPC."""
        new_mode = not self._interactive_mode
        result = self._client.call("set_interactive_mode", {"enabled": new_mode})
        if result is not None:
            self._interactive_mode = bool(result.get("interactive_mode", new_mode))
            mode_str = "INTERACTIVE" if self._interactive_mode else "AUTONOMOUS"
            self._write_status(f"Mode: [{mode_str}]")
            if self._interactive_mode:
                self._write_status(
                    "  Keystrokes forwarded to agent. Press 'i' or Esc to exit."
                )
        else:
            # Fallback: toggle locally if RPC fails
            self._interactive_mode = new_mode
            mode_str = "INTERACTIVE" if self._interactive_mode else "AUTONOMOUS"
            self._write_status(f"Mode: [{mode_str}] (RPC unconfirmed)")

    def _handle_event(self, event: dict[str, Any]) -> None:
        """Handle a JSON-RPC event notification."""
        if "method" not in event or event.get("method") != "event":
            return

        params = event.get("params", {})
        event_type = params.get("type", "")
        data = params.get("data", {})

        if event_type == "output":
            line = data.get("line", "")
            if line:
                self._write_output(line)
        elif event_type == "state_change":
            self._handle_state_change(data)

    def _handle_state_change(self, data: dict[str, Any]) -> None:
        """Handle a state change event."""
        if "interactive_mode" in data:
            self._interactive_mode = bool(data["interactive_mode"])
            mode_str = "INTERACTIVE" if self._interactive_mode else "AUTONOMOUS"
            self._write_status(f"Mode: [{mode_str}]")

        if "status" in data:
            status = data["status"]
            if status in ("completed", "stopped", "failed", "checkpointed"):
                self._write_status(f"Session {status}")
                self._running = False

        if "iteration" in data:
            iteration = data["iteration"]
            self._write_status(f"Iteration: {iteration}")

        if "current_story" in data:
            story_id = data["current_story"]
            self._write_status(f"Story: {story_id}")

    def _print_header(self, status: dict[str, Any]) -> None:
        """Print the attach view header with full status info."""
        mode_str = "INTERACTIVE" if self._interactive_mode else "AUTONOMOUS"
        iteration = status.get("iteration", 0)
        max_iterations = status.get("max_iterations", 50)
        current_story = status.get("current_story", "N/A")
        agent = status.get("agent", "unknown")
        session_status = status.get("status", "unknown")

        sys.stdout.write(f"\r\n--- Ralph Attach: {self._task_name} ---\r\n")
        sys.stdout.write(f"  Status:    {session_status}\r\n")
        sys.stdout.write(f"  Iteration: {iteration}/{max_iterations}\r\n")
        sys.stdout.write(f"  Story:     {current_story}\r\n")
        sys.stdout.write(f"  Agent:     {agent}\r\n")
        sys.stdout.write(f"  Mode:      [{mode_str}]\r\n")
        sys.stdout.write(
            "  Hotkeys:   [i] toggle mode  [s] stop  [c] checkpoint  [q] quit\r\n"
        )

        # Show recent output if available
        recent = status.get("recent_output", [])
        if recent:
            sys.stdout.write("---\r\n")
            # Show last 10 lines of recent output
            for line in recent[-10:]:
                sys.stdout.write(f"{line}\r\n")

        sys.stdout.write("---\r\n")
        sys.stdout.flush()

    def _write_output(self, line: str) -> None:
        """Write agent output to the terminal."""
        sys.stdout.write(f"{line}\r\n")
        sys.stdout.flush()

    def _write_status(self, msg: str) -> None:
        """Write a status message to the terminal."""
        sys.stdout.write(f"\r\n>>> {msg}\r\n")
        sys.stdout.flush()

    def _restore_terminal(self) -> None:
        """Restore terminal to original mode."""
        if self._original_termios is not None and sys.stdin.isatty():
            termios.tcsetattr(
                sys.stdin.fileno(), termios.TCSADRAIN, self._original_termios
            )
            self._original_termios = None
