use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Global config (~/.config/input-remapper-rs/config.json)
#[derive(Debug, Deserialize, Serialize)]
pub struct GlobalConfig {
    pub version: String,
    #[serde(default)]
    pub autoload: HashMap<String, String>,
}

/// A single input event trigger
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct InputConfig {
    #[serde(rename = "type")]
    pub event_type: u16,
    pub code: u16,
    #[serde(default)]
    pub origin_hash: Option<String>,
}

/// A single mapping entry in a preset
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MappingEntry {
    pub input_combination: Vec<InputConfig>,
    pub target_uinput: String,
    pub output_symbol: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub mapping_type: String,
}

/// A preset is a list of mapping entries
pub type Preset = Vec<MappingEntry>;

/// Symbol name to evdev keycode mapping
pub type SymbolMap = HashMap<String, u16>;

pub fn load_preset(path: &Path) -> std::io::Result<Preset> {
    let content = std::fs::read_to_string(path)?;
    let preset: Preset = serde_json::from_str(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(preset)
}

pub fn load_global_config(path: &Path) -> std::io::Result<GlobalConfig> {
    let content = std::fs::read_to_string(path)?;
    let config: GlobalConfig = serde_json::from_str(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(config)
}

pub fn load_symbol_map(path: &Path) -> std::io::Result<SymbolMap> {
    let content = std::fs::read_to_string(path)?;
    let map: SymbolMap = serde_json::from_str(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(map)
}

/// Built-in symbol map for KEY_* names that don't need xmodmap
fn builtin_symbols() -> SymbolMap {
    let mut map = SymbolMap::new();
    // Common KEY_* names used in output_symbol
    let pairs = [
        ("KEY_PLAYPAUSE", 164),
        ("KEY_STOPCD", 166),
        ("KEY_PREVIOUSSONG", 165),
        ("KEY_NEXTSONG", 163),
        ("KEY_VOLUMEDOWN", 114),
        ("KEY_VOLUMEUP", 115),
        ("KEY_MUTE", 113),
    ];
    for (name, code) in pairs {
        map.insert(name.to_string(), code);
    }
    map
}

/// Resolve a symbol name (e.g. "Control_L", "KEY_PLAYPAUSE", "a") to a keycode.
pub fn resolve_symbol(name: &str, xmodmap: &SymbolMap) -> Option<u16> {
    // Try xmodmap first
    if let Some(&code) = xmodmap.get(name) {
        return Some(code);
    }
    // Try built-in KEY_* names
    if let Some(&code) = builtin_symbols().get(name) {
        return Some(code);
    }
    None
}

/// Parse an output_symbol like "Control_L + c" into a list of keycodes.
pub fn parse_output_symbol(symbol: &str, xmodmap: &SymbolMap) -> Option<Vec<u16>> {
    let parts: Vec<&str> = symbol.split('+').map(|s| s.trim()).collect();
    let mut codes = Vec::new();

    for part in parts {
        if let Some(code) = resolve_symbol(part, xmodmap) {
            codes.push(code);
        } else {
            return None;
        }
    }

    Some(codes)
}
