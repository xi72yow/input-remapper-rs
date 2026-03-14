# input-remapper-rs

Remap input device events on kernel level (evdev/uinput). Works on Wayland and X11.

Rust rewrite of [input-remapper](https://github.com/sezanzeb/input-remapper), focusing on performance and simplicity. Compatible with existing input-remapper preset files.

<img width="1576" height="1066" alt="image" src="https://github.com/user-attachments/assets/78a50482-9e85-4baa-ab16-6cafa35e1031" />

## Features

- Key-to-key and key-to-combination remapping (e.g. mouse button -> Ctrl+C)
- Multi-device support with simultaneous mapping
- Terminal UI (TUI) for interactive configuration
- Daemon with systemd service and autoload
- ~1.4MB static binary, microsecond-level latency

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

# Enable and start the service
sudo systemctl enable --now input-remapper-rs
```

### Manual .deb install

```bash
sudo dpkg -i input-remapper-rs_0.1.0-1_amd64.deb
sudo systemctl enable --now input-remapper-rs
```

## Usage

Once the daemon is running via systemd, open the TUI to configure your devices:

```bash
sudo input-remapper-rs tui
```

The TUI lets you:

- Browse and select input devices
- Create, edit, and delete presets
- Record input events to capture key/button codes
- Search and assign output symbols (keysyms)
- Apply presets and monitor injection status

Each mapped device gets its own virtual clone via uinput. Multiple devices can be mapped simultaneously.

## Configuration

Config is stored in `/etc/input-remapper-rs/`:

```
/etc/input-remapper-rs/
  config.json          # autoload settings
  xmodmap.json         # symbol -> keycode map
  <Device Name>/
    <Preset>.json
```

Presets configured with autoload in `config.json` are automatically applied when the daemon starts:

```json
{
    "version": "2.1.1",
    "autoload": {
        "USB Gaming Mouse": "My Preset"
    }
}
```

## Use Cases

- **MMO mice** – Remap side buttons (e.g. Corsair Scimitar, UTechSmart) to keyboard shortcuts or macros
- **Azeron keypads** – Map analog stick and keys to custom key combinations for gaming or productivity

## Development

```bash
# Build
docker compose run --rm dev cargo build

# Build release
docker compose run --rm dev cargo build --release

# Run tests (needs /dev/uinput, runs as root in container)
docker compose run --rm dev cargo test

# Quick dev install (after build)
docker compose run --rm dev cp /app/target/debug/input-remapper-rs /app/dist/
sudo systemctl stop input-remapper-rs
sudo cp dist/input-remapper-rs /usr/bin/
sudo systemctl start input-remapper-rs

# Build .deb package
docker compose run --rm dev cargo deb
```

## License

GPL-3.0
