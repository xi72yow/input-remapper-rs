use evdev::{EventType, InputEvent};
use std::collections::HashMap;

use super::config::{parse_output_symbol, MappingEntry, SymbolMap};

/// A resolved mapping: input keycode → list of output keycodes
#[derive(Debug)]
struct KeyMapping {
    output_codes: Vec<u16>,
}

/// The mapping handler applies remapping rules to input events.
pub struct MappingHandler {
    /// Maps input keycode → output keycodes
    mappings: HashMap<u16, KeyMapping>,
    debug: bool,
}

fn syn_report() -> InputEvent {
    InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0)
}

impl MappingHandler {
    /// Build a handler from preset entries and a symbol map.
    pub fn from_preset(entries: &[MappingEntry], xmodmap: &SymbolMap, debug: bool) -> Self {
        let mut mappings = HashMap::new();

        for entry in entries {
            // Only handle key_macro type for now
            if entry.mapping_type != "key_macro" {
                continue;
            }

            // Only handle single-key input combinations for now
            if entry.input_combination.len() != 1 {
                continue;
            }

            let input = &entry.input_combination[0];
            // Only handle EV_KEY (type 1)
            if input.event_type != EventType::KEY.0 {
                continue;
            }

            if let Some(ref symbol) = entry.output_symbol {
                if let Some(codes) = parse_output_symbol(symbol, xmodmap) {
                    if debug {
                        eprintln!("  Mapping: code {} -> {} (keycodes {:?})", input.code, symbol, codes);
                    }
                    mappings.insert(
                        input.code,
                        KeyMapping {
                            output_codes: codes,
                        },
                    );
                } else {
                    eprintln!("  WARNING: Could not resolve symbol '{}' for input code {}", symbol, input.code);
                }
            }
        }

        Self { mappings, debug }
    }

    /// Remap a single input event, appending the results to `out`.
    /// For unmapped events, the original event is appended unchanged.
    /// A single syn_report() is appended at the end of each remapped group.
    pub fn remap_into(&self, event: &InputEvent, out: &mut Vec<InputEvent>) {
        // Only remap KEY events
        if event.event_type() != EventType::KEY {
            out.push(*event);
            return;
        }

        if let Some(mapping) = self.mappings.get(&event.code()) {
            if self.debug {
                eprintln!("  Remap: code {} -> {:?} (value={})", event.code(), mapping.output_codes, event.value());
            }
            let value = event.value(); // 1=press, 0=release, 2=repeat

            if mapping.output_codes.len() == 1 {
                // Simple key remap
                out.push(InputEvent::new(EventType::KEY.0, mapping.output_codes[0], value));
                out.push(syn_report());
                return;
            }

            // Key combination (e.g. Ctrl+C)
            if value == 1 {
                // Press: press all keys, single SYN at the end
                for &code in &mapping.output_codes {
                    out.push(InputEvent::new(EventType::KEY.0, code, 1));
                }
                out.push(syn_report());
            } else if value == 0 {
                // Release: release in reverse order, single SYN at the end
                for &code in mapping.output_codes.iter().rev() {
                    out.push(InputEvent::new(EventType::KEY.0, code, 0));
                }
                out.push(syn_report());
            }
            // Ignore repeat (value=2) for combinations
        } else {
            // No mapping, pass through
            out.push(*event);
        }
    }
}
