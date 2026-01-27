# Ralph Stop Hook - Exits Claude after each response to trigger next iteration
# This hook fires when Claude finishes responding (not on user interrupts)
#
# Installed to: ~/.config/ralph/hooks/stop-iteration.ps1
# Used by: ralph-tui via --settings flag

# Read the hook input from stdin
$input = $input | Out-String

# Parse JSON input
try {
    $hookData = $input | ConvertFrom-Json
    $stopHookActive = $hookData.stop_hook_active
} catch {
    $stopHookActive = $false
}

# Check if this is already a continuation (prevent infinite loops)
if ($stopHookActive -eq $true) {
    # Already in a hook continuation, don't exit again
    Write-Output '{"continue": true}'
    exit 0
}

# Exit Claude to trigger next iteration
# ralph-tui will detect child_exited and restart
Write-Output '{"continue": false, "stopReason": "Iteration complete - ralph-tui will start next iteration"}'
exit 0
