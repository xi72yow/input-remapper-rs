use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, InputEvent, KeyCode, RelativeAxisCode};

pub struct DeviceWriter {
    device: VirtualDevice,
}

impl DeviceWriter {
    /// Create a virtual keyboard device that can emit any key.
    #[allow(dead_code)]
    pub fn new_keyboard(name: &str) -> std::io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for code in 0..768u16 {
            keys.insert(KeyCode(code));
        }

        let device = VirtualDevice::builder()?
            .name(name)
            .with_keys(&keys)?
            .build()?;

        Ok(Self { device })
    }

    /// Create a virtual device with both keyboard and mouse capabilities.
    /// This allows forwarding mouse movement, scrolling, and button events.
    pub fn new_keyboard_mouse(name: &str) -> std::io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for code in 0..768u16 {
            keys.insert(KeyCode(code));
        }

        let mut rel_axes = AttributeSet::<RelativeAxisCode>::new();
        rel_axes.insert(RelativeAxisCode::REL_X);
        rel_axes.insert(RelativeAxisCode::REL_Y);
        rel_axes.insert(RelativeAxisCode::REL_WHEEL);
        rel_axes.insert(RelativeAxisCode::REL_HWHEEL);
        rel_axes.insert(RelativeAxisCode::REL_WHEEL_HI_RES);
        rel_axes.insert(RelativeAxisCode::REL_HWHEEL_HI_RES);

        let device = VirtualDevice::builder()?
            .name(name)
            .with_keys(&keys)?
            .with_relative_axes(&rel_axes)?
            .build()?;

        Ok(Self { device })
    }

    pub fn emit(&mut self, events: &[InputEvent]) -> std::io::Result<()> {
        self.device.emit(events)
    }
}
