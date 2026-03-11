use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::tui::widgets::popup::centered_rect;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    query: &str,
    filtered: &[(String, u16)],
    cursor: usize,
    slots: &[String],
    slot_cursor: usize,
) {
    let popup_area = centered_rect(65, 70, area);
    frame.render_widget(Clear, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // chain display
            Constraint::Length(3), // search input
            Constraint::Min(0),   // results list
        ])
        .split(popup_area);

    // Chain display: [ Key1 ] + [ Key2 ] + [ [+] ]
    let mut spans = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" + ", Style::default().fg(Color::DarkGray)));
        }
        let style = if i == slot_cursor {
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
    let add_style = if slot_cursor == slots.len() {
        Style::default().bg(Color::Green).fg(Color::Black)
    } else {
        Style::default().fg(Color::Green)
    };
    spans.push(Span::styled(" [+] ", add_style));

    let chain = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .title(" Chain ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .alignment(Alignment::Center);
    frame.render_widget(chain, chunks[0]);

    // Search input
    let display = if query.is_empty() {
        "Type to search symbols...".to_string()
    } else {
        format!("{}|", query)
    };

    let input = Paragraph::new(display)
        .block(
            Block::default()
                .title(" Symbol Search ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().fg(if query.is_empty() {
            Color::DarkGray
        } else {
            Color::White
        }));
    frame.render_widget(input, chunks[1]);

    // Results list
    let items: Vec<ListItem> = filtered
        .iter()
        .take(50)
        .map(|(name, code)| {
            ListItem::new(format!("{:<30} (code {})", name, code))
        })
        .collect();

    let count_info = format!(" Results ({}) ", filtered.len());
    let list = List::new(items)
        .block(
            Block::default()
                .title(count_info)
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black))
        .highlight_symbol("> ");

    let mut state = ListState::default();
    if !filtered.is_empty() {
        state.select(Some(cursor));
    }

    frame.render_stateful_widget(list, chunks[2], &mut state);
}
