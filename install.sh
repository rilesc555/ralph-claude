#!/bin/bash
# Ralph Installer
# Installs ralph to ~/.local/bin and ~/.local/share/ralph

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Installation paths
BIN_DIR="$HOME/.local/bin"
DATA_DIR="$HOME/.local/share/ralph"
SKILLS_DIR="$HOME/.claude/skills"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() {
  echo -e "${GREEN}[+]${NC} $1"
}

warn() {
  echo -e "${YELLOW}[!]${NC} $1"
}

error() {
  echo -e "${RED}[x]${NC} $1"
}

uninstall() {
  echo "Uninstalling Ralph..."
  echo ""

  if [ -f "$BIN_DIR/ralph" ]; then
    rm "$BIN_DIR/ralph"
    info "Removed $BIN_DIR/ralph"
  fi

  if [ -d "$DATA_DIR" ]; then
    rm -rf "$DATA_DIR"
    info "Removed $DATA_DIR"
  fi

  if [ -d "$SKILLS_DIR/prd" ]; then
    rm -rf "$SKILLS_DIR/prd"
    info "Removed $SKILLS_DIR/prd"
  fi

  if [ -d "$SKILLS_DIR/ralph" ]; then
    rm -rf "$SKILLS_DIR/ralph"
    info "Removed $SKILLS_DIR/ralph"
  fi

  echo ""
  echo "Ralph uninstalled successfully."
  exit 0
}

# Check for uninstall flag
if [ "$1" = "--uninstall" ] || [ "$1" = "-u" ]; then
  uninstall
fi

echo ""
echo "======================================================================="
echo "  Ralph Installer"
echo "======================================================================="
echo ""

# Check prerequisites
if ! command -v jq &> /dev/null; then
  error "jq is required but not installed."
  echo "  Install with: sudo apt install jq  (or brew install jq on macOS)"
  exit 1
fi

# Check source files exist
if [ ! -f "$SCRIPT_DIR/ralph.sh" ]; then
  error "ralph.sh not found in $SCRIPT_DIR"
  exit 1
fi

if [ ! -f "$SCRIPT_DIR/prompt.md" ]; then
  error "prompt.md not found in $SCRIPT_DIR"
  exit 1
fi

# Create directories
info "Creating directories..."
mkdir -p "$BIN_DIR"
mkdir -p "$DATA_DIR"
mkdir -p "$SKILLS_DIR"

# Install ralph script
info "Installing ralph to $BIN_DIR/ralph"
cp "$SCRIPT_DIR/ralph.sh" "$BIN_DIR/ralph"
chmod +x "$BIN_DIR/ralph"

# Install prompt.md
info "Installing prompt.md to $DATA_DIR/"
cp "$SCRIPT_DIR/prompt.md" "$DATA_DIR/prompt.md"

# Install skills
if [ -d "$SCRIPT_DIR/skills/prd" ]; then
  info "Installing prd skill to $SKILLS_DIR/"
  cp -r "$SCRIPT_DIR/skills/prd" "$SKILLS_DIR/"
fi

if [ -d "$SCRIPT_DIR/skills/ralph" ]; then
  info "Installing ralph skill to $SKILLS_DIR/"
  cp -r "$SCRIPT_DIR/skills/ralph" "$SKILLS_DIR/"
fi

echo ""
echo "======================================================================="
echo "  Installation complete!"
echo "======================================================================="
echo ""
echo "  Installed:"
echo "    - $BIN_DIR/ralph"
echo "    - $DATA_DIR/prompt.md"
if [ -d "$SKILLS_DIR/prd" ]; then
  echo "    - $SKILLS_DIR/prd/"
fi
if [ -d "$SKILLS_DIR/ralph" ]; then
  echo "    - $SKILLS_DIR/ralph/"
fi
echo ""

# Check if ~/.local/bin is in PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
  warn "$BIN_DIR is not in your PATH"
  echo ""
  echo "  Add this to your shell config (~/.bashrc, ~/.zshrc, etc.):"
  echo ""
  echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
  echo ""
  echo "  Then restart your shell or run: source ~/.bashrc"
  echo ""
else
  info "$BIN_DIR is already in PATH"
  echo ""
  echo "  You can now run 'ralph' from any directory!"
fi

echo ""
echo "Quick start:"
echo "  cd your-project"
echo "  ralph --init          # Create tasks/ directory"
echo "  # Use /prd in Claude Code to create a PRD"
echo "  # Use /ralph to convert it to prd.json"
echo "  ralph                 # Run Ralph!"
echo ""
echo "For help: ralph --help"
echo ""
