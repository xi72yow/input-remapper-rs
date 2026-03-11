use evdev::{Device, InputEvent};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use std::os::fd::AsFd;
use std::path::Path;

pub struct DeviceReader {
    device: Device,
    grabbed: bool,
}

impl DeviceReader {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let device = Device::open(path)?;
        Ok(Self {
            device,
            grabbed: false,
        })
    }

    pub fn grab(&mut self) -> std::io::Result<()> {
        self.device.grab()?;
        self.grabbed = true;
        Ok(())
    }

    pub fn read_events(&mut self, timeout_ms: u16) -> std::io::Result<Option<Vec<InputEvent>>> {
        let pollfd = PollFd::new(self.device.as_fd(), PollFlags::POLLIN);
        let ready = poll(&mut [pollfd], PollTimeout::from(timeout_ms))?;

        if ready == 0 {
            return Ok(None);
        }

        let events: Vec<InputEvent> = self.device.fetch_events()?.collect();
        Ok(Some(events))
    }

    pub fn fd(&self) -> &impl AsFd {
        &self.device
    }

    pub fn fetch_events(&mut self) -> std::io::Result<Vec<InputEvent>> {
        Ok(self.device.fetch_events()?.collect())
    }

    #[allow(dead_code)]
    pub fn name(&self) -> &str {
        self.device.name().unwrap_or("Unknown")
    }
}

impl Drop for DeviceReader {
    fn drop(&mut self) {
        if self.grabbed {
            let _ = self.device.ungrab();
        }
    }
}
