use ratatui::prelude::*;
use ratatui::widgets::canvas::{Canvas, Context, Rectangle};
use ratatui::widgets::{Block, Borders};

use crate::ipc::protocol::KeyInfoResponse;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyState {
    Available,
    Mapped,
    Selected,
    Pressed,
}

pub struct KeyMatrixState {
    pub keys: Vec<KeyInfoResponse>,
    pub mapped_codes: Vec<u16>,
    pub selected: usize,
    pub pressed_codes: Vec<u16>,
}

impl KeyMatrixState {
    pub fn key_state(&self, index: usize) -> KeyState {
        let code = self.keys[index].code;
        if self.pressed_codes.contains(&code) {
            KeyState::Pressed
        } else if index == self.selected {
            KeyState::Selected
        } else if self.mapped_codes.contains(&code) {
            KeyState::Mapped
        } else {
            KeyState::Available
        }
    }

    pub fn selected_key(&self) -> Option<&KeyInfoResponse> {
        self.keys.get(self.selected)
    }

    pub fn move_left(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.selected + 1 < self.keys.len() {
            self.selected += 1;
        }
    }

    pub fn move_up(&mut self, cols: usize) {
        if self.selected >= cols {
            self.selected -= cols;
        }
    }

    pub fn move_down(&mut self, cols: usize) {
        if self.selected + cols < self.keys.len() {
            self.selected += cols;
        }
    }
}

/// Calculate how many columns fit in the given width.
/// Each key cell is `cell_w` wide with 1 unit gap.
fn grid_cols(area_width: u16, cell_w: f64) -> usize {
    let gap = 1.0;
    let usable = area_width as f64;
    let cols = ((usable + gap) / (cell_w + gap)).floor() as usize;
    cols.max(1)
}

pub fn render(frame: &mut Frame, area: Rect, state: &KeyMatrixState, title: &str) {
    if state.keys.is_empty() {
        let block = Block::default()
            .title(format!(" {} (no keys) ", title))
            .borders(Borders::ALL);
        frame.render_widget(block, area);
        return;
    }

    let cell_w = 3.0;
    let cell_h = 2.0;
    let gap = 1.0;

    let cols = grid_cols(area.width.saturating_sub(2), cell_w);
    let rows = (state.keys.len() + cols - 1) / cols;

    let canvas_w = cols as f64 * (cell_w + gap) - gap;
    let canvas_h = rows as f64 * (cell_h + gap) - gap;

    let canvas = Canvas::default()
        .block(
            Block::default()
                .title(format!(" {} ({} keys) ", title, state.keys.len()))
                .borders(Borders::ALL),
        )
        .x_bounds([0.0, canvas_w])
        .y_bounds([0.0, canvas_h])
        .paint(|ctx: &mut Context| {
            for (i, _key) in state.keys.iter().enumerate() {
                let col = i % cols;
                let row = i / cols;

                let x = col as f64 * (cell_w + gap);
                // Invert y so row 0 is at top
                let y = canvas_h - (row as f64 * (cell_h + gap)) - cell_h;

                let color = match state.key_state(i) {
                    KeyState::Pressed => Color::Red,
                    KeyState::Selected => Color::Cyan,
                    KeyState::Mapped => Color::Yellow,
                    KeyState::Available => Color::Green,
                };

                ctx.draw(&Rectangle {
                    x,
                    y,
                    width: cell_w,
                    height: cell_h,
                    color,
                });
            }
        });

    frame.render_widget(canvas, area);
}
