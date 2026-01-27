# Ralph Wiggum for Claude Code - Long-running AI agent loop (Windows PowerShell)
# Usage: .\ralph.ps1 [task-directory] [-Iterations N] [-RotateAt N] [-Yes]
# Example: .\ralph.ps1 tasks/fix-auth-timeout -Iterations 20

param(
    [Parameter(Position=0)]
    [string]$TaskDir = "",

    [Alias("i")]
    [int]$Iterations = 0,

    [Alias("y")]
    [switch]$Yes,

    [int]$RotateAt = 300,

    [Alias("h")]
    [switch]$Help
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if ($Help) {
    Write-Host "Ralph Wiggum - Autonomous Agent Loop (Windows)"
    Write-Host ""
    Write-Host "Usage: .\ralph.ps1 [task-directory] [-Iterations N] [-RotateAt N] [-Yes]"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -Iterations, -i N    Max iterations (default: 10)"
    Write-Host "  -Yes, -y             Skip confirmation prompts"
    Write-Host "  -RotateAt N          Rotate progress file at N lines (default: 300)"
    Write-Host "  -Help, -h            Show this help message"
    Write-Host ""
    Write-Host "For interactive mode with tmux, use: .\ralph-i.ps1"
    exit 0
}

# Function to find active tasks (directories with prd.json, excluding archived)
function Find-ActiveTasks {
    $tasks = @()
    if (Test-Path "tasks") {
        $prdFiles = Get-ChildItem -Path "tasks" -Filter "prd.json" -Recurse -Depth 2 -ErrorAction SilentlyContinue
        foreach ($prd in $prdFiles) {
            $taskPath = $prd.DirectoryName
            # Exclude archived tasks
            if ($taskPath -notlike "*\tasks\archived\*" -and $taskPath -notlike "*\tasks\archived") {
                $relativePath = $taskPath -replace [regex]::Escape((Get-Location).Path + "\"), ""
                $relativePath = $relativePath -replace "\\", "/"
                $tasks += $relativePath
            }
        }
    }
    return $tasks | Sort-Object
}

# Function to display task info
function Show-TaskInfo {
    param([string]$TaskPath)

    $prdFile = Join-Path $TaskPath "prd.json"
    try {
        $prd = Get-Content $prdFile -Raw | ConvertFrom-Json
        $description = if ($prd.description) { $prd.description.Substring(0, [Math]::Min(60, $prd.description.Length)) } else { "No description" }
        $total = if ($prd.userStories) { $prd.userStories.Count } else { "?" }
        $done = if ($prd.userStories) { ($prd.userStories | Where-Object { $_.passes -eq $true }).Count } else { "?" }
        $type = if ($prd.type) { $prd.type } else { "feature" }
        Write-Host ("{0,-35} [{1}/{2}] ({3})" -f $TaskPath, $done, $total, $type)
    } catch {
        Write-Host ("{0,-35} [?/?] (unknown)" -f $TaskPath)
    }
}

# If no task directory provided, find and prompt
if ([string]::IsNullOrEmpty($TaskDir)) {
    $activeTasks = @(Find-ActiveTasks)
    $taskCount = $activeTasks.Count

    if ($taskCount -eq 0) {
        Write-Host "No active tasks found."
        Write-Host ""
        Write-Host "To create a new task:"
        Write-Host "  1. Use /prd to create a PRD in tasks/{effort-name}/"
        Write-Host "  2. Use /ralph to convert it to prd.json"
        Write-Host "  3. Run .\ralph.ps1 tasks/{effort-name}"
        exit 1
    } elseif ($taskCount -eq 1) {
        $TaskDir = $activeTasks[0]
        Write-Host "Found one active task: $TaskDir"
        Write-Host ""
    } else {
        Write-Host ""
        Write-Host ([char]0x2554 + ([string][char]0x2550 * 63) + [char]0x2557)
        Write-Host ([char]0x2551 + "  Ralph Wiggum - Select a Task                                 " + [char]0x2551)
        Write-Host ([char]0x255A + ([string][char]0x2550 * 63) + [char]0x255D)
        Write-Host ""
        Write-Host "Active tasks:"
        Write-Host ""

        for ($i = 0; $i -lt $activeTasks.Count; $i++) {
            Write-Host -NoNewline ("  {0}) " -f ($i + 1))
            Show-TaskInfo $activeTasks[$i]
        }

        Write-Host ""
        $selection = Read-Host "Select task [1-$taskCount]"

        if (-not ($selection -match '^\d+$') -or [int]$selection -lt 1 -or [int]$selection -gt $taskCount) {
            Write-Host "Invalid selection. Exiting."
            exit 1
        }

        $TaskDir = $activeTasks[[int]$selection - 1]
        Write-Host ""
        Write-Host "Selected: $TaskDir"
        Write-Host ""
    }
}

# Prompt for iterations if not provided
if ($Iterations -eq 0) {
    $iterInput = Read-Host "Max iterations [10]"
    if ([string]::IsNullOrEmpty($iterInput)) {
        $Iterations = 10
    } elseif ($iterInput -match '^\d+$') {
        $Iterations = [int]$iterInput
    } else {
        Write-Host "Invalid number. Using default of 10."
        $Iterations = 10
    }
}

# Resolve task directory
if ([System.IO.Path]::IsPathRooted($TaskDir)) {
    $FullTaskDir = $TaskDir
} else {
    $FullTaskDir = Join-Path (Get-Location) $TaskDir
}

$PrdFile = Join-Path $FullTaskDir "prd.json"
$ProgressFile = Join-Path $FullTaskDir "progress.txt"
$PromptFile = Join-Path $ScriptDir "prompt.md"

# Validate task directory exists
if (-not (Test-Path $FullTaskDir -PathType Container)) {
    Write-Host "Error: Task directory not found: $TaskDir"
    exit 1
}

# Validate prd.json exists
if (-not (Test-Path $PrdFile)) {
    Write-Host "Error: prd.json not found in $TaskDir"
    Write-Host "Run the /ralph skill first to convert your PRD to JSON format."
    exit 1
}

# Initialize progress file if it doesn't exist
if (-not (Test-Path $ProgressFile)) {
    $effortName = Split-Path $TaskDir -Leaf
    $prd = Get-Content $PrdFile -Raw | ConvertFrom-Json
    $prdType = if ($prd.type) { $prd.type } else { "feature" }

    $progressContent = @"
# Ralph Progress Log
Effort: $effortName
Type: $prdType
Started: $(Get-Date)
---
"@
    Set-Content -Path $ProgressFile -Value $progressContent -Encoding UTF8
}

# Global variable for rotation confirmation
$script:RotationConfirmed = $false
$script:RotateThreshold = $RotateAt

# Function to rotate progress file if needed
function Rotate-ProgressIfNeeded {
    $lines = (Get-Content $ProgressFile).Count
    $hasPriorRotation = Test-Path (Join-Path $FullTaskDir "progress-1.txt")

    $withinThresholdRange = $script:RotateThreshold - 50
    if ($lines -gt $withinThresholdRange -or $hasPriorRotation) {
        if (-not $Yes -and -not $script:RotationConfirmed) {
            Write-Host ""
            Write-Host "Progress file has $lines lines (rotation threshold: $($script:RotateThreshold))"
            $newThreshold = Read-Host "Rotation threshold [$($script:RotateThreshold)]"
            if ($newThreshold -match '^\d+$') {
                $script:RotateThreshold = [int]$newThreshold
            }
            $script:RotationConfirmed = $true
        }
    }

    if ($lines -gt $script:RotateThreshold) {
        Write-Host ""
        Write-Host "Progress file exceeds $($script:RotateThreshold) lines. Rotating..."

        # Find next rotation number
        $n = 1
        while (Test-Path (Join-Path $FullTaskDir "progress-$n.txt")) {
            $n++
        }

        $rotatedFile = Join-Path $FullTaskDir "progress-$n.txt"
        Move-Item -Path $ProgressFile -Destination $rotatedFile -Force

        # Read rotated file content
        $rotatedContent = Get-Content $rotatedFile -Raw

        # Extract patterns section
        $patternsSection = ""
        if ($rotatedContent -match "## Codebase Patterns") {
            $matches = [regex]::Match($rotatedContent, "(## Codebase Patterns.*?)(?=## [^C]|$)", [System.Text.RegularExpressions.RegexOptions]::Singleline)
            if ($matches.Success) {
                $patternsSection = $matches.Groups[1].Value.TrimEnd()
            }
        }

        # Get effort info from rotated file
        $effortName = ""
        $effortType = ""
        $started = ""
        foreach ($line in (Get-Content $rotatedFile)) {
            if ($line -match "^Effort:") { $effortName = $line }
            if ($line -match "^Type:") { $effortType = $line }
            if ($line -match "^Started:") { $started = $line }
        }

        # Count stories completed
        $storyCount = (Select-String -Path $rotatedFile -Pattern "^## .* - S\d+" -AllMatches).Matches.Count

        # Build prior reference
        $priorRef = ""
        if ($n -gt 1) {
            $priorRef = " (continues from progress-$($n-1).txt)"
        }

        # Create new progress.txt
        $newProgressContent = @"
# Ralph Progress Log
$effortName
$effortType
$started
Rotation: $n (rotated at $(Get-Date))

$patternsSection

## Prior Progress
Completed $storyCount iterations in progress-$n.txt$priorRef.
_See progress-$n.txt for detailed iteration logs._

---
"@
        Set-Content -Path $ProgressFile -Value $newProgressContent -Encoding UTF8

        Write-Host "Created summary. Previous progress saved to progress-$n.txt"
        Write-Host ""
    }
}

# Get info from prd.json for display
$prd = Get-Content $PrdFile -Raw | ConvertFrom-Json
$Description = if ($prd.description) { $prd.description } else { "No description" }
$BranchName = if ($prd.branchName) { $prd.branchName } else { "unknown" }
$TotalStories = if ($prd.userStories) { $prd.userStories.Count } else { "?" }
$CompletedStories = if ($prd.userStories) { ($prd.userStories | Where-Object { $_.passes -eq $true }).Count } else { "?" }

Write-Host ""
Write-Host ([char]0x2554 + ([string][char]0x2550 * 63) + [char]0x2557)
Write-Host ([char]0x2551 + "  Ralph Wiggum - Autonomous Agent Loop                         " + [char]0x2551)
Write-Host ([char]0x255A + ([string][char]0x2550 * 63) + [char]0x255D)
Write-Host ""
Write-Host "  Task:       $TaskDir"
Write-Host "  Branch:     $BranchName"
Write-Host "  Progress:   $CompletedStories / $TotalStories stories complete"
Write-Host "  Max iters:  $Iterations"
Write-Host ""
Write-Host "  $Description"
Write-Host ""

# Spinner characters
$spinnerChars = @([char]0x28FB, [char]0x28D9, [char]0x28F9, [char]0x28F8, [char]0x28FC, [char]0x28F4, [char]0x28E6, [char]0x28E7, [char]0x28C7, [char]0x28CF)

for ($i = 1; $i -le $Iterations; $i++) {
    # Check and rotate progress file if needed
    Rotate-ProgressIfNeeded

    # Refresh progress count
    $prd = Get-Content $PrdFile -Raw | ConvertFrom-Json
    $CompletedStories = if ($prd.userStories) { ($prd.userStories | Where-Object { $_.passes -eq $true }).Count } else { "?" }

    Write-Host ""
    Write-Host ("=" * 67)
    Write-Host "  Iteration $i of $Iterations ($CompletedStories/$TotalStories complete)"
    Write-Host ("=" * 67)

    # Build the prompt with task directory context
    $promptContent = Get-Content $PromptFile -Raw
    $prompt = @"
# Ralph Agent Instructions

Task Directory: $TaskDir
PRD File: $TaskDir/prd.json
Progress File: $TaskDir/progress.txt

$promptContent
"@

    # Create temp file for output
    $outputFile = [System.IO.Path]::GetTempFileName()

    # Start time tracking
    $startTime = Get-Date
    $lastStatus = "Starting..."

    Write-Host ""
    Write-Host ""

    # Run claude in background
    $job = Start-Job -ScriptBlock {
        param($prompt, $outputFile)
        $prompt | & claude --dangerously-skip-permissions --print --output-format stream-json --verbose 2>&1 | Out-File -FilePath $outputFile -Encoding UTF8
    } -ArgumentList $prompt, $outputFile

    # Show spinner while claude runs
    $spinnerIndex = 0
    while ($job.State -eq "Running") {
        $elapsed = (Get-Date) - $startTime
        $mins = [Math]::Floor($elapsed.TotalMinutes)
        $secs = $elapsed.Seconds

        # Parse output for status updates
        if (Test-Path $outputFile) {
            try {
                $lastLines = Get-Content $outputFile -Tail 20 -ErrorAction SilentlyContinue
                foreach ($line in $lastLines) {
                    if ($line -match '"tool_name":"([^"]*)"') {
                        $lastStatus = "Using $($Matches[1])..."
                    } elseif ($line -match '"text":"([^"]*)"') {
                        $textPreview = $Matches[1]
                        if ($textPreview.Length -gt 60) {
                            $textPreview = $textPreview.Substring(0, 60)
                        }
                        if (-not [string]::IsNullOrWhiteSpace($textPreview)) {
                            $lastStatus = $textPreview
                        }
                    }
                }
            } catch {
                # Ignore read errors
            }
        }

        # Move cursor up and update spinner
        Write-Host "`e[2A" -NoNewline
        $spinnerChar = $spinnerChars[$spinnerIndex % $spinnerChars.Count]
        $timeStr = "{0:D2}:{1:D2}" -f $mins, $secs
        Write-Host "`r`e[K  $spinnerChar Claude working... $timeStr"
        $truncatedStatus = if ($lastStatus.Length -gt 70) { $lastStatus.Substring(0, 70) } else { $lastStatus }
        Write-Host "`e[K  `e[90m$truncatedStatus`e[0m"

        $spinnerIndex++
        Start-Sleep -Milliseconds 100
    }

    # Wait for job to complete
    $null = Wait-Job $job
    Remove-Job $job

    # Show completion
    $elapsed = (Get-Date) - $startTime
    $mins = [Math]::Floor($elapsed.TotalMinutes)
    $secs = $elapsed.Seconds
    $timeStr = "{0:D2}:{1:D2}" -f $mins, $secs
    Write-Host "`e[2A" -NoNewline
    Write-Host "`r`e[K  $([char]0x2713) Claude finished in $timeStr"
    Write-Host "`e[K"

    # Read output
    $output = ""
    if (Test-Path $outputFile) {
        $fileContent = Get-Content $outputFile -Raw

        # Try to extract result from JSON
        $resultMatch = [regex]::Match($fileContent, '"type":"result".*?"result":"([^"]*)"')
        if ($resultMatch.Success) {
            $output = $resultMatch.Groups[1].Value -replace '\\n', "`n" -replace '\\t', "`t"
        } else {
            $output = $fileContent
        }

        Remove-Item $outputFile -Force -ErrorAction SilentlyContinue
    }

    # Show output
    Write-Host ""
    Write-Host $output

    # Check for completion signal
    if ($output -match "<promise>COMPLETE</promise>") {
        Write-Host ""
        Write-Host ([char]0x2554 + ([string][char]0x2550 * 63) + [char]0x2557)
        Write-Host ([char]0x2551 + "  Ralph completed all tasks!                                   " + [char]0x2551)
        Write-Host ([char]0x255A + ([string][char]0x2550 * 63) + [char]0x255D)
        Write-Host ""
        Write-Host "  Completed at iteration $i of $Iterations"
        Write-Host "  Check $ProgressFile for details."
        Write-Host ""
        Write-Host "  To archive this completed effort:"
        Write-Host "    New-Item -ItemType Directory -Force -Path tasks\archived; Move-Item $TaskDir tasks\archived\"
        Write-Host ""
        exit 0
    }

    Write-Host ""
    Write-Host "Iteration $i complete. Continuing in 2 seconds..."
    Start-Sleep -Seconds 2
}

Write-Host ""
Write-Host ([char]0x2554 + ([string][char]0x2550 * 63) + [char]0x2557)
Write-Host ([char]0x2551 + "  Ralph reached max iterations                                 " + [char]0x2551)
Write-Host ([char]0x255A + ([string][char]0x2550 * 63) + [char]0x255D)
Write-Host ""
$prd = Get-Content $PrdFile -Raw | ConvertFrom-Json
$CompletedStories = if ($prd.userStories) { ($prd.userStories | Where-Object { $_.passes -eq $true }).Count } else { "?" }
Write-Host "  Completed $CompletedStories of $TotalStories stories in $Iterations iterations."
Write-Host "  Check $ProgressFile for status."
Write-Host "  Run again with more iterations: .\ralph.ps1 $TaskDir -Iterations <more>"
exit 1
