"""Post-install script to copy Ralph skills to Claude Code skills directory."""

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


def get_skills_target_dir() -> Path:
    """Get the Claude Code skills directory."""
    return Path.home() / ".claude" / "skills"


def install_skills(verbose: bool = True) -> int:
    """Install Ralph skills to ~/.claude/skills/.

    Returns:
        0 on success, 1 on failure.
    """
    source_dir = get_skills_source_dir()
    target_dir = get_skills_target_dir()

    if not source_dir.exists():
        if verbose:
            print(f"Error: Skills source directory not found: {source_dir}")
        return 1

    # Find all skill directories (directories containing SKILL.md)
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

    if verbose:
        total = skills_installed + skills_updated
        if total == 0:
            print("No skills found to install.")
        else:
            print(f"\nInstalled {skills_installed} new, updated {skills_updated} existing.")
            print(f"Skills directory: {target_dir}")

    return 0


def main() -> None:
    """Entry point for post-install hook."""
    print("Installing Ralph skills to Claude Code...")
    sys.exit(install_skills())


if __name__ == "__main__":
    main()
