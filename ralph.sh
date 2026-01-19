#!/bin/bash
# Ralph - Multi-agent autonomous coding loop
# Usage: ralph [task-directory] [-i iterations] [-a agent]
# Example: ralph tasks/fix-auth-timeout -i 20 -a claude

set -e

VERSION="1.1.0"
RALPH_DATA_DIR="${RALPH_DATA_DIR:-$HOME/.local/share/ralph}"

# =============================================================================
# tmux Dependency Check
# =============================================================================
check_tmux() {
  if ! command -v tmux &>/dev/null; then
    echo "Error: tmux is required but not installed."
    echo ""
    echo "Install tmux:"
    echo "  Ubuntu/Debian: sudo apt install tmux"
    echo "  macOS:         brew install tmux"
    echo "  Arch:          sudo pacman -S tmux"
    echo ""
    echo "tmux allows you to watch Ralph's progress in real-time."
    exit 1
  fi
}

# =============================================================================
# tmux Session Management
# =============================================================================

# Get session name from task directory
# Arguments: task_dir (e.g., "tasks/my-feature" or full path)
get_session_name() {
  local task_dir="$1"
  local task_name=$(basename "$task_dir")
  echo "ralph-$task_name"
}

# Check if a tmux session exists
# Arguments: session_name
session_exists() {
  local session_name="$1"
  tmux has-session -t "$session_name" 2>/dev/null
}

# Get the PID of the Ralph process in a tmux session
# Arguments: session_name
get_session_pid() {
  local session_name="$1"
  # Get the PID file we create when starting
  local pid_file="/tmp/ralph-${session_name}.pid"
  if [ -f "$pid_file" ]; then
    cat "$pid_file"
  else
    echo ""
  fi
}

# List all running Ralph sessions
list_sessions() {
  tmux list-sessions -F "#{session_name}" 2>/dev/null | grep "^ralph-" || true
}

# Kill a tmux session
# Arguments: session_name
kill_session() {
  local session_name="$1"
  if session_exists "$session_name"; then
    tmux kill-session -t "$session_name" 2>/dev/null
    # Clean up PID file
    rm -f "/tmp/ralph-${session_name}.pid"
    return 0
  fi
  return 1
}

# =============================================================================
# Checkpoint State
# =============================================================================
CHECKPOINT_REQUESTED=false
CURRENT_ITERATION=0

# Signal handler for checkpoint (SIGUSR1)
handle_checkpoint_signal() {
  echo ""
  echo "  >>> Checkpoint requested. Will checkpoint after current operation..."
  CHECKPOINT_REQUESTED=true
}

# Trap SIGUSR1 for checkpoint
trap 'handle_checkpoint_signal' SIGUSR1

# Write checkpoint to progress.txt and update prd.json
# Arguments: reason (user|error)
write_checkpoint() {
  local reason="${1:-user}"
  local current_story_title=""
  local completed_count=0
  local total_count=0
  
  if [ -f "$PRD_FILE" ]; then
    completed_count=$(jq '[.userStories[] | select(.passes == true)] | length' "$PRD_FILE" 2>/dev/null || echo "0")
    total_count=$(jq '.userStories | length' "$PRD_FILE" 2>/dev/null || echo "0")
    # Get current story (first non-passing one)
    current_story_title=$(jq -r '[.userStories[] | select(.passes != true)][0].title // "None"' "$PRD_FILE" 2>/dev/null || echo "Unknown")
  fi
  
  # Write to progress.txt
  cat >> "$PROGRESS_FILE" << EOF

---
CHECKPOINT at $(date)
Iteration: $CURRENT_ITERATION/$MAX_ITERATIONS
Stories completed: $completed_count/$total_count
Current story: "$current_story_title"
Agent: $CURRENT_AGENT
Reason: $reason

To resume: ralph $TASK_DIR
---
EOF

  # Update prd.json with checkpoint state
  if [ -f "$PRD_FILE" ]; then
    local tmp_file=$(mktemp)
    jq --arg reason "$reason" \
       --argjson iter "$CURRENT_ITERATION" \
       '. + {checkpointed: true, lastIteration: $iter, checkpointReason: $reason}' \
       "$PRD_FILE" > "$tmp_file" && mv "$tmp_file" "$PRD_FILE"
  fi
  
  echo ""
  echo "======================================================================="
  echo "  Checkpoint saved"
  echo "======================================================================="
  echo ""
  echo "  Iteration: $CURRENT_ITERATION/$MAX_ITERATIONS"
  echo "  Stories:   $completed_count/$total_count complete"
  echo "  Agent:     $(get_agent_display_name "$CURRENT_AGENT")"
  echo ""
  echo "  To resume: ralph $TASK_DIR"
  echo "  To change agent: ralph $TASK_DIR -a <agent>"
  echo ""
}

# Clear checkpoint state from prd.json (called on resume)
clear_checkpoint() {
  if [ -f "$PRD_FILE" ]; then
    local tmp_file=$(mktemp)
    jq 'del(.checkpointed, .lastIteration, .checkpointReason)' "$PRD_FILE" > "$tmp_file" && mv "$tmp_file" "$PRD_FILE"
  fi
}

# =============================================================================
# Agent Configuration
# =============================================================================
# Supported agents in fallback priority order (customize as needed)
AGENT_PRIORITY_ORDER=("opencode" "claude" "codex" "amp" "aider")

# Agent detection - checks if agent CLI is available
detect_installed_agents() {
  local installed=()
  for agent in "${AGENT_PRIORITY_ORDER[@]}"; do
    if command -v "$agent" &>/dev/null; then
      installed+=("$agent")
    fi
  done
  echo "${installed[@]}"
}

