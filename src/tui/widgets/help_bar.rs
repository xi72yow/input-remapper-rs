use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::tui::app::{Overlay, Screen};

/// Key hint with a key label and description.
struct Hint {
    key: &'static str,
    desc: &'static str,
    color: Color,
}

impl Hint {
    fn new(key: &'static str, desc: &'static str, color: Color) -> Self {
        Self { key, desc, color }
    }
}

fn hints_to_line(hints: &[Hint]) -> Line<'static> {
    let bg = Color::DarkGray;
    let mut spans = Vec::new();
    for (i, h) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default().bg(bg)));
        }
        spans.push(Span::styled(
            h.key.to_string(),
            Style::default().fg(h.color).bg(bg).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {}", h.desc),
            Style::default().fg(Color::White).bg(bg),
        ));
    }
    Line::from(spans)
}

pub fn render(frame: &mut Frame, area: Rect, screen: Screen, overlay: &Overlay) {
    let key = Color::Yellow;
    let nav = Color::Cyan;
    let danger = Color::Red;
    let good = Color::Green;

    let line = match overlay {
        Overlay::None => match screen {
            Screen::Devices => hints_to_line(&[
                Hint::new("Enter", "Select", nav),
                Hint::new("r", "Refresh", key),
                Hint::new("Tab", "Status", nav),
                Hint::new("q", "Quit", danger),
            ]),
            Screen::Presets => hints_to_line(&[
                Hint::new("Enter", "Edit", nav),
                Hint::new("n", "New", good),
                Hint::new("d", "Delete", danger),
                Hint::new("Esc", "Back", nav),
                Hint::new("Tab", "Status", nav),
                Hint::new("q", "Quit", danger),
            ]),
            Screen::Editor => hints_to_line(&[
                Hint::new("a", "Add", good),
                Hint::new("Enter", "Edit", nav),
                Hint::new("n", "Rename", key),
                Hint::new("d", "Delete", danger),
                Hint::new("s", "Save", good),
                Hint::new("p", "Apply", key),
                Hint::new("Esc", "Back", nav),
                Hint::new("q", "Quit", danger),
            ]),
            Screen::Status => hints_to_line(&[
                Hint::new("s", "Stop", danger),
                Hint::new("S", "Stop All", danger),
                Hint::new("r", "Refresh", key),
                Hint::new("Tab", "Config", nav),
                Hint::new("q", "Quit", danger),
            ]),
        },
        Overlay::Record { .. } => hints_to_line(&[
            Hint::new("Enter", "Use selected", good),
            Hint::new("Esc", "Cancel", danger),
        ]),
        Overlay::SymbolSearch { .. } => hints_to_line(&[
            Hint::new("Enter", "Select", good),
            Hint::new("←/→", "Navigate chain", nav),
            Hint::new("Del", "Remove", danger),
            Hint::new("Esc", "Save & Close", nav),
        ]),
        Overlay::TextInput { .. } => hints_to_line(&[
            Hint::new("Enter", "Confirm", good),
            Hint::new("Esc", "Cancel", danger),
        ]),
        Overlay::Confirm { .. } => hints_to_line(&[
            Hint::new("←/→", "Select", nav),
            Hint::new("Enter", "Confirm", good),
            Hint::new("y", "Yes", good),
            Hint::new("n/Esc", "No", danger),
        ]),
    };

    let bar = Paragraph::new(line)
        .style(Style::default().bg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(bar, area);
}
