use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, TableState};

use crate::tui::app::App;
use crate::tui::widgets::key_matrix;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // header
            Constraint::Min(8),    // key matrix
            Constraint::Length(3), // selected key info
            Constraint::Min(6),    // mapping table
        ])
        .split(area);

    // Header
    let device = app.device_name().unwrap_or("(none)");
    let preset = app.preset_name().unwrap_or("(none)");
    let modified = if app.unsaved_changes { " [modified]" } else { "" };
    let header = Paragraph::new(format!(
        "  Device: {}  |  Preset: {}{}",
        device, preset, modified
    ))
    .style(Style::default().fg(Color::Cyan));
    frame.render_widget(header, chunks[0]);

    // Key matrix
    if let Some(ref matrix) = app.key_matrix {
        key_matrix::render(frame, chunks[1], matrix, device);

        // Selected key info bar
        if let Some(key_info) = matrix.selected_key() {
            let mapping = app.entries.iter().find(|e| {
                e.input_combination
                    .first()
                    .is_some_and(|i| i.code == key_info.code)
            });
            let info = if let Some(entry) = mapping {
                let output = entry.output_symbol.as_deref().unwrap_or("(not set)");
                format!(
                    "  {} (code {})  ->  {}",
                    key_info.name, key_info.code, output
                )
            } else {
                format!(
                    "  {} (code {})  [no mapping]",
                    key_info.name, key_info.code
                )
            };

            let style = if mapping.is_some() {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let info_bar = Paragraph::new(info)
                .block(Block::default().borders(Borders::TOP))
                .style(style);
            frame.render_widget(info_bar, chunks[2]);
        }
    } else {
        let placeholder = Paragraph::new("  No device keys available")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(placeholder, chunks[1]);
    }

    // Mapping table (compact)
    let header_row = Row::new(vec!["Input", "Output"])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1);

    let rows: Vec<Row> = app
        .entries
        .iter()
        .map(|entry| {
            let input = entry
                .name
                .as_deref()
                .unwrap_or_else(|| {
                    entry
                        .input_combination
                        .first()
                        .map(|_| "key")
                        .unwrap_or("?")
                })
                .to_string();

            let input_detail = entry
                .input_combination
                .first()
                .map(|i| format!("{} ({})", input, i.code))
                .unwrap_or(input);

            let output = entry
                .output_symbol
                .as_deref()
                .unwrap_or("(not set)")
                .to_string();

            Row::new(vec![input_detail, output])
        })
        .collect();

    let widths = [Constraint::Percentage(50), Constraint::Percentage(50)];

    let table = Table::new(rows, widths)
        .header(header_row)
        .block(
            Block::default()
                .title(" Mappings ")
                .borders(Borders::ALL),
        )
        .row_highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .highlight_symbol("> ");

    let mut state = TableState::default();
    if !app.entries.is_empty() {
        state.select(Some(app.selected_entry));
    }

    frame.render_stateful_widget(table, chunks[3], &mut state);
}
