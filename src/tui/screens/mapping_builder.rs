use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::tui::widgets::popup::centered_rect;

pub fn render(frame: &mut Frame, area: Rect, slots: &[String], cursor: usize) {
    let popup_area = centered_rect(60, 20, area);
    frame.render_widget(Clear, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title / instruction
            Constraint::Min(3),   // slot chain
        ])
        .split(popup_area);

    // Title
    let instruction = Paragraph::new("Build key combination (macro)")
        .block(
            Block::default()
                .title(" Mapping Builder ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(instruction, chunks[0]);

    // Build the slot chain display: [ Key1 ] + [ Key2 ] + [ + ]
    let mut spans = Vec::new();

    for (i, slot) in slots.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" + ", Style::default().fg(Color::DarkGray)));
        }

        let style = if i == cursor {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        } else {
            Style::default().fg(Color::White)
        };

        spans.push(Span::styled(format!(" {} ", slot), style));
    }

    // [+] button
    if !slots.is_empty() {
        spans.push(Span::styled(" + ", Style::default().fg(Color::DarkGray)));
    }

    let add_style = if cursor == slots.len() {
        Style::default().bg(Color::Green).fg(Color::Black)
    } else {
        Style::default().fg(Color::Green)
    };
    spans.push(Span::styled(" [+] ", add_style));

    let chain = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .title(" Chain ")
                .borders(Borders::ALL),
        )
        .alignment(Alignment::Center);
    frame.render_widget(chain, chunks[1]);
}
