# Ralph Installation Script (Windows PowerShell)
# Installs skills, prompt.md, and hooks with version detection
# Usage: .\install.ps1 [-Force]

param(
    [Alias("f")]
    [switch]$Force,

    [Alias("h")]
    [switch]$Help
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# Installation paths (Windows)
$SkillsInstallDir = Join-Path $env:USERPROFILE ".claude\skills"
$PromptInstallDir = Join-Path $env:USERPROFILE ".config\ralph"
$BinInstallDir = Join-Path $env:USERPROFILE ".local\bin"

if ($Help) {
    Write-Host "Usage: .\install.ps1 [OPTIONS]"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -Force, -f     Skip version prompts and always upgrade"
    Write-Host "  -Help, -h      Show this help message"
    exit 0
}

# Parse version from SKILL.md YAML frontmatter
# Looks for: version: "X.Y" or version: X.Y between --- delimiters
# Returns 0.0 if not found
function Get-SkillVersion {
    param([string]$SkillFile)

    if (-not (Test-Path $SkillFile)) {
        return "0.0"
    }

    $content = Get-Content $SkillFile -Raw
    $inFrontmatter = $false
    $foundStart = $false

    foreach ($line in (Get-Content $SkillFile)) {
        if ($line -match "^---$") {
            if (-not $foundStart) {
                $foundStart = $true
                $inFrontmatter = $true
                continue
            } else {
                break
            }
        }

        if ($inFrontmatter -and $line -match "^version:\s*[`"']?([0-9]+\.[0-9]+)[`"']?") {
            return $Matches[1]
        }
    }

    return "0.0"
}

# Parse version from prompt.md HTML comment
# Looks for: <!-- version: X.Y -->
# Returns 0.0 if not found
function Get-PromptVersion {
    param([string]$PromptFile)

    if (-not (Test-Path $PromptFile)) {
        return "0.0"
    }

    $content = Get-Content $PromptFile -Raw
    if ($content -match "<!--\s*version:\s*([0-9]+\.[0-9]+)\s*-->") {
        return $Matches[1]
    }

    return "0.0"
}

# Compare two version strings
# Returns: 0 if equal, 1 if v1 > v2, -1 if v1 < v2
function Compare-Versions {
    param(
        [string]$v1,
        [string]$v2
    )

    $v1Parts = $v1 -split "\."
    $v2Parts = $v2 -split "\."

    $v1Major = [int]$v1Parts[0]
    $v1Minor = if ($v1Parts.Length -gt 1) { [int]$v1Parts[1] } else { 0 }
    $v2Major = [int]$v2Parts[0]
    $v2Minor = if ($v2Parts.Length -gt 1) { [int]$v2Parts[1] } else { 0 }

    if ($v1Major -gt $v2Major) { return 1 }
    if ($v1Major -lt $v2Major) { return -1 }
    if ($v1Minor -gt $v2Minor) { return 1 }
    if ($v1Minor -lt $v2Minor) { return -1 }
    return 0
}

# Check if upgrade is needed and prompt user
# Returns $true if should install, $false if should skip
function Test-AndPromptUpgrade {
    param(
        [string]$Name,
        [string]$InstalledVersion,
        [string]$RepoVersion
    )

    $cmpResult = Compare-Versions $InstalledVersion $RepoVersion

    if ($cmpResult -eq 0) {
        # Versions match
        Write-Host "$([char]0x2713) $Name is up to date (v$RepoVersion)" -ForegroundColor Green
        return $false
    } elseif ($cmpResult -eq 1) {
        # Installed is newer
        Write-Host "$([char]0x26A0) $Name`: installed (v$InstalledVersion) is newer than repo (v$RepoVersion)" -ForegroundColor Yellow
        if ($Force) {
            return $true
        }
        $reply = Read-Host "  Overwrite with repo version? [y/N]"
        return ($reply -match "^[Yy]")
    } else {
        # Repo is newer
        Write-Host "$([char]0x2191) $Name`: upgrade available (v$InstalledVersion -> v$RepoVersion)" -ForegroundColor Cyan
        if ($Force) {
            return $true
        }
        $reply = Read-Host "  Upgrade? [Y/n]"
        return (-not ($reply -match "^[Nn]"))
    }
}

# Create backup of a file
function New-Backup {
    param([string]$File)

    if (Test-Path $File) {
        $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
        $backup = "$File.backup-$timestamp"
        Copy-Item $File $backup
        Write-Host "  Backed up to: $backup" -ForegroundColor Yellow
    }
}

