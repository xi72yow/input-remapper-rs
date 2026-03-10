use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::device::discover;
use crate::ipc::protocol::{InjectionStatus, Request, Response};
use crate::mapping::config;

struct RunningInjection {
    device_name: String,
    preset_name: String,
    stop_flag: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
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
        }
    }

    fn start_injection(&mut self, device_name: &str, preset_name: &str) -> Response {
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
            .join("presets")
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
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);
        let dev_name_for_log = dev_info.name.clone();
        let preset_for_log = preset_name.to_string();

        let thread = std::thread::spawn(move || {
            eprintln!(
                "Starting injection for '{}' with preset '{}'",
                dev_name_for_log, preset_for_log
            );

            let mut service =
                crate::daemon::service::InjectionService::from_entries(
                    device_paths, &entries, &xmodmap, debug,
                );
            service.set_stop_flag(stop_clone);

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
                stop_flag,
                thread: Some(thread),
            },
        );

        Response::Ok {
            message: format!("Started injection for '{}'", device_name),
        }
    }

    fn stop_injection(&mut self, device_name: &str) -> Response {
        if let Some(mut injection) = self.injections.remove(device_name) {
            injection.stop_flag.store(true, Ordering::Relaxed);
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
        for name in &device_names {
            if let Some(injection) = self.injections.get(name) {
                injection.stop_flag.store(true, Ordering::Relaxed);
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
