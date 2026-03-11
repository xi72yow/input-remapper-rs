use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::tui::widgets::popup::centered_rect;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    query: &str,
    filtered: &[(String, u16)],
    cursor: usize,
    combination: &[String],
) {
    let popup_area = centered_rect(60, 60, area);
    frame.render_widget(Clear, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // search input
            Constraint::Length(2), // combination display
            Constraint::Min(0),   // results list
        ])
        .split(popup_area);

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
    frame.render_widget(input, chunks[0]);

    // Combination display
    if !combination.is_empty() {
        let combo_text = format!("  Building: {} + ...", combination.join(" + "));
        let combo = Paragraph::new(combo_text)
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(combo, chunks[1]);
    }

    // Results list
    let items: Vec<ListItem> = filtered
        .iter()
        .take(50) // limit display for performance
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
