use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, TableState};

use crate::tui::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(30),          // left: header + mapping table
            Constraint::Length(34),        // right: detail/legend panel
        ])
        .split(area);

    render_left(frame, main_chunks[0], app);
    render_right(frame, main_chunks[1], app);
}

fn render_left(frame: &mut Frame, area: Rect, app: &App) {
    let device = app.device_name().unwrap_or("(none)");
    let preset = app.preset_name().unwrap_or("(none)");
    let modified = if app.unsaved_changes { " *" } else { "" };

    // Responsive: single line if wide enough, two lines otherwise
    let one_line = format!(" {} | {}{}", device, preset, modified);
    let header_height = if area.width >= one_line.len() as u16 + 2 { 2 } else { 3 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(6),
        ])
        .split(area);

    let header_lines = if header_height <= 2 {
        vec![Line::from(vec![
            Span::styled(format!(" {} ", device), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("| ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", preset), Style::default().fg(Color::Cyan)),
            Span::styled(modified, Style::default().fg(Color::Yellow)),
        ])]
    } else {
        vec![
            Line::from(vec![
                Span::styled(" D: ", Style::default().fg(Color::DarkGray)),
                Span::styled(device, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled(" P: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{}{}", preset, modified), Style::default().fg(Color::Cyan)),
            ]),
        ]
    };

    let header = Paragraph::new(header_lines);
    frame.render_widget(header, chunks[0]);

    // Mapping table
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
            let input_name = entry
                .input_combination
                .first()
                .map(|i| {
                    if i.event_type == 1 {
                        format!("{:?}", evdev::KeyCode(i.code))
                    } else {
                        format!("{}:{}", i.event_type, i.code)
                    }
                })
                .unwrap_or_else(|| "?".into());

            let output = entry
                .output_symbol
                .as_deref()
                .unwrap_or("(not set)")
                .to_string();

            Row::new(vec![input_name, output])
        })
        .collect();

    let empty_msg = if app.entries.is_empty() {
        " No mappings \u{2014} press 'a' to record a key "
    } else {
        " Mappings "
    };

    let widths = [Constraint::Percentage(50), Constraint::Percentage(50)];

    let table = Table::new(rows, widths)
        .header(header_row)
        .block(
            Block::default()
                .title(empty_msg)
                .borders(Borders::ALL),
        )
        .row_highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .highlight_symbol("> ");

    let mut state = TableState::default();
    if !app.entries.is_empty() {
        state.select(Some(app.selected_entry));
    }

    frame.render_stateful_widget(table, chunks[1], &mut state);
}

fn render_right(frame: &mut Frame, area: Rect, app: &App) {
    let detail_text = if !app.entries.is_empty() {
        let entry = &app.entries[app.selected_entry];

        let input_parts: Vec<String> = entry
            .input_combination
            .iter()
            .map(|i| {
                if i.event_type == 1 {
                    format!("{:?} ({})", evdev::KeyCode(i.code), i.code)
                } else {
                    format!("type:{} code:{}", i.event_type, i.code)
                }
            })
            .collect();

        let output = entry
            .output_symbol
            .as_deref()
            .unwrap_or("(not set)");

        let mut lines = vec![
            Line::from(Span::styled("Input:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        ];
        for part in &input_parts {
            lines.push(Line::from(format!("  {}", part)));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Output:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
        lines.push(Line::from(format!("  {}", output)));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Target:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
        lines.push(Line::from(format!("  {}", entry.target_uinput)));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Type:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
        lines.push(Line::from(format!("  {}", entry.mapping_type)));

        if let Some(ref name) = entry.name {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("Name:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
            lines.push(Line::from(format!("  {}", name)));
        }

        lines
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                " No mapping selected",
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
    frame.render_widget(detail, area);
}
