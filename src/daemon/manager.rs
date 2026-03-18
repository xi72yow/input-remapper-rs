use std::collections::HashMap;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread::JoinHandle;

use crate::device::discover;
use crate::ipc::protocol::{InjectionStatus, Request, Response};
use crate::mapping::config;

/// Validate that a device or preset name doesn't contain path traversal characters.
pub(crate) fn validate_name(name: &str, label: &str) -> Result<(), Response> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name == "."
        || name == ".."
        || name.contains("..")
    {
        Err(Response::Error {
            message: format!("Invalid {} name: '{}'", label, name),
        })
    } else {
        Ok(())
    }
}

struct RunningInjection {
    device_name: String,
    preset_name: String,
    stop_writer: UnixStream,
    thread: Option<JoinHandle<()>>,
}

impl RunningInjection {
    fn signal_stop(&mut self) {
        // Writing a byte wakes up the poll loop; if the write fails
        // (e.g. reader already dropped), that's fine too.
        let _ = self.stop_writer.write_all(&[1]);
    }
}

pub struct DaemonManager {
    config_dir: PathBuf,
    xmodmap: config::SymbolMap,
    injections: HashMap<String, RunningInjection>,
    debug: bool,
}

impl DaemonManager {
    pub fn new(config_dir: PathBuf, debug: bool) -> Self {
        let xmodmap_path = config_dir.join("xmodmap.json");
        let xmodmap = if xmodmap_path.exists() {
            config::load_symbol_map(&xmodmap_path).unwrap_or_default()
        } else {
            config::SymbolMap::new()
        };

        Self {
            config_dir,
            xmodmap,
            injections: HashMap::new(),
            debug,
        }
    }

    pub fn handle_request(&mut self, request: Request) -> Response {
        match request {
            Request::Start { device, preset } => self.start_injection(&device, &preset),
            Request::Stop { device } => self.stop_injection(&device),
            Request::StopAll => self.stop_all(),
            Request::Status => self.status(),
            Request::Autoload => self.autoload(),
            Request::ListPresets { device } => self.list_presets(&device),
            Request::GetPreset { device, preset } => self.get_preset(&device, &preset),
            Request::SavePreset {
                device,
                preset,
                entries,
            } => self.save_preset(&device, &preset, &entries),
            Request::DeletePreset { device, preset } => self.delete_preset(&device, &preset),
            // ListDevices, GetDeviceKeys and Record are handled by the server directly
            Request::ListDevices | Request::GetDeviceKeys { .. } | Request::Record { .. } => {
                Response::Error {
                    message: "Should be handled by server".into(),
                }
            }
        }
    }

    fn start_injection(&mut self, device_name: &str, preset_name: &str) -> Response {
        if let Err(e) = validate_name(device_name, "device") { return e; }
        if let Err(e) = validate_name(preset_name, "preset") { return e; }

        // Stop existing injection for this device
        if self.injections.contains_key(device_name) {
            self.stop_injection(device_name);
        }

        // Find device
        let dev_info = match discover::find_device_by_name(device_name) {
            Some(info) => info,
            None => {
                return Response::Error {
                    message: format!("Device '{}' not found", device_name),
                }
            }
        };

        // Load preset
        let preset_path = self
            .config_dir
            .join(&dev_info.name)
            .join(format!("{}.json", preset_name));

        if !preset_path.exists() {
            return Response::Error {
                message: format!("Preset not found: {}", preset_path.display()),
            };
        }

        let entries = match config::load_preset(&preset_path) {
            Ok(e) => e,
            Err(e) => {
                return Response::Error {
                    message: format!("Failed to load preset: {}", e),
                }
            }
        };

        let xmodmap = self.xmodmap.clone();
        let debug = self.debug;
        let device_paths = dev_info.paths.clone();
        let dev_name_for_log = dev_info.name.clone();
        let preset_for_log = preset_name.to_string();

        // Create the service and stop signal before spawning the thread
        let mut service =
            crate::daemon::service::InjectionService::from_entries(
                device_paths, &entries, &xmodmap, debug,
            );

        let stop_writer = match service.create_stop_signal() {
            Ok(w) => w,
            Err(e) => {
                return Response::Error {
                    message: format!("Failed to create stop signal: {}", e),
                }
            }
        };

        let thread = std::thread::spawn(move || {
            eprintln!(
                "Starting injection for '{}' with preset '{}'",
                dev_name_for_log, preset_for_log
            );

            if let Err(e) = service.run() {
                eprintln!("Injection error for '{}': {}", dev_name_for_log, e);
            }

            eprintln!("Injection stopped for '{}'", dev_name_for_log);
        });

        let device_key = dev_info.name.clone();
        self.injections.insert(
            device_key,
            RunningInjection {
                device_name: dev_info.name,
                preset_name: preset_name.to_string(),
                stop_writer,
                thread: Some(thread),
            },
        );

        Response::Ok {
            message: format!("Started injection for '{}'", device_name),
        }
    }

