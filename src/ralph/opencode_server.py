"""OpenCode server client for ralph.

Provides HTTP client methods to interact with a systemd-managed opencode server.
The server is expected to be running on port 14096 (standard systemd port).

Architecture:
- health_check(): Verifies GET /global/health returns OK
- create_session(): Creates a new opencode session with directory context and
  default permissions that allow all operations for autonomous agent use
- send_prompt(): Sends a prompt via POST /session/:id/message
- wait_for_idle(): Monitors SSE events for session.idle
- abort_session(): Stops processing via POST /session/:id/abort

The server is managed by systemd (opencode.service), not by ralph.
Sessions are scoped to a project directory via the ?directory= query parameter.
Permissions are passed at session creation time via the ruleset format.
"""

from __future__ import annotations

import json
import logging
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.error import URLError
from urllib.request import Request, urlopen


def _get_logger() -> logging.Logger:
    """Get or create the opencode-server logger."""
    logger = logging.getLogger("ralph.opencode_server")
    if not logger.handlers:
        log_dir = Path.home() / ".local" / "state" / "ralph"
        log_dir.mkdir(parents=True, exist_ok=True)
        handler = logging.FileHandler(log_dir / "opencode-server.log")
        handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s"))
        logger.addHandler(handler)
        logger.setLevel(logging.DEBUG)
    return logger


# Default systemd server port
DEFAULT_SERVER_PORT = 14096

# Timeouts
HEALTH_CHECK_TIMEOUT = 5  # seconds to wait for server health check
HEALTH_CHECK_INTERVAL = 0.5  # seconds between health check attempts
HTTP_TIMEOUT = 30  # seconds for HTTP requests
SSE_POLL_INTERVAL = 0.5  # seconds between SSE read attempts


class OpencodeServerError(Exception):
    """Raised when an opencode server operation fails."""


class OpencodeServerNotRunning(OpencodeServerError):
    """Raised when the systemd server is not running."""


@dataclass
class OpencodeSession:
    """Represents an opencode session on the server."""

    session_id: str
    url: str


