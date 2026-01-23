"""Attach command: connect to a running ralph-uv session.

Dispatches by transport (local vs ziti) and session_type (tmux vs opencode-server):

| Transport | Session Type     | Attach Method                              |
|-----------|------------------|--------------------------------------------|
| local     | tmux             | tmux attach-session -t <name>              |
| local     | opencode-server  | opencode attach http://localhost:<port>     |
| ziti      | opencode-server  | opencode attach http://<ziti-intercept>:<port> |
| ziti      | tmux             | ssh -o ProxyCommand='ziti dial' tmux attach |
"""

from __future__ import annotations

import subprocess
import sys

from ralph_uv.session import (
    SessionDB,
    SessionInfo,
    opencode_server_alive,
    tmux_attach_session,
    tmux_session_alive,
    tmux_session_exists,
    tmux_session_name,
)


def attach(task_name: str) -> int:
    """Attach to a running ralph-uv session.

    Dispatches based on transport × session_type:
    - local opencode-server: opencode attach http://localhost:<port>
    - local tmux: tmux attach-session -t <name>
    - remote opencode-server: opencode attach <server_url>
    - remote tmux: SSH-over-Ziti with tmux attach

    Args:
        task_name: The task name to attach to.

    Returns:
        Exit code (0 = normal exit, 1 = error).
    """
    db = SessionDB()
    session = db.get(task_name)

    if session is None:
        print(
            f"Error: No session found for task '{task_name}'.",
            file=sys.stderr,
        )
        print(
            f"  Start one with: ralph-uv run tasks/{task_name}/",
            file=sys.stderr,
        )
        return 1

    # Dispatch by transport × session_type
    if session.is_remote:
        if session.session_type == "opencode-server":
            return _attach_remote_opencode(task_name, session, db)
        else:
            return _attach_remote_tmux(task_name, session, db)
    elif session.session_type == "opencode-server":
        return _attach_opencode_server(task_name, session.server_port, db)
    else:
        return _attach_tmux(task_name, db)


def _attach_opencode_server(task_name: str, port: int | None, db: SessionDB) -> int:
    """Attach to an opencode-server session via `opencode attach`.

    Args:
        task_name: The task name.
        port: The opencode serve port.
        db: Session database.

    Returns:
        Exit code.
    """
    if port is None:
        print(
            f"Error: Session '{task_name}' has no server port recorded.",
            file=sys.stderr,
        )
        return 1

    if not opencode_server_alive(port):
        # Server is not responding — mark as failed
        db.update_status(task_name, "failed")
        print(
            f"Error: OpenCode server for '{task_name}' is not responding "
            f"(port {port}).",
            file=sys.stderr,
        )
        print(
            f"  Restart with: ralph-uv run tasks/{task_name}/",
            file=sys.stderr,
        )
        return 1

    url = f"http://localhost:{port}"
    print(f"Attaching to opencode server at {url}...")

    # Run opencode attach — it takes over the terminal
    result = subprocess.run(["opencode", "attach", url])
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
                f"  Restart with: ralph-uv run tasks/{task_name}/",
                file=sys.stderr,
            )
        else:
            print(
                f"Error: No session found for task '{task_name}'.",
                file=sys.stderr,
            )
            print(
                f"  Start one with: ralph-uv run tasks/{task_name}/",
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


def _attach_remote_opencode(task_name: str, session: SessionInfo, db: SessionDB) -> int:
    """Attach to a remote opencode-server session via Ziti-proxied URL.

    The session's server_url contains the Ziti intercept address that the
    local Ziti tunneler transparently proxies to the remote opencode serve.
    The opencode attach command is already an HTTP client, so no special
    handling is needed — just pass the remote URL.

    Args:
        task_name: The task name.
        session: The session info with remote details.
        db: Session database.

    Returns:
        Exit code.
    """
    url = session.server_url
    if not url:
        print(
            f"Error: Remote session '{task_name}' has no server_url recorded.",
            file=sys.stderr,
        )
        return 1

    print(
        f"Attaching to remote opencode server at {url} (via {session.remote_host})..."
    )

    # opencode attach works with any HTTP URL — Ziti tunneler handles routing
    result = subprocess.run(["opencode", "attach", url])
    return result.returncode


def _attach_remote_tmux(task_name: str, session: SessionInfo, db: SessionDB) -> int:
    """Attach to a remote tmux session via SSH-over-Ziti.

    Uses SSH with a Ziti ProxyCommand to establish a connection to the
    remote machine, then runs tmux attach on the remote session.

    The Ziti service for SSH is named: ralph-ssh-{remote_host}
    The remote tmux session name follows the standard ralph- prefix.

    Args:
        task_name: The task name.
        session: The session info with remote details.
        db: Session database.

    Returns:
        Exit code.
    """
    if not session.remote_host:
        print(
            f"Error: Remote session '{task_name}' has no remote_host recorded.",
            file=sys.stderr,
        )
        return 1

    if not session.ziti_identity:
        print(
            f"Error: Remote session '{task_name}' has no Ziti identity path.",
            file=sys.stderr,
        )
        return 1

    remote_session_name = session.tmux_session or f"ralph-{task_name}"

    print(
        f"Attaching to remote tmux session '{remote_session_name}' "
        f"on {session.remote_host} via SSH-over-Ziti..."
    )

    # SSH-over-Ziti: use ProxyCommand to route through Ziti overlay
    # The SSH Ziti service is per-remote-machine, not per-loop
    ssh_service = f"ralph-ssh-{session.remote_host}"
    proxy_cmd = (
        f"ziti-edge-tunnel proxy -i {session.ziti_identity} "
        f"-s {ssh_service} -p {{port}}"
    )

    result = subprocess.run(
        [
            "ssh",
            "-o",
            f"ProxyCommand={proxy_cmd}",
            "-t",  # Force TTY allocation for tmux
            session.remote_host,
            "--",
            "tmux",
            "attach-session",
            "-t",
            remote_session_name,
        ]
    )
    return result.returncode
