//! Story card rendering functions

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge, Paragraph},
};

use crate::models::StoryState;
use crate::theme::{
    get_pulse_color, BG_SECONDARY, BG_TERTIARY, BORDER_SUBTLE, CYAN_DIM, CYAN_PRIMARY,
    GREEN_ACTIVE, GREEN_SUCCESS, ROUNDED_BORDERS, TEXT_MUTED, TEXT_SECONDARY,
};

/// Render a single user story card
/// Returns the height of the card:
/// - Completed/Pending: 3 lines (border + content + border)
/// - Active: 5 lines (border + title + progress bar + percentage + border)
pub fn render_story_card(
    area: Rect,
    story_id: &str,
    story_title: &str,
    state: StoryState,
    tick: u64,
    progress_percent: u16,
    criteria_passed: usize,
    criteria_total: usize,
    selected: bool,
    frame: &mut Frame,
) {
    // Determine colors based on state
    // For active state, use pulsing indicator color
    let (indicator, indicator_color, text_color, bg_color) = match state {
        StoryState::Completed => ("●", GREEN_SUCCESS, CYAN_PRIMARY, BG_SECONDARY),
        StoryState::Active => {
            let pulse_color = get_pulse_color(tick, GREEN_ACTIVE, CYAN_DIM);
            ("●", pulse_color, CYAN_PRIMARY, BG_TERTIARY)
        }
        StoryState::Pending => ("○", TEXT_MUTED, TEXT_SECONDARY, BG_SECONDARY),
    };

    // Use highlight border for selected card, normal for others
    let border_color = if selected { CYAN_PRIMARY } else { BORDER_SUBTLE };

    // Create card block with rounded borders
    let card_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ROUNDED_BORDERS)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    // Format story ID as #XX (extract numeric part)
    let story_num = story_id.trim_start_matches(|c: char| !c.is_ascii_digit());
    let formatted_id = format!("#{}", story_num);

    // Build card content - single line with indicator, ID, and truncated title
    let inner_width = area.width.saturating_sub(4) as usize; // Account for borders and padding
    let prefix = format!("{} {} ", indicator, formatted_id);
    let prefix_len = prefix.chars().count();
    let available_title_width = inner_width.saturating_sub(prefix_len);

    let title_char_count = story_title.chars().count();
    let truncated_title = if title_char_count > available_title_width {
        // Safely truncate using character boundaries
        let take_chars = available_title_width.saturating_sub(3);
        let truncated: String = story_title.chars().take(take_chars).collect();
        format!("{}...", truncated)
    } else {
        story_title.to_string()
    };

    let title_line = Line::from(vec![
        Span::styled(
            format!("{} ", indicator),
            Style::default().fg(indicator_color),
        ),
        Span::styled(
            format!("{} ", formatted_id),
            Style::default()
                .fg(text_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(truncated_title, Style::default().fg(text_color)),
    ]);

    // For active state, show progress bar and percentage
    if state == StoryState::Active {
        // Render block first to get inner area
        let inner_area = card_block.inner(area);
        frame.render_widget(card_block, area);

        // Split inner area: title (1 line), progress bar (1 line), percentage (1 line)
        let inner_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Title line
                Constraint::Length(1), // Progress bar
                Constraint::Length(1), // Percentage
            ])
            .split(inner_area);

        // Render title
        let title_paragraph = Paragraph::new(vec![title_line]);
        frame.render_widget(title_paragraph, inner_layout[0]);

        // Render progress bar (Gauge widget)
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(CYAN_PRIMARY).bg(BG_SECONDARY))
            .percent(progress_percent)
            .label(""); // No label on the gauge itself
        frame.render_widget(gauge, inner_layout[1]);

        // Render criteria count below the progress bar (e.g., "2/5 criteria")
        let criteria_text = format!(
            "{}/{} criteria ({}%)",
            criteria_passed, criteria_total, progress_percent
        );
        let percent_line = Line::from(Span::styled(criteria_text, Style::default().fg(TEXT_MUTED)));
        let percent_paragraph = Paragraph::new(vec![percent_line]);
        frame.render_widget(percent_paragraph, inner_layout[2]);
    } else {
        // Completed and Pending states - simple single line card
        let paragraph = Paragraph::new(vec![title_line]).block(card_block);
        frame.render_widget(paragraph, area);
    }
}

/// Render progress stat cards (stories left + completion %) in a given area
pub fn render_progress_cards(area: Rect, completed: usize, total: usize, frame: &mut Frame) {
    // Split area horizontally for two cards
    let card_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left card: Stories Left
    let stories_left = total.saturating_sub(completed);
    let left_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ROUNDED_BORDERS)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(BG_SECONDARY));

    let left_content = vec![
        Line::from(vec![
            Span::styled("  ", Style::default().fg(CYAN_PRIMARY)),
            Span::styled(
                format!("{}", stories_left),
                Style::default()
                    .fg(CYAN_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![Span::styled(
            "STORIES LEFT",
            Style::default().fg(TEXT_MUTED),
        )]),
    ];

    let left_paragraph = Paragraph::new(left_content)
        .block(left_block)
        .alignment(Alignment::Center);

    frame.render_widget(left_paragraph, card_layout[0]);

    // Right card: Progress percentage
    let progress_pct = if total > 0 {
        (completed as f32 / total as f32 * 100.0) as u8
    } else {
        0
    };

    let right_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ROUNDED_BORDERS)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(BG_SECONDARY));

    let progress_color = if progress_pct == 100 {
        GREEN_SUCCESS
    } else {
        CYAN_PRIMARY
    };

    let right_content = vec![
        Line::from(vec![
            Span::styled("  ", Style::default().fg(progress_color)),
            Span::styled(
                format!("{}%", progress_pct),
                Style::default()
                    .fg(progress_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![Span::styled(
            "PROGRESS",
            Style::default().fg(TEXT_MUTED),
        )]),
    ];

    let right_paragraph = Paragraph::new(right_content)
        .block(right_block)
        .alignment(Alignment::Center);

    frame.render_widget(right_paragraph, card_layout[1]);
}
