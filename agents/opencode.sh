#!/bin/bash
# OpenCode CLI wrapper script for Ralph agent abstraction
# Accepts prompt via stdin, outputs to stdout
#
# Usage: echo "prompt" | ./opencode.sh [options]
#
# Environment variables:
#   MODEL              - Model to use in provider/model format (e.g., "anthropic/claude-sonnet-4")
#   RALPH_VERBOSE      - Set to "true" for verbose output
#
# Supported options (via environment):
#   OUTPUT_FORMAT      - Output format: "json" or "default" (default: "default")

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source common utilities
source "$SCRIPT_DIR/common.sh"

# ============================================================================
# Configuration
# ============================================================================

# Default output format
OUTPUT_FORMAT="${OUTPUT_FORMAT:-default}"

# Verbose output
VERBOSE="${RALPH_VERBOSE:-false}"

# Model selection (OpenCode supports provider/model format)
MODEL="${MODEL:-}"

# ============================================================================
# Build command arguments
# ============================================================================

build_opencode_args() {
  local args=()

  # Handle output format
  case "$OUTPUT_FORMAT" in
    json)
      args+=("--format" "json")
      ;;
    default|*)
      args+=("--format" "default")
      ;;
  esac

  # Model selection
  if [ -n "$MODEL" ]; then
    args+=("--model" "$MODEL")
  fi

  # Verbose mode - use print-logs for debug output
  if [ "$VERBOSE" = "true" ]; then
    args+=("--print-logs")
    args+=("--log-level" "DEBUG")
  fi

  echo "${args[@]}"
}

# ============================================================================
# Main execution
# ============================================================================

main() {
  # Read prompt from stdin
  local prompt=""
  if [ ! -t 0 ]; then
    prompt=$(cat)
  fi

  if [ -z "$prompt" ]; then
    log_error "No prompt provided via stdin"
    echo "Usage: echo 'your prompt' | $0" >&2
    exit 1
  fi

  # Build command arguments
  local args
  args=$(build_opencode_args)

  # Log invocation if verbose
  if [ "$VERBOSE" = "true" ]; then
    log_info "Invoking OpenCode CLI with args: $args"
    log_info "Model: ${MODEL:-default}"
  fi

  # Execute opencode run with prompt as argument
  # OpenCode run takes the message as positional arguments
  # shellcheck disable=SC2086
  opencode run $args "$prompt"
}

# Run main function
main "$@"