# Get agent display name
get_agent_display_name() {
  case "$1" in
    claude) echo "Claude Code (Anthropic)" ;;
    codex) echo "Codex CLI (OpenAI)" ;;
    opencode) echo "OpenCode" ;;
    aider) echo "Aider" ;;
    amp) echo "Amp (Sourcegraph)" ;;
    *) echo "$1" ;;
  esac
}

# Build the command for running an agent in non-interactive mode
# Arguments: agent_name prompt_text
build_agent_command() {
  local agent="$1"
  local prompt_file="$2"
  
  case "$agent" in
    claude)
      # Claude Code: --print for non-interactive, --dangerously-skip-permissions to skip prompts
      # --output-format stream-json for parseable output, --verbose for status
      echo "claude --dangerously-skip-permissions --print --output-format stream-json --verbose"
      ;;
    codex)
      # OpenAI Codex: exec for non-interactive, --yolo to skip approvals/sandbox
      # --json for JSONL output, --full-auto for workspace write access
      echo "codex exec --dangerously-bypass-approvals-and-sandbox --json --full-auto"
      ;;
    opencode)
      # OpenCode: run for non-interactive mode, --format json for structured output
      echo "opencode run --format json"
      ;;
    aider)
      # Aider: --message for non-interactive, --yes-always to skip confirmations
      # Note: aider reads from file with --message-file
      echo "aider --yes-always --message-file"
      ;;
    amp)
      # Amp: --execute for non-interactive, --dangerously-allow-all to skip permissions
      # --stream-json for structured output
      echo "amp --execute --dangerously-allow-all --stream-json"
      ;;
    *)
      echo ""
      ;;
  esac
}

# Check if output indicates an error that should trigger fallback
# Arguments: agent_name output_text exit_code
check_agent_error() {
  local agent="$1"
  local output="$2"
  local exit_code="$3"
  
  # Common error patterns across all agents
  local auth_patterns="invalid api key|authentication failed|unauthorized|invalid credentials|auth error|login required|sign in required|api key not found|invalid token|access denied"
  local rate_limit_patterns="rate limit|too many requests|429|quota exceeded|throttled|capacity"
  local context_patterns="context length|too many tokens|token limit|context window|maximum context|input too long|prompt too long"
  
  # Check exit code first
  if [ "$exit_code" -eq 0 ]; then
    echo "success"
    return
  fi
  
  # Convert output to lowercase for pattern matching
  local output_lower=$(echo "$output" | tr '[:upper:]' '[:lower:]')
  
  # Check for authentication errors
  if echo "$output_lower" | grep -qiE "$auth_patterns"; then
    echo "auth_error"
    return
  fi
  
  # Check for rate limit errors
  if echo "$output_lower" | grep -qiE "$rate_limit_patterns"; then
    echo "rate_limit"
    return
  fi
  
  # Check for context/token limit errors  
  if echo "$output_lower" | grep -qiE "$context_patterns"; then
    echo "context_limit"
    return
  fi
  
  # Generic error
  echo "unknown_error"
}

# Get human-readable error message
get_error_message() {
  case "$1" in
    auth_error) echo "Authentication failed - check API key or login status" ;;
    rate_limit) echo "Rate limit exceeded - too many requests" ;;
    context_limit) echo "Context/token limit exceeded - prompt too long" ;;
    unknown_error) echo "Unknown error occurred" ;;
    *) echo "Error: $1" ;;
  esac
}

# =============================================================================
# CLI Help and Version
# =============================================================================

show_help() {
  cat << EOF
Ralph - Autonomous Multi-Agent Coding Loop

Usage: ralph [command] [task-directory] [options]

Commands:
  (default)             Start or resume a task (runs in tmux background)
  attach [task]         Watch running session output (read-only)
  checkpoint [task]     Gracefully stop with state summary
  stop [task]           Force stop running session
  status                List running Ralph sessions

Options:
  -i, --iterations N    Maximum iterations (default: 10)
  -a, --agent NAME      Agent to use (claude, codex, opencode, aider, amp)
  -m, --model MODEL     Model to use (passed to agent, e.g. "anthropic/claude-sonnet-4-5")
  -y, --yes             Skip confirmation prompts
  -p, --prompt FILE     Use custom prompt file
  --init                Initialize tasks/ directory in current project
  --version             Show version
  -h, --help            Show this help

Examples:
  ralph                           # Interactive mode - select from active tasks
  ralph tasks/my-feature          # Start task in background
  ralph attach                    # Attach to running session
  ralph checkpoint                # Checkpoint and exit gracefully
  ralph tasks/my-feature -i 20    # Run with explicit iteration count
  ralph tasks/my-feature -a claude # Use specific agent
  ralph tasks/my-feature -m opus  # Use specific model
  ralph --init                    # Create tasks/ directory structure

Watching output:
  Ralph runs in a tmux session. Use 'ralph attach' to watch real-time output.
  Press Ctrl+B d to detach (Ralph keeps running).
  To checkpoint, run 'ralph checkpoint' from another terminal.

Prompt file resolution (in priority order):
  1. --prompt flag
  2. \$RALPH_PROMPT environment variable
  3. ./prompt.md (project-local override)
  4. ~/.local/share/ralph/prompt.md (global default)

Supported agents (in fallback priority order):
  - opencode   OpenCode
  - claude     Claude Code (Anthropic)
  - codex      Codex CLI (OpenAI)
  - amp        Amp (Sourcegraph)
  - aider      Aider

For more information: https://github.com/anomalyco/ralph-claude
EOF
}

show_version() {
  echo "ralph version $VERSION"
}

# =============================================================================
# Subcommands: attach, status, stop, checkpoint
# =============================================================================

