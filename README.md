# input-remapper-rs

Remap input device events on kernel level (evdev/uinput). Works on Wayland and X11.

Rust rewrite of [input-remapper](https://github.com/sezanzeb/input-remapper), focusing on performance and simplicity. Compatible with existing input-remapper preset files.

## Features

- Key-to-key and key-to-combination remapping (e.g. mouse button -> Ctrl+C)
- Multi-device support with simultaneous polling
- Daemon mode with Unix socket IPC
- Systemd service with autoload
- ~1.4MB static binary, microsecond-level latency

## Usage

```bash
# List devices
input-remapper-rs list-devices --json

# Start/stop via daemon
input-remapper-rs daemon                                          # Terminal 1
input-remapper-rs start --device "USB Gaming Mouse" --preset "My Preset"
input-remapper-rs status
input-remapper-rs stop --device "USB Gaming Mouse"

# Or run directly without daemon
input-remapper-rs run-foreground --device "USB Gaming Mouse" --preset "My Preset"

# Record events from a device
input-remapper-rs record --device "USB Gaming Mouse"
```

## Install

### APT Repository (recommended)

```bash
curl -fsSL https://xi72yow.github.io/input-remapper-rs/install.sh | sudo bash
```

Or manually:

```bash
# Add GPG key
curl -fsSL https://xi72yow.github.io/input-remapper-rs/key.gpg | sudo gpg --dearmor -o /usr/share/keyrings/input-remapper-rs.gpg

# Add repository
echo "deb [arch=amd64 signed-by=/usr/share/keyrings/input-remapper-rs.gpg] https://xi72yow.github.io/input-remapper-rs stable main" \
  | sudo tee /etc/apt/sources.list.d/input-remapper-rs.list

# Install
sudo apt update && sudo apt install input-remapper-rs
```

### Manual .deb install

```bash
sudo dpkg -i input-remapper-rs_0.1.0-1_amd64.deb
```

Config goes in `/etc/input-remapper-rs/`:
```
/etc/input-remapper-rs/
  config.json          # autoload settings
  xmodmap.json         # symbol -> keycode map
  <Device Name>/
    <Preset>.json
```

### Example: MMO Mouse (Utech Smart Venus)

`/etc/input-remapper-rs/config.json`:
```json
{
    "version": "2.1.1",
    "autoload": {
        "USB Gaming Mouse": "Utech Smart Mouse"
    }
}
```

`/etc/input-remapper-rs/USB Gaming Mouse/Utech Smart Mouse.json`:
```json
[
    {
        "input_combination": [{ "type": 1, "code": 2, "origin_hash": "..." }],
        "target_uinput": "keyboard",
        "output_symbol": "Control_L + c",
        "mapping_type": "key_macro"
    },
    {
        "input_combination": [{ "type": 1, "code": 3, "origin_hash": "..." }],
        "target_uinput": "keyboard",
        "output_symbol": "Control_L + v",
        "mapping_type": "key_macro"
    },
    {
        "input_combination": [{ "type": 1, "code": 8, "origin_hash": "..." }],
        "target_uinput": "keyboard",
        "output_symbol": "XF86Back",
        "mapping_type": "key_macro"
    },
    {
        "input_combination": [{ "type": 1, "code": 5, "origin_hash": "..." }],
        "target_uinput": "keyboard",
        "output_symbol": "XF86Forward",
        "mapping_type": "key_macro"
    }
]
```

Use `input-remapper-rs record --device "USB Gaming Mouse"` to find the correct `type`/`code` values for your buttons. The `origin_hash` is the device hash shown in `list-devices --json`.

## Build

```bash
# Dev environment (Docker)
docker compose run --rm dev cargo build --release
docker compose run --rm dev cargo test

# Build .deb
docker compose run --rm dev cargo deb
```

## License

GPL-3.0
