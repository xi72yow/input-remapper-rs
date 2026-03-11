use evdev::Device;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub name: String,
    pub paths: Vec<PathBuf>,
    pub key: String,
    pub vendor: u16,
    pub product: u16,
    /// All supported key/button codes across all sub-devices
    pub supported_keys: Vec<KeyInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyInfo {
    pub code: u16,
    pub name: String,
}

/// Generate a unique key for a physical device, used to group multiple
/// /dev/input/eventX nodes that belong to the same hardware.
fn device_group_key(dev: &Device) -> String {
    let id = dev.input_id();
    let phys = dev.physical_path().unwrap_or("");
    // Strip everything after first '/' in phys to group subdevices
    let phys_base = phys.split('/').next().unwrap_or("");
    let uniq = dev.unique_name().unwrap_or("");
    format!(
        "{}_{:04x}_{:04x}_{}_{}",
        id.bus_type().0,
        id.vendor(),
        id.product(),
        uniq,
        phys_base
    )
}

/// Discover all input devices grouped by physical hardware.
pub fn discover_devices() -> Vec<DeviceInfo> {
    let mut groups: HashMap<String, DeviceInfo> = HashMap::new();

    for (path, device) in evdev::enumerate() {
        let key = device_group_key(&device);
        let name = device.name().unwrap_or("Unknown").to_string();

        // Collect supported keys from this sub-device
        let mut sub_keys = Vec::new();
        if let Some(keys) = device.supported_keys() {
            for key_code in keys.iter() {
                sub_keys.push(key_code.0);
            }
        }

        groups
            .entry(key.clone())
            .and_modify(|info| {
                info.paths.push(path.clone());
                // Use shortest name
                if name.len() < info.name.len() {
                    info.name = name.clone();
                }
                // Merge supported keys (deduplicate)
                for code in &sub_keys {
                    if !info.supported_keys.iter().any(|k| k.code == *code) {
                        info.supported_keys.push(KeyInfo {
                            code: *code,
                            name: format!("{:?}", evdev::KeyCode(*code)),
                        });
                    }
                }
            })
            .or_insert_with(|| {
                let id = device.input_id();
                let supported_keys = sub_keys
                    .iter()
                    .map(|&code| KeyInfo {
                        code,
                        name: format!("{:?}", evdev::KeyCode(code)),
                    })
                    .collect();
                DeviceInfo {
                    name: name.clone(),
                    paths: vec![path.clone()],
                    key: key.clone(),
                    vendor: id.vendor(),
                    product: id.product(),
                    supported_keys,
                }
            });
    }

    let mut devices: Vec<DeviceInfo> = groups.into_values().collect();
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    // Sort keys by code within each device
    for dev in &mut devices {
        dev.supported_keys.sort_by_key(|k| k.code);
    }
    devices
}

/// Find a device group by name (partial match).
pub fn find_device_by_name(name: &str) -> Option<DeviceInfo> {
    discover_devices()
        .into_iter()
        .find(|d| d.name.contains(name))
}
