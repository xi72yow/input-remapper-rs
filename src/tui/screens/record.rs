use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::ipc::protocol::RecordEvent;
use crate::tui::widgets::popup::centered_rect;

pub fn render(frame: &mut Frame, area: Rect, events: &[RecordEvent], selected: usize) {
    let popup_area = centered_rect(60, 50, area);
    frame.render_widget(Clear, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(popup_area);

    // Instruction
    let block = Block::default()
        .title(" Record Mode ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    let instruction = Paragraph::new("Press a button on the device...")
        .block(block)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Green));
    frame.render_widget(instruction, chunks[0]);

    // Captured events
    let items: Vec<ListItem> = events
        .iter()
        .map(|ev| {
            ListItem::new(format!(
                "{}  (type={}, code={})",
                ev.code_name, ev.event_type, ev.code
            ))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Captured Events ")
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().bg(Color::Green).fg(Color::Black))
        .highlight_symbol("> ");

    let mut state = ListState::default();
    if !events.is_empty() {
        state.select(Some(selected));
    }

    frame.render_stateful_widget(list, chunks[1], &mut state);
}