    fn stop_injection(&mut self, device_name: &str) -> Response {
        if let Some(mut injection) = self.injections.remove(device_name) {
            injection.signal_stop();
            if let Some(thread) = injection.thread.take() {
                let _ = thread.join();
            }
            Response::Ok {
                message: format!("Stopped injection for '{}'", device_name),
            }
        } else {
            Response::Error {
                message: format!("No injection running for '{}'", device_name),
            }
        }
    }

    fn stop_all(&mut self) -> Response {
        let device_names: Vec<String> = self.injections.keys().cloned().collect();
        // Signal all to stop
        for name in &device_names {
            if let Some(injection) = self.injections.get_mut(name) {
                injection.signal_stop();
            }
        }
        // Wait for all threads
        for name in &device_names {
            if let Some(mut injection) = self.injections.remove(name) {
                if let Some(thread) = injection.thread.take() {
                    let _ = thread.join();
                }
            }
        }
        Response::Ok {
            message: format!("Stopped {} injection(s)", device_names.len()),
        }
    }

    fn status(&self) -> Response {
        let injections: Vec<InjectionStatus> = self
            .injections
            .values()
            .map(|inj| InjectionStatus {
                device: inj.device_name.clone(),
                preset: inj.preset_name.clone(),
            })
            .collect();
        Response::Status { injections }
    }

