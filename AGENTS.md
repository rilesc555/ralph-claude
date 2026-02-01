# Ralph Agent Instructions

## Overview

Ralph is an autonomous AI agent loop that runs Claude Code repeatedly until all PRD items are complete. Each iteration is a fresh Claude Code instance with clean context.

Supports both **feature development** and **bug investigations**.

## Directory Structure

Each effort gets its own subdirectory under `tasks/`:

```
tasks/
├── device-system-refactor/
│   ├── prd.md           # The requirements document
│   ├── prd.json         # Ralph-format JSON
│   └── progress.txt     # Iteration logs
├── fix-auth-timeout/
│   ├── prd.md
│   ├── prd.json
│   └── progress.txt
└── ...
```

## Commands

```bash
# Run Ralph for a specific task
./ralph.sh tasks/device-system-refactor

# Run with more iterations
./ralph.sh tasks/fix-auth-timeout 20

# Run the flowchart dev server
cd flowchart && npm run dev
```

## Key Files

- `ralph.sh` - The bash loop that spawns fresh Claude Code instances
- `prompt.md` - Instructions given to each Claude Code instance
- `skills/prd/` - Skill for generating PRDs (features and bugs)
- `skills/ralph/` - Skill for converting PRDs to JSON
- `prd.json.example` - Example PRD format
- `flowchart/` - Interactive React Flow diagram explaining how Ralph works

## PRD Types

### Feature
Standard feature development with dependency-ordered stories.

### Bug Investigation
Follows: Reproduce → Instrument → Analyze → Evaluate → Implement → Validate

## Browser Testing with Playwright MCP

When the Playwright MCP server is connected, agents can perform browser-based testing using these tools:

### Core Testing Workflow

```
1. Navigate:     mcp__playwright__browser_navigate (url)
2. Snapshot:     mcp__playwright__browser_snapshot () - Get accessibility tree (preferred over screenshots)
3. Interact:     mcp__playwright__browser_click (ref, element)
                 mcp__playwright__browser_type (ref, text)
                 mcp__playwright__browser_fill_form (fields)
4. Verify:       mcp__playwright__browser_snapshot () - Check results
5. Debug:        mcp__playwright__browser_console_messages (level)
                 mcp__playwright__browser_network_requests (includeStatic)
```

### Key Tools

| Tool | Purpose |
|------|---------|
| `browser_navigate` | Go to a URL |
| `browser_snapshot` | Get page accessibility tree (better than screenshot for automation) |
| `browser_click` | Click elements using `ref` from snapshot |
| `browser_type` | Type into input fields |
| `browser_fill_form` | Fill multiple form fields at once |
| `browser_press_key` | Press keyboard keys (Enter, Escape, arrows, etc.) |
| `browser_wait_for` | Wait for text to appear/disappear or time to pass |
| `browser_console_messages` | Get console logs for debugging |
| `browser_network_requests` | Inspect network activity |
| `browser_take_screenshot` | Visual screenshot (use sparingly) |
| `browser_evaluate` | Run JavaScript on the page |

### Element References

After calling `browser_snapshot`, you get an accessibility tree with `ref` values like:
```yaml
- button "Submit" [ref=s1e5]
- textbox "Email" [ref=s1e3]
```

Use these refs to interact:
```
mcp__playwright__browser_click(ref="s1e5", element="Submit button")
mcp__playwright__browser_type(ref="s1e3", text="user@example.com")
```

### Testing Tips

1. **Always snapshot first** - Get the page state before interacting
2. **Use accessibility tree** - `browser_snapshot` is more reliable than screenshots for finding elements
3. **Check console for errors** - Use `browser_console_messages(level="error")` to catch JS errors
4. **Wait for dynamic content** - Use `browser_wait_for(text="Loading complete")` before asserting
5. **Close when done** - Use `browser_close` to clean up

### Example: Testing a Web App

```
# 1. Navigate to the app
browser_navigate(url="http://localhost:3000")

# 2. Get page structure
browser_snapshot()

# 3. Fill login form (using refs from snapshot)
browser_fill_form(fields=[
  {name: "Email", type: "textbox", ref: "s1e3", value: "test@example.com"},
  {name: "Password", type: "textbox", ref: "s1e4", value: "password123"}
])

# 4. Click submit
browser_click(ref="s1e5", element="Login button")

# 5. Wait and verify
browser_wait_for(text="Dashboard")
browser_snapshot()  # Verify we're on dashboard
```

## Patterns

- Each iteration spawns a fresh Claude Code instance with clean context
- Memory persists via git history, `progress.txt`, and `prd.json`
- Stories should be small enough to complete in one context window
- Use the `notes` field in stories to pass context between iterations
- Always update AGENTS.md with discovered patterns for future iterations
