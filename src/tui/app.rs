use std::io::BufRead;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ipc::client;
use crate::ipc::protocol::{
    DeviceInfoResponse, InjectionStatus, RecordEvent, Request, Response,
};
use crate::mapping::config::{InputConfig, MappingEntry};

use super::event::{AppEvent, IpcOp};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Devices,
    Presets,
    Editor,
    Status,
}

impl Screen {
    /// Is this screen part of the Config flow (Devices → Presets → Editor)?
    pub fn is_config(self) -> bool {
        matches!(self, Screen::Devices | Screen::Presets | Screen::Editor)
    }

    #[allow(dead_code)]
    pub fn title(self) -> &'static str {
        match self {
            Screen::Devices => "Devices",
            Screen::Presets => "Presets",
            Screen::Editor => "Editor",
            Screen::Status => "Status",
        }
    }

    /// Breadcrumb label for the Config chain
    pub fn config_breadcrumb(self) -> &'static str {
        match self {
            Screen::Devices => "Config > Devices",
            Screen::Presets => "Config > Presets",
            Screen::Editor => "Config > Editor",
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
    /// Combined chain builder + symbol search
    SymbolSearch {
        query: String,
        filtered: Vec<(String, u16)>,
        cursor: usize,
        /// The chain slots being built (e.g. ["Control_L", "KEY_A"])
        slots: Vec<String>,
        /// Which slot is active; slot_cursor == slots.len() means the [+] button
        slot_cursor: usize,
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
        /// false = Yes selected, true = No selected
        selected_no: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    NewPreset,
    RenameMapping { entry_index: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    DeletePreset,
    #[allow(dead_code)]
    DeleteMapping,
    LeaveEditor,
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
    pub loading: Option<&'static str>,
    pub tick: usize,
    record_cancel: Option<Arc<AtomicBool>>,
    /// Preset that was active before recording (to restart after)
    record_prev_injection: Option<(String, String)>,
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
            loading: None,
            tick: 0,
            record_cancel: None,
            record_prev_injection: None,
        }
    }

    pub fn device_name(&self) -> Option<&str> {
        self.devices.get(self.selected_device).map(|d| d.name.as_str())
    }

    pub fn preset_name(&self) -> Option<&str> {
        self.presets.get(self.selected_preset).map(|s| s.as_str())
    }

    /// Spawn an IPC request on a background thread.
    fn spawn_ipc(tx: &mpsc::Sender<AppEvent>, op: IpcOp, request: Request) {
        let tx = tx.clone();
        thread::spawn(move || {
            let result = client::send_request(&request).map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::IpcResult(op, result));
        });
    }

    pub fn refresh_devices(&mut self, tx: &mpsc::Sender<AppEvent>) {
        self.loading = Some("Loading devices...");
        Self::spawn_ipc(tx, IpcOp::RefreshDevices, Request::ListDevices);
    }

    pub fn refresh_presets(&mut self, tx: &mpsc::Sender<AppEvent>) {
        let device = match self.device_name() {
            Some(d) => d.to_string(),
            None => return,
        };
        self.loading = Some("Loading presets...");
        Self::spawn_ipc(tx, IpcOp::RefreshPresets, Request::ListPresets { device });
    }

    pub fn refresh_entries(&mut self, tx: &mpsc::Sender<AppEvent>) {
        let device = match self.device_name() {
            Some(d) => d.to_string(),
            None => return,
        };
        let preset = match self.preset_name() {
            Some(p) => p.to_string(),
            None => return,
        };
        self.loading = Some("Loading mappings...");
        Self::spawn_ipc(tx, IpcOp::RefreshEntries, Request::GetPreset { device, preset });
    }

    pub fn refresh_status(&mut self, tx: &mpsc::Sender<AppEvent>) {
        self.loading = Some("Loading status...");
        Self::spawn_ipc(tx, IpcOp::RefreshStatus, Request::Status);
    }

    /// Handle a completed IPC result from a background thread.
    pub fn handle_ipc_result(
        &mut self,
        op: IpcOp,
        result: Result<Response, String>,
        tx: &mpsc::Sender<AppEvent>,
    ) {
        self.loading = None;
        match result {
            Err(e) => {
                self.error = Some(format!("Daemon not reachable: {}", e));
            }
            Ok(resp) => match (op, resp) {
                (IpcOp::RefreshDevices, Response::Devices { devices }) => {
                    self.devices = devices;
                    self.selected_device = 0;
                }
                (IpcOp::RefreshPresets, Response::Presets { presets }) => {
                    self.presets = presets;
                    self.selected_preset = 0;
                }
                (IpcOp::RefreshEntries, Response::PresetData { entries }) => {
                    self.entries = entries;
                    self.selected_entry = 0;
                    self.unsaved_changes = false;
                }
                (IpcOp::RefreshStatus, Response::Status { injections }) => {
                    self.injections = injections;
                    self.selected_injection = 0;
                }
                (IpcOp::SavePreset, Response::Ok { message }) => {
                    self.unsaved_changes = false;
                    self.error = Some(message);
                }
                (IpcOp::ApplyPreset, Response::Ok { message }) => {
                    self.error = Some(message);
                }
                (IpcOp::StopInjection, Response::Ok { message }) => {
                    self.error = Some(message);
                    self.refresh_status(tx);
                }
                (IpcOp::StopAll, Response::Ok { message }) => {
                    self.error = Some(message);
                    self.refresh_status(tx);
                }
                (IpcOp::CreatePreset, Response::Ok { .. }) => {
                    self.refresh_presets(tx);
                }
                (IpcOp::DeletePreset, Response::Ok { .. }) => {
                    self.refresh_presets(tx);
                }
                (_, Response::Error { message }) => {
                    self.error = Some(message);
                }
                _ => {}
            },
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, event_tx: &mpsc::Sender<AppEvent>) {
        // Clear transient error on any keypress
        if self.error.is_some() && self.overlay == Overlay::None {
            self.error = None;
        }

        // Dispatch to overlay handler first (Record needs to capture all keys)
        if self.overlay != Overlay::None {
            self.handle_overlay_key(key, event_tx);
            return;
        }

        // Ctrl+C quits (only outside overlays)
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
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
                        selected_no: false,
                    };
                } else {
                    self.should_quit = true;
                }
                return;
            }
            KeyCode::Tab => {
                // Toggle between Config (starts at Devices) and Status
                if self.screen.is_config() {
                    self.switch_screen(Screen::Status, event_tx);
                } else {
                    self.switch_screen(Screen::Devices, event_tx);
                }
                return;
            }
            KeyCode::Esc => {
                // Back navigation in the Config chain
                match self.screen {
                    Screen::Editor => {
                        if self.unsaved_changes {
                            self.overlay = Overlay::Confirm {
                                title: "Leave Editor".into(),
                                message: "Unsaved changes. Leave anyway?".into(),
                                action: ConfirmAction::LeaveEditor,
                                selected_no: false,
                            };
                        } else {
                            self.switch_screen(Screen::Devices, event_tx);
                        }
                        return;
                    }
                    Screen::Presets => {
                        self.switch_screen(Screen::Devices, event_tx);
                        return;
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        // Screen-specific keys
        match self.screen {
            Screen::Devices => self.handle_devices_key(key, event_tx),
            Screen::Presets => self.handle_presets_key(key, event_tx),
            Screen::Editor => self.handle_editor_key(key, event_tx),
            Screen::Status => self.handle_status_key(key, event_tx),
        }
    }

    fn switch_screen(&mut self, screen: Screen, tx: &mpsc::Sender<AppEvent>) {
        self.screen = screen;
        match screen {
            Screen::Devices => self.refresh_devices(tx),
            Screen::Presets => self.refresh_presets(tx),
            Screen::Editor => self.refresh_entries(tx),
            Screen::Status => self.refresh_status(tx),
        }
    }

    fn handle_devices_key(&mut self, key: KeyEvent, tx: &mpsc::Sender<AppEvent>) {
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
                    self.screen = Screen::Presets;
                    self.refresh_presets(tx);
                }
            }
            KeyCode::Char('r') => self.refresh_devices(tx),
            _ => {}
        }
    }

    fn handle_presets_key(&mut self, key: KeyEvent, tx: &mpsc::Sender<AppEvent>) {
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
                    self.screen = Screen::Editor;
                    self.refresh_entries(tx);
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
                        selected_no: false,
                    };
                }
            }
            KeyCode::Char('r') => self.refresh_presets(tx),
            _ => {}
        }
    }

    fn handle_editor_key(&mut self, key: KeyEvent, event_tx: &mpsc::Sender<AppEvent>) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_entry > 0 {
                    self.selected_entry -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_entry + 1 < self.entries.len() {
                    self.selected_entry += 1;
                }
            }
            KeyCode::Enter => {
                // Edit output of selected entry via combined symbol search + chain builder
                if !self.entries.is_empty() {
                    let existing_slots: Vec<String> = self.entries[self.selected_entry]
                        .output_symbol
                        .as_deref()
                        .map(|s| s.split(" + ").map(|p| p.trim().to_string()).collect())
                        .unwrap_or_default();
                    let slot_cursor = existing_slots.len(); // on [+]
                    self.open_symbol_search_with_slots(existing_slots, slot_cursor);
                }
            }
            KeyCode::Char('a') => {
                // Add new mapping via record
                self.start_recording(event_tx);
            }
            KeyCode::Char('d') => {
                // Delete selected mapping
                if !self.entries.is_empty() {
                    let entry = &self.entries[self.selected_entry];
                    let name = entry
                        .input_combination
                        .first()
                        .map(|i| format!("{:?}", evdev::KeyCode(i.code)))
                        .unwrap_or_else(|| "?".into());
                    self.overlay = Overlay::Confirm {
                        title: "Delete Mapping".into(),
                        message: format!("Delete mapping for '{}'?", name),
                        action: ConfirmAction::DeleteMapping,
                        selected_no: false,
                    };
                }
            }
            KeyCode::Char('n') => {
                // Rename selected mapping
                if !self.entries.is_empty() {
                    let current_name = self.entries[self.selected_entry]
                        .name
                        .clone()
                        .unwrap_or_default();
                    self.overlay = Overlay::TextInput {
                        title: "Rename Mapping".into(),
                        value: current_name,
                        action: InputAction::RenameMapping {
                            entry_index: self.selected_entry,
                        },
                    };
                }
            }
            KeyCode::Char('s') => self.save_preset(event_tx),
            KeyCode::Char('p') => self.apply_preset(event_tx),
            _ => {}
        }
    }

    fn handle_status_key(&mut self, key: KeyEvent, tx: &mpsc::Sender<AppEvent>) {
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
                if let Some(inj) = self.injections.get(self.selected_injection) {
                    let device = inj.device.clone();
                    self.loading = Some("Stopping...");
                    Self::spawn_ipc(tx, IpcOp::StopInjection, Request::Stop { device });
                }
            }
            KeyCode::Char('S') => {
                self.loading = Some("Stopping all...");
                Self::spawn_ipc(tx, IpcOp::StopAll, Request::StopAll);
            }
            KeyCode::Char('r') => self.refresh_status(tx),
            _ => {}
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent, event_tx: &mpsc::Sender<AppEvent>) {
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
                        // Store the recorded input, then open mapping builder
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
                        // Open symbol search with empty chain
                        self.open_symbol_search_with_slots(Vec::new(), 0);
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
                slots,
                slot_cursor,
            } => match key.code {
                KeyCode::Left => {
                    // Navigate chain slots
                    if *slot_cursor > 0 {
                        *slot_cursor -= 1;
                    }
                }
                KeyCode::Right => {
                    // Navigate chain slots
                    if *slot_cursor < slots.len() {
                        *slot_cursor += 1;
                    }
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    self.refilter_symbols();
                }
                KeyCode::Backspace => {
                    if !query.is_empty() {
                        query.pop();
                        self.refilter_symbols();
                    } else if *slot_cursor < slots.len() {
                        // Delete current slot when query is empty
                        slots.remove(*slot_cursor);
                        if *slot_cursor > 0 && *slot_cursor >= slots.len() {
                            *slot_cursor = slots.len();
                        }
                    }
                }
                KeyCode::Delete => {
                    // Delete current slot
                    if *slot_cursor < slots.len() {
                        slots.remove(*slot_cursor);
                        if *slot_cursor > 0 && *slot_cursor >= slots.len() {
                            *slot_cursor = slots.len();
                        }
                    }
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
                    // Select symbol and insert into chain at slot_cursor position
                    if let Some((name, _)) = filtered.get(*cursor).cloned() {
                        if *slot_cursor < slots.len() {
                            slots[*slot_cursor] = name;
                        } else {
                            slots.push(name);
                        }
                        *slot_cursor = (*slot_cursor + 1).min(slots.len());
                        // Clear search for next symbol
                        query.clear();
                        self.refilter_symbols();
                    }
                }
                KeyCode::Esc => {
                    // Save combination and close
                    let slots = slots.clone();
                    if !slots.is_empty() {
                        let symbol = slots.join(" + ");
                        if let Some(entry) = self.entries.get_mut(self.selected_entry) {
                            entry.output_symbol = Some(symbol);
                            self.unsaved_changes = true;
                        }
                    }
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
                    self.handle_text_input_submit(&val, &act, event_tx);
                }
                KeyCode::Esc => self.overlay = Overlay::None,
                _ => {}
            },
            Overlay::Confirm { action, selected_no, .. } => match key.code {
                KeyCode::Left | KeyCode::Right => {
                    *selected_no = !*selected_no;
                }
                KeyCode::Enter => {
                    if *selected_no {
                        self.overlay = Overlay::None;
                    } else {
                        let act = action.clone();
                        self.overlay = Overlay::None;
                        self.handle_confirm(&act, event_tx);
                    }
                }
                KeyCode::Char('y') => {
                    let act = action.clone();
                    self.overlay = Overlay::None;
                    self.handle_confirm(&act, event_tx);
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.overlay = Overlay::None;
                }
                _ => {}
            },
            Overlay::None => {}
        }
    }

    fn handle_text_input_submit(
        &mut self,
        value: &str,
        action: &InputAction,
        tx: &mpsc::Sender<AppEvent>,
    ) {
        match action {
            InputAction::NewPreset => {
                if value.is_empty() {
                    return;
                }
                let device = match self.device_name() {
                    Some(d) => d.to_string(),
                    None => return,
                };
                self.loading = Some("Creating preset...");
                Self::spawn_ipc(
                    tx,
                    IpcOp::CreatePreset,
                    Request::SavePreset {
                        device,
                        preset: value.to_string(),
                        entries: Vec::new(),
                    },
                );
            }
            InputAction::RenameMapping { entry_index } => {
                if let Some(entry) = self.entries.get_mut(*entry_index) {
                    entry.name = if value.is_empty() { None } else { Some(value.to_string()) };
                    self.unsaved_changes = true;
                }
            }
        }
    }

    fn handle_confirm(&mut self, action: &ConfirmAction, tx: &mpsc::Sender<AppEvent>) {
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
                self.loading = Some("Deleting preset...");
                Self::spawn_ipc(
                    tx,
                    IpcOp::DeletePreset,
                    Request::DeletePreset { device, preset },
                );
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
            ConfirmAction::LeaveEditor => {
                self.unsaved_changes = false;
                self.switch_screen(Screen::Devices, tx);
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

        // Query current injection status and stop if active (grab must be released)
        let active_preset = match client::send_request(&Request::Status) {
            Ok(Response::Status { injections }) => injections
                .into_iter()
                .find(|inj| inj.device == device)
                .map(|inj| inj.preset),
            _ => None,
        };
        if active_preset.is_some() {
            let _ = client::send_request(&Request::Stop {
                device: device.clone(),
            });
        }
        self.record_prev_injection =
            active_preset.map(|preset| (device.clone(), preset));

        self.overlay = Overlay::Record {
            events: Vec::new(),
            selected: 0,
        };

        let cancel = Arc::new(AtomicBool::new(false));
        self.record_cancel = Some(Arc::clone(&cancel));

        let tx = event_tx.clone();

        // Do everything in the thread — no try_clone needed
        thread::spawn(move || {
            let mut stream = match UnixStream::connect(crate::ipc::protocol::SOCKET_PATH) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(AppEvent::RecordError(format!("Connect failed: {}", e)));
                    return;
                }
            };

            // Set a read timeout so we can check the cancel flag periodically
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));

            // Send Record request
            let request = Request::Record {
                device: device.clone(),
            };
            let mut json = match serde_json::to_string(&request) {
                Ok(j) => j,
                Err(e) => {
                    let _ = tx.send(AppEvent::RecordError(format!("Serialize: {}", e)));
                    return;
                }
            };
            json.push('\n');
            if let Err(e) = std::io::Write::write_all(&mut stream, json.as_bytes()) {
                let _ = tx.send(AppEvent::RecordError(format!("Write: {}", e)));
                return;
            }
            let _ = std::io::Write::flush(&mut stream);

            let mut reader = std::io::BufReader::new(&stream);
            let mut line_buf = String::new();
            loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                line_buf.clear();
                match reader.read_line(&mut line_buf) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let line = line_buf.trim();
                        if line.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<Response>(line) {
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
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        continue; // timeout, check cancel flag
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(AppEvent::RecordStopped);
        });
    }

    fn stop_recording(&mut self) {
        if let Some(cancel) = self.record_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        // Restart injection if it was running before recording
        if let Some((device, preset)) = self.record_prev_injection.take() {
            let _ = client::send_request(&Request::Start { device, preset });
        }
    }

    pub fn handle_record_event(&mut self, ev: RecordEvent) {
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
        self.record_cancel = None;
    }

    fn open_symbol_search_with_slots(&mut self, slots: Vec<String>, slot_cursor: usize) {
        let filtered = filter_symbols(&self.symbols, "");
        self.overlay = Overlay::SymbolSearch {
            query: String::new(),
            filtered,
            cursor: 0,
            slots,
            slot_cursor,
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

    fn save_preset(&mut self, tx: &mpsc::Sender<AppEvent>) {
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
        self.loading = Some("Saving...");
        Self::spawn_ipc(
            tx,
            IpcOp::SavePreset,
            Request::SavePreset {
                device,
                preset,
                entries: self.entries.clone(),
            },
        );
    }

    fn apply_preset(&mut self, tx: &mpsc::Sender<AppEvent>) {
        let device = match self.device_name() {
            Some(d) => d.to_string(),
            None => return,
        };
        let preset = match self.preset_name() {
            Some(p) => p.to_string(),
            None => return,
        };
        self.loading = Some("Applying...");
        Self::spawn_ipc(tx, IpcOp::ApplyPreset, Request::Start { device, preset });
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
    use crate::ipc::protocol::RecordEvent;
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

    // --- App handle_record_event tests ---

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
        let (tx, _rx) = mpsc::channel();
        app.handle_confirm(&ConfirmAction::DeleteMapping, &tx);
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
        let (tx, _rx) = mpsc::channel();
        app.handle_confirm(&ConfirmAction::DeleteMapping, &tx);
        assert!(app.entries.is_empty());
        assert_eq!(app.selected_entry, 0);
    }

    #[test]
    fn confirm_quit_sets_should_quit() {
        let mut app = make_app();
        let (tx, _rx) = mpsc::channel();
        app.handle_confirm(&ConfirmAction::Quit, &tx);
        assert!(app.should_quit);
    }

    // --- Screen navigation tests ---

    #[test]
    fn screen_is_config() {
        assert!(Screen::Devices.is_config());
        assert!(Screen::Presets.is_config());
        assert!(Screen::Editor.is_config());
        assert!(!Screen::Status.is_config());
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
    fn tab_toggles_config_and_status() {
        let mut app = make_app();
        let (tx, _rx) = mpsc::channel();
        // Starts at Devices (Config), Tab goes to Status
        assert_eq!(app.screen, Screen::Devices);
        app.handle_key(press(KeyCode::Tab), &tx);
        assert_eq!(app.screen, Screen::Status);
        // Tab from Status goes back to Devices (Config start)
        app.handle_key(press(KeyCode::Tab), &tx);
        assert_eq!(app.screen, Screen::Devices);
    }

    #[test]
    fn esc_from_editor_goes_to_devices() {
        let mut app = make_app();
        app.screen = Screen::Editor;
        let (tx, _rx) = mpsc::channel();
        app.handle_key(press(KeyCode::Esc), &tx);
        assert_eq!(app.screen, Screen::Devices);
    }

    #[test]
    fn esc_from_presets_goes_to_devices() {
        let mut app = make_app();
        app.screen = Screen::Presets;
        let (tx, _rx) = mpsc::channel();
        app.handle_key(press(KeyCode::Esc), &tx);
        assert_eq!(app.screen, Screen::Devices);
    }

    // --- Editor list navigation tests ---

    #[test]
    fn editor_move_down_and_up() {
        let mut app = make_app();
        app.screen = Screen::Editor;
        app.entries = vec![
            MappingEntry {
                input_combination: vec![InputConfig { event_type: 1, code: 30, origin_hash: None }],
                target_uinput: "keyboard".into(),
                output_symbol: Some("KEY_B".into()),
                name: Some("KEY_A".into()),
                mapping_type: "key_macro".into(),
            },
            MappingEntry {
                input_combination: vec![InputConfig { event_type: 1, code: 48, origin_hash: None }],
                target_uinput: "keyboard".into(),
                output_symbol: Some("KEY_C".into()),
                name: Some("KEY_B".into()),
                mapping_type: "key_macro".into(),
            },
        ];
        app.selected_entry = 0;
        let (tx, _rx) = mpsc::channel();

        app.handle_key(press(KeyCode::Down), &tx);
        assert_eq!(app.selected_entry, 1);

        app.handle_key(press(KeyCode::Up), &tx);
        assert_eq!(app.selected_entry, 0);
    }

    #[test]
    fn editor_move_up_at_top_stays() {
        let mut app = make_app();
        app.screen = Screen::Editor;
        app.entries = vec![MappingEntry {
            input_combination: vec![InputConfig { event_type: 1, code: 30, origin_hash: None }],
            target_uinput: "keyboard".into(),
            output_symbol: None,
            name: None,
            mapping_type: "key_macro".into(),
        }];
        app.selected_entry = 0;
        let (tx, _rx) = mpsc::channel();

        app.handle_key(press(KeyCode::Up), &tx);
        assert_eq!(app.selected_entry, 0);
    }

    #[test]
    fn editor_move_down_at_bottom_stays() {
        let mut app = make_app();
        app.screen = Screen::Editor;
        app.entries = vec![MappingEntry {
            input_combination: vec![InputConfig { event_type: 1, code: 30, origin_hash: None }],
            target_uinput: "keyboard".into(),
            output_symbol: None,
            name: None,
            mapping_type: "key_macro".into(),
        }];
        app.selected_entry = 0;
        let (tx, _rx) = mpsc::channel();

        app.handle_key(press(KeyCode::Down), &tx);
        assert_eq!(app.selected_entry, 0);
    }

    #[test]
    fn editor_enter_opens_symbol_search_with_slots() {
        let mut app = make_app();
        app.screen = Screen::Editor;
        app.entries = vec![MappingEntry {
            input_combination: vec![InputConfig { event_type: 1, code: 30, origin_hash: None }],
            target_uinput: "keyboard".into(),
            output_symbol: None,
            name: None,
            mapping_type: "key_macro".into(),
        }];
        let (tx, _rx) = mpsc::channel();

        app.handle_key(press(KeyCode::Enter), &tx);
        assert!(matches!(app.overlay, Overlay::SymbolSearch { .. }));
    }

    #[test]
    fn editor_enter_on_empty_does_nothing() {
        let mut app = make_app();
        app.screen = Screen::Editor;
        let (tx, _rx) = mpsc::channel();

        app.handle_key(press(KeyCode::Enter), &tx);
        assert!(matches!(app.overlay, Overlay::None));
    }

    #[test]
    fn editor_d_shows_confirm_dialog() {
        let mut app = make_app();
        app.screen = Screen::Editor;
        app.entries = vec![MappingEntry {
            input_combination: vec![InputConfig { event_type: 1, code: 30, origin_hash: None }],
            target_uinput: "keyboard".into(),
            output_symbol: Some("KEY_B".into()),
            name: Some("KEY_A".into()),
            mapping_type: "key_macro".into(),
        }];
        let (tx, _rx) = mpsc::channel();

        app.handle_key(press(KeyCode::Char('d')), &tx);
        assert!(matches!(
            app.overlay,
            Overlay::Confirm { action: ConfirmAction::DeleteMapping, .. }
        ));
    }

    #[test]
    fn editor_d_on_empty_does_nothing() {
        let mut app = make_app();
        app.screen = Screen::Editor;
        let (tx, _rx) = mpsc::channel();

        app.handle_key(press(KeyCode::Char('d')), &tx);
        assert!(matches!(app.overlay, Overlay::None));
    }

    // --- Symbol search overlay ---

    #[test]
    fn open_symbol_search_sets_overlay() {
        let mut app = make_app();
        app.open_symbol_search_with_slots(Vec::new(), 0);
        if let Overlay::SymbolSearch { query, filtered, cursor, slots, slot_cursor } = &app.overlay {
            assert!(query.is_empty());
            assert_eq!(*cursor, 0);
            assert_eq!(filtered.len(), app.symbols.len());
            assert!(slots.is_empty());
            assert_eq!(*slot_cursor, 0);
        } else {
            panic!("Expected SymbolSearch overlay");
        }
    }

    #[test]
    fn refilter_symbols_updates_filtered() {
        let mut app = make_app();
        app.open_symbol_search_with_slots(Vec::new(), 0);
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
