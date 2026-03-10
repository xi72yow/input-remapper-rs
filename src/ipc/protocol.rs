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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InjectionStatus {
    pub device: String,
    pub preset: String,
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
}
