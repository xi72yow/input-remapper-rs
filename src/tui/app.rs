use std::io::BufRead;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ipc::client;
use crate::ipc::protocol::{
    DeviceInfoResponse, InjectionStatus, RecordEvent, Request, Response,
};
use crate::mapping::config::{InputConfig, MappingEntry};

use super::event::AppEvent;
use super::widgets::key_matrix::KeyMatrixState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Devices,
    Presets,
    Editor,
    Status,
}

impl Screen {
    pub const ALL: [Screen; 4] = [
        Screen::Devices,
        Screen::Presets,
        Screen::Editor,
        Screen::Status,
    ];

    pub fn index(self) -> usize {
        match self {
            Screen::Devices => 0,
            Screen::Presets => 1,
            Screen::Editor => 2,
            Screen::Status => 3,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Screen::Devices => "Devices",
            Screen::Presets => "Presets",
            Screen::Editor => "Editor",
            Screen::Status => "Status",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    None,
    Record {
        events: Vec<RecordEvent>,
        selected: usize,
    },
    SymbolSearch {
        query: String,
        filtered: Vec<(String, u16)>,
        cursor: usize,
        /// If true, we're building a combination (e.g. Control_L + ...)
        combination: Vec<String>,
    },
    TextInput {
        title: String,
        value: String,
        action: InputAction,
    },
    Confirm {
        title: String,
        message: String,
        action: ConfirmAction,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    NewPreset,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    DeletePreset,
    #[allow(dead_code)]
    DeleteMapping,
    Quit,
}

pub struct App {
    pub screen: Screen,
    pub overlay: Overlay,
    pub devices: Vec<DeviceInfoResponse>,
    pub selected_device: usize,
    pub presets: Vec<String>,
    pub selected_preset: usize,
    pub entries: Vec<MappingEntry>,
    pub selected_entry: usize,
    pub injections: Vec<InjectionStatus>,
    pub selected_injection: usize,
    pub symbols: Vec<(String, u16)>,
    pub error: Option<String>,
    pub should_quit: bool,
    pub unsaved_changes: bool,
    pub key_matrix: Option<KeyMatrixState>,
    record_stream: Option<UnixStream>,
}

impl App {
    pub fn new(symbols: Vec<(String, u16)>) -> Self {
        Self {
            screen: Screen::Devices,
            overlay: Overlay::None,
            devices: Vec::new(),
            selected_device: 0,
            presets: Vec::new(),
            selected_preset: 0,
            entries: Vec::new(),
            selected_entry: 0,
            injections: Vec::new(),
            selected_injection: 0,
            symbols,
            error: None,
            should_quit: false,
            unsaved_changes: false,
            key_matrix: None,
            record_stream: None,
        }
    }

    pub fn device_name(&self) -> Option<&str> {
        self.devices.get(self.selected_device).map(|d| d.name.as_str())
    }

    pub fn preset_name(&self) -> Option<&str> {
        self.presets.get(self.selected_preset).map(|s| s.as_str())
    }

    pub fn refresh_devices(&mut self) {
        match client::send_request(&Request::ListDevices) {
            Ok(Response::Devices { devices }) => {
                self.devices = devices;
                self.selected_device = 0;
            }
            Ok(Response::Error { message }) => self.error = Some(message),
            Err(e) => self.error = Some(format!("Daemon not reachable: {}", e)),
            _ => {}
        }
    }

    pub fn refresh_presets(&mut self) {
        let device = match self.device_name() {
            Some(d) => d.to_string(),
            None => return,
        };
        match client::send_request(&Request::ListPresets { device }) {
            Ok(Response::Presets { presets }) => {
                self.presets = presets;
                self.selected_preset = 0;
            }
            Ok(Response::Error { message }) => self.error = Some(message),
            Err(e) => self.error = Some(format!("Error: {}", e)),
            _ => {}
        }
    }

    pub fn refresh_entries(&mut self) {
        let device = match self.device_name() {
            Some(d) => d.to_string(),
            None => return,
        };
        let preset = match self.preset_name() {
            Some(p) => p.to_string(),
            None => return,
        };
        match client::send_request(&Request::GetPreset { device, preset }) {
            Ok(Response::PresetData { entries }) => {
                self.entries = entries;
                self.selected_entry = 0;
                self.unsaved_changes = false;
            }
            Ok(Response::Error { message }) => self.error = Some(message),
            Err(e) => self.error = Some(format!("Error: {}", e)),
            _ => {}
        }
    }

    pub fn refresh_device_keys(&mut self) {
        let device = match self.device_name() {
            Some(d) => d.to_string(),
            None => return,
        };
        match client::send_request(&Request::GetDeviceKeys { device }) {
            Ok(Response::DeviceKeys { keys }) => {
                let mapped_codes: Vec<u16> = self
                    .entries
                    .iter()
                    .flat_map(|e| e.input_combination.iter().map(|i| i.code))
                    .collect();
                self.key_matrix = Some(KeyMatrixState {
                    keys,
                    mapped_codes,
                    selected: 0,
                    pressed_codes: Vec::new(),
                });
            }
            Ok(Response::Error { message }) => self.error = Some(message),
            Err(e) => self.error = Some(format!("Error: {}", e)),
            _ => {}
        }
    }

    /// Update key_matrix.mapped_codes from current entries
    fn sync_matrix_mapped_codes(&mut self) {
        if let Some(ref mut matrix) = self.key_matrix {
            matrix.mapped_codes = self
                .entries
                .iter()
                .flat_map(|e| e.input_combination.iter().map(|i| i.code))
                .collect();
        }
    }

    pub fn refresh_status(&mut self) {
        match client::send_request(&Request::Status) {
            Ok(Response::Status { injections }) => {
                self.injections = injections;
                self.selected_injection = 0;
            }
            Ok(Response::Error { message }) => self.error = Some(message),
            Err(e) => self.error = Some(format!("Daemon not reachable: {}", e)),
            _ => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, event_tx: &mpsc::Sender<AppEvent>) {
        // Clear transient error on any keypress
        if self.error.is_some() && self.overlay == Overlay::None {
            self.error = None;
        }

        // Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Dispatch to overlay handler first
        if self.overlay != Overlay::None {
            self.handle_overlay_key(key, event_tx);
            return;
        }

        // Global keys
        match key.code {
            KeyCode::Char('q') => {
                if self.unsaved_changes {
                    self.overlay = Overlay::Confirm {
                        title: "Quit".into(),
                        message: "Unsaved changes. Quit anyway?".into(),
                        action: ConfirmAction::Quit,
                    };
                } else {
                    self.should_quit = true;
                }
                return;
            }
            KeyCode::Tab => {
                let idx = (self.screen.index() + 1) % Screen::ALL.len();
                self.switch_screen(Screen::ALL[idx]);
                return;
            }
            KeyCode::BackTab => {
                let idx = (self.screen.index() + Screen::ALL.len() - 1) % Screen::ALL.len();
                self.switch_screen(Screen::ALL[idx]);
                return;
            }
            KeyCode::Char('1') => { self.switch_screen(Screen::Devices); return; }
            KeyCode::Char('2') => { self.switch_screen(Screen::Presets); return; }
            KeyCode::Char('3') => { self.switch_screen(Screen::Editor); return; }
            KeyCode::Char('4') => { self.switch_screen(Screen::Status); return; }
            _ => {}
        }

        // Screen-specific keys
        match self.screen {
            Screen::Devices => self.handle_devices_key(key),
            Screen::Presets => self.handle_presets_key(key),
            Screen::Editor => self.handle_editor_key(key, event_tx),
            Screen::Status => self.handle_status_key(key),
        }
    }

    fn switch_screen(&mut self, screen: Screen) {
        self.screen = screen;
        match screen {
            Screen::Devices => self.refresh_devices(),
            Screen::Presets => self.refresh_presets(),
            Screen::Editor => {
                self.refresh_entries();
                self.refresh_device_keys();
            }
            Screen::Status => self.refresh_status(),
        }
    }

    fn handle_devices_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_device > 0 {
                    self.selected_device -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_device + 1 < self.devices.len() {
                    self.selected_device += 1;
                }
            }
            KeyCode::Enter => {
                if !self.devices.is_empty() {
                    self.refresh_presets();
                    self.screen = Screen::Presets;
                }
            }
            KeyCode::Char('r') => self.refresh_devices(),
            _ => {}
        }
    }

    fn handle_presets_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_preset > 0 {
                    self.selected_preset -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_preset + 1 < self.presets.len() {
                    self.selected_preset += 1;
                }
            }
            KeyCode::Enter => {
                if !self.presets.is_empty() {
                    self.refresh_entries();
                    self.screen = Screen::Editor;
                }
            }
            KeyCode::Char('n') => {
                self.overlay = Overlay::TextInput {
                    title: "New Preset".into(),
                    value: String::new(),
                    action: InputAction::NewPreset,
                };
            }
            KeyCode::Char('d') => {
                if !self.presets.is_empty() {
                    let name = self.presets[self.selected_preset].clone();
                    self.overlay = Overlay::Confirm {
                        title: "Delete Preset".into(),
                        message: format!("Delete preset '{}'?", name),
                        action: ConfirmAction::DeletePreset,
                    };
                }
            }
            KeyCode::Char('r') => self.refresh_presets(),
            _ => {}
        }
    }

