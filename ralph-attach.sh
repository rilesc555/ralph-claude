#!/bin/bash
# ralph-attach.sh - Helper script to find and attach to running Ralph tmux sessions
#
# Usage:
#   ralph-attach.sh              # List sessions, auto-attach if only one
#   ralph-attach.sh <session>    # Attach to a specific session by name or number
#
# This script helps users connect to running Ralph agent sessions,
# especially useful when accessing a remote Docker container via SSH.

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Print colored output
print_info() {
    echo -e "${BLUE}$1${NC}"
}

print_success() {
    echo -e "${GREEN}$1${NC}"
}

print_warning() {
    echo -e "${YELLOW}$1${NC}"
}

print_error() {
    echo -e "${RED}$1${NC}" >&2
}

# Check if tmux is installed
if ! command -v tmux &> /dev/null; then
    print_error "Error: tmux is not installed"
    exit 1
fi

# Get list of Ralph tmux sessions (sessions starting with "ralph-")
get_ralph_sessions() {
    tmux list-sessions -F "#{session_name}" 2>/dev/null | grep "^ralph-" || true
}

# Display sessions with numbers for selection
display_sessions() {
    local sessions=("$@")
    local i=1
    
    echo ""
    print_info "Active Ralph tmux sessions:"
    echo "----------------------------"
    
    for session in "${sessions[@]}"; do
        # Get additional session info
        local window_count=$(tmux list-windows -t "$session" 2>/dev/null | wc -l)
        local created=$(tmux display-message -t "$session" -p "#{session_created}" 2>/dev/null)
        local created_human=""
        
        if [[ -n "$created" ]]; then
            created_human=$(date -d "@$created" "+%Y-%m-%d %H:%M:%S" 2>/dev/null || date -r "$created" "+%Y-%m-%d %H:%M:%S" 2>/dev/null || echo "unknown")
        fi
        
        printf "  ${GREEN}[%d]${NC} %s" "$i" "$session"
        if [[ -n "$created_human" && "$created_human" != "unknown" ]]; then
            printf " ${YELLOW}(started: %s)${NC}" "$created_human"
        fi
        echo ""
        i=$((i + 1))
    done
    echo ""
}

# Attach to a session
attach_to_session() {
    local session="$1"
    
    print_success "Attaching to session: $session"
    print_info "Tip: Press Ctrl+B, D to detach from the session"
    echo ""
    
    # Small delay to let the user read the message
    sleep 0.5
    
    exec tmux attach-session -t "$session"
}

# Main logic
main() {
    local target_session="$1"
    
    # Get list of Ralph sessions
    local sessions_raw
    sessions_raw=$(get_ralph_sessions)
    
    # Check if any Ralph sessions exist
    if [[ -z "$sessions_raw" ]]; then
        print_warning "No active Ralph tmux sessions found."
        echo ""
        echo "Sessions are created when running Ralph in interactive mode:"
        echo "  ralph-i.sh tasks/your-task"
        echo ""
        echo "To list all tmux sessions (not just Ralph):"
        echo "  tmux list-sessions"
        exit 0
    fi
    
    # Convert to array
    local sessions=()
    while IFS= read -r line; do
        sessions+=("$line")
    done <<< "$sessions_raw"
    
    local session_count=${#sessions[@]}
    
    # If a specific session was requested
    if [[ -n "$target_session" ]]; then
        # Check if it's a number (selection from list)
        if [[ "$target_session" =~ ^[0-9]+$ ]]; then
            local index=$((target_session - 1))
            if [[ $index -ge 0 && $index -lt $session_count ]]; then
                attach_to_session "${sessions[$index]}"
            else
                print_error "Invalid selection: $target_session"
                print_info "Valid range: 1-$session_count"
                exit 1
            fi
        else
            # Treat as session name
            if tmux has-session -t "$target_session" 2>/dev/null; then
                attach_to_session "$target_session"
            else
                print_error "Session not found: $target_session"
                display_sessions "${sessions[@]}"
                exit 1
            fi
        fi
    fi
    
    # Auto-attach if only one session
    if [[ $session_count -eq 1 ]]; then
        print_info "Found 1 Ralph session, auto-attaching..."
        attach_to_session "${sessions[0]}"
    fi
    
    # Multiple sessions - prompt for selection
    display_sessions "${sessions[@]}"
    
    # Check if we're in an interactive terminal
    if [[ ! -t 0 ]]; then
        print_warning "Non-interactive mode: specify session name or number as argument"
        echo "  ralph-attach.sh 1"
        echo "  ralph-attach.sh ${sessions[0]}"
        exit 0
    fi
    
    # Prompt for selection
    while true; do
        printf "Select session [1-%d] or 'q' to quit: " "$session_count"
        read -r selection
        
        case "$selection" in
            q|Q|quit|exit)
                echo "Cancelled."
                exit 0
                ;;
            "")
                # Default to first session
                attach_to_session "${sessions[0]}"
                ;;
            *)
                if [[ "$selection" =~ ^[0-9]+$ ]]; then
                    local index=$((selection - 1))
                    if [[ $index -ge 0 && $index -lt $session_count ]]; then
                        attach_to_session "${sessions[$index]}"
                    else
                        print_error "Invalid selection. Please enter a number between 1 and $session_count"
                    fi
                else
                    # Try as session name
                    if tmux has-session -t "$selection" 2>/dev/null; then
                        attach_to_session "$selection"
                    else
                        print_error "Invalid input. Enter a number or session name."
                    fi
                fi
                ;;
        esac
    done
}

# Handle --help flag
if [[ "$1" == "-h" || "$1" == "--help" ]]; then
    echo "ralph-attach.sh - Find and attach to running Ralph tmux sessions"
    echo ""
    echo "Usage:"
    echo "  ralph-attach.sh              List sessions, auto-attach if only one"
    echo "  ralph-attach.sh <number>     Attach to session by list number"
    echo "  ralph-attach.sh <name>       Attach to session by name"
    echo ""
    echo "Ralph sessions use the naming pattern: ralph-<pid>-<iteration>"
    echo ""
    echo "Once attached, use Ctrl+B, D to detach from the session."
    exit 0
fi

main "$@"