# Find session for a task (or the only running session)
# Arguments: optional task_dir
find_session() {
  local task_dir="$1"
  
  if [ -n "$task_dir" ]; then
    # Specific task requested
    get_session_name "$task_dir"
  else
    # Find running sessions
    local sessions=($(list_sessions))
    local count=${#sessions[@]}
    
    if [ $count -eq 0 ]; then
      echo ""
    elif [ $count -eq 1 ]; then
      echo "${sessions[0]}"
    else
      # Multiple sessions - return empty, caller should handle
      echo "MULTIPLE"
    fi
  fi
}

cmd_attach() {
  check_tmux
  local task_dir="$1"
  local session_name=$(find_session "$task_dir")
  
  if [ -z "$session_name" ]; then
    echo "No Ralph session found."
    echo ""
    echo "Start a task first:"
    echo "  ralph tasks/your-task"
    echo ""
    echo "Or check running sessions:"
    echo "  ralph status"
    exit 1
  fi
  
  if [ "$session_name" = "MULTIPLE" ]; then
    echo "Multiple Ralph sessions running. Specify which task:"
    echo ""
    list_sessions | while read -r sess; do
      echo "  ralph attach ${sess#ralph-}"
    done
    exit 1
  fi
  
  if ! session_exists "$session_name"; then
    echo "Session '$session_name' not found."
    echo ""
    echo "Running sessions:"
    local sessions=$(list_sessions)
    if [ -z "$sessions" ]; then
      echo "  (none)"
    else
      echo "$sessions" | while read -r sess; do
        echo "  $sess"
      done
    fi
    exit 1
  fi
  
  echo "Attaching to $session_name (read-only)..."
  echo ""
  echo "  To detach:     Ctrl+B then d  (Ralph keeps running)"
  echo "  To checkpoint: ralph checkpoint (from another terminal)"
  echo ""
  
  # Attach in read-only mode
  tmux attach-session -t "$session_name" -r
}

cmd_status() {
  check_tmux
  local sessions=($(list_sessions))
  local count=${#sessions[@]}
  
  if [ $count -eq 0 ]; then
    echo "No Ralph sessions running."
    echo ""
    echo "Start a task:"
    echo "  ralph tasks/your-task"
    exit 0
  fi
  
  echo "Running Ralph sessions:"
  echo ""
  
  for session_name in "${sessions[@]}"; do
    local task_name="${session_name#ralph-}"
    local task_dir="tasks/$task_name"
    local prd_file="$task_dir/prd.json"
    
    local info=""
    if [ -f "$prd_file" ]; then
      local completed=$(jq '[.userStories[] | select(.passes == true)] | length' "$prd_file" 2>/dev/null || echo "?")
      local total=$(jq '.userStories | length' "$prd_file" 2>/dev/null || echo "?")
      local agent=$(jq -r '.agent // "unknown"' "$prd_file" 2>/dev/null)
      info="($completed/$total stories, $agent)"
    fi
    
    echo "  $session_name  $info"
  done
  
  echo ""
  echo "Commands:"
  echo "  ralph attach $task_name    # Watch output"
  echo "  ralph checkpoint $task_name # Graceful stop"
  echo "  ralph stop $task_name      # Force stop"
}

cmd_stop() {
  check_tmux
  local task_dir="$1"
  local session_name=$(find_session "$task_dir")
  
  if [ -z "$session_name" ]; then
    echo "No Ralph session found."
    exit 1
  fi
  
  if [ "$session_name" = "MULTIPLE" ]; then
    echo "Multiple Ralph sessions running. Specify which task:"
    echo ""
    list_sessions | while read -r sess; do
      echo "  ralph stop ${sess#ralph-}"
    done
    exit 1
  fi
  
  if ! session_exists "$session_name"; then
    echo "Session '$session_name' not found."
    exit 1
  fi
  
  echo "Stopping $session_name..."
  kill_session "$session_name"
  echo "Stopped."
}

cmd_checkpoint() {
  check_tmux
  local task_dir="$1"
  local session_name=$(find_session "$task_dir")
  
  if [ -z "$session_name" ]; then
    echo "No Ralph session found."
    exit 1
  fi
  
  if [ "$session_name" = "MULTIPLE" ]; then
    echo "Multiple Ralph sessions running. Specify which task:"
    echo ""
    list_sessions | while read -r sess; do
      echo "  ralph checkpoint ${sess#ralph-}"
    done
    exit 1
  fi
  
  if ! session_exists "$session_name"; then
    echo "Session '$session_name' not found."
    exit 1
  fi
  
  # Get the PID and send SIGUSR1
  local pid=$(get_session_pid "$session_name")
  
  if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
    echo "Cannot find Ralph process for $session_name."
    echo "Try: ralph stop $session_name"
    exit 1
  fi
  
  echo "Sending checkpoint signal to $session_name (PID $pid)..."
  kill -USR1 "$pid"
  echo "Checkpoint requested. Ralph will save state and exit after current operation."
  echo ""
  echo "Watch progress: ralph attach ${session_name#ralph-}"
}

init_project() {
  if [ -d "tasks" ]; then
    echo "tasks/ directory already exists."
    ls -la tasks/
  else
    mkdir -p tasks
    echo "Created tasks/ directory."
    echo ""
    echo "Next steps:"
    echo "  1. Use /prd in Claude Code to create a PRD"
    echo "  2. Use /ralph to convert it to prd.json"
    echo "  3. Run: ralph tasks/{effort-name}"
  fi
  exit 0
}

# =============================================================================
# Prompt File Resolution
# =============================================================================

resolve_prompt_file() {
  # 1. --prompt flag
  if [ -n "$CUSTOM_PROMPT" ]; then
    if [ -f "$CUSTOM_PROMPT" ]; then
      echo "$CUSTOM_PROMPT"
      return 0
    else
      echo "Error: Prompt file not found: $CUSTOM_PROMPT" >&2
      exit 1
    fi
  fi

  # 2. RALPH_PROMPT environment variable
  if [ -n "$RALPH_PROMPT" ]; then
    if [ -f "$RALPH_PROMPT" ]; then
      echo "$RALPH_PROMPT"
      return 0
    else
      echo "Error: RALPH_PROMPT file not found: $RALPH_PROMPT" >&2
      exit 1
    fi
  fi

  # 3. Project-local prompt.md
  if [ -f "./prompt.md" ]; then
    echo "./prompt.md"
    return 0
  fi

  # 4. Global default
  if [ -f "$RALPH_DATA_DIR/prompt.md" ]; then
    echo "$RALPH_DATA_DIR/prompt.md"
    return 0
  fi

  # Not found anywhere
  echo "Error: prompt.md not found." >&2
  echo "" >&2
  echo "Looked in:" >&2
  echo "  - ./prompt.md (project-local)" >&2
  echo "  - $RALPH_DATA_DIR/prompt.md (global)" >&2
  echo "" >&2
  echo "Run the installer or create a local prompt.md file." >&2
  echo "See: https://github.com/anomalyco/ralph-claude" >&2
  exit 1
}

# =============================================================================
# Parse Command Line Arguments
# =============================================================================

TASK_DIR=""
MAX_ITERATIONS=""
SKIP_PROMPTS=false
SELECTED_AGENT=""
SELECTED_MODEL=""
CUSTOM_PROMPT=""
SUBCOMMAND=""
RUNNING_IN_TMUX="${RALPH_TMUX_SESSION:-}"

# Check for subcommands first
case "${1:-}" in
  attach|status|stop|checkpoint)
    SUBCOMMAND="$1"
    shift
    # Get optional task argument for subcommands
    if [[ $# -gt 0 && ! "$1" =~ ^- ]]; then
      TASK_DIR="$1"
      shift
    fi
    ;;
esac

# Handle subcommands immediately
case "$SUBCOMMAND" in
  attach)
    cmd_attach "$TASK_DIR"
    exit 0
    ;;
  status)
    cmd_status
    exit 0
    ;;
  stop)
    cmd_stop "$TASK_DIR"
    exit 0
    ;;
  checkpoint)
    cmd_checkpoint "$TASK_DIR"
    exit 0
    ;;
