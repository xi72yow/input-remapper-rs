use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::tui::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(area);

    // Device header
    let device_name = app.device_name().unwrap_or("(none)");
    let header = Paragraph::new(format!("  Device: {}", device_name))
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(header, chunks[0]);

    // Preset list
    let items: Vec<ListItem> = app
        .presets
        .iter()
        .map(|p| ListItem::new(p.as_str()))
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Presets ")
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .highlight_symbol("> ");

    let mut state = ListState::default();
    if !app.presets.is_empty() {
        state.select(Some(app.selected_preset));
    }

    frame.render_stateful_widget(list, chunks[1], &mut state);
}
