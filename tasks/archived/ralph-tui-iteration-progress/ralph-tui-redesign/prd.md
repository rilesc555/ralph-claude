# PRD: Ralph TUI Visual Redesign

## Type
Feature

## Introduction

Redesign the ralph-tui application with a modern "midnight developer cockpit" aesthetic. The goal is to transform the current basic TUI into a visually stunning, card-based interface with a deep space terminal color palette, rounded corners, well-spaced elements, and subtle animations. No functionality changes—purely visual/UX improvements.

The target aesthetic is inspired by modern terminal emulators like Warp, Hyper, and tools like Linear and Raycast—professional, focused, easy on the eyes for long coding sessions.

## Goals

- Transform the UI to match the reference mockup design closely
- Implement a cohesive "deep space terminal" color palette
- Add card-based layouts with rounded corners
- Implement status indicators with animations (spinners, pulsing dots, progress bars)
- Improve visual hierarchy and information density
- Maintain all existing functionality without changes

## Color Palette

### Base Colors
| Name | Hex | Usage |
|------|-----|-------|
| `bg_primary` | `#0a0e14` | Main background |
| `bg_secondary` | `#12161c` | Card backgrounds |
| `bg_tertiary` | `#1a1f26` | Elevated cards, hover states |
| `border_subtle` | `#1e2530` | Card borders |

### Accent Colors
| Name | Hex | Usage |
|------|-----|-------|
| `cyan_primary` | `#00d4aa` | Primary accent, headers, links |
| `cyan_dim` | `#0a8a6e` | Dimmed cyan for secondary elements |
| `green_success` | `#4ade80` | Success states, completed items |
| `green_active` | `#22c55e` | Active/running indicators |
| `amber_warning` | `#fbbf24` | Warning states |
| `red_error` | `#f87171` | Error states |

### Text Colors
| Name | Hex | Usage |
|------|-----|-------|
| `text_primary` | `#e2e8f0` | Primary text |
| `text_secondary` | `#94a3b8` | Secondary/muted text |
| `text_muted` | `#64748b` | Timestamps, labels |

## User Stories

### US-001: Implement Deep Space Color Palette Module
**Description:** As a developer, I need a centralized color palette module so that colors are consistent throughout the application.

**Acceptance Criteria:**
- [ ] Create a `theme.rs` module with all color constants defined
- [ ] Define RGB values for all palette colors (bg, accent, text)
- [ ] Export color constants for use throughout the application
- [ ] Typecheck passes

### US-002: Redesign Left Sidebar Header
**Description:** As a user, I want to see a styled header with the Ralph branding so that I know which tool I'm using.

**Acceptance Criteria:**
- [ ] Display "● RALPH LOOP" with green status dot at top
- [ ] Show "Terminal v{version}" in cyan below the title
- [ ] Use proper spacing and typography hierarchy
- [ ] Header uses `bg_primary` background
- [ ] Typecheck passes

### US-003: Implement Statistics Cards Row
**Description:** As a user, I want to see iteration and completion stats in elegant card widgets so I can quickly assess progress.

**Acceptance Criteria:**
- [ ] Create two side-by-side stat cards: "ITERATIONS" and "COMPLETED"
- [ ] Each card has an icon, large number value, and label
- [ ] Cards have `bg_secondary` background with rounded corners
- [ ] Values use `cyan_primary` color for emphasis
- [ ] Labels use `text_muted` color
- [ ] Typecheck passes

### US-004: Implement System Stats Cards Row
**Description:** As a user, I want to see CPU/token usage and cost stats in card format so I can monitor resource usage.

**Acceptance Criteria:**
- [ ] Create two side-by-side stat cards for token stats and cost
- [ ] Display input/output token counts with appropriate icons
- [ ] Display session cost estimate in USD
- [ ] Cards match the style of US-003 (rounded, proper colors)
- [ ] Typecheck passes

### US-005: Implement Active Phase Section
**Description:** As a user, I want to see the current phase and uptime clearly displayed so I know what Ralph is doing.

**Acceptance Criteria:**
- [ ] Display section header "✦ ACTIVE PHASE" with muted styling
- [ ] Show current phase name (e.g., "Execute Iteration Cycle") prominently
- [ ] Display uptime with clock icon in `text_muted` color
- [ ] Use proper vertical spacing from cards above
- [ ] Typecheck passes

