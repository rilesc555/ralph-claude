"""Attach command: connect to a running ralph session.

Dispatches by session_type:
- tmux: tmux attach-session -t <name>
- opencode-server: opencode attach http://localhost:14096 (systemd server)
"""

from __future__ import annotations

import subprocess
import sys

from ralph.opencode_server import DEFAULT_SERVER_PORT, check_server_running
from ralph.session import (
    SessionDB,
    SessionInfo,
    tmux_attach_session,
    tmux_session_alive,
    tmux_session_exists,
    tmux_session_name,
)


def _find_most_recent_session(db: SessionDB) -> SessionInfo | None:
    """Find the most recently active session across all tasks.

    Sessions are already ordered by started_at DESC from list_all().
    Prefers running sessions, then falls back to most recent non-running.

    Returns:
        The most recently active session, or None if no sessions exist.
    """
    sessions = db.list_all()
    if not sessions:
        return None

    # First, try to find a running session (most recent)
    for s in sessions:
        if s.status == "running":
            return s

    # Fall back to the most recent session regardless of status
    return sessions[0] if sessions else None


def attach(
    task_name: str | None = None,
    session_id: str | None = None,
) -> int:
    """Attach to a running ralph session.

    Dispatches based on session_type:
    - opencode-server: opencode attach http://localhost:<port>
    - tmux: tmux attach-session -t <name>

    Args:
        task_name: The task name to attach to. If None, attaches to the most
            recently active session across all tasks.
        session_id: Optional specific opencode session ID to attach to.
            Overrides the stored session ID for opencode-server sessions.

    Returns:
        Exit code (0 = normal exit, 1 = error).
    """
    db = SessionDB()

    # If no task specified, find the most recently active session
    if task_name is None:
        session = _find_most_recent_session(db)
        if session is None:
            print(
                "Error: No sessions found.",
                file=sys.stderr,
            )
            print(
                "  Start one with: ralph run <task>",
                file=sys.stderr,
            )
            return 1
        task_name = session.task_name
        print(f"Attaching to most recent session: {task_name}")
    else:
        session = db.get(task_name)

    if session is None:
        print(
            f"Error: No session found for task '{task_name}'.",
            file=sys.stderr,
        )
        print(
            f"  Start one with: ralph run tasks/{task_name}/",
            file=sys.stderr,
        )
        return 1

    if session.session_type == "opencode-server":
        # Use provided session_id or fall back to stored one
        effective_session_id = session_id or session.opencode_session_id
        return _attach_opencode_server(task_name, effective_session_id, db)
    else:
        return _attach_tmux(task_name, db)


def _attach_opencode_server(
    task_name: str,
    opencode_session_id: str,
    db: SessionDB,
) -> int:
    """Attach to an opencode-server session via `opencode attach`.

    Uses the systemd-managed global server on port 14096.

    Args:
        task_name: The task name.
        opencode_session_id: The active opencode session ID to attach to.
        db: Session database.

    Returns:
        Exit code.
    """
    # Get current session status to make correct status transition decisions
    session = db.get(task_name)
    current_status = session.status if session else None

    # Check if systemd server is running
    if not check_server_running(DEFAULT_SERVER_PORT):
        print(
            f"Error: OpenCode server is not running on port {DEFAULT_SERVER_PORT}.",
            file=sys.stderr,
        )
        print(
            "  Start it with: systemctl --user start opencode",
            file=sys.stderr,
        )
        return 1

    # Check if session is already in a terminal state
    if current_status in ("stopped", "completed"):
        print(
            f"Session '{task_name}' is {current_status}.",
            file=sys.stderr,
        )
        print(
            f"  Restart with: ralph run {task_name}",
            file=sys.stderr,
        )
        return 1

    # Verify session ID is available for attach
    if not opencode_session_id:
        print(
            f"Error: No opencode session ID recorded for '{task_name}'.",
            file=sys.stderr,
        )
        print(
            "  The session may have completed or never started properly.",
            file=sys.stderr,
        )
        print(
            f"  Restart with: ralph run {task_name}",
            file=sys.stderr,
        )
        return 1

    url = f"http://localhost:{DEFAULT_SERVER_PORT}"

    # Build attach command with session ID
    cmd = ["opencode", "attach", url, "--session", opencode_session_id]
    print(f"Attaching to opencode session {opencode_session_id} at {url}...")

    # Run opencode attach — it takes over the terminal
    result = subprocess.run(cmd)
    return result.returncode


def _attach_tmux(task_name: str, db: SessionDB) -> int:
    """Attach to a tmux session.

    Args:
        task_name: The task name.
        db: Session database.

    Returns:
        Exit code.
    """
    session_name = tmux_session_name(task_name)

    if not tmux_session_exists(session_name):
        # No tmux session at all — check SQLite for context
        session = db.get(task_name)
        if session is not None:
            # Stale DB entry — mark as failed
            if session.status == "running":
                db.update_status(task_name, "failed")
            print(
                f"Error: Session '{task_name}' is no longer running "
                f"(tmux session gone, last status: {session.status}).",
                file=sys.stderr,
            )
            print(
                f"  Restart with: ralph run tasks/{task_name}/",
                file=sys.stderr,
            )
        else:
            print(
                f"Error: No session found for task '{task_name}'.",
                file=sys.stderr,
            )
            print(
                f"  Start one with: ralph run tasks/{task_name}/",
                file=sys.stderr,
            )
        return 1

    if not tmux_session_alive(session_name):
        # Session exists (remain-on-exit) but process is dead
        print(
            f"Error: Session '{task_name}' process has exited.",
            file=sys.stderr,
        )
        print(
            "  The tmux pane still has output. Attach to view crash log:",
            file=sys.stderr,
        )
        print(f"  tmux attach -t {session_name}", file=sys.stderr)
        print(
            f"  Then kill it: tmux kill-session -t {session_name}",
            file=sys.stderr,
        )
        return 1

    # Attach to the tmux session
    return tmux_attach_session(session_name)
