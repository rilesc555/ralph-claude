"""Version constants for Ralph.

All version numbers are centralized here to ensure consistency across:
- The ralph CLI tool
- The prd.json schema
- The prompt.md instructions

Versioning scheme:
- TOOL_VERSION: semver (MAJOR.MINOR.PATCH) for the ralph CLI
- SCHEMA_VERSION: MAJOR.MINOR for prd.json schema compatibility
- PROMPT_VERSION: MAJOR.MINOR for prompt.md instruction format

These versions should be updated together when making breaking changes.
"""

from __future__ import annotations

# Ralph CLI tool version (semver)
TOOL_VERSION = "0.2.0"

# PRD JSON schema version
# Increment MAJOR for breaking changes, MINOR for backwards-compatible additions
SCHEMA_VERSION = "2.4"

# Prompt instruction format version
# Should match SCHEMA_VERSION since they evolve together
PROMPT_VERSION = "2.4"

# Supported schema versions (for backwards compatibility)
SUPPORTED_SCHEMA_VERSIONS = ("2.0", "2.1", "2.2", "2.3", "2.4")

# Minimum supported schema version
MIN_SCHEMA_VERSION = "2.0"


def check_schema_version(version: str) -> tuple[bool, str]:
    """Check if a schema version is supported.

    Args:
        version: The schemaVersion from a prd.json file.

    Returns:
        Tuple of (is_supported, message).
    """
    if version in SUPPORTED_SCHEMA_VERSIONS:
        return True, ""

    try:
        major, minor = map(int, version.split("."))
        min_major, min_minor = map(int, MIN_SCHEMA_VERSION.split("."))

        if major < min_major or (major == min_major and minor < min_minor):
            return False, (
                f"Schema version {version} is too old. "
                f"Minimum supported version is {MIN_SCHEMA_VERSION}."
            )

        # Future version - warn but allow
        return True, (
            f"Schema version {version} is newer than ralph {TOOL_VERSION} supports. "
            f"Some features may not work correctly."
        )
    except ValueError:
        return False, f"Invalid schema version format: {version}"
