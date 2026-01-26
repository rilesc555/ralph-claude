"""OpenZiti integration for the Ralph daemon.

This module provides:
- Ziti identity loading and context management
- Control service binding for receiving RPC requests
- Connection handling for concurrent clients
- Graceful teardown on shutdown
"""

from __future__ import annotations

import asyncio
import logging
import socket
from pathlib import Path
from typing import Any, Protocol, runtime_checkable

# Try to import openziti, track availability
try:
    import openziti  # type: ignore[import-untyped]

    ZITI_AVAILABLE = True
except ImportError:
    openziti = None
    ZITI_AVAILABLE = False


@runtime_checkable
class ConnectionHandler(Protocol):
    """Protocol for handling incoming connections."""

    async def handle_connection(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        """Handle an incoming connection.

        Args:
            reader: Stream reader for receiving data
            writer: Stream writer for sending data
        """
        ...


class ZitiService:
    """Manages a Ziti service binding.

    Handles:
    - Loading Ziti identity
    - Binding to a service
    - Accepting multiple concurrent connections
    - Graceful shutdown
    """

    def __init__(
        self,
        identity_path: Path,
        service_name: str,
        handler: ConnectionHandler,
        hostname: str,
    ) -> None:
        """Initialize the Ziti service.

        Args:
            identity_path: Path to Ziti identity JSON file
            service_name: Name of the Ziti service to bind to
            handler: Connection handler for incoming connections
            hostname: Hostname for logging/identification
        """
        self.identity_path = identity_path
        self.service_name = service_name
        self.handler = handler
        self.hostname = hostname

        self._log = logging.getLogger("ralphd.ziti")
        self._context: Any = None  # openziti.context.ZitiContext when loaded
        self._server_socket: socket.socket | None = None
        self._accept_task: asyncio.Task[None] | None = None
        self._active_connections: set[asyncio.Task[None]] = set()
        self._shutdown_event = asyncio.Event()
        self._bound = False

    @property
    def is_bound(self) -> bool:
        """Return True if the service is currently bound."""
        return self._bound

    def load_identity(self) -> bool:
        """Load the Ziti identity from file.

        Returns:
            True if identity loaded successfully, False otherwise
        """
        if not ZITI_AVAILABLE or openziti is None:
            self._log.error("openziti package not installed")
            return False

        if not self.identity_path.is_file():
            self._log.error("Ziti identity file not found: %s", self.identity_path)
            return False

        try:
            self._log.info("Loading Ziti identity from %s", self.identity_path)
            self._context, err = openziti.load(str(self.identity_path))

            if err != 0:
                self._log.error("Failed to load Ziti identity: error code %d", err)
                return False

            self._log.info("Ziti identity loaded successfully")
            return True

        except Exception as e:
            self._log.exception("Exception loading Ziti identity: %s", e)
            return False

    def bind(self) -> bool:
        """Bind to the Ziti service.

        Returns:
            True if bound successfully, False otherwise
        """
        if not ZITI_AVAILABLE:
            self._log.error("openziti package not installed")
            return False

        if self._context is None:
            self._log.error("No Ziti context loaded - call load_identity() first")
            return False

        try:
            self._log.info("Binding to Ziti service: %s", self.service_name)
            self._server_socket = self._context.bind(self.service_name)
            if self._server_socket is not None:
                self._server_socket.listen(5)
            self._bound = True
            self._log.info(
                "Successfully bound to service %s on %s",
                self.service_name,
                self.hostname,
            )
            return True

        except Exception as e:
            self._log.exception("Failed to bind to Ziti service: %s", e)
            return False

    async def start_accepting(self) -> None:
        """Start accepting connections in the background.

        This runs in a loop until shutdown is requested.
        """
        if not self._bound or self._server_socket is None:
            self._log.error("Cannot accept connections - service not bound")
            return

        self._log.info("Starting to accept connections on %s", self.service_name)

        loop = asyncio.get_running_loop()

        while not self._shutdown_event.is_set():
            try:
                # Use run_in_executor since accept() is blocking
                conn, peer = await loop.run_in_executor(None, self._accept_connection)

                if conn is None:
                    # Socket was closed or error occurred, check if we should continue
                    if self._shutdown_event.is_set():
                        break
                    continue

                self._log.info("Accepted connection from peer: %s", peer)

                # Create a task to handle this connection
                task = asyncio.create_task(self._handle_connection_wrapper(conn, peer))
                self._active_connections.add(task)
                task.add_done_callback(self._active_connections.discard)

            except Exception as e:
                if self._shutdown_event.is_set():
                    break
                self._log.exception("Error accepting connection: %s", e)
                # Small delay to prevent tight loop on repeated errors
                await asyncio.sleep(0.1)

        self._log.info("Stopped accepting connections")

    def _accept_connection(self) -> tuple[socket.socket | None, Any]:
        """Accept a connection (blocking call for run_in_executor).

        Returns:
            Tuple of (connection socket, peer info) or (None, None) on error
        """
        if self._server_socket is None:
            return None, None

        try:
            # Set a timeout so we can check shutdown periodically
            self._server_socket.settimeout(1.0)
            return self._server_socket.accept()
        except socket.timeout:
            return None, None
        except OSError as e:
            # Socket was likely closed
            if not self._shutdown_event.is_set():
                self._log.debug("Accept error (socket may be closed): %s", e)
            return None, None

    async def _handle_connection_wrapper(self, conn: socket.socket, peer: Any) -> None:
        """Wrap a socket connection in asyncio streams and handle it.

        Args:
            conn: The connected socket
            peer: Peer identification info
        """
        try:
            # Wrap the socket in asyncio streams
            loop = asyncio.get_running_loop()
            reader = asyncio.StreamReader()
            protocol = asyncio.StreamReaderProtocol(reader)

            # Create transport from the socket
            transport, _ = await loop.create_connection(lambda: protocol, sock=conn)
            writer = asyncio.StreamWriter(transport, protocol, reader, loop)

            # Call the handler
            await self.handler.handle_connection(reader, writer)

        except Exception as e:
            self._log.exception("Error handling connection from %s: %s", peer, e)

        finally:
            try:
                conn.close()
            except Exception:
                pass
            self._log.debug("Connection closed for peer: %s", peer)

    async def shutdown(self) -> None:
        """Gracefully shutdown the Ziti service.

        This will:
        1. Stop accepting new connections
        2. Wait for active connections to complete (with timeout)
        3. Close the server socket
        4. Clean up the Ziti context
        """
        self._log.info("Shutting down Ziti service: %s", self.service_name)

        # Signal shutdown
        self._shutdown_event.set()

        # Cancel the accept task if running
        if self._accept_task is not None and not self._accept_task.done():
            self._accept_task.cancel()
            try:
                await self._accept_task
            except asyncio.CancelledError:
                pass

        # Wait for active connections to complete (with timeout)
        if self._active_connections:
            self._log.info(
                "Waiting for %d active connection(s) to complete...",
                len(self._active_connections),
            )
            done, pending = await asyncio.wait(self._active_connections, timeout=5.0)
            if pending:
                self._log.warning("Forcefully closing %d connection(s)", len(pending))
                for task in pending:
                    task.cancel()

        # Close the server socket
        if self._server_socket is not None:
            try:
                self._server_socket.close()
            except Exception as e:
                self._log.debug("Error closing server socket: %s", e)
            self._server_socket = None

        self._bound = False
        self._log.info("Ziti service shutdown complete")


class ZitiControlService:
    """Control service for the Ralph daemon.

    Manages the main control service that clients connect to for
    starting/stopping loops and querying status.
    """

    def __init__(
        self,
        identity_path: Path,
        hostname: str,
        handler: ConnectionHandler,
    ) -> None:
        """Initialize the control service.

        Args:
            identity_path: Path to Ziti identity JSON file
            hostname: Hostname used for service naming
            handler: Handler for incoming RPC connections
        """
        self.identity_path = identity_path
        self.hostname = hostname
        self.handler = handler

        self._log = logging.getLogger("ralphd.ziti.control")
        self._service: ZitiService | None = None

    @property
    def service_name(self) -> str:
        """Return the control service name."""
        return f"ralph-control-{self.hostname}"

    @property
    def is_bound(self) -> bool:
        """Return True if the control service is bound."""
        return self._service is not None and self._service.is_bound

    async def start(self) -> bool:
        """Start the control service.

        Returns:
            True if started successfully, False otherwise
        """
        if not ZITI_AVAILABLE:
            self._log.error(
                "Cannot start control service: openziti package not installed"
            )
            return False

        self._log.info("Starting control service: %s", self.service_name)

        # Create the Ziti service
        self._service = ZitiService(
            identity_path=self.identity_path,
            service_name=self.service_name,
            handler=self.handler,
            hostname=self.hostname,
        )

        # Load identity
        if not self._service.load_identity():
            self._log.error("Failed to load Ziti identity")
            return False

        # Bind to service
        if not self._service.bind():
            self._log.error("Failed to bind to control service")
            return False

        # Start accepting connections in the background
        self._service._accept_task = asyncio.create_task(
            self._service.start_accepting()
        )

        self._log.info("Control service started successfully")
        return True

    async def shutdown(self) -> None:
        """Shutdown the control service."""
        if self._service is not None:
            await self._service.shutdown()
            self._service = None


class ZitiLoopService:
    """Per-loop Ziti service that proxies connections to the opencode serve port.

    Each active loop gets its own Ziti service named ralph-loop-{task}-{uuid}.
    This allows clients to connect directly to a running loop through the Ziti overlay.

    The service proxies TCP connections to the local opencode serve HTTP port,
    allowing clients to use `opencode attach http://{ziti-intercept}:{port}`.
    """

    def __init__(
        self,
        identity_path: Path,
        loop_id: str,
        task_name: str,
        target_port: int,
        hostname: str,
    ) -> None:
        """Initialize the loop service.

        Args:
            identity_path: Path to Ziti identity JSON file
            loop_id: Unique loop identifier (e.g., "loop-abc123")
            task_name: Task name for service naming
            target_port: Local port to proxy to (opencode serve port)
            hostname: Hostname for logging/identification
        """
        self.identity_path = identity_path
        self.loop_id = loop_id
        self.task_name = task_name
        self.target_port = target_port
        self.hostname = hostname

        self._log = logging.getLogger("ralphd.ziti.loop")
        self._context: Any = None
        self._server_socket: socket.socket | None = None
        self._accept_task: asyncio.Task[None] | None = None
        self._active_connections: set[asyncio.Task[None]] = set()
        self._shutdown_event = asyncio.Event()
        self._bound = False

    @property
    def service_name(self) -> str:
        """Return the Ziti service name for this loop."""
        # Use task name and loop ID suffix for uniqueness
        # e.g., ralph-loop-remote-daemon-abc123
        short_id = self.loop_id.replace("loop-", "")
        return f"ralph-loop-{self.task_name}-{short_id}"

    @property
    def is_bound(self) -> bool:
        """Return True if the service is currently bound."""
        return self._bound

    def load_identity(self) -> bool:
        """Load the Ziti identity from file.

        Returns:
            True if identity loaded successfully, False otherwise
        """
        if not ZITI_AVAILABLE or openziti is None:
            self._log.error("openziti package not installed")
            return False

        if not self.identity_path.is_file():
            self._log.error("Ziti identity file not found: %s", self.identity_path)
            return False

        try:
            self._log.debug("Loading Ziti identity from %s", self.identity_path)
            self._context, err = openziti.load(str(self.identity_path))

            if err != 0:
                self._log.error("Failed to load Ziti identity: error code %d", err)
                return False

            self._log.debug("Ziti identity loaded for loop service")
            return True

        except Exception as e:
            self._log.exception("Exception loading Ziti identity: %s", e)
            return False

    def bind(self) -> bool:
        """Bind to the Ziti service.

        Returns:
            True if bound successfully, False otherwise
        """
        if not ZITI_AVAILABLE:
            self._log.error("openziti package not installed")
            return False

        if self._context is None:
            self._log.error("No Ziti context loaded - call load_identity() first")
            return False

        try:
            self._log.info("Binding to Ziti service: %s", self.service_name)
            self._server_socket = self._context.bind(self.service_name)
            if self._server_socket is not None:
                self._server_socket.listen(5)
            self._bound = True
            self._log.info(
                "Loop %s: Ziti service bound - %s -> 127.0.0.1:%d",
                self.loop_id,
                self.service_name,
                self.target_port,
            )
            return True

        except Exception as e:
            self._log.exception("Failed to bind to Ziti service: %s", e)
            return False

    async def start_accepting(self) -> None:
        """Start accepting connections and proxying them to the local port.

        This runs in a loop until shutdown is requested.
        """
        if not self._bound or self._server_socket is None:
            self._log.error("Cannot accept connections - service not bound")
            return

        self._log.info(
            "Loop %s: starting to accept connections on %s",
            self.loop_id,
            self.service_name,
        )

        loop = asyncio.get_running_loop()

        while not self._shutdown_event.is_set():
            try:
                # Use run_in_executor since accept() is blocking
                conn, peer = await loop.run_in_executor(None, self._accept_connection)

                if conn is None:
                    if self._shutdown_event.is_set():
                        break
                    continue

                self._log.info(
                    "Loop %s: accepted connection from %s, proxying to port %d",
                    self.loop_id,
                    peer,
                    self.target_port,
                )

                # Create a task to proxy this connection
                task = asyncio.create_task(
                    self._proxy_connection(conn, peer),
                    name=f"proxy-{self.loop_id}-{id(conn)}",
                )
                self._active_connections.add(task)
                task.add_done_callback(self._active_connections.discard)

            except Exception as e:
                if self._shutdown_event.is_set():
                    break
                self._log.exception(
                    "Loop %s: error accepting connection: %s",
                    self.loop_id,
                    e,
                )
                await asyncio.sleep(0.1)

        self._log.info("Loop %s: stopped accepting connections", self.loop_id)

    def _accept_connection(self) -> tuple[socket.socket | None, Any]:
        """Accept a connection (blocking call for run_in_executor).

        Returns:
            Tuple of (connection socket, peer info) or (None, None) on error
        """
        if self._server_socket is None:
            return None, None

        try:
            self._server_socket.settimeout(1.0)
            return self._server_socket.accept()
        except socket.timeout:
            return None, None
        except OSError as e:
            if not self._shutdown_event.is_set():
                self._log.debug(
                    "Loop %s: accept error (socket may be closed): %s",
                    self.loop_id,
                    e,
                )
            return None, None

    async def _proxy_connection(self, ziti_conn: socket.socket, peer: Any) -> None:
        """Proxy data between a Ziti connection and the local opencode serve port.

        Args:
            ziti_conn: The connected Ziti socket from the client
            peer: Peer identification info
        """
        local_conn: socket.socket | None = None

        try:
            # Connect to local opencode serve port
            local_conn = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            local_conn.setblocking(False)

            loop = asyncio.get_running_loop()
            await loop.sock_connect(local_conn, ("127.0.0.1", self.target_port))

            self._log.debug(
                "Loop %s: connected to local port %d for peer %s",
                self.loop_id,
                self.target_port,
                peer,
            )

            # Set both sockets to non-blocking for asyncio
            ziti_conn.setblocking(False)

            # Create bidirectional proxy tasks
            ziti_to_local = asyncio.create_task(
                self._copy_data(ziti_conn, local_conn, "ziti->local"),
                name=f"proxy-ziti-local-{id(ziti_conn)}",
            )
            local_to_ziti = asyncio.create_task(
                self._copy_data(local_conn, ziti_conn, "local->ziti"),
                name=f"proxy-local-ziti-{id(local_conn)}",
            )

            # Wait for either direction to complete
            done, pending = await asyncio.wait(
                [ziti_to_local, local_to_ziti],
                return_when=asyncio.FIRST_COMPLETED,
            )

            # Cancel the other direction
            for task in pending:
                task.cancel()
                try:
                    await task
                except asyncio.CancelledError:
                    pass

            self._log.debug(
                "Loop %s: proxy connection closed for peer %s",
                self.loop_id,
                peer,
            )

        except ConnectionRefusedError:
            self._log.warning(
                "Loop %s: connection refused to local port %d (opencode serve may be down)",
                self.loop_id,
                self.target_port,
            )
        except Exception as e:
            self._log.exception(
                "Loop %s: error proxying connection from %s: %s",
                self.loop_id,
                peer,
                e,
            )

        finally:
            # Close both sockets
            try:
                ziti_conn.close()
            except Exception:
                pass

            if local_conn is not None:
                try:
                    local_conn.close()
                except Exception:
                    pass

    async def _copy_data(
        self,
        source: socket.socket,
        dest: socket.socket,
        direction: str,
    ) -> None:
        """Copy data from source to destination socket.

        Args:
            source: Source socket to read from
            dest: Destination socket to write to
            direction: Human-readable direction for logging
        """
        loop = asyncio.get_running_loop()
        buffer_size = 16384

        try:
            while True:
                try:
                    data = await loop.sock_recv(source, buffer_size)
                except (OSError, ConnectionResetError):
                    break

                if not data:
                    break

                try:
                    await loop.sock_sendall(dest, data)
                except (OSError, ConnectionResetError, BrokenPipeError):
                    break

        except asyncio.CancelledError:
            pass
        except Exception as e:
            self._log.debug(
                "Loop %s: error in %s: %s",
                self.loop_id,
                direction,
                e,
            )

    async def shutdown(self) -> None:
        """Gracefully shutdown the loop service.

        This will:
        1. Stop accepting new connections
        2. Wait for active proxy connections to complete (with timeout)
        3. Close the server socket
        """
        self._log.info(
            "Loop %s: shutting down Ziti service %s",
            self.loop_id,
            self.service_name,
        )

        # Signal shutdown
        self._shutdown_event.set()

        # Cancel the accept task if running
        if self._accept_task is not None and not self._accept_task.done():
            self._accept_task.cancel()
            try:
                await self._accept_task
            except asyncio.CancelledError:
                pass

        # Wait for active connections to complete (with timeout)
        if self._active_connections:
            self._log.info(
                "Loop %s: waiting for %d active connection(s) to complete...",
                self.loop_id,
                len(self._active_connections),
            )
            done, pending = await asyncio.wait(self._active_connections, timeout=5.0)
            if pending:
                self._log.warning(
                    "Loop %s: forcefully closing %d connection(s)",
                    self.loop_id,
                    len(pending),
                )
                for task in pending:
                    task.cancel()

        # Close the server socket
        if self._server_socket is not None:
            try:
                self._server_socket.close()
            except Exception as e:
                self._log.debug(
                    "Loop %s: error closing server socket: %s",
                    self.loop_id,
                    e,
                )
            self._server_socket = None

        self._bound = False
        self._log.info(
            "Loop %s: Ziti service deregistered - %s",
            self.loop_id,
            self.service_name,
        )


class ZitiLoopServiceManager:
    """Manages per-loop Ziti services for the daemon.

    Creates and tracks ZitiLoopService instances for each active loop,
    handling registration and deregistration.
    """

    def __init__(self, identity_path: Path, hostname: str) -> None:
        """Initialize the loop service manager.

        Args:
            identity_path: Path to Ziti identity JSON file
            hostname: Hostname for service naming
        """
        self.identity_path = identity_path
        self.hostname = hostname
        self._log = logging.getLogger("ralphd.ziti.loop-manager")
        self._services: dict[str, ZitiLoopService] = {}
        self._lock = asyncio.Lock()

    @property
    def active_service_count(self) -> int:
        """Return the number of active loop services."""
        return len(self._services)

    def get_service(self, loop_id: str) -> ZitiLoopService | None:
        """Get a loop service by loop ID."""
        return self._services.get(loop_id)

    async def register_loop_service(
        self,
        loop_id: str,
        task_name: str,
        target_port: int,
    ) -> ZitiLoopService | None:
        """Register a new Ziti service for a loop.

        Args:
            loop_id: Unique loop identifier
            task_name: Task name for service naming
            target_port: Local port to proxy to (opencode serve port)

        Returns:
            ZitiLoopService if successful, None otherwise
        """
        async with self._lock:
            if loop_id in self._services:
                self._log.warning(
                    "Loop %s: service already registered",
                    loop_id,
                )
                return self._services[loop_id]

            service = ZitiLoopService(
                identity_path=self.identity_path,
                loop_id=loop_id,
                task_name=task_name,
                target_port=target_port,
                hostname=self.hostname,
            )

            # Load identity
            if not service.load_identity():
                self._log.error(
                    "Loop %s: failed to load Ziti identity for loop service",
                    loop_id,
                )
                return None

            # Bind to service
            if not service.bind():
                self._log.error(
                    "Loop %s: failed to bind loop service",
                    loop_id,
                )
                return None

            # Start accepting connections in the background
            service._accept_task = asyncio.create_task(
                service.start_accepting(),
                name=f"ziti-accept-{loop_id}",
            )

            self._services[loop_id] = service
            self._log.info(
                "Loop %s: Ziti service registered - %s",
                loop_id,
                service.service_name,
            )
            return service

    async def deregister_loop_service(self, loop_id: str) -> None:
        """Deregister a loop's Ziti service.

        Args:
            loop_id: Loop identifier
        """
        async with self._lock:
            service = self._services.pop(loop_id, None)
            if service is None:
                self._log.debug("Loop %s: no service to deregister", loop_id)
                return

        # Shutdown outside the lock
        await service.shutdown()
        self._log.info(
            "Loop %s: Ziti service deregistered",
            loop_id,
        )

    async def shutdown_all(self) -> None:
        """Shutdown all active loop services."""
        async with self._lock:
            loop_ids = list(self._services.keys())

        self._log.info("Shutting down %d loop service(s)", len(loop_ids))

        for loop_id in loop_ids:
            try:
                await self.deregister_loop_service(loop_id)
            except Exception as e:
                self._log.error(
                    "Loop %s: error deregistering service: %s",
                    loop_id,
                    e,
                )


def check_ziti_available() -> bool:
    """Check if the openziti package is available.

    Returns:
        True if openziti is available, False otherwise
    """
    return ZITI_AVAILABLE


def check_identity_valid(identity_path: Path) -> tuple[bool, str]:
    """Check if a Ziti identity file is valid.

    Args:
        identity_path: Path to the identity file

    Returns:
        Tuple of (is_valid, message)
    """
    if not ZITI_AVAILABLE or openziti is None:
        return False, "openziti package not installed"

    if not identity_path.exists():
        return False, f"Identity file not found: {identity_path}"

    if not identity_path.is_file():
        return False, f"Identity path is not a file: {identity_path}"

    # Try to load the identity to validate it
    try:
        ctx, err = openziti.load(str(identity_path))
        if err != 0:
            return False, f"Failed to load identity: error code {err}"
        return True, "Identity valid"
    except Exception as e:
        return False, f"Exception loading identity: {e}"
