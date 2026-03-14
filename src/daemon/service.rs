use crate::device::{reader::DeviceReader, writer::DeviceWriter};
use crate::mapping::config::{self, SymbolMap};
use crate::mapping::handler::MappingHandler;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

pub struct InjectionService {
    device_paths: Vec<PathBuf>,
    handler: MappingHandler,
    stop_reader: Option<UnixStream>,
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
            stop_reader: None,
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
            stop_reader: None,
        }
    }

    /// Create a stop signal pair. Returns the writer end that the caller
    /// can use to signal stop (by writing a byte or dropping it).
    pub fn create_stop_signal(&mut self) -> std::io::Result<UnixStream> {
        let (reader, writer) = UnixStream::pair()?;
        reader.set_nonblocking(true)?;
        self.stop_reader = Some(reader);
        Ok(writer)
    }

    pub fn run(&mut self) -> std::io::Result<()> {
        let mut readers: Vec<DeviceReader> = Vec::new();
        for path in &self.device_paths {
            let mut reader = DeviceReader::open(path)?;
            reader.grab()?;
            readers.push(reader);
        }

        let mut writer = DeviceWriter::new_keyboard_mouse("input-remapper-rs")?;
        let mut remapped = Vec::new();

        loop {
            let ready_indices = {
                let mut borrowed_fds: Vec<_> =
                    readers.iter().map(|r| r.fd().as_fd()).collect();

                // Append stop fd as the last entry
                let has_stop_fd = self.stop_reader.is_some();
                if let Some(ref stop) = self.stop_reader {
                    borrowed_fds.push(stop.as_fd());
                }

                let mut pollfds: Vec<PollFd> = borrowed_fds
                    .iter()
                    .map(|fd| PollFd::new(*fd, PollFlags::POLLIN))
                    .collect();

                poll(&mut pollfds, PollTimeout::NONE)?;

                // Check if stop fd was signaled
                if has_stop_fd {
                    let stop_idx = pollfds.len() - 1;
                    if pollfds[stop_idx]
                        .revents()
                        .is_some_and(|r| r.intersects(PollFlags::POLLIN | PollFlags::POLLHUP))
                    {
                        break;
                    }
                }

                // Check if any device fd has an error/hangup (device removed)
                let device_error = pollfds.iter().take(readers.len()).any(|pfd| {
                    pfd.revents().is_some_and(|r| {
                        r.intersects(PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL)
                    })
                });

                if device_error {
                    eprintln!("Device removed or error detected, stopping injection");
                    break;
                }

                pollfds
                    .iter()
                    .enumerate()
                    .take(readers.len())
                    .filter(|(_, pfd)| {
                        pfd.revents()
                            .is_some_and(|r| r.contains(PollFlags::POLLIN))
                    })
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>()
            };

            // Batch events from all ready devices into a single emit
            remapped.clear();
            for i in ready_indices {
                match readers[i].fetch_events() {
                    Ok(events) => {
                        for event in &events {
                            self.handler.remap_into(event, &mut remapped);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading events from reader {}: {}", i, e);
                    }
                }
            }
            if !remapped.is_empty() {
                writer.emit(&remapped)?;
            }
        }

        Ok(())
    }
}
