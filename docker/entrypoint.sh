#!/bin/bash
# Ralph Container Entrypoint
# Sets up the project environment and hands off to CMD
#
# Environment Variables:
#   RALPH_PROJECT_GIT_URL    - Git URL to clone (e.g., https://github.com/user/repo.git)
#   RALPH_PROJECT_BRANCH     - Branch to checkout (default: main)
#   RALPH_SETUP_COMMANDS     - Commands to run after clone (e.g., "npm install")
#   RALPH_PROJECT_SSH_KEY    - SSH private key content for private repos (optional)
#
# If RALPH_PROJECT_GIT_URL is not set, falls through to CMD with existing /app/project contents.
# This allows mounting a local project directory as a volume instead of cloning.

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
# Main Entry Point
# ============================================================================
main() {
    log "Ralph container starting..."
    log "Working directory: $PROJECT_DIR"
    
    # Setup SSH key if provided (for private repos)
    setup_ssh_key
    
    # Clone project if URL is provided
    clone_project
    
    # Ensure we're on the correct branch
    checkout_branch
    
    # Run any setup commands (npm install, etc.)
    run_setup_commands
    
    # Change to project directory for CMD
    cd "$PROJECT_DIR"
    
    log "Entrypoint complete, executing command: $*"
    log "----------------------------------------"
    
    # Execute the CMD (default: /bin/bash from Dockerfile)
    exec "$@"
}

# Run main with all arguments
main "$@"