    fn handle_editor_key(&mut self, key: KeyEvent, event_tx: &mpsc::Sender<AppEvent>) {
        // Matrix navigation with arrow keys
        let matrix_cols = 10; // default grid columns
        match key.code {
            KeyCode::Up => {
                if let Some(ref mut matrix) = self.key_matrix {
                    matrix.move_up(matrix_cols);
                }
            }
            KeyCode::Down => {
                if let Some(ref mut matrix) = self.key_matrix {
                    matrix.move_down(matrix_cols);
                }
            }
            KeyCode::Left => {
                if let Some(ref mut matrix) = self.key_matrix {
                    matrix.move_left();
                }
            }
            KeyCode::Right => {
                if let Some(ref mut matrix) = self.key_matrix {
                    matrix.move_right();
                }
            }
            KeyCode::Enter => {
                // Open symbol search for selected key in matrix
                if let Some(ref matrix) = self.key_matrix {
                    if let Some(key_info) = matrix.selected_key() {
                        let code = key_info.code;
                        let name = key_info.name.clone();
                        // Find existing entry for this key or create new
                        let entry_idx = self.entries.iter().position(|e| {
                            e.input_combination.first().is_some_and(|i| i.code == code)
                        });
                        if let Some(idx) = entry_idx {
                            self.selected_entry = idx;
                        } else {
                            // Add a new entry for this key
                            self.entries.push(MappingEntry {
                                input_combination: vec![InputConfig {
                                    event_type: 1,
                                    code,
                                    origin_hash: None,
                                }],
                                target_uinput: "keyboard".into(),
                                output_symbol: None,
                                name: Some(name),
                                mapping_type: "key_macro".into(),
                            });
                            self.selected_entry = self.entries.len() - 1;
                            self.unsaved_changes = true;
                            self.sync_matrix_mapped_codes();
                        }
                        self.open_symbol_search();
                    }
                }
            }
            KeyCode::Char('a') => {
                // Add via record
                self.start_recording(event_tx);
            }
            KeyCode::Char('d') => {
                // Delete mapping for selected key in matrix
                if let Some(ref matrix) = self.key_matrix {
                    if let Some(key_info) = matrix.selected_key() {
                        let code = key_info.code;
                        if let Some(idx) = self.entries.iter().position(|e| {
                            e.input_combination.first().is_some_and(|i| i.code == code)
                        }) {
                            self.entries.remove(idx);
                            self.unsaved_changes = true;
                            self.sync_matrix_mapped_codes();
                        }
                    }
                }
            }
            KeyCode::Char('s') => self.save_preset(),
            KeyCode::Char('p') => self.apply_preset(),
            _ => {}
        }
    }

