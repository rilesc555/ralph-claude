"""Session management for ralph.

Provides dual-mode session running (tmux for claude, opencode serve for opencode)
with SQLite registry for tracking multiple concurrent loops. Supports status
queries, graceful stop, and checkpoint/pause operations.

Session Types:
- tmux: Claude agent runs in a detached tmux session
- opencode-server: OpenCode agent runs via opencode serve HTTP API
"""

from __future__ import annotations

import json
import os
import signal
import sqlite3
import subprocess
import sys
from dataclasses import asdict, dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

import libtmux
from libtmux.exc import LibTmuxException

# Default paths
DATA_DIR = Path.home() / ".local" / "share" / "ralph"
DB_PATH = DATA_DIR / "sessions.db"

# Tmux session name prefix to avoid collisions
TMUX_PREFIX = "ralph-"

# Signal file for stop/checkpoint communication
SIGNAL_DIR = DATA_DIR / "signals"


@dataclass
class SessionInfo:
    """Information about a ralph session.

    Session types:
    - tmux: Claude agent runs in a detached tmux session
    - opencode-server: OpenCode agent runs via systemd-managed opencode server

    PID tracking:
    - pid: The loop worker process PID (for stop signals)

    For tmux mode, pid is the tmux pane process PID.
    For opencode-server mode, the server is managed by systemd (not ralph).

    Workspace support:
    - workspace_dir: If set, the session runs in an isolated git worktree
    - workspace_name: The worktree name (for cleanup and display)
    """

    task_name: str
    task_dir: str
    pid: int  # Loop worker PID (the process running LoopRunner)
    tmux_session: str
    agent: str
    status: str  # "running", "stopped", "completed", "failed", "checkpointed"
    started_at: str
    updated_at: str
    iteration: int = 0
    current_story: str = ""
    max_iterations: int = 50
    session_type: str = "tmux"  # "tmux" or "opencode-server"
    server_port: int | None = None  # Port for opencode server (historical reference)
    server_url: str = ""  # Full URL for opencode attach
    opencode_session_id: str = ""  # Current opencode session ID (for attach --session)
    workspace_dir: str = ""  # Worktree directory (empty if not using workspace)
    workspace_name: str = ""  # Worktree name for cleanup and display

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        return asdict(self)


