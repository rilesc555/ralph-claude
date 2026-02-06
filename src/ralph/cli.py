"""Ralph CLI entrypoint using Click."""

from __future__ import annotations

import json
import os
import shlex
import shutil
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import TYPE_CHECKING

import click
from click.shell_completion import CompletionItem

from ralph import __version__
from ralph.agents import VALID_AGENTS
from ralph.attach import attach
from ralph.loop import LoopConfig, LoopRunner
from ralph.opencode_server import (
    DEFAULT_SERVER_PORT,
    OpencodeClient,
    OpencodeServerNotRunning,
)
from ralph.session import (
    SessionDB,
    SessionInfo,
    checkpoint_session,
    get_status,
    stop_session,
    task_name_from_dir,
    tmux_create_session,
    tmux_kill_session,
    tmux_session_alive,
    tmux_session_exists,
    tmux_session_name,
)

if TYPE_CHECKING:
    from click import Context, Parameter

DEFAULT_ITERATIONS = 10


# --- Helpers ---


def _get_git_root(from_dir: Path | None = None) -> Path | None:
    """Find the git root directory.

    Args:
        from_dir: Directory to search from. If None, uses current working directory.

    Returns:
        Path to the git root directory, or None if not in a git repository.
    """
    import subprocess

    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            check=True,
            cwd=str(from_dir) if from_dir else None,
        )
        return Path(result.stdout.strip())
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None


def _resolve_task_dir(task_input: str) -> Path | None:
    """Resolve a task directory from user input.

    Supports:
    - Full path: tasks/my-feature or /abs/path/tasks/my-feature
    - Task name only: my-feature (resolves to tasks/my-feature at git root)

    Returns:
        Resolved Path if found, None otherwise.
    """
    input_path = Path(task_input)

    # If it's already a valid directory with prd.json, use it
    if input_path.is_dir() and (input_path / "prd.json").is_file():
        return input_path.resolve()

    # Try resolving as absolute path
    if input_path.is_absolute():
        return None  # Absolute path didn't exist

    # Try as relative path from cwd
    cwd_path = Path.cwd() / input_path
    if cwd_path.is_dir() and (cwd_path / "prd.json").is_file():
        return cwd_path.resolve()

    # Try as task name under tasks/ at git root
    git_root = _get_git_root()
    if git_root:
        task_path = git_root / "tasks" / task_input
        if task_path.is_dir() and (task_path / "prd.json").is_file():
            return task_path.resolve()

    return None


def _find_active_tasks() -> list[Path]:
    """Find active task directories (those with prd.json, excluding archived)."""
    tasks_dir = Path("tasks")
    if not tasks_dir.is_dir():
        return []

    results: list[Path] = []
    for prd_file in sorted(tasks_dir.rglob("prd.json")):
        if "archived" in prd_file.parts:
            continue
        results.append(prd_file.parent)

    return results


def _display_task_info(task_dir: Path) -> str:
    """Format a task directory for display."""
    prd_file = task_dir / "prd.json"
    total = "?"
    done = "?"
    prd_type = "feature"

    try:
        prd = json.loads(prd_file.read_text())
        stories = prd.get("userStories", [])
        total = str(len(stories))
        done = str(sum(1 for s in stories if s.get("passes", False)))
        prd_type = str(prd.get("type", "feature"))
    except (json.JSONDecodeError, OSError):
        pass

    return f"{str(task_dir):<35} [{done}/{total}] ({prd_type})"


def _detect_installed_agents() -> list[str]:
    """Detect which supported agents are installed."""
    return [agent for agent in VALID_AGENTS if shutil.which(agent) is not None]


def _prompt_task_selection() -> Path | None:
    """Interactively prompt the user to select a task directory."""
    tasks = _find_active_tasks()

    if not tasks:
        if not Path("tasks").is_dir():
            click.echo("No tasks/ directory found in current project.")
        else:
            click.echo("No active tasks found in tasks/.")
        click.echo()
        click.echo("To create a new task:")
        click.echo("  1. Use /prd in Claude Code to create a PRD")
        click.echo("  2. Use /ralph to convert it to prd.json")
        click.echo("  3. Run: ralph run tasks/{effort-name}")
        return None

    if len(tasks) == 1:
        click.echo(f"Found one active task: {tasks[0]}")
        return tasks[0]

    # Multiple tasks — prompt
    click.echo()
    click.echo("=" * 67)
    click.echo("  Ralph - Select a Task")
    click.echo("=" * 67)
    click.echo()

    for i, task in enumerate(tasks, 1):
        click.echo(f"  {i}) {_display_task_info(task)}")

    click.echo()
    selection: int = click.prompt(f"Select task [1-{len(tasks)}]", type=int, default=1)

    idx = selection - 1
    if 0 <= idx < len(tasks):
        return tasks[idx]

    click.echo("Invalid selection.")
    return None