    fn handle_status_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_injection > 0 {
                    self.selected_injection -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_injection + 1 < self.injections.len() {
                    self.selected_injection += 1;
                }
            }
            KeyCode::Char('s') => {
                // Stop selected injection
                if let Some(inj) = self.injections.get(self.selected_injection) {
                    let device = inj.device.clone();
                    match client::send_request(&Request::Stop { device }) {
                        Ok(Response::Ok { .. }) => self.refresh_status(),
                        Ok(Response::Error { message }) => self.error = Some(message),
                        Err(e) => self.error = Some(format!("Error: {}", e)),
                        _ => {}
                    }
                }
            }
            KeyCode::Char('S') => {
                match client::send_request(&Request::StopAll) {
                    Ok(Response::Ok { .. }) => self.refresh_status(),
                    Ok(Response::Error { message }) => self.error = Some(message),
                    Err(e) => self.error = Some(format!("Error: {}", e)),
                    _ => {}
                }
            }
            KeyCode::Char('r') => self.refresh_status(),
            _ => {}
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent, _event_tx: &mpsc::Sender<AppEvent>) {
        match &mut self.overlay {
            Overlay::Record { events, selected } => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected + 1 < events.len() {
                        *selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(ev) = events.get(*selected).cloned() {
                        self.stop_recording();
                        // Store the recorded input, then open symbol search
                        let input = InputConfig {
                            event_type: ev.event_type,
                            code: ev.code,
                            origin_hash: None,
                        };
                        // Add a placeholder entry
                        self.entries.push(MappingEntry {
                            input_combination: vec![input],
                            target_uinput: "keyboard".into(),
                            output_symbol: None,
                            name: Some(ev.code_name.clone()),
                            mapping_type: "key_macro".into(),
                        });
                        self.selected_entry = self.entries.len() - 1;
                        self.unsaved_changes = true;
                        self.overlay = Overlay::None;
                        // Open symbol search for the new entry
                        self.open_symbol_search();
                    }
                }
                KeyCode::Esc => {
                    self.stop_recording();
                    self.overlay = Overlay::None;
                }
                _ => {}
            },
            Overlay::SymbolSearch {
                query,
                filtered,
                cursor,
                combination,
            } => match key.code {
                KeyCode::Char('+') if !combination.is_empty() || !query.is_empty() => {
                    // Add current selection to combination
                    if let Some((name, _)) = filtered.get(*cursor) {
                        combination.push(name.clone());
                        query.clear();
                    }
                    self.refilter_symbols();
                    return;
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    self.refilter_symbols();
                    return;
                }
                KeyCode::Backspace => {
                    query.pop();
                    self.refilter_symbols();
                    return;
                }
                KeyCode::Up => {
                    if *cursor > 0 {
                        *cursor -= 1;
                    }
                }
                KeyCode::Down => {
                    if *cursor + 1 < filtered.len() {
                        *cursor += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some((name, _)) = filtered.get(*cursor).cloned() {
                        let mut parts = combination.clone();
                        parts.push(name);
                        let symbol = parts.join(" + ");
                        // Apply to current entry
                        if let Some(entry) = self.entries.get_mut(self.selected_entry) {
                            entry.output_symbol = Some(symbol);
                            self.unsaved_changes = true;
                        }
                        self.overlay = Overlay::None;
                    }
                }
                KeyCode::Esc => {
                    self.overlay = Overlay::None;
                }
                _ => {}
            },
            Overlay::TextInput {
                value, action, ..
            } => match key.code {
                KeyCode::Char(c) => value.push(c),
                KeyCode::Backspace => { value.pop(); }
                KeyCode::Enter => {
                    let val = value.clone();
                    let act = action.clone();
                    self.overlay = Overlay::None;
                    self.handle_text_input_submit(&val, &act);
                }
                KeyCode::Esc => self.overlay = Overlay::None,
                _ => {}
            },
            Overlay::Confirm { action, .. } => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    let act = action.clone();
                    self.overlay = Overlay::None;
                    self.handle_confirm(&act);
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.overlay = Overlay::None;
                }
                _ => {}
            },
            Overlay::None => {}
        }
    }

    fn handle_text_input_submit(&mut self, value: &str, action: &InputAction) {
        match action {
            InputAction::NewPreset => {
                if value.is_empty() {
                    return;
                }
                let device = match self.device_name() {
                    Some(d) => d.to_string(),
                    None => return,
                };
                // Create empty preset
                match client::send_request(&Request::SavePreset {
                    device,
                    preset: value.to_string(),
                    entries: Vec::new(),
                }) {
                    Ok(Response::Ok { .. }) => {
                        self.refresh_presets();
                    }
                    Ok(Response::Error { message }) => self.error = Some(message),
                    Err(e) => self.error = Some(format!("Error: {}", e)),
                    _ => {}
                }
            }
        }
    }

    fn handle_confirm(&mut self, action: &ConfirmAction) {
        match action {
            ConfirmAction::DeletePreset => {
                let device = match self.device_name() {
                    Some(d) => d.to_string(),
                    None => return,
                };
                let preset = match self.preset_name() {
                    Some(p) => p.to_string(),
                    None => return,
                };
                match client::send_request(&Request::DeletePreset { device, preset }) {
                    Ok(Response::Ok { .. }) => self.refresh_presets(),
                    Ok(Response::Error { message }) => self.error = Some(message),
                    Err(e) => self.error = Some(format!("Error: {}", e)),
                    _ => {}
                }
            }
            ConfirmAction::DeleteMapping => {
                if self.selected_entry < self.entries.len() {
                    self.entries.remove(self.selected_entry);
                    if self.selected_entry > 0 && self.selected_entry >= self.entries.len() {
                        self.selected_entry -= 1;
                    }
                    self.unsaved_changes = true;
                }
            }
            ConfirmAction::Quit => {
                self.should_quit = true;
            }
        }
    }

    fn start_recording(&mut self, event_tx: &mpsc::Sender<AppEvent>) {
        let device = match self.device_name() {
            Some(d) => d.to_string(),
            None => {
                self.error = Some("No device selected".into());
                return;
            }
        };

        self.overlay = Overlay::Record {
            events: Vec::new(),
            selected: 0,
        };

        // Start record stream in background thread
        let tx = event_tx.clone();
        let device_clone = device.clone();
        match client::start_record_stream(&device_clone) {
            Ok((stream, reader)) => {
                self.record_stream = Some(stream);
                thread::spawn(move || {
                    for line in reader.lines() {
                        match line {
                            Ok(line) if line.is_empty() => continue,
                            Ok(line) => {
                                match serde_json::from_str::<Response>(&line) {
                                    Ok(Response::RecordEvent(ev)) => {
                                        if tx.send(AppEvent::RecordEvent(ev)).is_err() {
                                            break;
                                        }
                                    }
                                    Ok(Response::Error { message }) => {
                                        let _ = tx.send(AppEvent::RecordError(message));
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    let _ = tx.send(AppEvent::RecordStopped);
                });
            }
            Err(e) => {
                self.overlay = Overlay::None;
                self.error = Some(format!("Failed to start recording: {}", e));
            }
        }
    }

    fn stop_recording(&mut self) {
        if let Some(stream) = self.record_stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    pub fn handle_record_event(&mut self, ev: RecordEvent) {
        // Update key matrix live highlighting
        if ev.event_type == 1 {
            if let Some(ref mut matrix) = self.key_matrix {
                if ev.value == 1 {
                    // Key pressed
                    if !matrix.pressed_codes.contains(&ev.code) {
                        matrix.pressed_codes.push(ev.code);
                    }
                } else if ev.value == 0 {
                    // Key released
                    matrix.pressed_codes.retain(|&c| c != ev.code);
                }
            }
        }

        // Only capture KEY press events (type=1, value=1) for record overlay
        if ev.event_type != 1 || ev.value != 1 {
            return;
        }
        if let Overlay::Record { events, .. } = &mut self.overlay {
            // Don't add duplicates
            if !events.iter().any(|e| e.code == ev.code) {
                events.push(ev);
            }
        }
    }

    pub fn handle_record_error(&mut self, msg: String) {
        self.stop_recording();
        self.overlay = Overlay::None;
        self.error = Some(msg);
    }

    pub fn handle_record_stopped(&mut self) {
        self.record_stream = None;
    }

    fn open_symbol_search(&mut self) {
        let filtered = filter_symbols(&self.symbols, "");
        self.overlay = Overlay::SymbolSearch {
            query: String::new(),
            filtered,
            cursor: 0,
            combination: Vec::new(),
        };
    }

    fn refilter_symbols(&mut self) {
        if let Overlay::SymbolSearch {
            query,
            filtered,
            cursor,
            ..
        } = &mut self.overlay
        {
            *filtered = filter_symbols(&self.symbols, query);
            *cursor = 0;
        }
    }

    fn save_preset(&mut self) {
        let device = match self.device_name() {
            Some(d) => d.to_string(),
            None => {
                self.error = Some("No device selected".into());
                return;
            }
        };
        let preset = match self.preset_name() {
            Some(p) => p.to_string(),
            None => {
                self.error = Some("No preset selected".into());
                return;
            }
        };
        match client::send_request(&Request::SavePreset {
            device,
            preset,
            entries: self.entries.clone(),
        }) {
            Ok(Response::Ok { message }) => {
                self.unsaved_changes = false;
                self.error = Some(message); // Show as success message
            }
            Ok(Response::Error { message }) => self.error = Some(message),
            Err(e) => self.error = Some(format!("Error: {}", e)),
            _ => {}
        }
    }

    fn apply_preset(&mut self) {
        let device = match self.device_name() {
            Some(d) => d.to_string(),
            None => return,
        };
        let preset = match self.preset_name() {
            Some(p) => p.to_string(),
            None => return,
        };
        match client::send_request(&Request::Start { device, preset }) {
            Ok(Response::Ok { message }) => self.error = Some(message),
            Ok(Response::Error { message }) => self.error = Some(message),
            Err(e) => self.error = Some(format!("Error: {}", e)),
            _ => {}
        }
    }
}

fn filter_symbols(symbols: &[(String, u16)], query: &str) -> Vec<(String, u16)> {
    if query.is_empty() {
        return symbols.to_vec();
    }
    let q = query.to_lowercase();
    let mut results: Vec<_> = symbols
        .iter()
        .filter(|(name, _)| name.to_lowercase().contains(&q))
        .cloned()
        .collect();
    // Prefix matches first
    results.sort_by(|(a, _), (b, _)| {
        let a_prefix = a.to_lowercase().starts_with(&q);
        let b_prefix = b.to_lowercase().starts_with(&q);
        b_prefix.cmp(&a_prefix).then(a.cmp(b))
    });
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::{KeyInfoResponse, RecordEvent};
    use crate::tui::widgets::key_matrix::{KeyMatrixState, KeyState};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn test_symbols() -> Vec<(String, u16)> {
        vec![
            ("KEY_A".into(), 30),
            ("KEY_B".into(), 48),
            ("KEY_BACKSPACE".into(), 14),
            ("KEY_ENTER".into(), 28),
            ("Control_L".into(), 29),
            ("Alt_L".into(), 56),
            ("KEY_AB".into(), 999),
        ]
    }

    fn make_key(code: u16, name: &str) -> KeyInfoResponse {
        KeyInfoResponse {
            code,
            name: name.into(),
        }
    }

    fn make_matrix(num_keys: usize) -> KeyMatrixState {
        let keys: Vec<KeyInfoResponse> = (0..num_keys)
            .map(|i| make_key(i as u16, &format!("KEY_{}", i)))
            .collect();
        KeyMatrixState {
            keys,
            mapped_codes: Vec::new(),
            selected: 0,
            pressed_codes: Vec::new(),
        }
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn make_app() -> App {
        let mut app = App::new(test_symbols());
        app.devices = vec![DeviceInfoResponse {
            name: "Test Device".into(),
            key: "/dev/input/event0".into(),
            vendor: 1,
            product: 2,
        }];
        app.presets = vec!["default".into(), "gaming".into()];
        app
    }

    // --- filter_symbols tests ---

    #[test]
    fn filter_symbols_empty_query_returns_all() {
        let syms = test_symbols();
        let result = filter_symbols(&syms, "");
        assert_eq!(result.len(), syms.len());
    }

    #[test]
    fn filter_symbols_exact_match() {
        let syms = test_symbols();
        let result = filter_symbols(&syms, "KEY_A");
        // Should match KEY_A and KEY_AB
        assert!(result.iter().any(|(n, _)| n == "KEY_A"));
        assert!(result.iter().any(|(n, _)| n == "KEY_AB"));
        assert!(!result.iter().any(|(n, _)| n == "KEY_B"));
    }

    #[test]
    fn filter_symbols_case_insensitive() {
        let syms = test_symbols();
        let result = filter_symbols(&syms, "key_b");
        assert!(result.iter().any(|(n, _)| n == "KEY_B"));
        assert!(result.iter().any(|(n, _)| n == "KEY_BACKSPACE"));
    }

    #[test]
    fn filter_symbols_prefix_first() {
        let syms = test_symbols();
        let result = filter_symbols(&syms, "key_a");
        // KEY_A and KEY_AB are prefix matches, should come before KEY_BACKSPACE (substring match for "a")
        assert_eq!(result[0].0, "KEY_A");
        assert_eq!(result[1].0, "KEY_AB");
    }

    #[test]
    fn filter_symbols_no_match() {
        let syms = test_symbols();
        let result = filter_symbols(&syms, "zzz_nothing");
        assert!(result.is_empty());
    }

    // --- KeyMatrixState tests ---

    #[test]
    fn matrix_key_state_available() {
        let matrix = make_matrix(5);
        // index 1 is not selected (0 is), not mapped, not pressed
        assert_eq!(matrix.key_state(1), KeyState::Available);
    }

    #[test]
    fn matrix_key_state_selected() {
        let matrix = make_matrix(5);
        assert_eq!(matrix.key_state(0), KeyState::Selected);
    }

    #[test]
    fn matrix_key_state_mapped() {
        let mut matrix = make_matrix(5);
        matrix.mapped_codes.push(2); // code for index 2
        assert_eq!(matrix.key_state(2), KeyState::Mapped);
    }

    #[test]
    fn matrix_key_state_pressed_overrides_mapped() {
        let mut matrix = make_matrix(5);
        matrix.mapped_codes.push(2);
        matrix.pressed_codes.push(2);
        assert_eq!(matrix.key_state(2), KeyState::Pressed);
    }

    #[test]
    fn matrix_key_state_pressed_overrides_selected() {
        let mut matrix = make_matrix(5);
        matrix.pressed_codes.push(0); // code for selected index 0
        assert_eq!(matrix.key_state(0), KeyState::Pressed);
    }

    #[test]
    fn matrix_move_right_and_left() {
        let mut matrix = make_matrix(5);
        assert_eq!(matrix.selected, 0);
        matrix.move_right();
        assert_eq!(matrix.selected, 1);
        matrix.move_right();
        assert_eq!(matrix.selected, 2);
        matrix.move_left();
        assert_eq!(matrix.selected, 1);
    }

    #[test]
    fn matrix_move_left_at_zero_stays() {
        let mut matrix = make_matrix(5);
        matrix.move_left();
        assert_eq!(matrix.selected, 0);
    }

    #[test]
    fn matrix_move_right_at_end_stays() {
        let mut matrix = make_matrix(5);
        matrix.selected = 4;
        matrix.move_right();
        assert_eq!(matrix.selected, 4);
    }

    #[test]
    fn matrix_move_up_down_grid() {
        let mut matrix = make_matrix(20); // 20 keys, 5 cols -> 4 rows
        let cols = 5;
        matrix.selected = 7; // row 1, col 2
        matrix.move_up(cols);
        assert_eq!(matrix.selected, 2); // row 0, col 2
        matrix.move_down(cols);
        assert_eq!(matrix.selected, 7); // back to row 1, col 2
    }

    #[test]
    fn matrix_move_up_at_top_stays() {
        let mut matrix = make_matrix(20);
        matrix.selected = 2; // top row
        matrix.move_up(5);
        assert_eq!(matrix.selected, 2);
    }

    #[test]
    fn matrix_move_down_at_bottom_stays() {
        let mut matrix = make_matrix(20);
        matrix.selected = 17; // last row
        matrix.move_down(5);
        assert_eq!(matrix.selected, 17);
    }

    #[test]
    fn matrix_selected_key() {
        let matrix = make_matrix(5);
        let key = matrix.selected_key().unwrap();
        assert_eq!(key.code, 0);
        assert_eq!(key.name, "KEY_0");
    }

    #[test]
    fn matrix_selected_key_empty() {
        let matrix = make_matrix(0);
        assert!(matrix.selected_key().is_none());
    }

    // --- App handle_record_event tests ---

    #[test]
    fn record_event_tracks_pressed_codes() {
        let mut app = make_app();
        app.key_matrix = Some(make_matrix(10));
        app.overlay = Overlay::Record {
            events: Vec::new(),
            selected: 0,
        };

        // Key press (type=1, value=1)
        app.handle_record_event(RecordEvent {
            event_type: 1,
            code: 5,
            value: 1,
            code_name: "KEY_5".into(),
        });
        assert!(app.key_matrix.as_ref().unwrap().pressed_codes.contains(&5));

        // Key release (type=1, value=0)
        app.handle_record_event(RecordEvent {
            event_type: 1,
            code: 5,
            value: 0,
            code_name: "KEY_5".into(),
        });
        assert!(!app.key_matrix.as_ref().unwrap().pressed_codes.contains(&5));
    }

    #[test]
    fn record_event_captures_press_deduplicates() {
        let mut app = make_app();
        app.overlay = Overlay::Record {
            events: Vec::new(),
            selected: 0,
        };

        let ev = RecordEvent {
            event_type: 1,
            code: 30,
            value: 1,
            code_name: "KEY_A".into(),
        };
        app.handle_record_event(ev.clone());
        app.handle_record_event(ev);

        if let Overlay::Record { events, .. } = &app.overlay {
            assert_eq!(events.len(), 1); // deduplicated
        } else {
            panic!("Expected Record overlay");
        }
    }

    #[test]
    fn record_event_ignores_release_in_overlay() {
        let mut app = make_app();
        app.overlay = Overlay::Record {
            events: Vec::new(),
            selected: 0,
        };

        // Release event should not be captured
        app.handle_record_event(RecordEvent {
            event_type: 1,
            code: 30,
            value: 0,
            code_name: "KEY_A".into(),
        });

        if let Overlay::Record { events, .. } = &app.overlay {
            assert!(events.is_empty());
        } else {
            panic!("Expected Record overlay");
        }
    }

    // --- App confirm/delete action tests ---

    #[test]
    fn confirm_delete_mapping_removes_entry() {
        let mut app = make_app();
        app.entries = vec![
            MappingEntry {
                input_combination: vec![InputConfig {
                    event_type: 1,
                    code: 30,
                    origin_hash: None,
                }],
                target_uinput: "keyboard".into(),
                output_symbol: Some("KEY_B".into()),
                name: Some("KEY_A".into()),
                mapping_type: "key_macro".into(),
            },
            MappingEntry {
                input_combination: vec![InputConfig {
                    event_type: 1,
                    code: 48,
                    origin_hash: None,
                }],
                target_uinput: "keyboard".into(),
                output_symbol: Some("KEY_C".into()),
                name: Some("KEY_B".into()),
                mapping_type: "key_macro".into(),
            },
        ];
        app.selected_entry = 0;
        app.handle_confirm(&ConfirmAction::DeleteMapping);
        assert_eq!(app.entries.len(), 1);
        assert_eq!(app.entries[0].name.as_deref(), Some("KEY_B"));
        assert!(app.unsaved_changes);
    }

    #[test]
    fn confirm_delete_last_mapping_adjusts_index() {
        let mut app = make_app();
        app.entries = vec![MappingEntry {
            input_combination: vec![InputConfig {
                event_type: 1,
                code: 30,
                origin_hash: None,
            }],
            target_uinput: "keyboard".into(),
            output_symbol: None,
            name: None,
            mapping_type: "key_macro".into(),
        }];
        app.selected_entry = 0;
        app.handle_confirm(&ConfirmAction::DeleteMapping);
        assert!(app.entries.is_empty());
        assert_eq!(app.selected_entry, 0);
    }

    #[test]
    fn confirm_quit_sets_should_quit() {
        let mut app = make_app();
        app.handle_confirm(&ConfirmAction::Quit);
        assert!(app.should_quit);
    }

    // --- Screen navigation tests ---

    #[test]
    fn screen_index_and_title() {
        assert_eq!(Screen::Devices.index(), 0);
        assert_eq!(Screen::Presets.index(), 1);
        assert_eq!(Screen::Editor.index(), 2);
        assert_eq!(Screen::Status.index(), 3);
        assert_eq!(Screen::Devices.title(), "Devices");
    }

    #[test]
    fn ctrl_c_quits() {
        let mut app = make_app();
        let (tx, _rx) = mpsc::channel();
        let key = KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        app.handle_key(key, &tx);
        assert!(app.should_quit);
    }

    #[test]
    fn q_with_unsaved_changes_shows_confirm() {
        let mut app = make_app();
        app.unsaved_changes = true;
        let (tx, _rx) = mpsc::channel();
        app.handle_key(press(KeyCode::Char('q')), &tx);
        assert!(!app.should_quit);
        assert!(matches!(
            app.overlay,
            Overlay::Confirm { action: ConfirmAction::Quit, .. }
        ));
    }

    #[test]
    fn q_without_unsaved_changes_quits() {
        let mut app = make_app();
        let (tx, _rx) = mpsc::channel();
        app.handle_key(press(KeyCode::Char('q')), &tx);
        assert!(app.should_quit);
    }

    #[test]
    fn number_keys_switch_screen() {
        let mut app = make_app();
        let (tx, _rx) = mpsc::channel();
        // These will fail to refresh (no daemon) but screen should still change
        app.handle_key(press(KeyCode::Char('4')), &tx);
        assert_eq!(app.screen, Screen::Status);
        app.handle_key(press(KeyCode::Char('1')), &tx);
        assert_eq!(app.screen, Screen::Devices);
    }

    // --- Editor key matrix integration ---

    #[test]
    fn editor_enter_on_matrix_creates_entry_and_opens_symbol_search() {
        let mut app = make_app();
        app.screen = Screen::Editor;
        app.key_matrix = Some(make_matrix(5));
        let (tx, _rx) = mpsc::channel();

        app.handle_key(press(KeyCode::Enter), &tx);

        // Should have created an entry
        assert_eq!(app.entries.len(), 1);
        assert_eq!(app.entries[0].input_combination[0].code, 0);
        assert!(app.unsaved_changes);
        // Should have opened symbol search
        assert!(matches!(app.overlay, Overlay::SymbolSearch { .. }));
    }

    #[test]
    fn editor_delete_removes_mapping_for_selected_key() {
        let mut app = make_app();
        app.screen = Screen::Editor;
        app.key_matrix = Some(make_matrix(5));
        app.entries = vec![MappingEntry {
            input_combination: vec![InputConfig {
                event_type: 1,
                code: 0, // matches key 0 in matrix
                origin_hash: None,
            }],
            target_uinput: "keyboard".into(),
            output_symbol: Some("KEY_B".into()),
            name: Some("KEY_0".into()),
            mapping_type: "key_macro".into(),
        }];
        let (tx, _rx) = mpsc::channel();

        app.handle_key(press(KeyCode::Char('d')), &tx);
        assert!(app.entries.is_empty());
        assert!(app.unsaved_changes);
    }

    // --- sync_matrix_mapped_codes ---

    #[test]
    fn sync_matrix_mapped_codes_updates_from_entries() {
        let mut app = make_app();
        app.key_matrix = Some(make_matrix(5));
        app.entries = vec![MappingEntry {
            input_combination: vec![InputConfig {
                event_type: 1,
                code: 2,
                origin_hash: None,
            }],
            target_uinput: "keyboard".into(),
            output_symbol: None,
            name: None,
            mapping_type: "key_macro".into(),
        }];
        app.sync_matrix_mapped_codes();
        assert_eq!(app.key_matrix.as_ref().unwrap().mapped_codes, vec![2]);
    }

    // --- Symbol search overlay ---

    #[test]
    fn open_symbol_search_sets_overlay() {
        let mut app = make_app();
        app.open_symbol_search();
        if let Overlay::SymbolSearch { query, filtered, cursor, combination } = &app.overlay {
            assert!(query.is_empty());
            assert_eq!(*cursor, 0);
            assert!(combination.is_empty());
            assert_eq!(filtered.len(), app.symbols.len());
        } else {
            panic!("Expected SymbolSearch overlay");
        }
    }

    #[test]
    fn refilter_symbols_updates_filtered() {
        let mut app = make_app();
        app.open_symbol_search();
        // Simulate typing "enter"
        if let Overlay::SymbolSearch { query, .. } = &mut app.overlay {
            query.push_str("enter");
        }
        app.refilter_symbols();
        if let Overlay::SymbolSearch { filtered, cursor, .. } = &app.overlay {
            assert_eq!(filtered.len(), 1);
            assert_eq!(filtered[0].0, "KEY_ENTER");
            assert_eq!(*cursor, 0); // reset
        } else {
            panic!("Expected SymbolSearch overlay");
        }
    }
}