### US-006: Implement User Stories List with Status Cards
**Description:** As a user, I want to see all user stories displayed as cards with visual status indicators so I can track progress at a glance.

**Acceptance Criteria:**
- [ ] Display section header "↳ USER STORIES / PHASES"
- [ ] Each story rendered as a card with rounded corners
- [ ] Completed stories: green filled circle (●), `bg_secondary` background, cyan text
- [ ] Active story: pulsing green circle (●), highlighted card, progress bar inside
- [ ] Pending stories: gray hollow circle (○), darker background, muted text
- [ ] Story cards show ID and title (e.g., "#01 Initialize Environment")
- [ ] Typecheck passes

### US-007: Implement Progress Bar Component
**Description:** As a user, I want to see animated progress bars for active tasks so I can visualize completion.

**Acceptance Criteria:**
- [ ] Create reusable progress bar component
- [ ] Progress bar uses `cyan_primary` for filled portion
- [ ] Background uses `bg_primary` (darker than card)
- [ ] Display percentage text below the bar
- [ ] Progress bar has subtle animation/glow effect when active
- [ ] Typecheck passes

### US-008: Implement Pulsing Status Indicator Animation
**Description:** As a user, I want to see a pulsing dot animation for active/running states so I know something is in progress.

**Acceptance Criteria:**
- [ ] Create animation state tracking in app state
- [ ] Implement pulse cycle (bright → dim → bright) for active indicators
- [ ] Animation runs at ~1 second cycle
- [ ] Used for: active story indicator, running PTY status
- [ ] Typecheck passes

### US-009: Implement Spinner Animation Component
**Description:** As a user, I want to see a spinner animation during loading/processing states so I know the system is working.

**Acceptance Criteria:**
- [ ] Create spinner component with rotating characters (⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ or similar)
- [ ] Spinner cycles through frames at ~100ms intervals
- [ ] Use `cyan_primary` color for spinner
- [ ] Can be displayed inline with text
- [ ] Typecheck passes

### US-010: Redesign Bottom Footer Bar
**Description:** As a user, I want to see session info and keybindings in a clean footer so I have quick reference.

**Acceptance Criteria:**
- [ ] Display "Session ID" label with value (e.g., "RL-7X9K2") on left
- [ ] Show keybinding hints on right side
- [ ] Footer uses subtle border-top or different background
- [ ] Text uses `text_muted` color with cyan accents for values
- [ ] Typecheck passes

### US-011: Redesign Right Panel Window Chrome
**Description:** As a user, I want the Claude Code panel to look like a proper terminal window so it feels integrated.

**Acceptance Criteria:**
- [ ] Add window chrome header with traffic light dots (●●●) in red/yellow/green
- [ ] Display terminal title ">_ claude-code - ralph-loop"
- [ ] Add subtle window control icons on right (minimize, maximize hints)
- [ ] Header uses `bg_tertiary` background
- [ ] Typecheck passes

### US-012: Implement ASCII Art Banner for Right Panel
**Description:** As a user, I want to see a stylish ASCII art banner when the terminal starts so it feels polished.

**Acceptance Criteria:**
- [ ] Create "RALPH LOOP" ASCII art banner in cyan
- [ ] Display subtitle "Ralph Loop Terminal v{version} - Iteration Tracking System"
- [ ] Show "Type help for available commands" hint
- [ ] Banner appears at top of terminal output area
- [ ] Typecheck passes

### US-013: Style Terminal Output with Timestamps
**Description:** As a user, I want terminal output to have timestamps and color-coded lines so I can follow the activity log.

**Acceptance Criteria:**
- [ ] Prefix log lines with timestamp in `text_muted` color (HH:MM:SS format)
- [ ] Color-code different message types:
  - Commands ($): white text
  - Info (ℹ): cyan text
  - Success (✓): green text
  - Warning (⚠): amber text
  - Error (✗): red text
- [ ] Worker messages use distinct colors for each worker
- [ ] Typecheck passes

### US-014: Implement Terminal Input Bar
**Description:** As a user, I want a styled input bar at the bottom of the terminal panel so I can see where to type.

**Acceptance Criteria:**
- [ ] Display prompt "> ralph@loop:~$" with cyan accent
- [ ] Show placeholder text "Enter command..." in muted color
- [ ] Input bar has subtle top border
- [ ] Cursor indicator visible when in Claude mode
- [ ] Typecheck passes

