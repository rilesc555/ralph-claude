#!/bin/bash
# Ralph Container Entrypoint
# Sets up the project environment and hands off to CMD
#
# Environment Variables:
#   RALPH_PROJECT_GIT_URL    - Git URL to clone (e.g., https://github.com/user/repo.git)
#   RALPH_PROJECT_BRANCH     - Branch to checkout (default: main)
#   RALPH_SETUP_COMMANDS     - Commands to run after clone (e.g., "npm install")
#   RALPH_PROJECT_SSH_KEY    - SSH private key content for private repos (optional)
#   RALPH_SSH_AUTHORIZED_KEYS - SSH public keys for remote access (for SSH server)
#   RALPH_SSH_PORT           - SSH server port (default: 22)
#   RALPH_STATUS_PORT        - Status API HTTP port (default: 8080)
#   RALPH_TASK_DIR           - Task directory to monitor (auto-detected if not set)
#   ENABLE_SSH               - Set by Dockerfile build arg (true/false)
#
# If RALPH_PROJECT_GIT_URL is not set, falls through to CMD with existing /app/project contents.
# This allows mounting a local project directory as a volume instead of cloning.
#
# SSH Server:
#   When ENABLE_SSH=true (set at build time), the entrypoint:
#   1. Starts the SSH server on RALPH_SSH_PORT (default 22)
#   2. Sets up authorized_keys from RALPH_SSH_AUTHORIZED_KEYS
#   3. Runs CMD as the ralph user (switches from root)
#
# Status API:
#   The entrypoint starts ralph-status.sh in the background on RALPH_STATUS_PORT (default 8080)
#   This provides a JSON endpoint for monitoring Ralph execution status:
#   - GET /        - Full status
#   - GET /status  - Full status (alias)
#   - GET /health  - Health check

set -e

# Log with timestamp
log() {
    echo "[entrypoint $(date '+%H:%M:%S')] $*"
}

log_error() {
    echo "[entrypoint $(date '+%H:%M:%S')] ERROR: $*" >&2
}

# Project directory (configured in Dockerfile)
PROJECT_DIR="/app/project"

# ============================================================================
# SSH Key Setup (for private repositories)
# ============================================================================
setup_ssh_key() {
    if [[ -n "${RALPH_PROJECT_SSH_KEY:-}" ]]; then
        log "Setting up SSH key for private repository access..."
        
        mkdir -p ~/.ssh
        chmod 700 ~/.ssh
        
        # Write the SSH key
        echo "$RALPH_PROJECT_SSH_KEY" > ~/.ssh/id_rsa
        chmod 600 ~/.ssh/id_rsa
        
        # Disable strict host key checking for git operations
        # This is necessary for automated clone operations
        cat > ~/.ssh/config << 'EOF'
Host *
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
    LogLevel ERROR
EOF
        chmod 600 ~/.ssh/config
        
        log "SSH key configured"
    fi
}