# Install a skill directory
function Install-Skill {
    param([string]$SkillName)

    $repoSkillDir = Join-Path $ScriptDir "skills\$SkillName"
    $installSkillDir = Join-Path $SkillsInstallDir $SkillName

    if (-not (Test-Path $repoSkillDir -PathType Container)) {
        Write-Host "$([char]0x2717) Skill '$SkillName' not found in repo" -ForegroundColor Red
        return
    }

    $repoVersion = Get-SkillVersion (Join-Path $repoSkillDir "SKILL.md")
    $installedVersion = "0.0"
    $installedSkillFile = Join-Path $installSkillDir "SKILL.md"
    if (Test-Path $installedSkillFile) {
        $installedVersion = Get-SkillVersion $installedSkillFile
    }

    if (Test-AndPromptUpgrade "Skill: $SkillName" $installedVersion $repoVersion) {
        # Create backup of existing skill
        if (Test-Path $installSkillDir -PathType Container) {
            New-Backup $installedSkillFile
        }

        # Install skill
        New-Item -ItemType Directory -Force -Path $installSkillDir | Out-Null
        Copy-Item -Path "$repoSkillDir\*" -Destination $installSkillDir -Recurse -Force
        Write-Host "  Installed $SkillName to $installSkillDir" -ForegroundColor Green
    }
}

# Install prompt.md
function Install-Prompt {
    $repoPrompt = Join-Path $ScriptDir "prompt.md"
    $installPrompt = Join-Path $PromptInstallDir "prompt.md"

    if (-not (Test-Path $repoPrompt)) {
        Write-Host "$([char]0x2717) prompt.md not found in repo" -ForegroundColor Red
        return
    }

    $repoVersion = Get-PromptVersion $repoPrompt
    $installedVersion = "0.0"
    if (Test-Path $installPrompt) {
        $installedVersion = Get-PromptVersion $installPrompt
    }

    if (Test-AndPromptUpgrade "prompt.md" $installedVersion $repoVersion) {
        # Create backup
        New-Backup $installPrompt

        # Install prompt
        New-Item -ItemType Directory -Force -Path $PromptInstallDir | Out-Null
        Copy-Item $repoPrompt $installPrompt -Force
        Write-Host "  Installed prompt.md to $installPrompt" -ForegroundColor Green
    }
}

# Install hooks and settings for ralph-tui
function Install-Hooks {
    $hooksDir = Join-Path $PromptInstallDir "hooks"
    $settingsFile = Join-Path $PromptInstallDir "settings.json"

    Write-Host "Installing hooks..."

    # Create hooks directory
    New-Item -ItemType Directory -Force -Path $hooksDir | Out-Null

    # Install stop-iteration hook (PowerShell version)
    $stopHookSrc = Join-Path $ScriptDir "hooks\stop-iteration.ps1"
    if (Test-Path $stopHookSrc) {
        Copy-Item $stopHookSrc (Join-Path $hooksDir "stop-iteration.ps1") -Force
        Write-Host "  Installed stop-iteration.ps1 to $hooksDir" -ForegroundColor Green
    } else {
        # Try bash version as fallback (might work with Git Bash)
        $stopHookBash = Join-Path $ScriptDir "hooks\stop-iteration.sh"
        if (Test-Path $stopHookBash) {
            Copy-Item $stopHookBash (Join-Path $hooksDir "stop-iteration.sh") -Force
            Write-Host "  Installed stop-iteration.sh to $hooksDir" -ForegroundColor Green
        }
    }

    # Generate Windows-specific settings.json with correct paths
    # Claude hooks on Windows need to invoke PowerShell to run .ps1 scripts
    $hookPath = Join-Path $hooksDir "stop-iteration.ps1"
    # Escape backslashes for JSON
    $hookPathEscaped = $hookPath -replace '\\', '\\\\'

    $settingsContent = @"
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -ExecutionPolicy Bypass -File \"$hookPathEscaped\""
          }
        ]
      }
    ]
  }
}
"@

    Set-Content -Path $settingsFile -Value $settingsContent -Encoding UTF8
    Write-Host "  Generated settings.json with Windows paths to $settingsFile" -ForegroundColor Green
}

