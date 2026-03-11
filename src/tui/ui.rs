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

    // Tab bar
    let titles: Vec<Line> = Screen::ALL
        .iter()
        .map(|s| {
            if *s == app.screen {
                Line::from(s.title()).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                Line::from(s.title()).style(Style::default().fg(Color::DarkGray))
            }
        })
        .collect();

    let tabs = Tabs::new(titles)
        .select(app.screen.index())
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .divider(" | ");
    frame.render_widget(tabs, chunks[0]);

    // Main content
    match app.screen {
        Screen::Devices => screens::devices::render(frame, chunks[1], app),
        Screen::Presets => screens::presets::render(frame, chunks[1], app),
        Screen::Editor => screens::editor::render(frame, chunks[1], app),
        Screen::Status => screens::status::render(frame, chunks[1], app),
    }

    // Help bar
    help_bar::render(frame, chunks[2], app.screen, &app.overlay);

    // Error/status bar
    if let Some(ref msg) = app.error {
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
            screens::record::render(frame, frame.area(), events, *selected);
        }
        Overlay::SymbolSearch {
            query,
            filtered,
            cursor,
            combination,
        } => {
            screens::symbols::render(frame, frame.area(), query, filtered, *cursor, combination);
        }
        Overlay::TextInput { title, value, .. } => {
            popup::render_input_popup(frame, title, value, frame.area());
        }
        Overlay::Confirm { title, message, .. } => {
            popup::render_popup(frame, title, message, frame.area());
        }
    }
}