# ============================================================================
# SSH Server Setup (for remote Claude session attachment)
# ============================================================================
# Sets up authorized_keys for SSH access when SSH server is enabled
# 
# Environment:
#   RALPH_SSH_AUTHORIZED_KEYS - Public SSH key(s) for authorized access
#                               Multiple keys can be separated by newlines
#   ENABLE_SSH                - Set by Dockerfile build arg
#   RALPH_SSH_PORT            - SSH port (default: 22)
setup_ssh_server() {
    # Only set up if SSH is enabled in the container
    if [[ "${ENABLE_SSH:-false}" != "true" ]]; then
        return 0
    fi
    
    log "SSH server is enabled"
    
    # Determine the ralph user home directory
    local ralph_home="/home/ralph"
    
    # Create .ssh directory with correct permissions for ralph user
    mkdir -p "$ralph_home/.ssh"
    chmod 700 "$ralph_home/.ssh"
    
    if [[ -n "${RALPH_SSH_AUTHORIZED_KEYS:-}" ]]; then
        log "Configuring SSH authorized keys..."
        
        # Write authorized keys (handles multi-line keys)
        echo "$RALPH_SSH_AUTHORIZED_KEYS" > "$ralph_home/.ssh/authorized_keys"
        chmod 600 "$ralph_home/.ssh/authorized_keys"
        
        # Ensure correct ownership (important when running as root)
        chown -R ralph:ralph "$ralph_home/.ssh" 2>/dev/null || true
        
        # Count keys added
        local key_count
        key_count=$(grep -c '^ssh-' "$ralph_home/.ssh/authorized_keys" 2>/dev/null || echo "0")
        log "SSH authorized keys configured ($key_count key(s) added)"
    else
        log "WARNING: No RALPH_SSH_AUTHORIZED_KEYS set - SSH login will not be possible"
        log "Set RALPH_SSH_AUTHORIZED_KEYS to your public SSH key to enable remote access"
    fi
    
    # Start SSH server in background
    # Note: sshd must run as root, container entrypoint runs as root before switching to ralph
    if [[ $EUID -eq 0 ]]; then
        log "Starting SSH server on port ${RALPH_SSH_PORT:-22}..."
        
        # Start sshd in daemon mode
        /usr/sbin/sshd -p "${RALPH_SSH_PORT:-22}" || {
            log_error "Failed to start SSH server"
            return 1
        }
        
        log "SSH server started - connect via: ssh -p ${RALPH_SSH_PORT:-22} ralph@<host>"
        log "Attach to Ralph session: tmux attach-session -t ralph"
    else
        log "WARNING: SSH server requires root to start"
        log "Container must be started as root (remove USER directive or use --user root)"
    fi
}

