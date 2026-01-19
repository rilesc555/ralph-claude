#!/bin/bash
# Ralph Stop Hook - Exits Claude after each response to trigger next iteration
# This hook fires when Claude finishes responding (not on user interrupts)
#
# Installed to: ~/.config/ralph/hooks/stop-iteration.sh
# Used by: ralph-tui via --settings flag

# Read the hook input from stdin
INPUT=$(cat)

# Check if this is already a continuation (prevent infinite loops)
STOP_HOOK_ACTIVE=$(echo "$INPUT" | jq -r '.stop_hook_active // false')

if [ "$STOP_HOOK_ACTIVE" = "true" ]; then
  # Already in a hook continuation, don't exit again
  echo '{"continue": true}'
  exit 0
fi

# Exit Claude to trigger next iteration
# ralph-tui will detect child_exited and restart
echo '{"continue": false, "stopReason": "Iteration complete - ralph-tui will start next iteration"}'
exit 0
