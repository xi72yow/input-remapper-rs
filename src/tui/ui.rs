use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Tabs};

use super::app::{App, Overlay, Screen};
use super::screens;
use super::widgets::{help_bar, popup};

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // tab bar
            Constraint::Min(0),   // content
            Constraint::Length(1), // help bar
            Constraint::Length(1), // error/status bar
        ])
        .split(frame.area());

    // Tab bar: Config | Status  +  breadcrumb
    let config_style = if app.screen.is_config() {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let status_style = if app.screen == Screen::Status {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let titles = vec![
        Line::from("Config").style(config_style),
        Line::from("Status").style(status_style),
    ];

    let tab_select = if app.screen.is_config() { 0 } else { 1 };
    let tabs = Tabs::new(titles)
        .select(tab_select)
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .divider(" | ");

    // Render tabs + breadcrumb side by side
    let tab_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20), // tabs
            Constraint::Min(0),    // breadcrumb
        ])
        .split(chunks[0]);

    frame.render_widget(tabs, tab_area[0]);

    let breadcrumb = Paragraph::new(app.screen.config_breadcrumb())
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Right);
    frame.render_widget(breadcrumb, tab_area[1]);

    // Main content
    match app.screen {
        Screen::Devices => screens::devices::render(frame, chunks[1], app),
        Screen::Presets => screens::presets::render(frame, chunks[1], app),
        Screen::Editor => screens::editor::render(frame, chunks[1], app),
        Screen::Status => screens::status::render(frame, chunks[1], app),
    }

    // Help bar
    help_bar::render(frame, chunks[2], app.screen, &app.overlay);

    // Error/status/loading bar
    if let Some(loading_msg) = app.loading {
        const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let frame_char = SPINNER[app.tick % SPINNER.len()];
        let bar = Paragraph::new(format!("  {} {} ", frame_char, loading_msg))
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(bar, chunks[3]);
    } else if let Some(ref msg) = app.error {
        let style = if msg.starts_with("Saved") || msg.starts_with("Started") || msg.starts_with("Stopped") || msg.starts_with("Autoloaded") {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };
        let bar = Paragraph::new(format!("  {}", msg)).style(style);
        frame.render_widget(bar, chunks[3]);
    }

    // Overlays
    match &app.overlay {
        Overlay::None => {}
        Overlay::Record { events, selected } => {
            screens::record::render(frame, frame.area(), events, *selected, &app.entries);
        }
        Overlay::SymbolSearch {
            query,
            filtered,
            cursor,
            slots,
            slot_cursor,
        } => {
            screens::symbols::render(frame, frame.area(), query, filtered, *cursor, slots, *slot_cursor);
        }
        Overlay::TextInput { title, value, .. } => {
            popup::render_input_popup(frame, title, value, frame.area());
        }
        Overlay::Confirm { title, message, selected_no, .. } => {
            popup::render_confirm_popup(frame, title, message, *selected_no, frame.area());
        }
    }
}
