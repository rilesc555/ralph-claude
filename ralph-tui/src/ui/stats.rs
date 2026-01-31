//! Stat card rendering functions

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::theme::{BG_SECONDARY, BORDER_SUBTLE, CYAN_PRIMARY, ROUNDED_BORDERS, TEXT_MUTED};

/// Render iteration and completion stat cards in a given area
/// Returns the widgets to be rendered: (left_card, right_card)
pub fn render_stat_cards(
    area: Rect,
    current_iteration: u32,
    max_iterations: u32,
    completed: usize,
    total: usize,
    frame: &mut Frame,
) {
    // Split area horizontally for two cards with a small gap
    let card_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left card: Iterations
    let iter_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ROUNDED_BORDERS)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(BG_SECONDARY));

    let iter_content = vec![
        Line::from(vec![
            Span::styled("  ", Style::default().fg(CYAN_PRIMARY)),
            Span::styled(
                format!("{}/{}", current_iteration, max_iterations),
                Style::default()
                    .fg(CYAN_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![Span::styled(
            "ITERATIONS",
            Style::default().fg(TEXT_MUTED),
        )]),
    ];

    let iter_paragraph = Paragraph::new(iter_content)
        .block(iter_block)
        .alignment(Alignment::Center);

    frame.render_widget(iter_paragraph, card_layout[0]);

    // Right card: Completed
    let comp_block = Block::default()
        .borders(Borders::ALL)
        .border_set(ROUNDED_BORDERS)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(BG_SECONDARY));

    let comp_content = vec![
        Line::from(vec![
            Span::styled("  ", Style::default().fg(CYAN_PRIMARY)),
            Span::styled(
                format!("{}/{}", completed, total),
                Style::default()
                    .fg(CYAN_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![Span::styled(
            "COMPLETED",
            Style::default().fg(TEXT_MUTED),
        )]),
    ];

    let comp_paragraph = Paragraph::new(comp_content)
        .block(comp_block)
        .alignment(Alignment::Center);

    frame.render_widget(comp_paragraph, card_layout[1]);
}