esac

# Parse remaining arguments for main command
while [[ $# -gt 0 ]]; do
  case $1 in
    -i|--iterations)
      MAX_ITERATIONS="$2"
      shift 2
      ;;
    -y|--yes)
      SKIP_PROMPTS=true
      shift
      ;;
    -a|--agent)
      SELECTED_AGENT="$2"
      shift 2
      ;;
    -m|--model)
      SELECTED_MODEL="$2"
      shift 2
      ;;
    -p|--prompt)
      CUSTOM_PROMPT="$2"
      shift 2
      ;;
    --init)
      init_project
      ;;
    --version)
      show_version
      exit 0
      ;;
    -h|--help)
      show_help
      exit 0
      ;;
    -*)
      echo "Unknown option: $1"
      echo "Run 'ralph --help' for usage information."
      exit 1
      ;;
    *)
      TASK_DIR="$1"
      shift
      ;;
  esac
done

# Check tmux is available (for main command)
check_tmux

# Resolve prompt file
PROMPT_FILE=$(resolve_prompt_file)

# =============================================================================
# Task Discovery and Selection
# =============================================================================

# Function to find active tasks (directories with prd.json, excluding archived)
find_active_tasks() {
  find tasks -maxdepth 2 -name "prd.json" -type f 2>/dev/null | \
    grep -v "tasks/archived/" | \
    xargs -I {} dirname {} | \
    sort
}

# Function to display task info
display_task_info() {
  local task_dir="$1"
  local prd_file="$task_dir/prd.json"
  local description=$(jq -r '.description // "No description"' "$prd_file" 2>/dev/null | head -c 60)
  local total=$(jq '.userStories | length' "$prd_file" 2>/dev/null || echo "?")
  local done=$(jq '[.userStories[] | select(.passes == true)] | length' "$prd_file" 2>/dev/null || echo "?")
  local type=$(jq -r '.type // "feature"' "$prd_file" 2>/dev/null)
  printf "%-35s [%s/%s] %s\n" "$task_dir" "$done" "$total" "($type)"
}

