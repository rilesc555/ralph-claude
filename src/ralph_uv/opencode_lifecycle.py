"""OpenCode serve lifecycle management for the Ralph daemon.

Manages opencode serve instances for each active loop:
- Port allocation starting at 4096
- Process startup with health check waiting
- Environment setup (OPENCODE_PERMISSION for yolo mode, API keys)
- Proper shutdown sequence (abort, SIGTERM, SIGKILL)
- PID and port tracking

This module differs from opencode_server.py in that it's designed
specifically for daemon loop management with:
- Configurable port range (starting at 4096 for daemon)
- Environment injection from daemon config
- Integration with LoopInfo tracking
- Structured async interface for daemon operations
"""

from __future__ import annotations

import asyncio
import logging
import os
import signal
import socket
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import aiohttp


# Default port range for daemon (different from opencode_server.py)
DEFAULT_PORT_START = 4096
DEFAULT_PORT_END = 5096

# Timeouts
HEALTH_CHECK_TIMEOUT = 30.0  # seconds to wait for server to become healthy
HEALTH_CHECK_INTERVAL = 0.5  # seconds between health check attempts
HTTP_TIMEOUT = 30.0  # seconds for HTTP requests
STOP_TIMEOUT = 10.0  # seconds to wait for graceful shutdown
KILL_TIMEOUT = 5.0  # seconds to wait after SIGKILL


class OpenCodeLifecycleError(Exception):
    """Raised when an opencode lifecycle operation fails."""

    pass


class ServerStartError(OpenCodeLifecycleError):
    """Server failed to start."""

    pass


class ServerHealthCheckError(OpenCodeLifecycleError):
    """Server failed health check."""

    pass


class SessionError(OpenCodeLifecycleError):
    """Session operation failed."""

    pass


@dataclass
class OpenCodeInstanceConfig:
    """Configuration for an opencode serve instance."""

    working_dir: Path
    port: int | None = None
    env_vars: dict[str, str] = field(default_factory=dict)
    yolo_mode: bool = True
    model: str = ""
    verbose: bool = False


@dataclass
class OpenCodeInstance:
    """Represents a running opencode serve instance.

    Tracks the process, port, and provides methods for interaction.
    """

    loop_id: str
    port: int
    pid: int | None
    working_dir: Path
    session_id: str | None = None
    _process: asyncio.subprocess.Process | None = field(
        default=None, repr=False, compare=False
    )
    _log: logging.Logger = field(
        default_factory=lambda: logging.getLogger("ralphd.opencode"),
        repr=False,
        compare=False,
    )

    @property
    def base_url(self) -> str:
        """Base URL for the opencode serve HTTP API."""
        return f"http://127.0.0.1:{self.port}"

    @property
    def is_running(self) -> bool:
        """Check if the server process is still running."""
        if self._process is None:
            return False
        return self._process.returncode is None


