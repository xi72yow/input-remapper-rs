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

    #[cfg(test)]
    pub fn has_stop_signal(&self) -> bool {
        self.stop_reader.is_some()
    }

    #[cfg(test)]
    pub fn device_paths(&self) -> &[PathBuf] {
        &self.device_paths
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::config::{InputConfig, MappingEntry, SymbolMap};
    use std::io::Write;

    fn empty_xmodmap() -> SymbolMap {
        SymbolMap::new()
    }

    fn sample_entries() -> Vec<MappingEntry> {
        vec![MappingEntry {
            input_combination: vec![InputConfig {
                event_type: 1,
                code: 30,
                origin_hash: None,
            }],
            target_uinput: "keyboard".to_string(),
            output_symbol: Some("b".to_string()),
            name: Some("test".to_string()),
            mapping_type: "key_macro".to_string(),
        }]
    }

    #[test]
    fn from_entries_creates_service() {
        let service = InjectionService::from_entries(
            vec![PathBuf::from("/dev/input/event0")],
            &sample_entries(),
            &empty_xmodmap(),
            false,
        );
        assert_eq!(service.device_paths(), &[PathBuf::from("/dev/input/event0")]);
        assert!(!service.has_stop_signal());
    }

    #[test]
    fn from_entries_empty() {
        let service = InjectionService::from_entries(
            vec![],
            &[],
            &empty_xmodmap(),
            false,
        );
        assert!(service.device_paths().is_empty());
    }

    #[test]
    fn create_stop_signal_sets_reader() {
        let mut service = InjectionService::from_entries(
            vec![],
            &[],
            &empty_xmodmap(),
            false,
        );
        assert!(!service.has_stop_signal());

        let _writer = service.create_stop_signal().unwrap();
        assert!(service.has_stop_signal());
    }

    #[test]
    fn stop_signal_can_be_written_to() {
        let mut service = InjectionService::from_entries(
            vec![],
            &[],
            &empty_xmodmap(),
            false,
        );
        let mut writer = service.create_stop_signal().unwrap();
        // Writing should succeed (this is what signals stop)
        writer.write_all(&[1]).unwrap();
    }

    #[test]
    fn new_with_preset_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let preset_path = tmp.path().join("test.json");
        std::fs::write(&preset_path, "[]").unwrap();

        let service = InjectionService::new(
            vec![PathBuf::from("/dev/input/event0")],
            &preset_path,
            &empty_xmodmap(),
            false,
        )
        .unwrap();
        assert_eq!(service.device_paths().len(), 1);
    }

    #[test]
    fn new_with_missing_preset_file() {
        let result = InjectionService::new(
            vec![],
            Path::new("/nonexistent/preset.json"),
            &empty_xmodmap(),
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn new_with_invalid_preset_json() {
        let tmp = tempfile::TempDir::new().unwrap();
        let preset_path = tmp.path().join("bad.json");
        std::fs::write(&preset_path, "not json").unwrap();

        let result = InjectionService::new(
            vec![],
            &preset_path,
            &empty_xmodmap(),
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn multiple_device_paths() {
        let paths = vec![
            PathBuf::from("/dev/input/event0"),
            PathBuf::from("/dev/input/event1"),
            PathBuf::from("/dev/input/event2"),
        ];
        let service = InjectionService::from_entries(
            paths.clone(),
            &[],
            &empty_xmodmap(),
            false,
        );
        assert_eq!(service.device_paths(), &paths);
    }
}