# ============================================================================
# Project Clone
# ============================================================================
clone_project() {
    local git_url="${RALPH_PROJECT_GIT_URL:-}"
    local branch="${RALPH_PROJECT_BRANCH:-main}"
    
    if [[ -z "$git_url" ]]; then
        log "RALPH_PROJECT_GIT_URL not set, skipping clone"
        log "Using existing /app/project contents (mount a volume or use default)"
        return 0
    fi
    
    log "Cloning project from: $git_url"
    log "Branch: $branch"
    
    # Check if project directory already has content
    if [[ -d "$PROJECT_DIR/.git" ]]; then
        log "Project already cloned, fetching latest changes..."
        cd "$PROJECT_DIR"
        
        # Fetch and checkout the specified branch
        git fetch origin
        git checkout "$branch" 2>/dev/null || git checkout -b "$branch" "origin/$branch"
        git pull origin "$branch" || true
        
        log "Project updated"
    else
        # Remove any placeholder files in /app/project
        rm -rf "$PROJECT_DIR"/*
        rm -rf "$PROJECT_DIR"/.[!.]* 2>/dev/null || true
        
        # Clone the repository
        if ! git clone --branch "$branch" "$git_url" "$PROJECT_DIR"; then
            log_error "Failed to clone repository: $git_url"
            log_error "Check that RALPH_PROJECT_GIT_URL is correct and accessible"
            exit 1
        fi
        
        log "Project cloned successfully"
    fi
    
    cd "$PROJECT_DIR"
}

# ============================================================================
# Branch Checkout
# ============================================================================
checkout_branch() {
    local branch="${RALPH_PROJECT_BRANCH:-main}"
    
    if [[ ! -d "$PROJECT_DIR/.git" ]]; then
        log "No git repository found, skipping branch checkout"
        return 0
    fi
    
    cd "$PROJECT_DIR"
    
    # Get current branch
    local current_branch
    current_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")
    
    if [[ "$current_branch" == "$branch" ]]; then
        log "Already on branch: $branch"
        return 0
    fi
    
    log "Checking out branch: $branch"
    
    # Try to checkout the branch
    if git checkout "$branch" 2>/dev/null; then
        log "Switched to branch: $branch"
    elif git checkout -b "$branch" "origin/$branch" 2>/dev/null; then
        log "Created and switched to tracking branch: $branch"
    else
        log_error "Failed to checkout branch: $branch"
        log "Available branches:"
        git branch -a
        exit 1
    fi
}

# ============================================================================
# Setup Commands
# ============================================================================
run_setup_commands() {
    local commands="${RALPH_SETUP_COMMANDS:-}"
    
    if [[ -z "$commands" ]]; then
        log "RALPH_SETUP_COMMANDS not set, skipping setup"
        return 0
    fi
    
    log "Running setup commands..."
    cd "$PROJECT_DIR"
    
    # Run the commands in a subshell with error handling
    if ! bash -c "$commands"; then
        log_error "Setup commands failed"
        log_error "Commands: $commands"
        exit 1
    fi
    
    log "Setup commands completed"
}

# ============================================================================
# Status API Server
# ============================================================================
# Starts the Ralph status API server in the background
# Environment:
#   RALPH_STATUS_PORT - HTTP port (default: 8080)
#   RALPH_TASK_DIR    - Task directory to monitor (auto-detected if not set)
start_status_api() {
    local port="${RALPH_STATUS_PORT:-8080}"
    local task_dir="${RALPH_TASK_DIR:-}"
    
    # Auto-detect task directory if not set
    # Look for first directory containing prd.json in /app/project/tasks/
    if [[ -z "$task_dir" ]]; then
        if [[ -d "$PROJECT_DIR/tasks" ]]; then
            for dir in "$PROJECT_DIR/tasks"/*; do
                if [[ -f "$dir/prd.json" ]]; then
                    task_dir="$dir"
                    log "Auto-detected task directory: $task_dir"
                    break
                fi
            done
        fi
    fi
    
    # If still no task directory, skip starting the API
    if [[ -z "$task_dir" ]] || [[ ! -f "$task_dir/prd.json" ]]; then
        log "No task directory with prd.json found, skipping status API"
        log "Set RALPH_TASK_DIR to enable status API"
        return 0
    fi
    
    log "Starting Ralph status API on port $port..."
    log "Task directory: $task_dir"
    
    # Export for the status script
    export RALPH_TASK_DIR="$task_dir"
    export RALPH_STATUS_PORT="$port"
    
    # Start the status API in the background
    # Note: The script uses Node.js which is available in the container
    nohup /app/ralph/ralph-status.sh "$task_dir" "$port" > /tmp/ralph-status.log 2>&1 &
    local pid=$!
    
    # Give it a moment to start
    sleep 1
    
    # Check if it's running
    if kill -0 "$pid" 2>/dev/null; then
        log "Status API started (PID: $pid)"
        log "Health check: curl http://localhost:$port/health"
        echo "$pid" > /tmp/ralph-status.pid
    else
        log_error "Status API failed to start"
        log_error "Check /tmp/ralph-status.log for details"
    fi
}

# ============================================================================
# Main Entry Point
# ============================================================================
main() {
    log "Ralph container starting..."
    log "Working directory: $PROJECT_DIR"
    
    # Setup SSH key if provided (for private repos)
    setup_ssh_key
    
    # Setup SSH server for remote Claude session attachment
    setup_ssh_server
    
    # Clone project if URL is provided
    clone_project
    
    # Ensure we're on the correct branch
    checkout_branch
    
    # Run any setup commands (npm install, etc.)
    run_setup_commands
    
    # Start the status API server in the background
    start_status_api
    
    # Change to project directory for CMD
    cd "$PROJECT_DIR"
    
    log "Entrypoint complete, executing command: $*"
    log "----------------------------------------"
    
    # If running as root (for SSH server support), switch to ralph user for CMD
    # This ensures the main process runs with appropriate permissions
    if [[ $EUID -eq 0 ]] && [[ "${ENABLE_SSH:-false}" == "true" ]]; then
        log "Switching to ralph user for command execution..."
        exec su -s /bin/bash ralph -c "cd $PROJECT_DIR && exec $*"
    else
        # Execute the CMD directly (already running as ralph user)
        exec "$@"
    fi
}

# Run main with all arguments
main "$@"
