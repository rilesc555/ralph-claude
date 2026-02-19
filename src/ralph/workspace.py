"""Workspace management for ralph using OpenCode worktrees.

Provides isolated git worktrees for ralph loops via OpenCode's HTTP API.
Worktrees are stored at ~/.local/share/opencode/worktree/<project-id>/<name>/.

The workspace feature allows ralph to run in a completely isolated copy of
the codebase, preventing conflicts with the user's working directory.

Usage:
    client = OpencodeClient(project_dir=Path("/path/to/project"))
    manager = WorkspaceManager(client)

    # Create a new workspace
    workspace = manager.create()  # auto-generated name
    workspace = manager.create("my-sandbox")  # named workspace

    # Wait for async initialization
    manager.wait_for_ready(workspace)

    # Reset workspace to main branch
    manager.reset(workspace.directory)

    # Clean up when done
    manager.remove(workspace.directory)
"""

from __future__ import annotations

import logging
import time
from dataclasses import dataclass
from pathlib import Path

from ralph.opencode_server import OpencodeClient, OpencodeServerError


def _get_logger() -> logging.Logger:
    """Get or create the workspace logger."""
    logger = logging.getLogger("ralph.workspace")
    if not logger.handlers:
        log_dir = Path.home() / ".local" / "state" / "ralph"
        log_dir.mkdir(parents=True, exist_ok=True)
        handler = logging.FileHandler(log_dir / "workspace.log")
        handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s"))
        logger.addHandler(handler)
        logger.setLevel(logging.DEBUG)
    return logger


@dataclass
class WorkspaceInfo:
    """Information about an OpenCode worktree workspace."""

    name: str  # Worktree name (e.g., "brave-forest")
    branch: str  # Git branch (e.g., "opencode/brave-forest")
    directory: Path  # Full path to worktree directory


class WorkspaceError(Exception):
    """Raised when workspace operations fail."""


class WorkspaceManager:
    """Manages OpenCode worktrees for ralph sessions.

    Creates and manages isolated git worktrees via the OpenCode HTTP API.
    Each worktree is a complete copy of the repository at a specific point,
    allowing ralph to work without affecting the user's working directory.
    """

    # Default timeout for waiting for worktree initialization
    DEFAULT_READY_TIMEOUT = 60.0  # seconds
    READY_POLL_INTERVAL = 0.5  # seconds between ready checks

    def __init__(self, client: OpencodeClient) -> None:
        """Initialize the workspace manager.

        Args:
            client: OpencodeClient configured for the project.
        """
        self.client = client
        self._log = _get_logger()

    def create(self, name: str | None = None) -> WorkspaceInfo:
        """Create a new worktree workspace.

        Args:
            name: Optional name for the worktree. If not provided, OpenCode
                generates a random adjective-noun name (e.g., "brave-forest").

        Returns:
            WorkspaceInfo with the worktree name, branch, and directory path.

        Raises:
            WorkspaceError: If worktree creation fails.

        Note:
            This method returns immediately. The worktree initialization
            (git reset --hard, startup scripts) runs asynchronously.
            Use wait_for_ready() to block until the worktree is usable.
        """
        self._log.info("Creating workspace (name=%s)", name)
        try:
            result = self.client.create_worktree(name=name)
        except OpencodeServerError as e:
            raise WorkspaceError(f"Failed to create worktree: {e}") from e

        workspace = WorkspaceInfo(
            name=result["name"],
            branch=result["branch"],
            directory=Path(result["directory"]),
        )
        self._log.info(
            "Workspace created: name=%s, branch=%s, directory=%s",
            workspace.name,
            workspace.branch,
            workspace.directory,
        )
        return workspace

    def list_workspaces(self) -> list[Path]:
        """List all worktree directories for the current project.

        Returns:
            List of worktree directory paths.

        Raises:
            WorkspaceError: If listing fails.
        """
        try:
            directories = self.client.list_worktrees()
            return [Path(d) for d in directories]
        except OpencodeServerError as e:
            raise WorkspaceError(f"Failed to list worktrees: {e}") from e

    def reset(self, directory: Path) -> None:
        """Reset a worktree to the default branch (main/master).

        This performs a git reset --hard to the base branch, giving a clean
        slate for a new ralph run without creating a new worktree.

        Args:
            directory: Full path to the worktree directory.

        Raises:
            WorkspaceError: If reset fails.
        """
        self._log.info("Resetting workspace: %s", directory)
        try:
            self.client.reset_worktree(str(directory))
        except OpencodeServerError as e:
            raise WorkspaceError(f"Failed to reset worktree: {e}") from e
        self._log.info("Workspace reset complete: %s", directory)

    def remove(self, directory: Path) -> None:
        """Remove a worktree and delete its branch.

        Cleans up the worktree directory and removes the associated git branch.

        Args:
            directory: Full path to the worktree directory.

        Raises:
            WorkspaceError: If removal fails.
        """
        self._log.info("Removing workspace: %s", directory)
        try:
            self.client.remove_worktree(str(directory))
        except OpencodeServerError as e:
            raise WorkspaceError(f"Failed to remove worktree: {e}") from e
        self._log.info("Workspace removed: %s", directory)

    def wait_for_ready(
        self,
        workspace: WorkspaceInfo,
        timeout: float = DEFAULT_READY_TIMEOUT,
    ) -> bool:
        """Wait for a worktree to be fully initialized.

        The worktree creation API returns immediately, but git operations
        and startup scripts run asynchronously. This method polls until
        the worktree directory exists and contains a .git reference.

        Args:
            workspace: The WorkspaceInfo returned from create().
            timeout: Maximum seconds to wait (default: 60).

        Returns:
            True if the worktree is ready, False if timeout reached.
        """
        self._log.info(
            "Waiting for workspace ready: %s (timeout=%ss)",
            workspace.directory,
            timeout,
        )
        deadline = time.time() + timeout

        while time.time() < deadline:
            if self._is_worktree_ready(workspace.directory):
                self._log.info("Workspace ready: %s", workspace.directory)
                return True
            time.sleep(self.READY_POLL_INTERVAL)

        self._log.warning(
            "Workspace ready timeout after %ss: %s", timeout, workspace.directory
        )
        return False

    def _is_worktree_ready(self, directory: Path) -> bool:
        """Check if a worktree directory is initialized and ready.

        A worktree is considered ready when:
        1. The directory exists
        2. It contains a .git file (worktrees use .git files, not directories)
        3. The .git file points to a valid git directory

        Args:
            directory: Path to check.

        Returns:
            True if the worktree is ready for use.
        """
        if not directory.exists():
            return False

        # Worktrees have a .git file (not directory) that points to the main repo
        git_file = directory / ".git"
        if not git_file.exists():
            return False

        # For worktrees, .git is a file containing:
        # "gitdir: /path/to/main/.git/worktrees/<name>"
        if git_file.is_file():
            try:
                content = git_file.read_text().strip()
                if content.startswith("gitdir:"):
                    return True
            except OSError:
                return False

        # If .git is a directory, it's the main repo (not a worktree)
        # This shouldn't happen but handle it anyway
        return git_file.is_dir()

    def find_by_name(self, name: str) -> WorkspaceInfo | None:
        """Find an existing workspace by name.

        Searches the list of worktrees for one matching the given name.
        Useful for reusing an existing workspace with --workspace <name>.

        Args:
            name: The worktree name to find.

        Returns:
            WorkspaceInfo if found, None otherwise.
        """
        try:
            directories = self.list_workspaces()
        except WorkspaceError:
            return None

        for directory in directories:
            # Worktree directory names match the worktree name
            if directory.name == name:
                # Reconstruct the WorkspaceInfo
                # Branch name follows OpenCode convention: opencode/<name>
                return WorkspaceInfo(
                    name=name,
                    branch=f"opencode/{name}",
                    directory=directory,
                )

        return None