class SessionDB:
    """SQLite registry for ralph sessions.

    Database is stored at ~/.local/share/ralph/sessions.db.
    """

    def __init__(self, db_path: Path | None = None) -> None:
        self.db_path = db_path or DB_PATH
        self._ensure_dir()
        self._init_db()

    def _ensure_dir(self) -> None:
        """Ensure the data directory exists."""
        self.db_path.parent.mkdir(parents=True, exist_ok=True)

    def _init_db(self) -> None:
        """Initialize the database schema."""
        with self._connect() as conn:
            conn.execute("""
                CREATE TABLE IF NOT EXISTS sessions (
                    task_name TEXT PRIMARY KEY,
                    task_dir TEXT NOT NULL,
                    pid INTEGER NOT NULL,
                    tmux_session TEXT NOT NULL,
                    agent TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'running',
                    started_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    iteration INTEGER NOT NULL DEFAULT 0,
                    current_story TEXT NOT NULL DEFAULT '',
                    max_iterations INTEGER NOT NULL DEFAULT 50,
                    session_type TEXT NOT NULL DEFAULT 'tmux',
                    server_port INTEGER,
                    server_url TEXT NOT NULL DEFAULT '',
                    opencode_session_id TEXT NOT NULL DEFAULT '',
                    workspace_dir TEXT NOT NULL DEFAULT '',
                    workspace_name TEXT NOT NULL DEFAULT ''
                )
            """)
            # Handle schema migrations for existing databases
            self._migrate_schema(conn)

    def _connect(self) -> sqlite3.Connection:
        """Create a database connection."""
        conn = sqlite3.connect(str(self.db_path))
        conn.row_factory = sqlite3.Row
        return conn

    def _migrate_schema(self, conn: sqlite3.Connection) -> None:
        """Apply schema migrations for new columns.

        SQLite doesn't support IF NOT EXISTS for ALTER TABLE, so we check
        existing columns first.
        """
        # Get existing columns
        cursor = conn.execute("PRAGMA table_info(sessions)")
        existing_columns = {row[1] for row in cursor.fetchall()}

        # Add workspace columns if missing
        if "workspace_dir" not in existing_columns:
            conn.execute(
                "ALTER TABLE sessions ADD COLUMN workspace_dir TEXT NOT NULL DEFAULT ''"
            )
        if "workspace_name" not in existing_columns:
            conn.execute(
                "ALTER TABLE sessions "
                "ADD COLUMN workspace_name TEXT NOT NULL DEFAULT ''"
            )

    def register(self, session: SessionInfo) -> None:
        """Register a new session or update existing one."""
        with self._connect() as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO sessions
                (task_name, task_dir, pid, tmux_session, agent, status,
                 started_at, updated_at, iteration, current_story, max_iterations,
                 session_type, server_port, server_url, opencode_session_id,
                 workspace_dir, workspace_name)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    session.task_name,
                    session.task_dir,
                    session.pid,
                    session.tmux_session,
                    session.agent,
                    session.status,
                    session.started_at,
                    session.updated_at,
                    session.iteration,
                    session.current_story,
                    session.max_iterations,
                    session.session_type,
                    session.server_port,
                    session.server_url,
                    session.opencode_session_id,
                    session.workspace_dir,
                    session.workspace_name,
                ),
            )

    def update_status(self, task_name: str, status: str) -> None:
        """Update session status."""
        now = datetime.now().isoformat()
        with self._connect() as conn:
            conn.execute(
                "UPDATE sessions SET status = ?, updated_at = ? WHERE task_name = ?",
                (status, now, task_name),
            )

    def update_progress(
        self, task_name: str, iteration: int, current_story: str
    ) -> None:
        """Update session progress (iteration and current story)."""
        now = datetime.now().isoformat()
        with self._connect() as conn:
            conn.execute(
                """UPDATE sessions
                SET iteration = ?, current_story = ?, updated_at = ?
                WHERE task_name = ?""",
                (iteration, current_story, now, task_name),
            )

    def update_opencode_session_id(
        self, task_name: str, opencode_session_id: str
    ) -> None:
        """Update the current opencode session ID for attach --session."""
        now = datetime.now().isoformat()
        with self._connect() as conn:
            conn.execute(
                """UPDATE sessions
                SET opencode_session_id = ?, updated_at = ?
                WHERE task_name = ?""",
                (opencode_session_id, now, task_name),
            )

    def get(self, task_name: str) -> SessionInfo | None:
        """Get session info by task name."""
        with self._connect() as conn:
            row = conn.execute(
                "SELECT * FROM sessions WHERE task_name = ?", (task_name,)
            ).fetchone()
            if row is None:
                return None
            return self._row_to_session(row)

    def list_all(self) -> list[SessionInfo]:
        """List all sessions."""
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT * FROM sessions ORDER BY started_at DESC"
            ).fetchall()
            return [self._row_to_session(row) for row in rows]

    def remove(self, task_name: str) -> None:
        """Remove a session entry."""
        with self._connect() as conn:
            conn.execute("DELETE FROM sessions WHERE task_name = ?", (task_name,))

    def _row_to_session(self, row: sqlite3.Row) -> SessionInfo:
        """Convert a database row to SessionInfo.

        Note: Old databases may have a server_pid column but it's no longer
        used since systemd manages the server. Also handles missing workspace
        columns for backwards compatibility.
        """
        # Handle missing workspace columns in older databases
        # The _migrate_schema should add them, but handle gracefully anyway
        workspace_dir = ""
        workspace_name = ""
        try:
            workspace_dir = row["workspace_dir"] or ""
            workspace_name = row["workspace_name"] or ""
        except (KeyError, IndexError):
            pass

        return SessionInfo(
            task_name=row["task_name"],
            task_dir=row["task_dir"],
            pid=row["pid"],
            tmux_session=row["tmux_session"],
            agent=row["agent"],
            status=row["status"],
            started_at=row["started_at"],
            updated_at=row["updated_at"],
            iteration=row["iteration"],
            current_story=row["current_story"],
            max_iterations=row["max_iterations"],
            session_type=row["session_type"],
            server_port=row["server_port"],
            server_url=row["server_url"],
            opencode_session_id=row["opencode_session_id"],
            workspace_dir=workspace_dir,
            workspace_name=workspace_name,
        )


# --- Tmux Operations (via libtmux) ---


def _get_server() -> libtmux.Server:
    """Get a libtmux Server instance."""
    return libtmux.Server()


