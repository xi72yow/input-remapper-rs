use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::tui::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .injections
        .iter()
        .map(|inj| {
            ListItem::new(format!(
                "{}  ->  {}  [running]",
                inj.device, inj.preset
            ))
        })
        .collect();

    let list = if items.is_empty() {
        List::new(vec![ListItem::new("No active injections")])
            .block(
                Block::default()
                    .title(" Active Injections ")
                    .borders(Borders::ALL),
            )
            .style(Style::default().fg(Color::DarkGray))
    } else {
        List::new(items)
            .block(
                Block::default()
                    .title(" Active Injections ")
                    .borders(Borders::ALL),
            )
            .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
            .highlight_symbol("> ")
    };

    let mut state = ListState::default();
    if !app.injections.is_empty() {
        state.select(Some(app.selected_injection));
    }

    frame.render_stateful_widget(list, area, &mut state);
}
