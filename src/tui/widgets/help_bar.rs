use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::tui::app::{Overlay, Screen};

pub fn render(frame: &mut Frame, area: Rect, screen: Screen, overlay: &Overlay) {
    let keys = match overlay {
        Overlay::None => match screen {
            Screen::Devices => "[Enter] Select  [r] Refresh  [Tab] Next  [q] Quit",
            Screen::Presets => "[Enter] Edit  [n] New  [d] Delete  [r] Refresh  [Tab] Next  [q] Quit",
            Screen::Editor => {
                "[Arrows] Navigate  [Enter] Map key  [d] Delete  [a] Record  [s] Save  [p] Apply  [q] Quit"
            }
            Screen::Status => "[s] Stop  [S] Stop All  [r] Refresh  [Tab] Next  [q] Quit",
        },
        Overlay::Record { .. } => "[Enter] Use selected  [Esc] Cancel",
        Overlay::SymbolSearch { combination, .. } => {
            if combination.is_empty() {
                "[Enter] Select  [+] Add modifier  [Esc] Cancel"
            } else {
                "[Enter] Select  [+] Add more  [Esc] Cancel"
            }
        }
        Overlay::TextInput { .. } => "[Enter] Confirm  [Esc] Cancel",
        Overlay::Confirm { .. } => "[y/Enter] Yes  [n/Esc] No",
    };

    let bar = Paragraph::new(keys)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .alignment(Alignment::Center);
    frame.render_widget(bar, area);
}
