mod daemon;
mod device;
mod ipc;
mod mapping;

use clap::{Parser, Subcommand};
use mapping::config;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "input-remapper-rs", version, about = "Remap input device events")]
struct Cli {
    /// Config directory path
    #[arg(long, default_value = "~/.config/input-remapper-rs")]
    config_dir: String,

    /// Enable debug output
    #[arg(long, global = true)]
    debug: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all input devices
    ListDevices {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start remapping a device (via daemon)
    Start {
        /// Device name or key
        #[arg(long)]
        device: String,
        /// Preset name
        #[arg(long)]
        preset: String,
    },
    /// Stop remapping a device
    Stop {
        /// Device name or key
        #[arg(long)]
        device: String,
    },
    /// Stop all running injections
    StopAll,
    /// Show status of running injections
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start the daemon (for systemd)
    Daemon,
    /// Autoload presets from config
    Autoload,
    /// Start a single injection in foreground (without daemon)
    RunForeground {
        /// Device name or key
        #[arg(long)]
        device: String,
        /// Preset name
        #[arg(long)]
        preset: String,
    },
    /// Record events from a device (for mapping in GUI)
    Record {
        /// Device name or key
        #[arg(long)]
        device: String,
    },
}

fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

fn main() {
    let cli = Cli::parse();
    let config_dir = expand_tilde(&cli.config_dir);

    match cli.command {
        Commands::ListDevices { json } => {
            let devices = device::discover::discover_devices();
            if json {
                println!("{}", serde_json::to_string_pretty(&devices).unwrap());
            } else {
                for dev in &devices {
                    println!("{}", dev.name);
                    for path in &dev.paths {
                        println!("  {}", path.display());
                    }
                }
            }
        }
        Commands::Daemon => {
            use std::sync::{Arc, Mutex};
            let manager = daemon::manager::DaemonManager::new(config_dir, cli.debug);
            let manager = Arc::new(Mutex::new(manager));

            // Handle SIGTERM/SIGINT gracefully
            let mgr_clone = Arc::clone(&manager);
            ctrlc::set_handler(move || {
                eprintln!("Shutting down daemon...");
                let mut mgr = mgr_clone.lock().unwrap();
                mgr.handle_request(ipc::protocol::Request::StopAll);
                std::process::exit(0);
            })
            .expect("Failed to set signal handler");

            let server = ipc::server::IpcServer::new(manager).unwrap_or_else(|e| {
                eprintln!("Failed to start daemon: {}", e);
                std::process::exit(1);
            });

            if let Err(e) = server.run() {
                eprintln!("Daemon error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Start { device, preset } => {
            let request = ipc::protocol::Request::Start { device, preset };
            match ipc::client::send_request(&request) {
                Ok(response) => print_response(&response),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Stop { device } => {
            let request = ipc::protocol::Request::Stop { device };
            match ipc::client::send_request(&request) {
                Ok(response) => print_response(&response),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::StopAll => {
            let request = ipc::protocol::Request::StopAll;
            match ipc::client::send_request(&request) {
                Ok(response) => print_response(&response),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Status { json } => {
            let request = ipc::protocol::Request::Status;
            match ipc::client::send_request(&request) {
                Ok(ipc::protocol::Response::Status { injections }) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&injections).unwrap());
                    } else if injections.is_empty() {
                        println!("No active injections.");
                    } else {
                        for inj in &injections {
                            println!("{} -> {}", inj.device, inj.preset);
                        }
                    }
                }
                Ok(response) => print_response(&response),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Autoload => {
            let request = ipc::protocol::Request::Autoload;
            match ipc::client::send_request(&request) {
                Ok(response) => print_response(&response),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::RunForeground { device, preset } => {
            let xmodmap_path = config_dir.join("xmodmap.json");
            let xmodmap = if xmodmap_path.exists() {
                config::load_symbol_map(&xmodmap_path).unwrap_or_default()
            } else {
                config::SymbolMap::new()
            };

            let dev_info = device::discover::find_device_by_name(&device)
                .unwrap_or_else(|| {
                    eprintln!("Device '{}' not found", device);
                    std::process::exit(1);
                });

            let preset_path = config_dir
                .join(&dev_info.name)
                .join(format!("{}.json", preset));

            if !preset_path.exists() {
                eprintln!("Preset not found: {}", preset_path.display());
                std::process::exit(1);
            }

            println!("Starting injection for '{}' with preset '{}'", dev_info.name, preset);

            let mut service = daemon::service::InjectionService::new(
                dev_info.paths,
                &preset_path,
                &xmodmap,
                cli.debug,
            )
            .unwrap_or_else(|e| {
                eprintln!("Failed to create service: {}", e);
                std::process::exit(1);
            });

            // Stop on Ctrl+C via stop signal
            let stop_writer = service
                .create_stop_signal()
                .expect("Failed to create stop signal");
            let stop_writer = std::sync::Mutex::new(Some(stop_writer));
            ctrlc::set_handler(move || {
                // Drop the writer to wake up the poll loop
                let _ = stop_writer.lock().unwrap().take();
            })
            .expect("Failed to set signal handler");

            if let Err(e) = service.run() {
                eprintln!("Injection error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Record { device } => {
            let dev_info = device::discover::find_device_by_name(&device)
                .unwrap_or_else(|| {
                    eprintln!("Device '{}' not found", device);
                    std::process::exit(1);
                });

            println!("Recording events from '{}'. Press Ctrl+C to stop.", dev_info.name);

            let mut reader = device::reader::DeviceReader::open(&dev_info.paths[0])
                .unwrap_or_else(|e| {
                    eprintln!("Failed to open device: {}", e);
                    std::process::exit(1);
                });

            loop {
                match reader.read_events(1000) {
                    Ok(Some(events)) => {
                        for event in &events {
                            println!(
                                "{{ \"type\": {}, \"code\": {}, \"value\": {} }}",
                                event.event_type().0,
                                event.code(),
                                event.value()
                            );
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        break;
                    }
                }
            }
        }
    }
}

fn print_response(response: &ipc::protocol::Response) {
    match response {
        ipc::protocol::Response::Ok { message } => println!("{}", message),
        ipc::protocol::Response::Error { message } => {
            eprintln!("Error: {}", message);
            std::process::exit(1);
        }
        ipc::protocol::Response::Status { injections } => {
            for inj in injections {
                println!("{} -> {}", inj.device, inj.preset);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::device::{reader::DeviceReader, writer::DeviceWriter};
    use crate::mapping::{config, handler::MappingHandler};
    use evdev::{uinput::VirtualDevice, AttributeSet, EventType, InputEvent, KeyCode};

    fn find_device_path(name: &str) -> std::path::PathBuf {
        evdev::enumerate()
            .find(|(_, d)| d.name().is_some_and(|n| n == name))
            .map(|(path, _)| path)
            .unwrap_or_else(|| panic!("Device '{}' not found", name))
    }

    #[test]
    fn fake_device_roundtrip() {
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode::KEY_A);
        keys.insert(KeyCode::KEY_B);

        let mut virt = VirtualDevice::builder()
            .unwrap()
            .name("input-remapper-test-roundtrip")
            .with_keys(&keys)
            .unwrap()
            .build()
            .unwrap();

        let devpath = find_device_path("input-remapper-test-roundtrip");
        let mut reader = DeviceReader::open(&devpath).unwrap();
        reader.grab().unwrap();

        let press = InputEvent::new(EventType::KEY.0, KeyCode::KEY_A.0, 1);
        let release = InputEvent::new(EventType::KEY.0, KeyCode::KEY_A.0, 0);
        let syn = InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0);
        virt.emit(&[press, syn, release, syn]).unwrap();

        let events = reader.read_events(2000).unwrap().expect("Should receive events");
        let key_events: Vec<&InputEvent> = events
            .iter()
            .filter(|e| e.event_type() == EventType::KEY)
            .collect();

        assert_eq!(key_events.len(), 2);
        assert_eq!(key_events[0].code(), KeyCode::KEY_A.0);
        assert_eq!(key_events[0].value(), 1);
        assert_eq!(key_events[1].code(), KeyCode::KEY_A.0);
        assert_eq!(key_events[1].value(), 0);
    }

    #[test]
    fn passthrough() {
        // Source: fake input device
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode::KEY_X);
        keys.insert(KeyCode::KEY_Y);

        let mut source = VirtualDevice::builder()
            .unwrap()
            .name("input-remapper-test-source")
            .with_keys(&keys)
            .unwrap()
            .build()
            .unwrap();

        let source_path = find_device_path("input-remapper-test-source");
        let mut reader = DeviceReader::open(&source_path).unwrap();
        reader.grab().unwrap();

        // Sink: our virtual output device
        let mut writer = DeviceWriter::new_keyboard("input-remapper-test-sink").unwrap();
        let sink_path = find_device_path("input-remapper-test-sink");
        let mut sink_reader = DeviceReader::open(&sink_path).unwrap();
        sink_reader.grab().unwrap();

        // Send KEY_X from source
        let press = InputEvent::new(EventType::KEY.0, KeyCode::KEY_X.0, 1);
        let release = InputEvent::new(EventType::KEY.0, KeyCode::KEY_X.0, 0);
        let syn = InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0);
        source.emit(&[press, syn, release, syn]).unwrap();

        // Read from source
        let events = reader.read_events(2000).unwrap().expect("Should receive source events");

        // Passthrough: write all events to sink
        writer.emit(&events).unwrap();

        // Read from sink and verify
        let output = sink_reader
            .read_events(2000)
            .unwrap()
            .expect("Should receive sink events");

        let key_events: Vec<&InputEvent> = output
            .iter()
            .filter(|e| e.event_type() == EventType::KEY)
            .collect();

        assert_eq!(key_events.len(), 2);
        assert_eq!(key_events[0].code(), KeyCode::KEY_X.0);
        assert_eq!(key_events[0].value(), 1);
        assert_eq!(key_events[1].code(), KeyCode::KEY_X.0);
        assert_eq!(key_events[1].value(), 0);
    }

    #[test]
    fn discover_devices() {
        let _virt = VirtualDevice::builder()
            .unwrap()
            .name("input-remapper-test-discover")
            .with_keys(&{
                let mut keys = AttributeSet::<KeyCode>::new();
                keys.insert(KeyCode::KEY_A);
                keys
            })
            .unwrap()
            .build()
            .unwrap();

        // Retry a few times — device node creation can be async
        let mut found = false;
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let devices = crate::device::discover::discover_devices();
            if devices.iter().any(|d| d.name.contains("input-remapper-test-discover")) {
                found = true;
                break;
            }
        }
        assert!(found, "Should discover our virtual device");
    }

    #[test]
    fn remap_key_to_key() {
        // Preset: Button code 2 → XF86Back (keycode 158)
        let preset_json = r#"[
            {
                "input_combination": [{ "type": 1, "code": 2 }],
                "target_uinput": "keyboard",
                "output_symbol": "XF86Back",
                "mapping_type": "key_macro"
            }
        ]"#;
        let xmodmap_json = r#"{ "XF86Back": 158 }"#;

        let entries: Vec<config::MappingEntry> = serde_json::from_str(preset_json).unwrap();
        let xmodmap: config::SymbolMap = serde_json::from_str(xmodmap_json).unwrap();
        let handler = MappingHandler::from_preset(&entries, &xmodmap, false);

        // Press button 2
        let press = InputEvent::new(EventType::KEY.0, 2, 1);
        let mut result = Vec::new();
        handler.remap_into(&press, &mut result);
        let keys: Vec<&InputEvent> = result.iter().filter(|e| e.event_type() == EventType::KEY).collect();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].code(), 158); // XF86Back
        assert_eq!(keys[0].value(), 1);

        // Release button 2
        let release = InputEvent::new(EventType::KEY.0, 2, 0);
        result.clear();
        handler.remap_into(&release, &mut result);
        let keys: Vec<&InputEvent> = result.iter().filter(|e| e.event_type() == EventType::KEY).collect();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].code(), 158);
        assert_eq!(keys[0].value(), 0);
    }

    #[test]
    fn remap_combination_emits_single_syn() {
        // Verify that a key combination (e.g. Ctrl+C) only emits one SYN_REPORT
        // at the end, not one after each key.
        let preset_json = r#"[
            {
                "input_combination": [{ "type": 1, "code": 2 }],
                "target_uinput": "keyboard",
                "output_symbol": "Control_L + c",
                "mapping_type": "key_macro"
            }
        ]"#;
        let xmodmap_json = r#"{ "Control_L": 29, "c": 46 }"#;

        let entries: Vec<config::MappingEntry> = serde_json::from_str(preset_json).unwrap();
        let xmodmap: config::SymbolMap = serde_json::from_str(xmodmap_json).unwrap();
        let handler = MappingHandler::from_preset(&entries, &xmodmap, false);

        let press = InputEvent::new(EventType::KEY.0, 2, 1);
        let mut result = Vec::new();
        handler.remap_into(&press, &mut result);

        let syn_count = result.iter().filter(|e| e.event_type() == EventType::SYNCHRONIZATION).count();
        assert_eq!(syn_count, 1, "Combination press should emit exactly one SYN_REPORT");
        // SYN should be the last event
        assert_eq!(result.last().unwrap().event_type(), EventType::SYNCHRONIZATION);

        // Same for release
        result.clear();
        let release = InputEvent::new(EventType::KEY.0, 2, 0);
        handler.remap_into(&release, &mut result);

        let syn_count = result.iter().filter(|e| e.event_type() == EventType::SYNCHRONIZATION).count();
        assert_eq!(syn_count, 1, "Combination release should emit exactly one SYN_REPORT");
        assert_eq!(result.last().unwrap().event_type(), EventType::SYNCHRONIZATION);
    }

    #[test]
    fn remap_key_to_combination() {
        // Preset: Button code 2 → Ctrl+C
        let preset_json = r#"[
            {
                "input_combination": [{ "type": 1, "code": 2 }],
                "target_uinput": "keyboard",
                "output_symbol": "Control_L + c",
                "mapping_type": "key_macro"
            }
        ]"#;
        let xmodmap_json = r#"{ "Control_L": 29, "c": 46 }"#;

        let entries: Vec<config::MappingEntry> = serde_json::from_str(preset_json).unwrap();
        let xmodmap: config::SymbolMap = serde_json::from_str(xmodmap_json).unwrap();
        let handler = MappingHandler::from_preset(&entries, &xmodmap, false);

        // Press button 2 → should emit Ctrl press, C press
        let press = InputEvent::new(EventType::KEY.0, 2, 1);
        let mut result = Vec::new();
        handler.remap_into(&press, &mut result);
        let keys: Vec<&InputEvent> = result.iter().filter(|e| e.event_type() == EventType::KEY).collect();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].code(), 29);  // Control_L press
        assert_eq!(keys[0].value(), 1);
        assert_eq!(keys[1].code(), 46);  // c press
        assert_eq!(keys[1].value(), 1);

        // Release button 2 → should emit C release, Ctrl release (reverse order)
        let release = InputEvent::new(EventType::KEY.0, 2, 0);
        result.clear();
        handler.remap_into(&release, &mut result);
        let keys: Vec<&InputEvent> = result.iter().filter(|e| e.event_type() == EventType::KEY).collect();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].code(), 46);  // c release first
        assert_eq!(keys[0].value(), 0);
        assert_eq!(keys[1].code(), 29);  // Control_L release
        assert_eq!(keys[1].value(), 0);
    }

    #[test]
    fn remap_end_to_end() {
        // Full pipeline: fake device → remap → uinput → verify
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode(2)); // BTN_RIGHT / mouse button
        keys.insert(KeyCode(3));

        let mut source = VirtualDevice::builder()
            .unwrap()
            .name("input-remapper-test-remap-src")
            .with_keys(&keys)
            .unwrap()
            .build()
            .unwrap();

        let source_path = find_device_path("input-remapper-test-remap-src");
        let mut reader = DeviceReader::open(&source_path).unwrap();
        reader.grab().unwrap();

        let mut writer = DeviceWriter::new_keyboard("input-remapper-test-remap-sink").unwrap();
        let sink_path = find_device_path("input-remapper-test-remap-sink");
        let mut sink_reader = DeviceReader::open(&sink_path).unwrap();
        sink_reader.grab().unwrap();

        // Setup: button 2 → Ctrl+C
        let preset_json = r#"[{
            "input_combination": [{ "type": 1, "code": 2 }],
            "target_uinput": "keyboard",
            "output_symbol": "Control_L + c",
            "mapping_type": "key_macro"
        }]"#;
        let xmodmap_json = r#"{ "Control_L": 29, "c": 46 }"#;
        let entries: Vec<config::MappingEntry> = serde_json::from_str(preset_json).unwrap();
        let xmodmap: config::SymbolMap = serde_json::from_str(xmodmap_json).unwrap();
        let handler = MappingHandler::from_preset(&entries, &xmodmap, false);

        // Send button 2 press+release from source
        let press = InputEvent::new(EventType::KEY.0, 2, 1);
        let release = InputEvent::new(EventType::KEY.0, 2, 0);
        let syn = InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0);
        source.emit(&[press, syn, release, syn]).unwrap();

        // Read, remap, write
        let events = reader.read_events(2000).unwrap().expect("events");
        let mut remapped = Vec::new();
        for event in &events {
            handler.remap_into(event, &mut remapped);
        }
        writer.emit(&remapped).unwrap();

        // Verify output
        let output = sink_reader.read_events(2000).unwrap().expect("output events");
        let key_events: Vec<&InputEvent> = output
            .iter()
            .filter(|e| e.event_type() == EventType::KEY)
            .collect();

        // Press: Ctrl down, C down. Release: C up, Ctrl up = 4 key events
        assert_eq!(key_events.len(), 4);
        assert_eq!(key_events[0].code(), 29);  // Ctrl press
        assert_eq!(key_events[1].code(), 46);  // c press
        assert_eq!(key_events[2].code(), 46);  // c release
        assert_eq!(key_events[3].code(), 29);  // Ctrl release
    }
}
