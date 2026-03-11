use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::ipc::protocol::RecordEvent;
use crate::mapping::config::MappingEntry;
use crate::tui::widgets::popup::centered_rect;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    events: &[RecordEvent],
    selected: usize,
    entries: &[MappingEntry],
) {
    let popup_area = centered_rect(70, 60, area);
    frame.render_widget(Clear, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // instruction
            Constraint::Min(0),   // content (list + detail)
        ])
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

    // Content area: list left, detail right
    let content = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55), // event list
            Constraint::Percentage(45), // detail panel
        ])
        .split(chunks[1]);

    // Captured events — mark already-mapped keys
    let items: Vec<ListItem> = events
        .iter()
        .map(|ev| {
            let is_mapped = entries.iter().any(|entry| {
                entry
                    .input_combination
                    .iter()
                    .any(|input| input.event_type == ev.event_type && input.code == ev.code)
            });
            let label = if is_mapped {
                format!("{} [mapped]", ev.code_name)
            } else {
                ev.code_name.clone()
            };
            let style = if is_mapped {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            ListItem::new(label).style(style)
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

    frame.render_stateful_widget(list, content[0], &mut state);

    // Detail panel for selected event
    let detail_text = if let Some(ev) = events.get(selected) {
        let existing = entries.iter().find(|entry| {
            entry
                .input_combination
                .iter()
                .any(|input| input.event_type == ev.event_type && input.code == ev.code)
        });

        let mut lines = vec![
            Line::from(Span::styled(
                "Key:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(format!("  {}", ev.code_name)),
            Line::from(""),
            Line::from(Span::styled(
                "Type:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(format!("  {} (code {})", ev.code_name, ev.code)),
            Line::from(""),
            Line::from(Span::styled(
                "Event Type:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(format!("  {}", match ev.event_type {
                1 => "KEY",
                2 => "REL (relative axis)",
                3 => "ABS (absolute axis)",
                _ => "other",
            })),
        ];

        if let Some(entry) = existing {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Existing Mapping:",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(format!(
                "  -> {}",
                entry.output_symbol.as_deref().unwrap_or("(not set)")
            )));
        }

        lines
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                " No event selected",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    };

    let detail = Paragraph::new(detail_text).block(
        Block::default()
            .title(" Detail ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(detail, content[1]);
}