# If no task directory provided, find and prompt
if [ -z "$TASK_DIR" ]; then
  # Check if tasks directory exists
  if [ ! -d "tasks" ]; then
    echo "No tasks/ directory found in current project."
    echo ""
    echo "Run 'ralph --init' to create one, or navigate to a project with tasks."
    exit 1
  fi

  # Find active tasks
  ACTIVE_TASKS=($(find_active_tasks))
  TASK_COUNT=${#ACTIVE_TASKS[@]}

  if [ $TASK_COUNT -eq 0 ]; then
    echo "No active tasks found in tasks/."
    echo ""
    echo "To create a new task:"
    echo "  1. Use /prd in Claude Code to create a PRD"
    echo "  2. Use /ralph to convert it to prd.json"
    echo "  3. Run: ralph tasks/{effort-name}"
    exit 1
  elif [ $TASK_COUNT -eq 1 ]; then
    # Only one task, use it automatically
    TASK_DIR="${ACTIVE_TASKS[0]}"
    echo "Found one active task: $TASK_DIR"
    echo ""
  else
    # Multiple tasks, prompt for selection
    echo ""
    echo "======================================================================="
    echo "  Ralph - Select a Task"
    echo "======================================================================="
    echo ""
    echo "Active tasks:"
    echo ""

    for i in "${!ACTIVE_TASKS[@]}"; do
      printf "  %d) " "$((i+1))"
      display_task_info "${ACTIVE_TASKS[$i]}"
    done

    echo ""
    read -p "Select task [1-$TASK_COUNT]: " SELECTION

    # Validate selection
    if ! [[ "$SELECTION" =~ ^[0-9]+$ ]] || [ "$SELECTION" -lt 1 ] || [ "$SELECTION" -gt $TASK_COUNT ]; then
      echo "Invalid selection. Exiting."
      exit 1
    fi

    TASK_DIR="${ACTIVE_TASKS[$((SELECTION-1))]}"
    echo ""
    echo "Selected: $TASK_DIR"
    echo ""
  fi
fi

# Prompt for iterations if not provided via -i flag
if [ -z "$MAX_ITERATIONS" ]; then
  read -p "Max iterations [10]: " ITER_INPUT
  if [ -z "$ITER_INPUT" ]; then
    MAX_ITERATIONS=10
  elif [[ "$ITER_INPUT" =~ ^[0-9]+$ ]]; then
    MAX_ITERATIONS="$ITER_INPUT"
  else
    echo "Invalid number. Using default of 10."
    MAX_ITERATIONS=10
  fi
fi

# =============================================================================
# Agent Selection
# =============================================================================

# Resolve task directory first (needed to check prd.json for saved agent)
if [[ "$TASK_DIR" = /* ]]; then
  FULL_TASK_DIR="$TASK_DIR"
else
  FULL_TASK_DIR="$(pwd)/$TASK_DIR"
fi

PRD_FILE="$FULL_TASK_DIR/prd.json"

# Detect installed agents
INSTALLED_AGENTS=($(detect_installed_agents))
INSTALLED_COUNT=${#INSTALLED_AGENTS[@]}

if [ $INSTALLED_COUNT -eq 0 ]; then
  echo ""
  echo "Error: No supported AI coding agents found."
  echo ""
  echo "Please install one of the following:"
  echo "  - Claude Code: npm install -g @anthropic-ai/claude-code"
  echo "  - OpenAI Codex: npm install -g @openai/codex"
  echo "  - OpenCode: curl -fsSL https://opencode.ai/install | bash"
  echo "  - Aider: pip install aider-chat"
  echo "  - Amp: curl -fsSL https://ampcode.com/install.sh | bash"
  exit 1
fi

# Check if agent was specified via CLI flag
if [ -n "$SELECTED_AGENT" ]; then
  # Validate the selected agent is installed
  if ! command -v "$SELECTED_AGENT" &>/dev/null; then
    echo "Error: Agent '$SELECTED_AGENT' is not installed."
    echo "Installed agents: ${INSTALLED_AGENTS[*]}"
    exit 1
  fi
  CURRENT_AGENT="$SELECTED_AGENT"
# Check if agent is saved in prd.json
elif [ -f "$PRD_FILE" ] && jq -e '.agent' "$PRD_FILE" &>/dev/null; then
  SAVED_AGENT=$(jq -r '.agent' "$PRD_FILE")
  if command -v "$SAVED_AGENT" &>/dev/null; then
    CURRENT_AGENT="$SAVED_AGENT"
    echo "Using saved agent: $(get_agent_display_name "$CURRENT_AGENT")"
  else
    echo "Warning: Saved agent '$SAVED_AGENT' is not installed. Please select a new agent."
    CURRENT_AGENT=""
  fi
else
  CURRENT_AGENT=""
fi

# If no agent selected yet and multiple agents available, prompt user
if [ -z "$CURRENT_AGENT" ]; then
  if [ $INSTALLED_COUNT -eq 1 ]; then
    CURRENT_AGENT="${INSTALLED_AGENTS[0]}"
    echo "Using only installed agent: $(get_agent_display_name "$CURRENT_AGENT")"
  else
    echo ""
    echo "======================================================================="
    echo "  Select AI Coding Agent"
    echo "======================================================================="
    echo ""
    echo "Available agents (in fallback priority order):"
    echo ""
    
    for i in "${!INSTALLED_AGENTS[@]}"; do
      local_agent="${INSTALLED_AGENTS[$i]}"
      printf "  %d) %s\n" "$((i+1))" "$(get_agent_display_name "$local_agent")"
    done
    
    echo ""
    read -p "Select agent [1-$INSTALLED_COUNT]: " AGENT_SELECTION
    
    # Validate selection
    if ! [[ "$AGENT_SELECTION" =~ ^[0-9]+$ ]] || [ "$AGENT_SELECTION" -lt 1 ] || [ "$AGENT_SELECTION" -gt $INSTALLED_COUNT ]; then
      echo "Invalid selection. Using first available: ${INSTALLED_AGENTS[0]}"
      CURRENT_AGENT="${INSTALLED_AGENTS[0]}"
    else
      CURRENT_AGENT="${INSTALLED_AGENTS[$((AGENT_SELECTION-1))]}"
    fi
    
    echo ""
    echo "Selected: $(get_agent_display_name "$CURRENT_AGENT")"
  fi
  
  # Save agent selection to prd.json
  if [ -f "$PRD_FILE" ]; then
    TMP_FILE=$(mktemp)
    jq --arg agent "$CURRENT_AGENT" '. + {agent: $agent}' "$PRD_FILE" > "$TMP_FILE" && mv "$TMP_FILE" "$PRD_FILE"
    echo "Agent preference saved to prd.json"
  fi
fi

# Build the fallback order starting with current agent
FALLBACK_AGENTS=("$CURRENT_AGENT")
for agent in "${INSTALLED_AGENTS[@]}"; do
  if [ "$agent" != "$CURRENT_AGENT" ]; then
    FALLBACK_AGENTS+=("$agent")
  fi
done

# =============================================================================
# Validate Task Directory and Files
# =============================================================================

PROGRESS_FILE="$FULL_TASK_DIR/progress.txt"

# Validate task directory exists
if [ ! -d "$FULL_TASK_DIR" ]; then
  echo "Error: Task directory not found: $TASK_DIR"
  exit 1
fi

# Validate prd.json exists
if [ ! -f "$PRD_FILE" ]; then
  echo "Error: prd.json not found in $TASK_DIR"
  echo "Run the /ralph skill first to convert your PRD to JSON format."
  exit 1
fi

# Initialize progress file if it doesn't exist
if [ ! -f "$PROGRESS_FILE" ]; then
  EFFORT_NAME=$(basename "$TASK_DIR")
  PRD_TYPE=$(jq -r '.type // "feature"' "$PRD_FILE" 2>/dev/null || echo "feature")
  echo "# Ralph Progress Log" > "$PROGRESS_FILE"
  echo "Effort: $EFFORT_NAME" >> "$PROGRESS_FILE"
  echo "Type: $PRD_TYPE" >> "$PROGRESS_FILE"
  echo "Started: $(date)" >> "$PROGRESS_FILE"
  echo "---" >> "$PROGRESS_FILE"
fi

# Get info from prd.json for display
DESCRIPTION=$(jq -r '.description // "No description"' "$PRD_FILE" 2>/dev/null || echo "Unknown")
BRANCH_NAME=$(jq -r '.branchName // "unknown"' "$PRD_FILE" 2>/dev/null || echo "unknown")
TOTAL_STORIES=$(jq '.userStories | length' "$PRD_FILE" 2>/dev/null || echo "?")
COMPLETED_STORIES=$(jq '[.userStories[] | select(.passes == true)] | length' "$PRD_FILE" 2>/dev/null || echo "?")

# =============================================================================
# Resume Detection
# =============================================================================
RESUME_FROM_ITERATION=1
IS_RESUMING=false

if [ -f "$PRD_FILE" ] && jq -e '.checkpointed == true' "$PRD_FILE" &>/dev/null; then
  IS_RESUMING=true
  LAST_ITERATION=$(jq -r '.lastIteration // 1' "$PRD_FILE" 2>/dev/null)
  CHECKPOINT_REASON=$(jq -r '.checkpointReason // "unknown"' "$PRD_FILE" 2>/dev/null)
  RESUME_FROM_ITERATION=$((LAST_ITERATION + 1))
  
  echo ""
  echo "======================================================================="
  echo "  Resuming from checkpoint"
  echo "======================================================================="
  echo ""
  echo "  Last checkpoint: iteration $LAST_ITERATION (reason: $CHECKPOINT_REASON)"
  echo "  Resuming from:   iteration $RESUME_FROM_ITERATION"
  echo ""
  
  # Clear checkpoint state
  clear_checkpoint
fi

# =============================================================================
# tmux Session Setup
# =============================================================================
SESSION_NAME=$(get_session_name "$TASK_DIR")

# Check if session already exists
if session_exists "$SESSION_NAME" && [ -z "$RUNNING_IN_TMUX" ]; then
  echo "Error: Ralph session '$SESSION_NAME' is already running."
  echo ""
  echo "Options:"
  echo "  ralph attach           # Watch the running session"
  echo "  ralph checkpoint       # Gracefully stop"
  echo "  ralph stop             # Force stop"
  exit 1
fi

# If not already running in tmux, spawn ourselves in a new tmux session
if [ -z "$RUNNING_IN_TMUX" ]; then
  echo ""
  echo "======================================================================="
  echo "  Starting Ralph in background"
  echo "======================================================================="
  echo ""
  echo "  Session:    $SESSION_NAME"
  echo "  Task:       $TASK_DIR"
  echo "  Progress:   $COMPLETED_STORIES / $TOTAL_STORIES stories complete"
  echo "  Max iters:  $MAX_ITERATIONS"
  echo "  Agent:      $(get_agent_display_name "$CURRENT_AGENT")"
  if [ -n "$SELECTED_MODEL" ]; then
    echo "  Model:      $SELECTED_MODEL"
  fi
  if [ "$IS_RESUMING" = true ]; then
    echo "  Resuming:   from iteration $RESUME_FROM_ITERATION"
  fi
  echo ""
  echo "  $DESCRIPTION"
  echo ""
  echo "-----------------------------------------------------------------------"
  echo ""
  echo "  To watch output:     ralph attach"
  echo "  To checkpoint:       ralph checkpoint"
  echo "  To stop:             ralph stop"
  echo ""
  
  # Build the command to run inside tmux
  # We pass RALPH_TMUX_SESSION to indicate we're inside tmux
  TMUX_CMD="RALPH_TMUX_SESSION='$SESSION_NAME' "
  TMUX_CMD+="'$0' '$TASK_DIR' -i $MAX_ITERATIONS -a '$CURRENT_AGENT'"
  [ -n "$SELECTED_MODEL" ] && TMUX_CMD+=" -m '$SELECTED_MODEL'"
  [ -n "$CUSTOM_PROMPT" ] && TMUX_CMD+=" -p '$CUSTOM_PROMPT'"
  [ "$SKIP_PROMPTS" = true ] && TMUX_CMD+=" -y"
  
  # Create tmux session in detached mode
  tmux new-session -d -s "$SESSION_NAME" -x 200 -y 50 "bash -c '$TMUX_CMD'"
  
  # Bind 'c' key to send checkpoint signal in this session
  # Use send-keys to the session to trigger checkpoint via the running ralph process
  TASK_BASENAME="${TASK_DIR##*/}"
  tmux send-keys -t "$SESSION_NAME" "" # ensure session is ready
  
  # Set a session-specific hook that binds 'c' when attaching
  # Note: We can't bind keys per-session, so we document 'c' but users run 'ralph checkpoint' instead
  
  exit 0
fi

# =============================================================================
# Running inside tmux - write PID and set up cleanup
# =============================================================================

# Write our PID so checkpoint command can find us
PID_FILE="/tmp/ralph-${SESSION_NAME}.pid"
echo $$ > "$PID_FILE"

# Cleanup function
cleanup_session() {
  rm -f "$PID_FILE"
  # Don't kill tmux session here - let it show final output
}
trap 'cleanup_session' EXIT

echo ""
echo "======================================================================="
echo "  Ralph - Autonomous Agent Loop"
echo "======================================================================="
echo ""
echo "  Task:       $TASK_DIR"
echo "  Branch:     $BRANCH_NAME"
echo "  Progress:   $COMPLETED_STORIES / $TOTAL_STORIES stories complete"
echo "  Max iters:  $MAX_ITERATIONS"
echo "  Agent:      $(get_agent_display_name "$CURRENT_AGENT")"
if [ -n "$SELECTED_MODEL" ]; then
  echo "  Model:      $SELECTED_MODEL"
fi
echo "  Prompt:     $PROMPT_FILE"
if [ ${#FALLBACK_AGENTS[@]} -gt 1 ]; then
  echo "  Fallbacks:  ${FALLBACK_AGENTS[*]:1}"
fi
if [ "$IS_RESUMING" = true ]; then
  echo "  Resuming:   from iteration $RESUME_FROM_ITERATION"
fi
echo ""
echo "  $DESCRIPTION"
echo ""
echo "  To checkpoint: ralph checkpoint (from another terminal)"
echo ""

# =============================================================================
# Run Agent Function
# =============================================================================
# Runs an agent in the foreground with full output streaming.
# Output is displayed in real-time AND captured to a file for parsing.
# Arguments: agent_name prompt_text prompt_file
# Returns: Sets AGENT_OUTPUT, AGENT_EXIT_CODE
run_agent() {
  local agent="$1"
  local prompt_text="$2"
  local prompt_file="$3"
  
  local output_file=$(mktemp)
  local agent_display=$(get_agent_display_name "$agent")
  local start_time=$(date +%s)
  
  # Build model flag if specified
  local model_flag=""
  if [ -n "$SELECTED_MODEL" ]; then
    model_flag="-m $SELECTED_MODEL"
  fi
  
  echo ""
  echo "  ─────────────────────────────────────────────────────────────────"
  if [ -n "$SELECTED_MODEL" ]; then
    echo "  $agent_display starting (model: $SELECTED_MODEL)..."
  else
    echo "  $agent_display starting..."
  fi
  echo "  ─────────────────────────────────────────────────────────────────"
  echo ""
  
  # Run agent in foreground, streaming output to terminal AND capturing to file
  # Use default/text format for human-readable output
  case "$agent" in
    claude)
      # Claude: --print for non-interactive, text output for readability
      if [ -n "$SELECTED_MODEL" ]; then
        echo "$prompt_text" | claude --dangerously-skip-permissions --print --model "$SELECTED_MODEL" 2>&1 | tee "$output_file"
      else
        echo "$prompt_text" | claude --dangerously-skip-permissions --print 2>&1 | tee "$output_file"
      fi
      AGENT_EXIT_CODE=${PIPESTATUS[1]}
      ;;
    codex)
      # Codex: exec for non-interactive (model flag not well documented, skip for now)
      codex exec --dangerously-bypass-approvals-and-sandbox --full-auto "$prompt_text" 2>&1 | tee "$output_file"
      AGENT_EXIT_CODE=${PIPESTATUS[0]}
      ;;
    opencode)
      # OpenCode: run with default format for readable output
      if [ -n "$SELECTED_MODEL" ]; then
        opencode run -m "$SELECTED_MODEL" "$prompt_text" 2>&1 | tee "$output_file"
      else
        opencode run "$prompt_text" 2>&1 | tee "$output_file"
      fi
      AGENT_EXIT_CODE=${PIPESTATUS[0]}
      ;;
    aider)
      # Aider: --message-file for non-interactive, --model for model selection
      echo "$prompt_text" > "$prompt_file"
      if [ -n "$SELECTED_MODEL" ]; then
        aider --yes-always --model "$SELECTED_MODEL" --message-file "$prompt_file" 2>&1 | tee "$output_file"
      else
        aider --yes-always --message-file "$prompt_file" 2>&1 | tee "$output_file"
      fi
      AGENT_EXIT_CODE=${PIPESTATUS[0]}
      ;;
    amp)
      # Amp: --execute for non-interactive
      amp --execute "$prompt_text" --dangerously-allow-all 2>&1 | tee "$output_file"
      AGENT_EXIT_CODE=${PIPESTATUS[0]}
      ;;
    *)
      echo "Unknown agent: $agent"
      AGENT_OUTPUT="Unknown agent: $agent"
      AGENT_EXIT_CODE=1
      rm -f "$output_file"
      return
      ;;
  esac
  
  # Calculate elapsed time
  local elapsed=$(($(date +%s) - start_time))
  local mins=$((elapsed / 60))
  local secs=$((elapsed % 60))
  
  echo ""
  echo "  ─────────────────────────────────────────────────────────────────"
  if [ $AGENT_EXIT_CODE -eq 0 ]; then
    printf "  $agent_display finished in %02d:%02d\n" $mins $secs
  else
    printf "  $agent_display exited with code $AGENT_EXIT_CODE in %02d:%02d\n" $mins $secs
  fi
  echo "  ─────────────────────────────────────────────────────────────────"
  echo ""
  
  # Capture full output for result parsing
  AGENT_OUTPUT=$(cat "$output_file")
  rm -f "$output_file"
}