class OpenCodeManager:
    """Manages opencode serve instances for daemon loops.

    Handles:
    - Port allocation from a configurable range
    - Process lifecycle (start, health check, stop)
    - Environment setup for yolo mode and API keys
    - Tracking of active instances
    """

    def __init__(
        self,
        env_vars: dict[str, str] | None = None,
        port_start: int = DEFAULT_PORT_START,
        port_end: int = DEFAULT_PORT_END,
    ) -> None:
        """Initialize the opencode manager.

        Args:
            env_vars: Environment variables to inject (API keys, etc.)
            port_start: Start of port range for allocation
            port_end: End of port range for allocation
        """
        self._log = logging.getLogger("ralphd.opencode")
        self._env_vars = env_vars or {}
        self._port_start = port_start
        self._port_end = port_end
        self._instances: dict[str, OpenCodeInstance] = {}
        self._used_ports: set[int] = set()
        self._lock = asyncio.Lock()

    @property
    def active_count(self) -> int:
        """Number of active opencode instances."""
        return len(self._instances)

    def get_instance(self, loop_id: str) -> OpenCodeInstance | None:
        """Get an instance by loop ID."""
        return self._instances.get(loop_id)

    async def start_instance(
        self, loop_id: str, config: OpenCodeInstanceConfig
    ) -> OpenCodeInstance:
        """Start a new opencode serve instance for a loop.

        Args:
            loop_id: Unique loop identifier
            config: Instance configuration

        Returns:
            OpenCodeInstance with port and PID

        Raises:
            ServerStartError: If the server fails to start
            ServerHealthCheckError: If health check fails
        """
        async with self._lock:
            if loop_id in self._instances:
                raise ServerStartError(f"Instance already exists for loop: {loop_id}")

            # Allocate port
            port = config.port or self._allocate_port()
            if port is None:
                raise ServerStartError("No available ports in range")

            self._log.info(
                "Starting opencode serve for loop %s: port=%d, cwd=%s",
                loop_id,
                port,
                config.working_dir,
            )

            # Build command
            cmd = self._build_command(port, config)

            # Build environment
            env = self._build_env(config)

            # Start process
            try:
                process = await asyncio.create_subprocess_exec(
                    *cmd,
                    stdin=asyncio.subprocess.DEVNULL,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                    env=env,
                    cwd=str(config.working_dir),
                    start_new_session=True,  # New process group for clean kill
                )
            except OSError as e:
                raise ServerStartError(f"Failed to start opencode serve: {e}") from e

            self._log.info(
                "opencode serve started for loop %s: pid=%d, port=%d",
                loop_id,
                process.pid,
                port,
            )

            # Create instance
            instance = OpenCodeInstance(
                loop_id=loop_id,
                port=port,
                pid=process.pid,
                working_dir=config.working_dir,
                _process=process,
            )

            # Track instance and port
            self._instances[loop_id] = instance
            self._used_ports.add(port)

        # Wait for health check (outside lock)
        try:
            await self._wait_for_health(instance)
        except ServerHealthCheckError:
            # Clean up on failure
            await self.stop_instance(loop_id)
            raise

        self._log.info(
            "opencode serve ready for loop %s: url=%s",
            loop_id,
            instance.base_url,
        )
        return instance

    async def stop_instance(
        self,
        loop_id: str,
        session_id: str | None = None,
        graceful: bool = True,
    ) -> None:
        """Stop an opencode serve instance.

        Performs orderly shutdown:
        1. POST /session/:id/abort (if session_id provided)
        2. SIGTERM to process group
        3. Wait for graceful shutdown
        4. SIGKILL if needed

        Args:
            loop_id: Loop identifier
            session_id: Optional session to abort first
            graceful: If True, attempt abort and SIGTERM before SIGKILL
        """
        async with self._lock:
            instance = self._instances.get(loop_id)
            if instance is None:
                self._log.debug("No instance found for loop %s", loop_id)
                return

            # Use provided session_id or instance's tracked session
            session = session_id or instance.session_id

            self._log.info(
                "Stopping opencode serve for loop %s (pid=%s, session=%s)",
                loop_id,
                instance.pid,
                session,
            )

            # 1. Abort session if we have one
            if graceful and session and instance.is_running:
                await self._abort_session_quiet(instance, session)

            # 2. Send SIGTERM
            if instance._process and instance.is_running:
                try:
                    # Kill process group
                    if instance.pid:
                        os.killpg(os.getpgid(instance.pid), signal.SIGTERM)
                except (OSError, ProcessLookupError):
                    pass

                # 3. Wait for graceful shutdown
                try:
                    await asyncio.wait_for(
                        instance._process.wait(),
                        timeout=STOP_TIMEOUT,
                    )
                    self._log.info(
                        "opencode serve for loop %s stopped cleanly", loop_id
                    )
                except asyncio.TimeoutError:
                    # 4. Force kill
                    self._log.warning(
                        "SIGTERM timeout for loop %s, sending SIGKILL", loop_id
                    )
                    try:
                        if instance.pid:
                            os.killpg(os.getpgid(instance.pid), signal.SIGKILL)
                        await asyncio.wait_for(
                            instance._process.wait(),
                            timeout=KILL_TIMEOUT,
                        )
                    except (OSError, ProcessLookupError, asyncio.TimeoutError):
                        pass

            # Remove from tracking
            self._used_ports.discard(instance.port)
            del self._instances[loop_id]
            self._log.info("Instance removed for loop %s", loop_id)

    async def stop_all(self) -> None:
        """Stop all active instances."""
        loop_ids = list(self._instances.keys())
        self._log.info("Stopping all %d opencode instances", len(loop_ids))

        for loop_id in loop_ids:
            try:
                await self.stop_instance(loop_id)
            except Exception as e:
                self._log.error("Error stopping instance %s: %s", loop_id, e)

    async def create_session(self, loop_id: str) -> str:
        """Create a new session on an opencode instance.

        Args:
            loop_id: Loop identifier

        Returns:
            Session ID

        Raises:
            SessionError: If session creation fails
        """
        instance = self._instances.get(loop_id)
        if instance is None:
            raise SessionError(f"No instance found for loop: {loop_id}")

        if not instance.is_running:
            raise SessionError(f"Instance not running for loop: {loop_id}")

        url = f"{instance.base_url}/session"
        self._log.info("Creating session for loop %s: POST %s", loop_id, url)

        try:
            async with aiohttp.ClientSession() as session:
                async with session.post(
                    url,
                    json={},
                    timeout=aiohttp.ClientTimeout(total=HTTP_TIMEOUT),
                ) as resp:
                    if resp.status != 200:
                        text = await resp.text()
                        raise SessionError(
                            f"Failed to create session: HTTP {resp.status}: {text}"
                        )
                    data = await resp.json()
                    session_id_val = data.get("id", "")
                    if not session_id_val:
                        raise SessionError(f"No session ID in response: {data}")
                    session_id = str(session_id_val)

                    # Track session on instance
                    instance.session_id = session_id
                    self._log.info(
                        "Session created for loop %s: %s", loop_id, session_id
                    )
                    return session_id

        except aiohttp.ClientError as e:
            raise SessionError(f"HTTP error creating session: {e}") from e

    async def abort_session(self, loop_id: str, session_id: str) -> bool:
        """Abort a session on an opencode instance.

        Args:
            loop_id: Loop identifier
            session_id: Session to abort

        Returns:
            True if successful
        """
        instance = self._instances.get(loop_id)
        if instance is None:
            self._log.warning("No instance found for loop %s", loop_id)
            return False

        return await self._abort_session_quiet(instance, session_id)

    # --- Private Methods ---

    def _allocate_port(self) -> int | None:
        """Allocate the next available port."""
        for port in range(self._port_start, self._port_end):
            if port in self._used_ports:
                continue
            if self._is_port_available(port):
                return port
        return None

    @staticmethod
    def _is_port_available(port: int) -> bool:
        """Check if a port is available for binding."""
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                s.bind(("127.0.0.1", port))
                return True
        except OSError:
            return False

    def _build_command(self, port: int, config: OpenCodeInstanceConfig) -> list[str]:
        """Build the opencode serve command."""
        cmd = [
            "opencode",
            "serve",
            "--port",
            str(port),
            "--hostname",
            "127.0.0.1",
        ]

        if config.model:
            cmd.extend(["--model", config.model])

        if config.verbose:
            cmd.extend(["--log-level", "DEBUG", "--print-logs"])

        return cmd

    def _build_env(self, config: OpenCodeInstanceConfig) -> dict[str, str]:
        """Build environment for the opencode serve process."""
        env = os.environ.copy()

        # Inject daemon's env vars (API keys, PATH extensions, etc.)
        env.update(self._env_vars)

        # Set yolo mode permission
        if config.yolo_mode:
            env["OPENCODE_PERMISSION"] = "allow"

        # Inject config-specific env vars
        env.update(config.env_vars)

        return env

    async def _wait_for_health(
        self,
        instance: OpenCodeInstance,
        timeout: float = HEALTH_CHECK_TIMEOUT,
    ) -> None:
        """Wait for an instance to pass health check.

        Raises ServerHealthCheckError if timeout exceeded.
        """
        url = f"{instance.base_url}/global/health"
        deadline = asyncio.get_event_loop().time() + timeout

        self._log.debug(
            "Waiting for health check on loop %s (timeout=%.1fs)",
            instance.loop_id,
            timeout,
        )

        while asyncio.get_event_loop().time() < deadline:
            # Check if process died
            if not instance.is_running:
                stderr = ""
                if instance._process and instance._process.stderr:
                    try:
                        stderr_bytes = await asyncio.wait_for(
                            instance._process.stderr.read(),
                            timeout=1.0,
                        )
                        stderr = stderr_bytes.decode(errors="replace")[:500]
                    except asyncio.TimeoutError:
                        pass
                raise ServerHealthCheckError(
                    f"opencode serve died during startup for loop {instance.loop_id}. "
                    f"Exit code: {instance._process.returncode if instance._process else '?'}. "
                    f"Stderr: {stderr}"
                )

            # Try health check
            if await self._health_check(url):
                return

            await asyncio.sleep(HEALTH_CHECK_INTERVAL)

        raise ServerHealthCheckError(
            f"Health check timeout after {timeout}s for loop {instance.loop_id}. "
            f"Server at {instance.base_url} not responding."
        )

    @staticmethod
    async def _health_check(url: str) -> bool:
        """Perform a single health check. Returns True if healthy."""
        try:
            async with aiohttp.ClientSession() as session:
                async with session.get(
                    url,
                    timeout=aiohttp.ClientTimeout(total=2.0),
                ) as resp:
                    return resp.status == 200
        except (aiohttp.ClientError, asyncio.TimeoutError):
            return False

    async def _abort_session_quiet(
        self, instance: OpenCodeInstance, session_id: str
    ) -> bool:
        """Abort a session without raising exceptions."""
        if not instance.is_running:
            return False

        url = f"{instance.base_url}/session/{session_id}/abort"
        self._log.debug("Aborting session %s on loop %s", session_id, instance.loop_id)

        try:
            async with aiohttp.ClientSession() as session:
                async with session.post(
                    url,
                    json={},
                    timeout=aiohttp.ClientTimeout(total=5.0),
                ) as resp:
                    success = resp.status == 200
                    if success:
                        self._log.info(
                            "Session %s aborted on loop %s",
                            session_id,
                            instance.loop_id,
                        )
                    return success
        except (aiohttp.ClientError, asyncio.TimeoutError) as e:
            self._log.warning(
                "Failed to abort session %s on loop %s: %s",
                session_id,
                instance.loop_id,
                e,
            )
            return False