def _resolve_agent(
    cli_agent: str | None,
    task_dir: Path,
    skip_prompts: bool,
) -> str:
    """Resolve which agent to use.

    Priority: CLI flag > prd.json saved > interactive prompt > only installed > default.
    """
    # 1. CLI override
    if cli_agent:
        if shutil.which(cli_agent) is None:
            click.echo(f"Warning: Agent '{cli_agent}' not found in PATH.", err=True)
        return cli_agent

    # 2. Check prd.json for saved agent
    prd_file = task_dir / "prd.json"
    if prd_file.is_file():
        try:
            prd = json.loads(prd_file.read_text())
            saved_agent = str(prd.get("agent", ""))
            if saved_agent and saved_agent in VALID_AGENTS:
                if shutil.which(saved_agent) is not None:
                    click.echo(f"Using saved agent: {saved_agent}")
                    return saved_agent
                else:
                    click.echo(
                        f"Warning: Saved agent '{saved_agent}' not installed.",
                        err=True,
                    )
        except (json.JSONDecodeError, OSError):
            pass

    # 3. Detect installed agents
    installed = _detect_installed_agents()

    if not installed:
        click.echo("Error: No supported AI coding agents found.", err=True)
        click.echo()
        click.echo("Please install one of the following:")
        click.echo("  - Claude Code: npm install -g @anthropic-ai/claude-code")
        click.echo("  - OpenCode: curl -fsSL https://opencode.ai/install | bash")
        raise SystemExit(1)

    if len(installed) == 1:
        click.echo(f"Using only installed agent: {installed[0]}")
        return installed[0]

    # Multiple agents available
    if skip_prompts:
        return installed[0]

    # Interactive prompt
    click.echo()
    click.echo("Available agents:")
    for i, agent in enumerate(installed, 1):
        click.echo(f"  {i}) {agent}")
    click.echo()

    selection: int = click.prompt(
        f"Select agent [1-{len(installed)}]", type=int, default=1
    )
    idx = selection - 1
    if 0 <= idx < len(installed):
        chosen = installed[idx]
        _save_agent_to_prd(prd_file, chosen)
        return chosen

    click.echo(f"Invalid selection. Using {installed[0]}.")
    return installed[0]


def _save_agent_to_prd(prd_file: Path, agent: str) -> None:
    """Save the agent selection to prd.json."""
    if not prd_file.is_file():
        return
    try:
        prd = json.loads(prd_file.read_text())
        prd["agent"] = agent
        prd_file.write_text(json.dumps(prd, indent=2) + "\n")
        click.echo("Agent preference saved to prd.json")
    except (json.JSONDecodeError, OSError):
        pass


# --- Shell Completion ---


def _complete_task_names(
    ctx: Context, param: Parameter, incomplete: str
) -> list[CompletionItem]:
    """Complete task names from registered sessions.

    Used for stop, checkpoint, and attach commands.
    """
    db = SessionDB()
    sessions = db.list_all()
    completions = []
    for s in sessions:
        if s.task_name.startswith(incomplete):
            # Include status as help text
            help_text = f"{s.status} ({s.agent})"
            completions.append(CompletionItem(s.task_name, help=help_text))
    return completions


def _complete_running_tasks(
    ctx: Context, param: Parameter, incomplete: str
) -> list[CompletionItem]:
    """Complete task names for running sessions only.

    Used for stop and checkpoint commands where only running sessions make sense.
    """
    db = SessionDB()
    sessions = db.list_all()
    completions = []
    for s in sessions:
        if s.task_name.startswith(incomplete) and s.status == "running":
            help_text = f"iter {s.iteration}/{s.max_iterations} ({s.agent})"
            completions.append(CompletionItem(s.task_name, help=help_text))
    return completions


