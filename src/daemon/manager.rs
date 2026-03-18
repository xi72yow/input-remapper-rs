use std::collections::HashMap;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread::JoinHandle;

use crate::device::discover;
use crate::ipc::protocol::{InjectionStatus, Request, Response};
use crate::mapping::config;

/// Validate that a device or preset name doesn't contain path traversal characters.
fn validate_name(name: &str, label: &str) -> Result<(), Response> {
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
