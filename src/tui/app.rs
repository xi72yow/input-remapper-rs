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
