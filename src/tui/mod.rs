mod app;
mod event;
mod screens;
mod ui;
mod widgets;

use std::time::Duration;

use app::App;
use event::{AppEvent, EventHandler};

pub fn run_tui() -> std::io::Result<()> {
    // Install panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = ratatui::try_restore();
        original_hook(panic_info);
    }));

    let mut terminal = ratatui::init();

    // Load symbols from xmodmap + builtins
    let symbols = load_all_symbols();

    let mut app = App::new(symbols);
    let events = EventHandler::new(Duration::from_millis(250));
    let tx = events.sender();

    app.refresh_devices(&tx);

    while !app.should_quit {
        terminal.draw(|frame| ui::render(frame, &app))?;

        match events.next() {
            Ok(AppEvent::Key(key)) => {
                // Only handle key press events (ignore release/repeat)
                if key.kind == crossterm::event::KeyEventKind::Press {
                    app.handle_key(key, &tx);
                }
            }
            Ok(AppEvent::IpcResult(op, result)) => app.handle_ipc_result(op, result, &tx),
            Ok(AppEvent::RecordEvent(ev)) => app.handle_record_event(ev),
            Ok(AppEvent::RecordError(msg)) => app.handle_record_error(msg),
            Ok(AppEvent::RecordStopped) => app.handle_record_stopped(),
            Ok(AppEvent::Resize(_, _)) => {} // ratatui handles resize
            Ok(AppEvent::Tick) => {
                app.tick = app.tick.wrapping_add(1);
            }
            Err(_) => break,
        }
    }

    ratatui::restore();
    Ok(())
}

fn load_all_symbols() -> Vec<(String, u16)> {
    let mut symbols = Vec::new();

    // Try loading xmodmap from config
    let config_dir = dirs_or_default();
    let xmodmap_path = config_dir.join("xmodmap.json");
    if xmodmap_path.exists() {
        if let Ok(map) = crate::mapping::config::load_symbol_map(&xmodmap_path) {
            for (name, code) in &map {
                symbols.push((name.clone(), *code));
            }
        }
    }

    // Add all evdev KEY_* codes
    for code in 0u16..768 {
        let key = evdev::KeyCode(code);
        let name = format!("{:?}", key);
        // Skip codes that just format as numbers
        if !name.starts_with("KeyCode(") {
            if !symbols.iter().any(|(n, _)| n == &name) {
                symbols.push((name, code));
            }
        }
    }

    symbols.sort_by(|(a, _), (b, _)| a.cmp(b));
    symbols
}

fn dirs_or_default() -> std::path::PathBuf {
    std::path::PathBuf::from("/etc/input-remapper-rs")
}