def tmux_session_exists(session_name: str) -> bool:
    """Check if a tmux session exists."""
    try:
        server = _get_server()
        matches = server.sessions.filter(session_name=session_name)
        return len(matches) > 0
    except LibTmuxException:
        return False


def tmux_session_alive(session_name: str) -> bool:
    """Check if a tmux session exists AND its pane process is still running.

    With remain-on-exit, the session stays around after the process dies.
    This checks pane_dead_status: None means alive, a value means exited.
    """
    try:
        server = _get_server()
        matches = server.sessions.filter(session_name=session_name)
        if not matches:
            return False
        session = matches[0]
        pane = session.active_window.active_pane
        if pane is None:
            return False
        # pane_dead_status is None when alive, exit code (int) when dead
        return pane.pane_dead_status is None
    except (LibTmuxException, AttributeError):
        return False


def tmux_create_session(
    session_name: str,
    command: str,
    cwd: str,
    *,
    environment: dict[str, str] | None = None,
    width: int = 200,
    height: int = 50,
) -> int:
    """Create a new detached tmux session running the given command.

    Args:
        session_name: Name for the tmux session.
        command: Shell command string to run in the session.
        cwd: Working directory for the session.
        environment: Environment variables to set in the session.
        width: Terminal width (default 200).
        height: Terminal height (default 50).

    Returns:
        The PID of the process running in the tmux pane.
    """
    import shlex

    server = _get_server()

    # Build the shell command with env vars as inline prefix.
    # tmux's window_command is run through the default shell, so
    # `KEY=value command args` works natively.
    if environment:
        exports = " ".join(f"{k}={shlex.quote(v)}" for k, v in environment.items())
        shell_cmd = f"{exports} {command}"
    else:
        shell_cmd = command

    session = server.new_session(
        session_name=session_name,
        start_directory=cwd,
        window_command=shell_cmd,
        attach=False,
        x=width,
        y=height,
    )

    # Keep the pane open if the process exits, so crash output is visible
    session.set_option("remain-on-exit", "on")

    pane = session.active_window.active_pane
    if pane is not None:
        pid_str = pane.pane_pid
        if pid_str:
            return int(pid_str)
    return os.getpid()


def tmux_kill_session(session_name: str) -> None:
    """Kill a tmux session."""
    try:
        server = _get_server()
        matches = server.sessions.filter(session_name=session_name)
        if matches:
            matches[0].kill()
    except LibTmuxException:
        pass


def tmux_attach_session(session_name: str) -> int:
    """Attach to a tmux session (takes over terminal).

    Returns the exit code from tmux attach.
    """
    result = subprocess.run(["tmux", "attach-session", "-t", session_name])
    return result.returncode


# --- OpenCode Server Operations ---


def opencode_server_alive(port: int | None) -> bool:
    """Check if an opencode server is alive by hitting its health endpoint.

    Args:
        port: The port the opencode serve is listening on.

    Returns:
        True if the health endpoint responds with 200.
    """
    if port is None:
        return False
    try:
        from urllib.request import urlopen

        with urlopen(f"http://127.0.0.1:{port}/global/health", timeout=2) as resp:
            return bool(resp.status == 200)
    except Exception:
        return False


# --- Signal File Operations ---


def get_signal_path(task_name: str) -> Path:
    """Get the signal file path for a task."""
    SIGNAL_DIR.mkdir(parents=True, exist_ok=True)
    return SIGNAL_DIR / f"{task_name}.signal"


def write_signal(task_name: str, signal_type: str) -> None:
    """Write a signal file for the given task.

    Signal types: "stop", "checkpoint"
    """
    signal_path = get_signal_path(task_name)
    data = {
        "type": signal_type,
        "timestamp": datetime.now().isoformat(),
    }
    signal_path.write_text(json.dumps(data))


def read_signal(task_name: str) -> dict[str, str] | None:
    """Read and consume a signal file. Returns None if no signal."""
    signal_path = get_signal_path(task_name)
    if not signal_path.is_file():
        return None
    try:
        data: dict[str, str] = json.loads(signal_path.read_text())
        signal_path.unlink()  # Consume the signal
        return data
    except (json.JSONDecodeError, OSError):
        return None


def clear_signal(task_name: str) -> None:
    """Clear any pending signal for a task."""
    signal_path = get_signal_path(task_name)
    if signal_path.is_file():
        signal_path.unlink()


# --- Task Name Utilities ---


