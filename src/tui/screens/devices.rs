use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::tui::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .devices
        .iter()
        .map(|d| {
            ListItem::new(format!(
                "{}  (vendor: {:04x}, product: {:04x})",
                d.name, d.vendor, d.product
            ))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Devices ")
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .highlight_symbol("> ");

    let mut state = ListState::default();
    if !app.devices.is_empty() {
        state.select(Some(app.selected_device));
    }

    frame.render_stateful_widget(list, area, &mut state);
}
