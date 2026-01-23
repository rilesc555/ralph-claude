"""Attach command: connect to a running ralph-uv tmux session.

Wraps `tmux attach-session` to connect the user's terminal directly to
the agent's TUI (opencode) or output (claude) running in the tmux pane.
"""

from __future__ import annotations

import os
import subprocess
import sys

from ralph_uv.session import (
    SessionDB,
    tmux_session_exists,
    tmux_session_name,
)


def attach(task_name: str) -> int:
    """Attach to a running ralph-uv tmux session.

    Connects the user's terminal directly to the tmux pane where the
    agent is running. For opencode, this shows the full TUI. For claude,
    this shows the streaming output.

    Args:
        task_name: The task name to attach to.

    Returns:
        Exit code (0 = normal exit, 1 = error).
    """
    session_name = tmux_session_name(task_name)

    # Check if session exists
    if not tmux_session_exists(session_name):
        # Check SQLite for more info
        db = SessionDB()
        session = db.get(task_name)
        if session is not None:
            print(
                f"Error: Session '{task_name}' is {session.status} "
                f"(tmux session no longer exists).",
                file=sys.stderr,
            )
        else:
            print(
                f"Error: No session found for task '{task_name}'.",
                file=sys.stderr,
            )
            print(
                f"  Start one with: ralph-uv run tasks/{task_name}",
                file=sys.stderr,
            )
        return 1

    # Attach to the tmux session
    result = subprocess.run(
        ["tmux", "attach-session", "-t", session_name],
    )
    return result.returncode