def task_name_from_dir(task_dir: Path) -> str:
    """Extract task name from task directory path.

    e.g., /path/to/tasks/my-feature -> my-feature
    """
    return task_dir.name


def tmux_session_name(task_name: str) -> str:
    """Generate tmux session name from task name."""
    return f"{TMUX_PREFIX}{task_name}"


# --- High-Level Session Operations ---


def stop_session(task_name: str, db: SessionDB | None = None) -> bool:
    """Send stop signal to a running session.

    Dispatches by transport and session type:
    - opencode-server: Write stop signal, send SIGTERM to loop worker process
    - tmux: Writes a signal file and sends SIGINT

    Note: The opencode server is managed by systemd, not ralph. Stopping a
    session only stops the loop worker, not the server itself.

    Returns True if the signal was sent successfully.
    """
    if db is None:
        db = SessionDB()

    session = db.get(task_name)
    if session is None:
        print(f"Error: No session found for task '{task_name}'", file=sys.stderr)
        return False

    if session.status != "running":
        print(
            f"Error: Session '{task_name}' is not running (status: {session.status})",
            file=sys.stderr,
        )
        return False

    if session.session_type == "opencode-server":
        # OpenCode server mode: stop the loop worker process
        # 1. Write stop signal (loop checks this between iterations)
        write_signal(task_name, "stop")

        # 2. Send SIGTERM to the loop worker process (pid)
        try:
            os.kill(session.pid, signal.SIGTERM)
        except (OSError, ProcessLookupError):
            pass  # Loop may already be gone

        db.update_status(task_name, "stopped")
        print(f"Stop signal sent to session '{task_name}'")
        return True

    # Local tmux sessions: write stop signal file
    write_signal(task_name, "stop")

    # Also send SIGINT to the process as a backup
    try:
        os.kill(session.pid, signal.SIGINT)
    except (OSError, ProcessLookupError):
        pass  # Process may already be gone

    print(f"Stop signal sent to session '{task_name}'")
    return True


def checkpoint_session(task_name: str, db: SessionDB | None = None) -> bool:
    """Send checkpoint signal to a running session.

    The session will save state and pause after the current iteration.
    Returns True if the signal was sent successfully.
    """
    if db is None:
        db = SessionDB()

    session = db.get(task_name)
    if session is None:
        print(f"Error: No session found for task '{task_name}'", file=sys.stderr)
        return False

    if session.status != "running":
        print(
            f"Error: Session '{task_name}' is not running (status: {session.status})",
            file=sys.stderr,
        )
        return False

    # Write checkpoint signal file
    write_signal(task_name, "checkpoint")
    print(f"Checkpoint signal sent to session '{task_name}'")
    return True


def get_status(as_json: bool = False, db: SessionDB | None = None) -> str:
    """Get status of all sessions.

    Returns formatted string for display, or JSON if as_json=True.
    Lists all session types: local tmux, local opencode, remote tmux, remote opencode.
    """
    if db is None:
        db = SessionDB()

    sessions = db.list_all()

    # Validate running sessions against actual state
    for s in sessions:
        if s.status == "running":
            if s.session_type == "opencode-server":
                if not opencode_server_alive(s.server_port):
                    db.update_status(s.task_name, "failed")
                    s.status = "failed"
            elif not tmux_session_exists(s.tmux_session):
                db.update_status(s.task_name, "failed")
                s.status = "failed"

    if as_json:
        return json.dumps([s.to_dict() for s in sessions], indent=2)

    if not sessions:
        return "No sessions found."

    lines: list[str] = []
    lines.append(
        f"{'Task':<25} {'Status':<12} {'Agent':<9} {'Iter':<8} {'Port':<7} {'Story'}"
    )
    lines.append("-" * 80)

    for s in sessions:
        iter_str = f"{s.iteration}/{s.max_iterations}"
        story = s.current_story or "-"
        # Show port only for running sessions to avoid confusion.
        # Dead/stopped sessions showing ports misleads users.
        if (
            s.session_type == "opencode-server"
            and s.server_port
            and s.status == "running"
        ):
            port_str = str(s.server_port)
        elif s.tmux_session and s.status == "running":
            port_str = s.tmux_session.replace("ralph-", "")[:6]
        else:
            port_str = "-"
        lines.append(
            f"{s.task_name:<25} {s.status:<12} {s.agent:<9} "
            f"{iter_str:<8} {port_str:<7} {story}"
        )

    return "\n".join(lines)