# =============================================================================
# Main Iteration Loop
# =============================================================================
for i in $(seq $RESUME_FROM_ITERATION $MAX_ITERATIONS); do
  # Update current iteration for checkpoint tracking
  CURRENT_ITERATION=$i
  
  # Check if checkpoint was requested
  if [ "$CHECKPOINT_REQUESTED" = true ]; then
    write_checkpoint "user"
    exit 0
  fi
  
  # Refresh progress count
  COMPLETED_STORIES=$(jq '[.userStories[] | select(.passes == true)] | length' "$PRD_FILE" 2>/dev/null || echo "?")

  echo ""
  echo "==================================================================="
  echo "  Iteration $i of $MAX_ITERATIONS ($COMPLETED_STORIES/$TOTAL_STORIES complete)"
  echo "==================================================================="

  # Build the prompt with task directory context
  PROMPT="# Ralph Agent Instructions

Task Directory: $TASK_DIR
PRD File: $TASK_DIR/prd.json
Progress File: $TASK_DIR/progress.txt

$(cat "$PROMPT_FILE")
"

  # Create temp file for prompt (needed by some agents)
  PROMPT_TEMP_FILE=$(mktemp)
  trap "rm -f $PROMPT_TEMP_FILE" EXIT
  
  # Try agents with fallback
  ITERATION_SUCCESS=false
  TRIED_AGENTS=()
  
  for try_agent in "${FALLBACK_AGENTS[@]}"; do
    TRIED_AGENTS+=("$try_agent")
    
    echo ""
    echo "  Running with $(get_agent_display_name "$try_agent")..."
    
    # Run the agent
    run_agent "$try_agent" "$PROMPT" "$PROMPT_TEMP_FILE"
    
    # Check for errors
    ERROR_TYPE=$(check_agent_error "$try_agent" "$AGENT_OUTPUT" "$AGENT_EXIT_CODE")
    
    if [ "$ERROR_TYPE" = "success" ]; then
      ITERATION_SUCCESS=true
      break
    fi
    
    # Handle error with potential fallback
    ERROR_MSG=$(get_error_message "$ERROR_TYPE")
    echo ""
    echo "  ! $ERROR_MSG"
    
    # Check if we have more agents to try
    if [ ${#TRIED_AGENTS[@]} -lt ${#FALLBACK_AGENTS[@]} ]; then
      NEXT_AGENT_IDX=${#TRIED_AGENTS[@]}
      NEXT_AGENT="${FALLBACK_AGENTS[$NEXT_AGENT_IDX]}"
      echo "  -> Falling back to $(get_agent_display_name "$NEXT_AGENT")..."
      sleep 1
    else
      echo "  x All agents failed. Output from last attempt:"
      echo ""
      echo "$AGENT_OUTPUT"
      echo ""
      echo "==================================================================="
      echo "  All agents exhausted. Please check your configuration."
      echo "==================================================================="
      exit 1
    fi
  done
  
  rm -f "$PROMPT_TEMP_FILE"
  
  # Check for completion (output was already shown in real-time)
  if [ "$ITERATION_SUCCESS" = true ]; then
    # Check for completion signal - must be careful to avoid false positives
    # from JSON output that contains the string embedded in other content
    COMPLETION_DETECTED=false
    
    # First, check for error indicators in the output that would invalidate completion
    if echo "$AGENT_OUTPUT" | grep -qE '"is_error"\s*:\s*true|"error_during_execution"|"subtype"\s*:\s*"error"'; then
      # Output contains error markers - don't treat as complete even if signal present
      echo ""
      echo "  ! Agent reported errors in output, continuing to next iteration..."
      COMPLETION_DETECTED=false
    # Check for completion signal - look for it in multiple formats:
    # 1. Standalone line (plain text output)
    # 2. Inside a JSON "text" field (may have other content before it)
    # 3. Inside a JSON "result" field
    elif echo "$AGENT_OUTPUT" | grep -qE '<promise>COMPLETE</promise>'; then
      COMPLETION_DETECTED=true
    fi
    
    if [ "$COMPLETION_DETECTED" = true ]; then
      # Verify completion by checking prd.json - all stories should have passes: true
      INCOMPLETE_STORIES=$(jq '[.userStories[] | select(.passes == false)] | length' "$PRD_FILE" 2>/dev/null || echo "1")
      
      if [ "$INCOMPLETE_STORIES" = "0" ]; then
        echo ""
        echo "======================================================================="
        echo "  Ralph completed all tasks!"
        echo "======================================================================="
        echo ""
        echo "  Completed at iteration $i of $MAX_ITERATIONS"
        echo "  Agent: $(get_agent_display_name "${TRIED_AGENTS[-1]}")"
        echo "  Check $PROGRESS_FILE for details."
        echo ""

        # Offer to archive
        echo "  To archive this completed effort:"
        echo "    mkdir -p tasks/archived && mv $TASK_DIR tasks/archived/"
        echo ""
        exit 0
      else
        echo ""
        echo "  ! Agent signaled completion but $INCOMPLETE_STORIES stories still incomplete."
        echo "  Continuing to next iteration..."
      fi
    fi
  fi

  # Check if checkpoint was requested during iteration
  if [ "$CHECKPOINT_REQUESTED" = true ]; then
    write_checkpoint "user"
    exit 0
  fi

  echo ""
  echo "Iteration $i complete. Continuing in 2 seconds..."
  sleep 2
done

echo ""
echo "======================================================================="
echo "  Ralph reached max iterations"
echo "======================================================================="
echo ""
COMPLETED_STORIES=$(jq '[.userStories[] | select(.passes == true)] | length' "$PRD_FILE" 2>/dev/null || echo "?")
echo "  Completed $COMPLETED_STORIES of $TOTAL_STORIES stories in $MAX_ITERATIONS iterations."
echo "  Agent: $(get_agent_display_name "$CURRENT_AGENT")"
echo "  Check $PROGRESS_FILE for status."
echo "  Run again with more iterations: ralph $TASK_DIR -i <more_iterations>"
exit 1