# Build and install ralph-tui binary
function Install-RalphTui {
    $tuiDir = Join-Path $ScriptDir "ralph-tui"
    $binaryName = "ralph-tui.exe"
    $installPath = Join-Path $BinInstallDir $binaryName

    Write-Host "Building ralph-tui..."

    # Check if ralph-tui directory exists
    if (-not (Test-Path $tuiDir -PathType Container)) {
        Write-Host "$([char]0x2717) ralph-tui directory not found" -ForegroundColor Red
        return
    }

    # Check if cargo is installed
    $cargoPath = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargoPath) {
        Write-Host "$([char]0x2717) cargo not found. Please install Rust: https://rustup.rs" -ForegroundColor Red
        return
    }

    # Build release version
    Write-Host "  Building release binary..." -ForegroundColor Cyan
    Push-Location $tuiDir
    try {
        # Temporarily allow errors since cargo writes progress to stderr
        $oldErrorAction = $ErrorActionPreference
        $ErrorActionPreference = "Continue"

        & cargo build --release 2>&1 | ForEach-Object {
            if ($_ -is [System.Management.Automation.ErrorRecord]) {
                # Cargo progress output comes through stderr - display it
                Write-Host "  $_" -ForegroundColor Gray
            } else {
                Write-Host "  $_"
            }
        }

        $buildExitCode = $LASTEXITCODE
        $ErrorActionPreference = $oldErrorAction

        if ($buildExitCode -ne 0) {
            Write-Host "$([char]0x2717) Build failed" -ForegroundColor Red
            return
        }
    } finally {
        Pop-Location
    }

    # Check if binary was created
    $builtBinary = Join-Path $tuiDir "target\release\ralph-tui.exe"
    if (-not (Test-Path $builtBinary)) {
        Write-Host "$([char]0x2717) Binary not found after build" -ForegroundColor Red
        return
    }

    # Create bin directory if needed
    New-Item -ItemType Directory -Force -Path $BinInstallDir | Out-Null

    # Copy binary
    Copy-Item $builtBinary $installPath -Force
    Write-Host "  Installed $binaryName to $installPath" -ForegroundColor Green

    # Check if bin directory is in PATH
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$BinInstallDir*") {
        Write-Host ""
        Write-Host "  Note: $BinInstallDir is not in your PATH" -ForegroundColor Yellow
        Write-Host "  To add it permanently, run:" -ForegroundColor Yellow
        Write-Host "    `$env:Path += `";$BinInstallDir`"" -ForegroundColor Gray
        Write-Host "    [Environment]::SetEnvironmentVariable(`"Path`", `$env:Path + `";$BinInstallDir`", `"User`")" -ForegroundColor Gray
    }
}

# Main installation
function Main {
    Write-Host ""
    Write-Host ([char]0x2554 + ([string][char]0x2550 * 63) + [char]0x2557)
    Write-Host ([char]0x2551 + "  Ralph Installation (Windows)                                 " + [char]0x2551)
    Write-Host ([char]0x255A + ([string][char]0x2550 * 63) + [char]0x255D)
    Write-Host ""

    # Create directories if needed
    New-Item -ItemType Directory -Force -Path $SkillsInstallDir | Out-Null
    New-Item -ItemType Directory -Force -Path $PromptInstallDir | Out-Null
    New-Item -ItemType Directory -Force -Path $BinInstallDir | Out-Null

    # Build and install ralph-tui binary
    Install-RalphTui

    Write-Host ""

    # Install skills
    Write-Host "Installing skills to $SkillsInstallDir..."
    Write-Host ""

    $skillsDir = Join-Path $ScriptDir "skills"
    if (Test-Path $skillsDir) {
        $skillDirs = Get-ChildItem -Path $skillsDir -Directory
        foreach ($skillDir in $skillDirs) {
            Install-Skill $skillDir.Name
        }
    }

    Write-Host ""

    # Install prompt.md
    Write-Host "Installing prompt.md to $PromptInstallDir..."
    Write-Host ""
    Install-Prompt

    Write-Host ""

    # Install hooks
    Install-Hooks

    Write-Host ""
    Write-Host "Installation complete!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Binary installed to:  $BinInstallDir\ralph-tui.exe"
    Write-Host "Skills installed to:  $SkillsInstallDir"
    Write-Host "Prompt installed to:  $PromptInstallDir\prompt.md"
    Write-Host "Hooks installed to:   $PromptInstallDir\hooks\"
}

Main
