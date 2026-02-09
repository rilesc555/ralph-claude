"""Post-install script to copy Ralph skills to Claude Code and OpenCode."""

from __future__ import annotations

import shutil
import sys
from importlib import resources
from pathlib import Path


def get_skills_source_dir() -> Path:
    """Get the path to bundled skills in the package."""
    # Use importlib.resources to find the skills directory
    try:
        # Python 3.9+ way
        files = resources.files("ralph")
        skills_path = files / "skills"
        # Convert to a real path (extracts if needed)
        with resources.as_file(skills_path) as path:
            return Path(path)
    except (TypeError, AttributeError):
        # Fallback: find relative to this file
        return Path(__file__).parent / "skills"


def get_skills_target_dirs() -> list[Path]:
    """Get the skills directories for Claude Code and OpenCode."""
    return [
        Path.home() / ".claude" / "skills",
        Path.home() / ".config" / "opencode" / "skills",
    ]


# Keep for backwards compatibility
def get_skills_target_dir() -> Path:
    """Get the Claude Code skills directory."""
    return Path.home() / ".claude" / "skills"


def _install_skills_to_dir(
    source_dir: Path,
    target_dir: Path,
    verbose: bool = True,
) -> tuple[int, int]:
    """Install skills from source to a single target directory.

    Returns:
        Tuple of (skills_installed, skills_updated).
    """
    skills_installed = 0
    skills_updated = 0

    for skill_dir in source_dir.iterdir():
        if not skill_dir.is_dir():
            continue

        skill_md = skill_dir / "SKILL.md"
        if not skill_md.exists():
            continue

        skill_name = skill_dir.name
        target_skill_dir = target_dir / skill_name

        # Check if skill already exists
        exists = target_skill_dir.exists()

        # Create target directory
        target_dir.mkdir(parents=True, exist_ok=True)

        # Copy the skill directory
        if exists:
            shutil.rmtree(target_skill_dir)
            skills_updated += 1
        else:
            skills_installed += 1

        shutil.copytree(skill_dir, target_skill_dir)

        if verbose:
            action = "Updated" if exists else "Installed"
            print(f"  {action}: {skill_name} -> {target_skill_dir}")

    return skills_installed, skills_updated


def install_skills(verbose: bool = True) -> int:
    """Install Ralph skills to ~/.claude/skills/ and ~/.config/opencode/skills/.

    Returns:
        0 on success, 1 on failure.
    """
    source_dir = get_skills_source_dir()

    if not source_dir.exists():
        if verbose:
            print(f"Error: Skills source directory not found: {source_dir}")
        return 1

    total_installed = 0
    total_updated = 0

    for target_dir in get_skills_target_dirs():
        if verbose:
            print(f"\nInstalling to {target_dir}:")
        installed, updated = _install_skills_to_dir(source_dir, target_dir, verbose)
        total_installed += installed
        total_updated += updated

    if verbose:
        total = total_installed + total_updated
        if total == 0:
            print("\nNo skills found to install.")
        else:
            print(f"\nTotal: {total_installed} new, {total_updated} updated.")

    return 0


def main() -> None:
    """Entry point for post-install hook."""
    print("Installing Ralph skills to Claude Code and OpenCode...")
    sys.exit(install_skills())


if __name__ == "__main__":
    main()