def _complete_task_dirs(
    ctx: Context, param: Parameter, incomplete: str
) -> list[CompletionItem]:
    """Complete task directory paths.

    Suggests directories under tasks/ that contain prd.json.
    """
    tasks = _find_active_tasks()
    completions = []
    for task_dir in tasks:
        task_str = str(task_dir)
        if task_str.startswith(incomplete) or incomplete == "":
            # Get description from prd.json for help text
            try:
                prd = json.loads((task_dir / "prd.json").read_text())
                desc = str(prd.get("description", ""))[:40]
                stories = prd.get("userStories", [])
                done = sum(1 for s in stories if s.get("passes", False))
                help_text = f"[{done}/{len(stories)}] {desc}"
            except (json.JSONDecodeError, OSError):
                help_text = ""
            completions.append(CompletionItem(task_str, help=help_text))
    return completions


def _complete_branch_names(
    ctx: Context, param: Parameter, incomplete: str
) -> list[CompletionItem]:
    """Complete git branch names.

    Suggests local and remote branch names, prioritizing common base branches.
    """
    import subprocess

    completions = []

    try:
        # Get all local and remote branches
        result = subprocess.run(
            ["git", "branch", "-a", "--format=%(refname:short)"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        if result.returncode != 0:
            return completions

        branches = set()
        for line in result.stdout.strip().split("\n"):
            if not line:
                continue
            # Strip "origin/" prefix for remote branches to get the base name
            if line.startswith("origin/"):
                branch = line[7:]  # Remove "origin/" prefix
                if branch == "HEAD":
                    continue
            else:
                branch = line
            branches.add(branch)

        # Prioritize common base branch names
        priority_branches = ["main", "master", "develop", "dev"]
        sorted_branches = []
        for pb in priority_branches:
            if pb in branches:
                sorted_branches.append(pb)
                branches.discard(pb)
        sorted_branches.extend(sorted(branches))

        for branch in sorted_branches:
            if branch.startswith(incomplete) or incomplete == "":
                # Add help text for priority branches
                help_text = ""
                if branch in priority_branches:
                    help_text = "common base branch"
                completions.append(CompletionItem(branch, help=help_text))

    except (subprocess.TimeoutExpired, FileNotFoundError, OSError):
        pass

    return completions


def _spawn_in_tmux(
    task_dir: Path,
    max_iterations: int,
    agent: str,
    base_branch: str | None,
    yolo: bool,
    verbose: bool,
    model: str | None,
) -> int:
    """Spawn ralph inside a tmux session.

    Creates a detached tmux session running ralph with the same arguments
    plus RALPH_TMUX_SESSION set. Registers the session in SQLite.
    Returns 0 on success.
    """
    task_name = task_name_from_dir(task_dir)
    session_name = tmux_session_name(task_name)

    # Check for existing session
    db = SessionDB()
    existing = db.get(task_name)
    if tmux_session_exists(session_name):
        if existing and existing.status == "running":
            click.echo(
                f"Error: Session already running for '{task_name}'. "
                f"Use 'ralph stop {task_name}' first.",
                err=True,
            )
            return 1
        # Stale tmux session — kill it
        tmux_kill_session(session_name)
    elif existing and existing.status == "running":
        # DB says running but tmux is gone — stale entry, clean it up
        db.update_status(task_name, "failed")

    # Build command to run inside tmux
    cmd_parts: list[str] = [
        sys.executable,
        "-m",
        "ralph.cli",
        "run",
        str(task_dir),
        "-i",
        str(max_iterations),
        "-a",
        agent,
        "-y",  # Skip prompts inside tmux
    ]
    if base_branch:
        cmd_parts.extend(["--base-branch", base_branch])
    if yolo:
        cmd_parts.append("--yolo")
    if verbose:
        cmd_parts.append("--verbose")
    if model:
        cmd_parts.extend(["--model", model])

    # Create tmux session via libtmux
    cmd_str = shlex.join(cmd_parts)
    project_root = str(task_dir.parent.parent)
    pid = tmux_create_session(
        session_name,
        cmd_str,
        project_root,
        environment={"RALPH_TMUX_SESSION": session_name},
    )

    # Give the inner process a moment to start, then verify it survived
    time.sleep(1.0)

    if not tmux_session_alive(session_name):
        # Capture any crash output from the pane before killing
        if tmux_session_exists(session_name):
            tmux_kill_session(session_name)
        click.echo(
            f"Error: tmux session '{session_name}' died immediately after starting.",
            err=True,
        )
        click.echo(
            "  The inner process likely crashed. Try running directly:",
            err=True,
        )
        click.echo(f"  RALPH_TMUX_SESSION={session_name} {cmd_str}", err=True)
        return 1

    now = datetime.now().isoformat()
    session = SessionInfo(
        task_name=task_name,
        task_dir=str(task_dir),
        pid=pid,
        tmux_session=session_name,
        agent=agent,
        status="running",
        started_at=now,
        updated_at=now,
        iteration=0,
        current_story="",
        max_iterations=max_iterations,
    )
    db.register(session)

    click.echo(f"  Started tmux session: {session_name}")
    click.echo(f"  Attach with: tmux attach -t {session_name}")
    return 0


def _run_opencode_worker(
    task_dir: Path,
    max_iterations: int,
    agent: str,
    base_branch: str | None,
    yolo: bool,
    verbose: bool,
    model: str | None,
) -> int:
    """Run the opencode loop directly (worker process or foreground mode).

    Connects to the systemd-managed opencode server on port 14096, registers
    the session in SQLite, then runs the loop (sending prompts via HTTP API).
    Returns 0 on success.
    """
    task_name = task_name_from_dir(task_dir)

    # Determine project root from git root of task directory
    project_root = _get_git_root(task_dir)
    if project_root is None:
        # Fall back to assuming tasks/<name> structure
        project_root = task_dir.parent.parent
        click.echo(
            f"  Warning: Could not determine git root, using {project_root}",
            err=True,
        )

    # Check for existing session
    db = SessionDB()
    existing = db.get(task_name)
    if existing and existing.status == "running":
        if existing.session_type == "opencode-server":
            from ralph.session import opencode_server_alive

            if opencode_server_alive(existing.server_port):
                click.echo(
                    f"Error: Session already running for '{task_name}'. "
                    f"Use 'ralph stop {task_name}' first.",
                    err=True,
                )
                return 1
        elif tmux_session_exists(tmux_session_name(task_name)):
            click.echo(
                f"Error: Session already running for '{task_name}'. "
                f"Use 'ralph stop {task_name}' first.",
                err=True,
            )
            return 1
        # Stale entry — clean it up
        db.update_status(task_name, "failed")

    # Connect to systemd-managed opencode server
    client = OpencodeClient(project_dir=project_root)

    try:
        client.check_server_running()
    except OpencodeServerNotRunning:
        click.echo(
            f"Error: OpenCode server not running on port {DEFAULT_SERVER_PORT}.\n"
            f"Start it with: systemctl --user start opencode",
            err=True,
        )
        return 1

    click.echo(f"  Connected to OpenCode server at {client.url}")

    # Register in database (no server_pid since systemd manages the server)
    now = datetime.now().isoformat()
    session = SessionInfo(
        task_name=task_name,
        task_dir=str(task_dir),
        pid=os.getpid(),  # loop worker process PID
        tmux_session="",  # Not used for opencode-server
        agent=agent,
        status="running",
        started_at=now,
        updated_at=now,
        iteration=0,
        current_story="",
        max_iterations=max_iterations,
        session_type="opencode-server",
        server_port=client.port,
        server_url=client.url,  # Full URL for attach command
    )
    db.register(session)

    click.echo(f"  Attach with: opencode attach {client.url}")

    # Run the loop directly (in this process) using the opencode client
    config = LoopConfig(
        task_dir=task_dir,
        max_iterations=max_iterations,
        agent=agent,
        agent_override=agent,
        base_branch=base_branch,
        yolo_mode=yolo,
        verbose=verbose,
        model=model,
    )
    # skip_session_register: already registered above with correct session_type
    runner = LoopRunner(config, opencode_server=client, skip_session_register=True)
    rc = 1
    try:
        rc = runner.run()
    finally:
        # Update session status (server continues running via systemd)
        final_status = "completed" if rc == 0 else "stopped"
        db.update_status(task_name, final_status)

    return rc


def _spawn_opencode_background(
    task_dir: Path,
    max_iterations: int,
    agent: str,
    base_branch: str | None,
    yolo: bool,
    verbose: bool,
    model: str | None,
) -> int:
    """Spawn a detached background worker for opencode mode.

    The parent process exits immediately after spawning the worker.
    The worker is immune to terminal close (SIGHUP).
    Returns 0 on success (worker started).
    """
    task_name = task_name_from_dir(task_dir)

    # Check for existing session
    db = SessionDB()
    existing = db.get(task_name)
    if existing and existing.status == "running":
        if existing.session_type == "opencode-server":
            from ralph.session import opencode_server_alive

            if opencode_server_alive(existing.server_port):
                click.echo(
                    f"Error: Session already running for '{task_name}'. "
                    f"Use 'ralph stop {task_name}' first.",
                    err=True,
                )
                return 1
        elif tmux_session_exists(tmux_session_name(task_name)):
            click.echo(
                f"Error: Session already running for '{task_name}'. "
                f"Use 'ralph stop {task_name}' first.",
                err=True,
            )
            return 1
        # Stale entry — clean it up
        db.update_status(task_name, "failed")

    # Build command to run inside the worker
    cmd_parts: list[str] = [
        sys.executable,
        "-m",
        "ralph.cli",
        "run",
        str(task_dir),
        "-i",
        str(max_iterations),
        "-a",
        agent,
        "-y",  # Skip prompts in worker
    ]
    if base_branch:
        cmd_parts.extend(["--base-branch", base_branch])
    if yolo:
        cmd_parts.append("--yolo")
    if verbose:
        cmd_parts.append("--verbose")
    if model:
        cmd_parts.extend(["--model", model])

    # Set up log files for stdout/stderr
    log_dir = Path.home() / ".local" / "state" / "ralph"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_file = log_dir / f"{task_name}-worker.log"

    # Spawn detached worker process
    import subprocess

    env = os.environ.copy()
    env["RALPH_WORKER"] = "1"

    with open(log_file, "w") as log_fh:
        proc = subprocess.Popen(
            cmd_parts,
            stdin=subprocess.DEVNULL,
            stdout=log_fh,
            stderr=subprocess.STDOUT,
            env=env,
            cwd=str(task_dir.parent.parent),  # Project root
            start_new_session=True,  # Detach from terminal (immune to SIGHUP)
        )

    # Give the worker a moment to start and check it survived
    time.sleep(1.0)

    if proc.poll() is not None:
        # Worker died immediately
        click.echo(
            f"Error: Background worker died immediately (exit code: {proc.returncode})",
            err=True,
        )
        click.echo(f"  Check logs: {log_file}", err=True)
        return 1

    # Print success message with helpful commands
    click.echo()
    click.echo(f"Started background loop for '{task_name}'")
    click.echo()
    click.echo("  ralph status              # Check progress")
    click.echo(f"  ralph attach {task_name}  # Watch/interact")
    click.echo(f"  ralph stop {task_name}    # Stop the loop")
    click.echo()
    click.echo(f"  Worker log: {log_file}")
    click.echo()

    return 0


# --- Click CLI ---


@click.group()
@click.version_option(version=__version__, prog_name="ralph")
def cli() -> None:
    """Ralph - Autonomous AI agent loop runner."""
    pass


@cli.command()
@click.argument(
    "task_dir",
    required=False,
    type=click.Path(exists=False),
    shell_complete=_complete_task_dirs,
)
@click.option(
    "-i",
    "--max-iterations",
    type=int,
    default=None,
    help=f"Maximum iterations (default: {DEFAULT_ITERATIONS}).",
)
@click.option(
    "-a",
    "--agent",
    type=click.Choice(list(VALID_AGENTS)),
    default=None,
    help="Agent to use.",
)
@click.option(
    "--base-branch",
    default=None,
    shell_complete=_complete_branch_names,
    help="Base branch to start from.",
)
@click.option(
    "-y",
    "--yes",
    "skip_prompts",
    is_flag=True,
    help="Skip interactive prompts, use defaults.",
)
@click.option("--yolo", is_flag=True, help="Skip agent permission checks.")
@click.option("--verbose", is_flag=True, help="Enable verbose agent output.")
@click.option(
    "--foreground",
    is_flag=True,
    help="Run in foreground (for debugging). Default is background for opencode.",
)
@click.option(
    "-M",
    "--model",
    default=None,
    help="Model to use (e.g., anthropic/claude-opus-4-5). Default: claude-opus-4-5.",
)
def run(
    task_dir: str | None,
    max_iterations: int | None,
    agent: str | None,
    base_branch: str | None,
    skip_prompts: bool,
    yolo: bool,
    verbose: bool,
    foreground: bool,
    model: str | None,
) -> None:
    """Run the agent loop for a task."""
    # --- Resolve task directory ---
    if task_dir:
        # Try smart resolution: full path, relative path, or task name
        resolved_dir = _resolve_task_dir(task_dir)
        if resolved_dir is None:
            # Provide helpful error message
            git_root = _get_git_root()
            if git_root:
                tasks_path = git_root / "tasks" / task_dir
                click.echo(
                    f"Error: Task not found: '{task_dir}'\n"
                    f"  Looked in: {tasks_path}\n"
                    f"  Available tasks:",
                    err=True,
                )
                for t in _find_active_tasks():
                    click.echo(f"    - {t.name}", err=True)
            else:
                click.echo(f"Error: Task directory not found: {task_dir}", err=True)
            raise SystemExit(1)
    elif skip_prompts:
        click.echo("Error: task_dir is required with --yes flag.", err=True)
        raise SystemExit(1)
    else:
        selected = _prompt_task_selection()
        if selected is None:
            raise SystemExit(1)
        resolved_dir = selected.resolve()

    # --- Resolve iterations ---
    if max_iterations is None:
        if skip_prompts:
            max_iterations = DEFAULT_ITERATIONS
        else:
            max_iterations = click.prompt(
                "Max iterations", type=int, default=DEFAULT_ITERATIONS
            )

    assert max_iterations is not None  # Guaranteed by prompt/default above

    # --- Resolve agent ---
    resolved_agent = _resolve_agent(agent, resolved_dir, skip_prompts)

    # --- Check if we're inside tmux or a worker process already ---
    running_in_tmux = os.environ.get("RALPH_TMUX_SESSION", "")
    running_as_worker = os.environ.get("RALPH_WORKER", "")

    if running_in_tmux:
        # We're inside tmux — run the loop directly
        config = LoopConfig(
            task_dir=resolved_dir,
            max_iterations=max_iterations,
            agent=resolved_agent,
            agent_override=agent,
            base_branch=base_branch,
            yolo_mode=yolo,
            verbose=verbose,
            model=model,
        )
        runner = LoopRunner(config)
        raise SystemExit(runner.run())
    elif running_as_worker:
        # We're a background worker — run opencode server mode directly
        rc = _run_opencode_worker(
            task_dir=resolved_dir,
            max_iterations=max_iterations,
            agent=resolved_agent,
            base_branch=base_branch,
            yolo=yolo,
            verbose=verbose,
            model=model,
        )
        raise SystemExit(rc)
    elif resolved_agent == "opencode":
        # OpenCode agent: spawn background worker (or run foreground if requested)
        if foreground:
            rc = _run_opencode_worker(
                task_dir=resolved_dir,
                max_iterations=max_iterations,
                agent=resolved_agent,
                base_branch=base_branch,
                yolo=yolo,
                verbose=verbose,
                model=model,
            )
        else:
            rc = _spawn_opencode_background(
                task_dir=resolved_dir,
                max_iterations=max_iterations,
                agent=resolved_agent,
                base_branch=base_branch,
                yolo=yolo,
                verbose=verbose,
                model=model,
            )
        raise SystemExit(rc)
    else:
        # Claude agent: spawn ourselves in a tmux session
        rc = _spawn_in_tmux(
            task_dir=resolved_dir,
            max_iterations=max_iterations,
            agent=resolved_agent,
            base_branch=base_branch,
            yolo=yolo,
            verbose=verbose,
            model=model,
        )
        raise SystemExit(rc)


@cli.command()
@click.option("--json", "json_output", is_flag=True, help="Output as JSON.")
def status(json_output: bool) -> None:
    """Show status of running sessions."""
    output = get_status(as_json=json_output)
    click.echo(output)


@cli.command()
@click.argument("task", shell_complete=_complete_running_tasks)
def stop(task: str) -> None:
    """Stop a running session."""
    success = stop_session(task)
    if not success:
        raise SystemExit(1)


@cli.command()
@click.argument("task", shell_complete=_complete_running_tasks)
def checkpoint(task: str) -> None:
    """Checkpoint a running session (pause after current iteration)."""
    success = checkpoint_session(task)
    if not success:
        raise SystemExit(1)


@cli.command(name="attach")
@click.argument("task", required=False, shell_complete=_complete_task_names)
@click.option(
    "--session",
    "session_id",
    default=None,
    help="Specific opencode session ID to attach to.",
)
def attach_cmd(task: str | None, session_id: str | None) -> None:
    """Attach to a running session.

    \b
    Usage:
      ralph attach              # Attach to most recently active session
      ralph attach <task>       # Attach to most recent session for <task>
      ralph attach <task> --session <id>  # Attach to specific session
    """
    rc = attach(task, session_id)
    if rc != 0:
        raise SystemExit(rc)


@cli.command()
@click.option(
    "--all", "clean_all", is_flag=True, help="Remove all sessions including running."
)
def clean(clean_all: bool) -> None:
    """Clean up stale session entries from the database.

    By default, removes completed, failed, and stopped sessions.
    Use --all to also remove sessions marked as running (useful if the
    database is out of sync with actual processes).
    """
    db = SessionDB()
    sessions = db.list_all()

    if not sessions:
        click.echo("No sessions in database.")
        return

    removed = 0
    for s in sessions:
        should_remove = False

        if clean_all:
            should_remove = True
        elif s.status in ("completed", "failed", "stopped", "checkpointed"):
            should_remove = True
        elif s.status == "running":
            # Check if it's actually running
            if s.session_type == "opencode-server":
                from ralph.session import opencode_server_alive

                if not opencode_server_alive(s.server_port):
                    should_remove = True
            else:
                if not tmux_session_exists(s.tmux_session):
                    should_remove = True

        if should_remove:
            db.remove(s.task_name)
            click.echo(f"  Removed: {s.task_name} ({s.status})")
            removed += 1

    if removed == 0:
        click.echo("No stale sessions found.")
    else:
        click.echo(f"\nCleaned {removed} session(s).")


@cli.command()
@click.argument("shell", type=click.Choice(["bash", "zsh", "fish"]))
@click.option(
    "--install",
    is_flag=True,
    help="Install completion to the appropriate shell config file.",
)
def completion(shell: str, install: bool) -> None:
    """Generate shell completion script.

    \b
    Usage:
      # Print the completion script
      ralph completion bash

      # Install automatically (appends to shell config)
      ralph completion bash --install

      # Or manually add to your shell config:
      # Bash (~/.bashrc):
        eval "$(ralph completion bash)"

      # Zsh (~/.zshrc):
        eval "$(ralph completion zsh)"

      # Fish (~/.config/fish/completions/ralph.fish):
        ralph completion fish > ~/.config/fish/completions/ralph.fish
    """
    import subprocess

    # Generate the completion script using Click's built-in mechanism
    env_var = "_RALPH_COMPLETE"
    cmd = ["ralph"]
    env = os.environ.copy()
    env[env_var] = f"{shell}_source"

    try:
        result = subprocess.run(
            cmd, env=env, capture_output=True, text=True, check=True
        )
        script = result.stdout
    except subprocess.CalledProcessError as e:
        click.echo(f"Error generating completion script: {e.stderr}", err=True)
        raise SystemExit(1)
    except FileNotFoundError:
        click.echo(
            "Error: 'ralph' command not found. "
            "Make sure it's installed and in your PATH.",
            err=True,
        )
        raise SystemExit(1)

    if not install:
        # Just print the script
        click.echo(script)
        return

    # Install to the appropriate config file
    config_files = {
        "bash": Path.home() / ".bashrc",
        "zsh": Path.home() / ".zshrc",
        "fish": Path.home() / ".config" / "fish" / "completions" / "ralph.fish",
    }
    config_file = config_files[shell]

    if shell == "fish":
        # Fish uses a dedicated completions file
        config_file.parent.mkdir(parents=True, exist_ok=True)
        config_file.write_text(script)
        click.echo(f"Completion installed to {config_file}")
    else:
        # Bash/Zsh: append eval line to config
        eval_cmd = f"_RALPH_COMPLETE={shell}_source ralph"
        eval_line = f'\n# Ralph shell completion\neval "$({eval_cmd})"\n'

        # Check if already installed
        if config_file.exists():
            content = config_file.read_text()
            if "_RALPH_COMPLETE" in content:
                click.echo(f"Completion already installed in {config_file}")
                return

        with open(config_file, "a") as f:
            f.write(eval_line)
        click.echo(f"Completion installed to {config_file}")
        click.echo("Restart your shell or run: source " + str(config_file))


if __name__ == "__main__":
    cli()
