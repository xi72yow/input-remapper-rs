use crate::device::{reader::DeviceReader, writer::DeviceWriter};
use crate::mapping::config::{self, SymbolMap};
use crate::mapping::handler::MappingHandler;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct InjectionService {
    device_paths: Vec<PathBuf>,
    handler: MappingHandler,
    stop_flag: Option<Arc<AtomicBool>>,
}

impl InjectionService {
    pub fn new(
        device_paths: Vec<PathBuf>,
        preset_path: &Path,
        xmodmap: &SymbolMap,
        debug: bool,
    ) -> std::io::Result<Self> {
        let entries = config::load_preset(preset_path)?;
        let handler = MappingHandler::from_preset(&entries, xmodmap, debug);

        Ok(Self {
            device_paths,
            handler,
            stop_flag: None,
        })
    }

    pub fn from_entries(
        device_paths: Vec<PathBuf>,
        entries: &[config::MappingEntry],
        xmodmap: &SymbolMap,
        debug: bool,
    ) -> Self {
        let handler = MappingHandler::from_preset(entries, xmodmap, debug);
        Self {
            device_paths,
            handler,
            stop_flag: None,
        }
    }

    /// Set an external stop flag (used by daemon manager)
    pub fn set_stop_flag(&mut self, flag: Arc<AtomicBool>) {
        self.stop_flag = Some(flag);
    }

    fn should_stop(&self) -> bool {
        self.stop_flag
            .as_ref()
            .is_some_and(|f| f.load(Ordering::Relaxed))
    }

    pub fn run(&mut self) -> std::io::Result<()> {
        let mut readers: Vec<DeviceReader> = Vec::new();
        for path in &self.device_paths {
            let mut reader = DeviceReader::open(path)?;
            reader.grab()?;
            readers.push(reader);
        }

        let mut writer = DeviceWriter::new_keyboard_mouse("input-remapper-rs")?;

        while !self.should_stop() {
            // Poll all FDs at once, collect which indices are ready
            let ready_indices = {
                let borrowed_fds: Vec<_> = readers.iter().map(|r| r.fd().as_fd()).collect();
                let mut pollfds: Vec<PollFd> = borrowed_fds
                    .iter()
                    .map(|fd| PollFd::new(*fd, PollFlags::POLLIN))
                    .collect();

                let ready = poll(&mut pollfds, PollTimeout::from(100u16))?;
                if ready == 0 {
                    continue;
                }

                pollfds
                    .iter()
                    .enumerate()
                    .filter(|(_, pfd)| {
                        pfd.revents()
                            .is_some_and(|r| r.contains(PollFlags::POLLIN))
                    })
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>()
            };
            // Borrow of readers is dropped here

            for i in ready_indices {
                match readers[i].fetch_events() {
                    Ok(events) => {
                        let mut remapped = Vec::new();
                        for event in &events {
                            remapped.extend(self.handler.remap(event));
                        }
                        if !remapped.is_empty() {
                            writer.emit(&remapped)?;
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading events from reader {}: {}", i, e);
                    }
                }
            }
        }

        Ok(())
    }
}