    fn list_presets(&self, device_name: &str) -> Response {
        if let Err(e) = validate_name(device_name, "device") { return e; }
        let device_dir = self.config_dir.join(device_name);
        if !device_dir.exists() {
            return Response::Presets {
                presets: Vec::new(),
            };
        }

        let mut presets = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&device_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    if let Some(stem) = path.file_stem() {
                        presets.push(stem.to_string_lossy().to_string());
                    }
                }
            }
        }
        presets.sort();
        Response::Presets { presets }
    }

    fn get_preset(&self, device_name: &str, preset_name: &str) -> Response {
        if let Err(e) = validate_name(device_name, "device") { return e; }
        if let Err(e) = validate_name(preset_name, "preset") { return e; }
        let preset_path = self
            .config_dir
            .join(device_name)
            .join(format!("{}.json", preset_name));

        match config::load_preset(&preset_path) {
            Ok(entries) => Response::PresetData { entries },
            Err(e) => Response::Error {
                message: format!("Failed to load preset: {}", e),
            },
        }
    }

    fn save_preset(
        &self,
        device_name: &str,
        preset_name: &str,
        entries: &[config::MappingEntry],
    ) -> Response {
        if let Err(e) = validate_name(device_name, "device") { return e; }
        if let Err(e) = validate_name(preset_name, "preset") { return e; }
        let device_dir = self.config_dir.join(device_name);
        if let Err(e) = std::fs::create_dir_all(&device_dir) {
            return Response::Error {
                message: format!("Failed to create directory: {}", e),
            };
        }

        let preset_path = device_dir.join(format!("{}.json", preset_name));
        let json = match serde_json::to_string_pretty(entries) {
            Ok(j) => j,
            Err(e) => {
                return Response::Error {
                    message: format!("Failed to serialize preset: {}", e),
                }
            }
        };

        match std::fs::write(&preset_path, json) {
            Ok(()) => Response::Ok {
                message: format!("Saved preset '{}'", preset_name),
            },
            Err(e) => Response::Error {
                message: format!("Failed to write preset: {}", e),
            },
        }
    }

    fn delete_preset(&self, device_name: &str, preset_name: &str) -> Response {
        if let Err(e) = validate_name(device_name, "device") { return e; }
        if let Err(e) = validate_name(preset_name, "preset") { return e; }
        let preset_path = self
            .config_dir
            .join(device_name)
            .join(format!("{}.json", preset_name));

        if !preset_path.exists() {
            return Response::Error {
                message: format!("Preset '{}' not found", preset_name),
            };
        }

        match std::fs::remove_file(&preset_path) {
            Ok(()) => Response::Ok {
                message: format!("Deleted preset '{}'", preset_name),
            },
            Err(e) => Response::Error {
                message: format!("Failed to delete preset: {}", e),
            },
        }
    }

    fn autoload(&mut self) -> Response {
        let config_path = self.config_dir.join("config.json");
        let global = match config::load_global_config(&config_path) {
            Ok(c) => c,
            Err(e) => {
                return Response::Error {
                    message: format!("Failed to load config: {}", e),
                }
            }
        };

        let mut started = 0;
        let mut errors = Vec::new();

        for (device, preset) in &global.autoload {
            match self.start_injection(device, preset) {
                Response::Ok { .. } => started += 1,
                Response::Error { message } => errors.push(message),
                _ => {}
            }
        }

        if errors.is_empty() {
            Response::Ok {
                message: format!("Autoloaded {} device(s)", started),
            }
        } else {
            Response::Error {
                message: format!(
                    "Autoloaded {} device(s), {} error(s): {}",
                    started,
                    errors.len(),
                    errors.join("; ")
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::{Request, Response};
    use crate::mapping::config::MappingEntry;
    use std::fs;
    use tempfile::TempDir;

    fn make_manager(tmp: &TempDir) -> DaemonManager {
        DaemonManager::new(tmp.path().to_path_buf(), false)
    }

    fn sample_entry() -> MappingEntry {
        MappingEntry {
            input_combination: vec![crate::mapping::config::InputConfig {
                event_type: 1,
                code: 30,
                origin_hash: None,
            }],
            target_uinput: "keyboard".to_string(),
            output_symbol: Some("b".to_string()),
            name: Some("test mapping".to_string()),
            mapping_type: "key_macro".to_string(),
        }
    }

    // ── validate_name ──────────────────────────────────────────

    #[test]
    fn validate_name_accepts_valid_names() {
        assert!(validate_name("my-device", "device").is_ok());
        assert!(validate_name("Logitech G Pro", "device").is_ok());
        assert!(validate_name("preset_1", "preset").is_ok());
    }

    #[test]
    fn validate_name_rejects_empty() {
        assert!(validate_name("", "device").is_err());
    }

    #[test]
    fn validate_name_rejects_slash() {
        assert!(validate_name("foo/bar", "device").is_err());
        assert!(validate_name("foo\\bar", "device").is_err());
    }

    #[test]
    fn validate_name_rejects_dot_dot() {
        assert!(validate_name("..", "device").is_err());
        assert!(validate_name(".", "device").is_err());
        assert!(validate_name("foo..bar", "device").is_err());
    }

    #[test]
    fn validate_name_rejects_null_byte() {
        assert!(validate_name("foo\0bar", "device").is_err());
    }

    // ── DaemonManager::status ──────────────────────────────────

    #[test]
    fn status_empty_returns_no_injections() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::Status) {
            Response::Status { injections } => assert!(injections.is_empty()),
            other => panic!("Expected Status, got {:?}", other),
        }
    }

    // ── DaemonManager::stop_injection ──────────────────────────

    #[test]
    fn stop_nonexistent_device_returns_error() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::Stop {
            device: "nonexistent".into(),
        }) {
            Response::Error { message } => {
                assert!(message.contains("No injection running"));
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // ── DaemonManager::stop_all ────────────────────────────────

    #[test]
    fn stop_all_with_no_injections() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::StopAll) {
            Response::Ok { message } => {
                assert!(message.contains("0 injection(s)"));
            }
            other => panic!("Expected Ok, got {:?}", other),
        }
    }

    // ── DaemonManager::list_presets ────────────────────────────

    #[test]
    fn list_presets_empty_device_dir() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::ListPresets {
            device: "my-device".into(),
        }) {
            Response::Presets { presets } => assert!(presets.is_empty()),
            other => panic!("Expected Presets, got {:?}", other),
        }
    }

    #[test]
    fn list_presets_finds_json_files() {
        let tmp = TempDir::new().unwrap();
        let device_dir = tmp.path().join("my-device");
        fs::create_dir_all(&device_dir).unwrap();
        fs::write(device_dir.join("alpha.json"), "[]").unwrap();
        fs::write(device_dir.join("beta.json"), "[]").unwrap();
        fs::write(device_dir.join("readme.txt"), "ignore me").unwrap();

        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::ListPresets {
            device: "my-device".into(),
        }) {
            Response::Presets { presets } => {
                assert_eq!(presets, vec!["alpha", "beta"]);
            }
            other => panic!("Expected Presets, got {:?}", other),
        }
    }

    #[test]
    fn list_presets_rejects_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::ListPresets {
            device: "../etc".into(),
        }) {
            Response::Error { message } => {
                assert!(message.contains("Invalid"));
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // ── DaemonManager::save_preset ─────────────────────────────

    #[test]
    fn save_and_get_preset() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        let entries = vec![sample_entry()];

        // Save
        match mgr.handle_request(Request::SavePreset {
            device: "dev1".into(),
            preset: "my-preset".into(),
            entries: entries.clone(),
        }) {
            Response::Ok { .. } => {}
            other => panic!("Expected Ok, got {:?}", other),
        }

        // Verify file exists
        assert!(tmp.path().join("dev1/my-preset.json").exists());

        // Get it back
        match mgr.handle_request(Request::GetPreset {
            device: "dev1".into(),
            preset: "my-preset".into(),
        }) {
            Response::PresetData {
                entries: loaded_entries,
            } => {
                assert_eq!(loaded_entries.len(), 1);
                assert_eq!(
                    loaded_entries[0].output_symbol,
                    Some("b".to_string())
                );
            }
            other => panic!("Expected PresetData, got {:?}", other),
        }
    }

    #[test]
    fn save_preset_rejects_traversal() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::SavePreset {
            device: "dev1".into(),
            preset: "../evil".into(),
            entries: vec![],
        }) {
            Response::Error { message } => assert!(message.contains("Invalid")),
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // ── DaemonManager::get_preset ──────────────────────────────

    #[test]
    fn get_preset_not_found() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::GetPreset {
            device: "dev1".into(),
            preset: "nonexistent".into(),
        }) {
            Response::Error { message } => {
                assert!(message.contains("Failed to load preset"));
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // ── DaemonManager::delete_preset ───────────────────────────

    #[test]
    fn delete_preset_removes_file() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);

        // Create preset first
        mgr.handle_request(Request::SavePreset {
            device: "dev1".into(),
            preset: "to-delete".into(),
            entries: vec![sample_entry()],
        });

        let path = tmp.path().join("dev1/to-delete.json");
        assert!(path.exists());

        match mgr.handle_request(Request::DeletePreset {
            device: "dev1".into(),
            preset: "to-delete".into(),
        }) {
            Response::Ok { .. } => {}
            other => panic!("Expected Ok, got {:?}", other),
        }
        assert!(!path.exists());
    }

    #[test]
    fn delete_preset_not_found() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::DeletePreset {
            device: "dev1".into(),
            preset: "nope".into(),
        }) {
            Response::Error { message } => assert!(message.contains("not found")),
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn delete_preset_rejects_traversal() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::DeletePreset {
            device: "..".into(),
            preset: "evil".into(),
        }) {
            Response::Error { message } => assert!(message.contains("Invalid")),
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // ── DaemonManager::start_injection (device not found) ──────

    #[test]
    fn start_injection_device_not_found() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::Start {
            device: "nonexistent-device-xyz".into(),
            preset: "my-preset".into(),
        }) {
            Response::Error { message } => {
                assert!(message.contains("not found"));
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn start_injection_rejects_traversal_device() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::Start {
            device: "../etc".into(),
            preset: "test".into(),
        }) {
            Response::Error { message } => assert!(message.contains("Invalid")),
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn start_injection_rejects_traversal_preset() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::Start {
            device: "dev1".into(),
            preset: "../evil".into(),
        }) {
            Response::Error { message } => assert!(message.contains("Invalid")),
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // ── DaemonManager::autoload ────────────────────────────────

    #[test]
    fn autoload_no_config_returns_error() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::Autoload) {
            Response::Error { message } => {
                assert!(message.contains("Failed to load config"));
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn autoload_empty_config() {
        let tmp = TempDir::new().unwrap();
        let config = r#"{"version":"1.0","autoload":{}}"#;
        fs::write(tmp.path().join("config.json"), config).unwrap();

        let mut mgr = make_manager(&tmp);
        match mgr.handle_request(Request::Autoload) {
            Response::Ok { message } => {
                assert!(message.contains("0 device(s)"));
            }
            other => panic!("Expected Ok, got {:?}", other),
        }
    }

    // ── DaemonManager::handle_request dispatches correctly ─────

    #[test]
    fn handle_request_server_only_requests_return_error() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = make_manager(&tmp);

        match mgr.handle_request(Request::ListDevices) {
            Response::Error { message } => {
                assert!(message.contains("handled by server"));
            }
            other => panic!("Expected Error, got {:?}", other),
        }

        match mgr.handle_request(Request::GetDeviceKeys {
            device: "dev".into(),
        }) {
            Response::Error { message } => {
                assert!(message.contains("handled by server"));
            }
            other => panic!("Expected Error, got {:?}", other),
        }

        match mgr.handle_request(Request::Record {
            device: "dev".into(),
        }) {
            Response::Error { message } => {
                assert!(message.contains("handled by server"));
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // ── DaemonManager::new with xmodmap ────────────────────────

    #[test]
    fn new_loads_xmodmap_when_present() {
        let tmp = TempDir::new().unwrap();
        let xmodmap = r#"{"Control_L": 29, "a": 30}"#;
        fs::write(tmp.path().join("xmodmap.json"), xmodmap).unwrap();

        let mgr = make_manager(&tmp);
        assert_eq!(mgr.xmodmap.get("Control_L"), Some(&29));
        assert_eq!(mgr.xmodmap.get("a"), Some(&30));
    }

    #[test]
    fn new_handles_missing_xmodmap() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        assert!(mgr.xmodmap.is_empty());
    }

    #[test]
    fn new_handles_invalid_xmodmap() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("xmodmap.json"), "not json").unwrap();

        let mgr = make_manager(&tmp);
        assert!(mgr.xmodmap.is_empty()); // falls back to default
    }
}