### US-015: Implement Rounded Corner Borders
**Description:** As a developer, I need rounded corner border drawing so cards look modern.

**Acceptance Criteria:**
- [ ] Create custom border set using Unicode rounded corners (╭╮╰╯)
- [ ] Apply rounded borders to all card components
- [ ] Ensure borders render correctly at all terminal sizes
- [ ] Typecheck passes

### US-016: Implement Card Spacing and Layout System
**Description:** As a developer, I need consistent spacing and layout helpers so the UI is well-organized.

**Acceptance Criteria:**
- [ ] Define spacing constants (padding, margins, gaps)
- [ ] Create layout helpers for card rows
- [ ] Ensure consistent vertical rhythm throughout sidebar
- [ ] Cards have proper internal padding
- [ ] Typecheck passes

### US-017: Update Mode Indicator Styling
**Description:** As a user, I want clear visual feedback when switching between Ralph and Claude modes.

**Acceptance Criteria:**
- [ ] Active panel has brighter border (`cyan_primary`)
- [ ] Inactive panel has subtle border (`border_subtle`)
- [ ] Mode indicator text updates in footer
- [ ] Smooth visual transition between modes
- [ ] Typecheck passes

### US-018: Implement Iteration Delay Countdown Styling
**Description:** As a user, I want the iteration delay countdown to be styled consistently with the new design.

**Acceptance Criteria:**
- [ ] Countdown uses large, prominent text
- [ ] "Starting next iteration in Xs..." message styled appropriately
- [ ] Maintains card-based layout during delay
- [ ] Typecheck passes

### US-019: Final Polish and Visual Consistency Pass
**Description:** As a user, I want the entire UI to feel cohesive and polished.

**Acceptance Criteria:**
- [ ] All colors match the defined palette
- [ ] All cards have consistent border radius
- [ ] Spacing is consistent throughout
- [ ] No visual glitches at different terminal sizes
- [ ] Animations are smooth and not jarring
- [ ] Typecheck passes

## Functional Requirements

- FR-1: All color values must be defined in a centralized theme module
- FR-2: Cards must use Unicode rounded corner characters (╭╮╰╯│─)
- FR-3: Animations must not impact performance (target 60fps render)
- FR-4: Layout must adapt gracefully to terminal sizes down to 80x24
- FR-5: All existing keyboard shortcuts must continue to work
- FR-6: PTY output rendering must remain functional
- FR-7: File watching for prd.json must continue to work
- FR-8: Mode switching between Ralph/Claude must remain functional

## Non-Goals

- No changes to the iteration loop logic
- No changes to PRD parsing or file handling
- No changes to PTY spawning or management
- No changes to Claude Code integration
- No changes to command-line argument parsing
- No new features or functionality beyond visual redesign

## Design Considerations

### Reference Image
See `tui-design.png` in this directory for the target visual design.

### Reference Design Elements
From the mockup images:
- Deep space blue-black backgrounds (#0a0e14)
- Card-based layout with rounded corners
- Green pulsing status dots for active items
- Progress bars inside cards
- Traffic light window chrome on terminal panel
- ASCII art banner
- Timestamped, color-coded log output
- Muted section headers with icons

### Unicode Characters to Use
- Rounded corners: ╭ ╮ ╰ ╯
- Lines: │ ─
- Status dots: ● ○
- Icons: ✦ ↳ ⏱ 💰 (or ASCII alternatives)
- Spinner: ⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏
- Checkmarks: ✓ ✗

### Ratatui Components to Leverage
- `Block` with custom `BorderType::Rounded`
- `Paragraph` for text content
- `Gauge` for progress bars (styled)
- `Layout` for spacing and arrangement
- Custom `Span` styling for colors

## Technical Considerations

- Ratatui 0.30 supports custom border types
- Animation state needs to be tracked in App struct
- Frame timing for animations (~16ms for 60fps)
- Color definitions should use `Color::Rgb(r, g, b)` for exact palette matching
- Consider extracting UI rendering into separate module for clarity

## Success Metrics

- Visual appearance closely matches reference mockups
- No regression in existing functionality
- Animations run smoothly without lag
- UI remains responsive during Claude Code execution
- Code is well-organized with clear separation of theme/layout concerns

## Open Questions

- Should we add a configuration option for users who prefer simpler styling?
- Should animation speed be configurable?
- How should we handle terminals that don't support full RGB colors?