class OpencodeClient:
    """HTTP client for interacting with a systemd-managed opencode server.

    The server is expected to be running via systemd on port 14096.
    All requests include the project directory for session scoping.

    Usage:
        client = OpencodeClient(project_dir=Path("/path/to/project"))
        client.check_server_running()  # Raises if not running
        session = client.create_session()
        client.send_prompt(session.session_id, "implement feature X")
        client.wait_for_idle(session.session_id)
    """

    def __init__(
        self,
        project_dir: Path,
        port: int = DEFAULT_SERVER_PORT,
        password: str = "",
    ) -> None:
        self.project_dir = project_dir.resolve()
        self.port = port
        self.password = password
        self._log = _get_logger()
        self._base_url = f"http://127.0.0.1:{self.port}"

    @property
    def url(self) -> str:
        """The base URL of the server."""
        return self._base_url

    def check_server_running(self) -> None:
        """Check if the systemd server is running.

        Raises OpencodeServerNotRunning if the server is not responding.
        """
        if not self._health_check():
            raise OpencodeServerNotRunning(
                f"OpenCode server not running on port {self.port}.\n"
                f"Start it with: systemctl --user start opencode"
            )
        self._log.info("Server health check passed")

    def _url_with_directory(self, path: str) -> str:
        """Build a URL with the ?directory= query parameter.

        All requests to the systemd server include the project directory
        so the server knows which project context to use.
        """
        separator = "&" if "?" in path else "?"
        return f"{self._base_url}{path}{separator}directory={self.project_dir}"

    # Default permissions for autonomous agent operation.
    # Uses ruleset format: array of {permission, pattern, action}.
    # The "*" permission wildcard matches all permissions.
    # Order matters: later rules override earlier ones.
    DEFAULT_PERMISSIONS: list[dict[str, str]] = [
        # Allow all operations by default
        {"permission": "*", "pattern": "*", "action": "allow"},
        # Explicitly allow external directories (outside project)
        {"permission": "external_directory", "pattern": "*", "action": "allow"},
        # Allow doom loop (rapid iteration without confirmation)
        {"permission": "doom_loop", "pattern": "*", "action": "allow"},
    ]

    def create_session(
        self,
        permissions: list[dict[str, str]] | None = None,
    ) -> OpencodeSession:
        """Create a new opencode session scoped to the project directory.

        Args:
            permissions: Optional permission ruleset. If not provided, uses
                DEFAULT_PERMISSIONS which allows all operations for autonomous use.

        Returns an OpencodeSession with the session ID.
        """
        url = self._url_with_directory("/session")
        self._log.info("Creating session: POST %s", url)

        # Use provided permissions or default to allow-all for autonomous operation
        ruleset = permissions if permissions is not None else self.DEFAULT_PERMISSIONS
        payload = {"permission": ruleset}

        response = self._http_post(url, payload)
        session_id = response.get("id", "")
        if not session_id:
            raise OpencodeServerError(
                f"Failed to create session: no ID in response: {response}"
            )

        self._log.info("Session created: %s (project=%s)", session_id, self.project_dir)
        return OpencodeSession(
            session_id=session_id,
            url=f"{self._base_url}/session/{session_id}",
        )

    def send_prompt(self, session_id: str, prompt: str) -> dict[str, Any]:
        """Send a prompt to a session synchronously.

        Uses POST /session/:id/message which blocks until the agent responds.
        Returns the response data.

        The opencode serve API expects a payload with a `parts` array:
        {"parts": [{"type": "text", "text": "..."}]}
        """
        url = self._url_with_directory(f"/session/{session_id}/message")
        self._log.info(
            "Sending prompt to session %s (length=%d)", session_id, len(prompt)
        )

        # OpenCode API expects parts array format
        payload = {"parts": [{"type": "text", "text": prompt}]}

        response = self._http_post(
            url,
            payload,
            timeout=None,  # No timeout for sync prompts
        )
        self._log.info("Prompt response received for session %s", session_id)
        return response

    def send_prompt_async(self, session_id: str, prompt: str) -> dict[str, Any]:
        """Send a prompt asynchronously (non-blocking).

        Uses POST /session/:id/prompt_async which returns immediately.
        Use wait_for_idle() or poll_session_status() to detect completion.

        The opencode serve API expects a payload with a `parts` array:
        {"parts": [{"type": "text", "text": "..."}]}
        """
        url = self._url_with_directory(f"/session/{session_id}/prompt_async")
        self._log.info(
            "Sending async prompt to session %s (length=%d)", session_id, len(prompt)
        )

        # OpenCode API expects parts array format
        payload = {"parts": [{"type": "text", "text": prompt}]}

        response = self._http_post(url, payload)
        self._log.info("Async prompt accepted for session %s", session_id)
        return response

    def poll_session_status(self, session_id: str) -> str:
        """Poll the status of a session via GET /session/status.

        Returns the status type: "idle", "busy", "retry", or "unknown".
        """
        url = self._url_with_directory("/session/status")

        try:
            req = self._build_request(url, method="GET")
            with urlopen(req, timeout=HTTP_TIMEOUT) as resp:
                resp_body = resp.read().decode("utf-8")
                if resp_body:
                    data: dict[str, Any] = json.loads(resp_body)
                    # Response is a map of session_id -> status info
                    # e.g., {"abc123": {"type": "busy", ...}}
                    session_status = data.get(session_id, {})
                    if isinstance(session_status, dict):
                        return str(session_status.get("type", "idle"))
                    # If session not in response, it's idle (completed)
                    return "idle"
        except (URLError, OSError, TimeoutError) as e:
            self._log.warning("poll_session_status: error polling %s: %s", url, e)
            return "unknown"
        except json.JSONDecodeError as e:
            self._log.warning("poll_session_status: invalid JSON: %s", e)
            return "unknown"

        return "unknown"

    def get_session_message_count(self, session_id: str) -> int:
        """Get the number of messages in a session.

        This can be used to detect if the user has sent additional messages
        while ralph is waiting, to avoid treating user-initiated completions
        as ralph iteration completions.

        Returns -1 on error.
        """
        # Use the /message endpoint - GET /session/:id only returns metadata
        url = self._url_with_directory(f"/session/{session_id}/message")

        try:
            req = self._build_request(url, method="GET")
            with urlopen(req, timeout=HTTP_TIMEOUT) as resp:
                resp_body = resp.read().decode("utf-8")
                if resp_body:
                    # Response is an array of messages directly
                    data = json.loads(resp_body)
                    return len(data) if isinstance(data, list) else -1
        except (URLError, OSError, TimeoutError, json.JSONDecodeError) as e:
            self._log.warning(
                "get_session_message_count: error for session %s: %s", session_id, e
            )
            return -1

        return -1

    def wait_for_idle(
        self,
        session_id: str,
        timeout: float | None = None,
        check_interval: float = SSE_POLL_INTERVAL,
    ) -> bool:
        """Wait for a session to become idle via SSE events.

        Connects to GET /event and watches for session.idle events
        matching the given session_id.

        Args:
            session_id: The session to monitor.
            timeout: Maximum seconds to wait (None = no timeout).
            check_interval: Seconds between polling the stream.

        Returns:
            True if idle was detected, False if timeout/error.
        """
        url = self._url_with_directory("/event")
        self._log.info(
            "Waiting for session.idle: session=%s, timeout=%s",
            session_id,
            timeout,
        )

        deadline = time.time() + timeout if timeout else None

        try:
            req = self._build_request(url, method="GET")
            req.add_header("Accept", "text/event-stream")
            req.add_header("Cache-Control", "no-cache")

            with urlopen(req, timeout=HTTP_TIMEOUT) as resp:
                # Read SSE stream line by line
                buffer = ""
                event_type = ""
                event_data = ""

                while True:
                    if deadline and time.time() > deadline:
                        self._log.warning("wait_for_idle: timeout reached")
                        return False

                    # Read available data (non-blocking would be ideal but
                    # urllib doesn't support it well, so we use a short timeout)
                    try:
                        chunk = resp.read(4096)
                        if not chunk:
                            # Connection closed
                            self._log.warning("wait_for_idle: SSE connection closed")
                            return False
                        buffer += chunk.decode("utf-8", errors="replace")
                    except TimeoutError:
                        # No data available yet, continue polling
                        time.sleep(check_interval)
                        continue

                    # Parse SSE events from buffer
                    while "\n\n" in buffer:
                        event_block, buffer = buffer.split("\n\n", 1)
                        lines = event_block.strip().split("\n")

                        event_type = ""
                        event_data = ""
                        for line in lines:
                            if line.startswith("event:"):
                                event_type = line[6:].strip()
                            elif line.startswith("data:"):
                                event_data = line[5:].strip()

                        if event_type == "session.idle":
                            try:
                                data = json.loads(event_data) if event_data else {}
                                idle_session = data.get("sessionID", "")
                                if idle_session == session_id or not idle_session:
                                    self._log.info(
                                        "wait_for_idle: session.idle received "
                                        "for session %s",
                                        session_id,
                                    )
                                    return True
                            except json.JSONDecodeError:
                                # If we can't parse, treat any session.idle as ours
                                self._log.info(
                                    "wait_for_idle: session.idle received (unparsed)"
                                )
                                return True

        except (URLError, OSError, TimeoutError) as e:
            self._log.error("wait_for_idle: SSE connection error: %s", e)
            return False

    def abort_session(self, session_id: str) -> bool:
        """Abort a running session.

        Uses POST /session/:id/abort to stop processing.
        Returns True if successful.
        """
        url = self._url_with_directory(f"/session/{session_id}/abort")
        self._log.info("Aborting session: %s", session_id)

        try:
            self._http_post(url, {})
            self._log.info("Session %s aborted successfully", session_id)
            return True
        except OpencodeServerError as e:
            self._log.error("Failed to abort session %s: %s", session_id, e)
            return False

    # --- Private Methods ---

    def _health_check(self) -> bool:
        """Perform a single health check. Returns True if healthy."""
        url = f"{self._base_url}/global/health"
        try:
            req = self._build_request(url, method="GET")
            with urlopen(req, timeout=2) as resp:
                return bool(resp.status == 200)
        except (URLError, OSError, TimeoutError):
            return False

    def _http_post(
        self, url: str, data: dict[str, Any], timeout: float | None = HTTP_TIMEOUT
    ) -> dict[str, Any]:
        """Make an HTTP POST request with JSON body.

        Returns the parsed JSON response.
        Raises OpencodeServerError on failure.
        """
        body = json.dumps(data).encode("utf-8")
        req = self._build_request(url, method="POST")
        req.add_header("Content-Type", "application/json")
        req.data = body

        try:
            with urlopen(req, timeout=timeout) as resp:
                resp_body = resp.read().decode("utf-8")
                if resp_body:
                    result: dict[str, Any] = json.loads(resp_body)
                    return result
                return {}
        except (URLError, OSError, TimeoutError) as e:
            raise OpencodeServerError(f"HTTP POST {url} failed: {e}") from e
        except json.JSONDecodeError as e:
            raise OpencodeServerError(f"Invalid JSON response from {url}: {e}") from e

    def _build_request(self, url: str, method: str = "GET") -> Request:
        """Build an HTTP request with optional auth headers."""
        req = Request(url, method=method)
        if self.password:
            import base64

            credentials = base64.b64encode(f":{self.password}".encode()).decode()
            req.add_header("Authorization", f"Basic {credentials}")
        return req

    def _http_get(self, url: str, timeout: float | None = HTTP_TIMEOUT) -> Any:
        """Make an HTTP GET request.

        Returns the parsed JSON response, or None if empty.
        Raises OpencodeServerError on failure.
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
        except json.JSONDecodeError as e:
            raise OpencodeServerError(f"Invalid JSON response from {url}: {e}") from e

    def _http_delete(
        self, url: str, data: dict[str, Any], timeout: float | None = HTTP_TIMEOUT
    ) -> Any:
        """Make an HTTP DELETE request with JSON body.

        Returns the parsed JSON response, or True if empty (success).
        Raises OpencodeServerError on failure.
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
        except json.JSONDecodeError as e:
            raise OpencodeServerError(f"Invalid JSON response from {url}: {e}") from e

    # --- Worktree API Methods ---

    def create_worktree(
        self,
        name: str | None = None,
        start_command: str | None = None,
    ) -> dict[str, str]:
        """Create a new worktree via POST /experimental/worktree.

        Args:
            name: Optional worktree name (auto-generated if not provided).
            start_command: Optional startup script to run after worktree init.

        Returns:
            Dict with "name", "branch", and "directory" keys.

        Note: Returns immediately. Git reset and startup scripts run async.
        """
        url = self._url_with_directory("/experimental/worktree")
        self._log.info("Creating worktree: POST %s (name=%s)", url, name)

        payload: dict[str, str] = {}
        if name:
            payload["name"] = name
        if start_command:
            payload["startCommand"] = start_command

        response = self._http_post(url, payload)
        self._log.info(
            "Worktree created: name=%s, branch=%s, directory=%s",
            response.get("name"),
            response.get("branch"),
            response.get("directory"),
        )
        result: dict[str, str] = {
            "name": str(response.get("name", "")),
            "branch": str(response.get("branch", "")),
            "directory": str(response.get("directory", "")),
        }
        return result

    def list_worktrees(self) -> list[str]:
        """List worktree directories for the current project.

        Returns a list of worktree directory paths.
        """
        url = self._url_with_directory("/experimental/worktree")
        self._log.info("Listing worktrees: GET %s", url)
        result = self._http_get(url)
        if result is None:
            return []
        if isinstance(result, list):
            return [str(d) for d in result]
        return []

    def reset_worktree(self, directory: str) -> bool:
        """Reset a worktree to the default branch (main/master).

        Args:
            directory: Full path to the worktree directory.

        Returns:
            True on success.
        """
        url = self._url_with_directory("/experimental/worktree/reset")
        self._log.info("Resetting worktree: POST %s (directory=%s)", url, directory)
        self._http_post(url, {"directory": directory})
        self._log.info("Worktree reset: %s", directory)
        return True

    def remove_worktree(self, directory: str) -> bool:
        """Remove a worktree and delete its branch.

        Args:
            directory: Full path to the worktree directory.

        Returns:
            True on success.
        """
        url = self._url_with_directory("/experimental/worktree")
        self._log.info("Removing worktree: DELETE %s (directory=%s)", url, directory)
        self._http_delete(url, {"directory": directory})
        self._log.info("Worktree removed: %s", directory)
        return True


# Backwards compatibility aliases
OpencodeServer = OpencodeClient


def check_server_running(port: int = DEFAULT_SERVER_PORT) -> bool:
    """Check if the systemd-managed opencode server is running.

    Args:
        port: The port to check (default: 14096).

    Returns:
        True if the server responds to health checks.
    """
    try:
        with urlopen(f"http://127.0.0.1:{port}/global/health", timeout=2) as resp:
            return bool(resp.status == 200)
    except (URLError, OSError, TimeoutError):
        return False
