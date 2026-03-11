use serde::{Deserialize, Serialize};

pub const SOCKET_PATH: &str = "/run/input-remapper-rs.sock";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command")]
pub enum Request {
    #[serde(rename = "start")]
    Start { device: String, preset: String },
    #[serde(rename = "stop")]
    Stop { device: String },
    #[serde(rename = "stop-all")]
    StopAll,
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "autoload")]
    Autoload,
    #[serde(rename = "list-devices")]
    ListDevices,
    #[serde(rename = "list-presets")]
    ListPresets { device: String },
    #[serde(rename = "get-preset")]
    GetPreset { device: String, preset: String },
    #[serde(rename = "save-preset")]
    SavePreset {
        device: String,
        preset: String,
        entries: Vec<crate::mapping::config::MappingEntry>,
    },
    #[serde(rename = "delete-preset")]
    DeletePreset { device: String, preset: String },
    #[serde(rename = "record")]
    Record { device: String },
    #[serde(rename = "get-device-keys")]
    GetDeviceKeys { device: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InjectionStatus {
    pub device: String,
    pub preset: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceInfoResponse {
    pub name: String,
    pub key: String,
    pub vendor: u16,
    pub product: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyInfoResponse {
    pub code: u16,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordEvent {
    pub event_type: u16,
    pub code: u16,
    pub code_name: String,
    pub value: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum Response {
    #[serde(rename = "ok")]
    Ok { message: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "status")]
    Status { injections: Vec<InjectionStatus> },
    #[serde(rename = "devices")]
    Devices { devices: Vec<DeviceInfoResponse> },
    #[serde(rename = "presets")]
    Presets { presets: Vec<String> },
    #[serde(rename = "preset-data")]
    PresetData {
        entries: Vec<crate::mapping::config::MappingEntry>,
    },
    #[serde(rename = "record-event")]
    RecordEvent(RecordEvent),
    #[serde(rename = "device-keys")]
    DeviceKeys { keys: Vec<KeyInfoResponse> },
}
