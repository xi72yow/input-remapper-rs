use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// Render a centered confirm popup with a title, body text, and Yes/No buttons.
pub fn render_confirm_popup(frame: &mut Frame, title: &str, body: &str, selected_no: bool, area: Rect) {
    let popup_area = centered_rect(50, 30, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // message
            Constraint::Length(1), // buttons
        ])
        .split(inner);

    let text = Paragraph::new(body)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    frame.render_widget(text, chunks[0]);

    let yes_style = if !selected_no {
        Style::default().bg(Color::Green).fg(Color::Black)
    } else {
        Style::default().fg(Color::Green)
    };
    let no_style = if selected_no {
        Style::default().bg(Color::Red).fg(Color::White)
    } else {
        Style::default().fg(Color::Red)
    };

    let buttons = Line::from(vec![
        Span::styled(" Yes ", yes_style),
        Span::raw("  "),
        Span::styled(" No ", no_style),
    ]);
    let buttons_p = Paragraph::new(buttons).alignment(Alignment::Center);
    frame.render_widget(buttons_p, chunks[1]);
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