def setup_workspace(
    client: OpencodeClient,
    name: str | None = None,
    reset: bool = False,
    timeout: float = WorkspaceManager.DEFAULT_READY_TIMEOUT,
) -> WorkspaceInfo:
    """Set up a workspace for ralph, creating or reusing as needed.

    This is the main entry point for workspace setup in the CLI.

    Args:
        client: OpencodeClient configured for the project.
        name: Optional workspace name. If provided and exists, reuses it.
            If not provided, creates a new workspace with auto-generated name.
        reset: If True and reusing an existing workspace, reset it to main.
        timeout: Timeout for waiting for workspace initialization.

    Returns:
        WorkspaceInfo for the ready-to-use workspace.

    Raises:
        WorkspaceError: If workspace setup fails.
    """
    manager = WorkspaceManager(client)

    # Try to find existing workspace by name
    workspace: WorkspaceInfo | None = None
    if name:
        workspace = manager.find_by_name(name)
        if workspace:
            if reset:
                manager.reset(workspace.directory)
            # Existing workspace, should be ready already
            return workspace

    # Create new workspace
    workspace = manager.create(name=name)

    # Wait for initialization
    if not manager.wait_for_ready(workspace, timeout=timeout):
        raise WorkspaceError(
            f"Workspace initialization timed out after {timeout}s. "
            f"Check OpenCode server logs for details."
        )

    return workspace


def cleanup_workspace(
    client: OpencodeClient,
    directory: Path,
    keep: bool = False,
) -> None:
    """Clean up a workspace after ralph completes.

    Args:
        client: OpencodeClient configured for the project.
        directory: The workspace directory to clean up.
        keep: If True, skip cleanup (for debugging).
    """
    if keep:
        return

    manager = WorkspaceManager(client)
    try:
        manager.remove(directory)
    except WorkspaceError as e:
        # Log warning but don't fail - workspace cleanup is best effort
        logger = _get_logger()
        logger.warning("Failed to clean up workspace %s: %s", directory, e)
