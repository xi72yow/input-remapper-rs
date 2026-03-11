use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// Render a centered popup with a title and body text.
pub fn render_popup(frame: &mut Frame, title: &str, body: &str, area: Rect) {
    let popup_area = centered_rect(60, 30, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let text = Paragraph::new(body)
        .block(block)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);

    frame.render_widget(text, popup_area);
}

/// Render a text input popup.
pub fn render_input_popup(frame: &mut Frame, title: &str, value: &str, area: Rect) {
    let popup_area = centered_rect(50, 20, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let display = if value.is_empty() {
        "Type a name...".to_string()
    } else {
        format!("{}|", value)
    };

    let text = Paragraph::new(display)
        .block(block)
        .style(Style::default().fg(if value.is_empty() {
            Color::DarkGray
        } else {
            Color::White
        }));

    frame.render_widget(text, popup_area);
}

/// Calculate a centered rect of given percentage width and height.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let layout_y = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    let layout_x = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(layout_y[1]);

    layout_x[1]
}
