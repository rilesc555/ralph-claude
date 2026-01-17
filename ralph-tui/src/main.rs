use std::io::{self, stdout};

use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

fn main() -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Run the app
    let result = run(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    loop {
        terminal.draw(|frame| {
            let area = frame.area();

            // Create main layout: content area + bottom bar
            let main_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),    // Main content area
                    Constraint::Length(1), // Bottom bar (single line)
                ])
                .split(area);

            let content_area = main_layout[0];
            let bottom_bar_area = main_layout[1];

            // Create horizontal split: 30% left panel, 70% right panel
            let panels = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(30), // Ralph Status panel
                    Constraint::Percentage(70), // Claude Code panel
                ])
                .split(content_area);

            let left_panel_area = panels[0];
            let right_panel_area = panels[1];

            // Left panel: Ralph Status
            let left_block = Block::default()
                .title(" Ralph Status ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            let left_content = Paragraph::new("Status information will appear here.")
                .block(left_block)
                .style(Style::default().fg(Color::White));

            frame.render_widget(left_content, left_panel_area);

            // Right panel: Claude Code
            let right_block = Block::default()
                .title(" Claude Code ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            let right_content = Paragraph::new("Claude Code output will appear here.")
                .block(right_block)
                .style(Style::default().fg(Color::White));

            frame.render_widget(right_content, right_panel_area);

            // Bottom bar with keybinding hints
            let keybindings = Paragraph::new(" q: Quit | i/Tab: Claude Mode | Esc: Ralph Mode ")
                .style(Style::default().fg(Color::Black).bg(Color::Cyan));

            frame.render_widget(keybindings, bottom_bar_area);
        })?;

        // Handle input
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    Ok(())
}
