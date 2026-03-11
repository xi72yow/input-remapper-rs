use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent};

use crate::ipc::protocol::RecordEvent;

pub enum AppEvent {
    Key(KeyEvent),
    #[allow(dead_code)]
    Resize(u16, u16),
    RecordEvent(RecordEvent),
    RecordError(String),
    RecordStopped,
    Tick,
}

pub struct EventHandler {
    rx: mpsc::Receiver<AppEvent>,
    tx: mpsc::Sender<AppEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();

        // Crossterm event polling thread
        let tx_key = tx.clone();
        thread::spawn(move || loop {
            if event::poll(tick_rate).unwrap_or(false) {
                if let Ok(ev) = event::read() {
                    let app_event = match ev {
                        Event::Key(key) => AppEvent::Key(key),
                        Event::Resize(w, h) => AppEvent::Resize(w, h),
                        _ => continue,
                    };
                    if tx_key.send(app_event).is_err() {
                        break;
                    }
                }
            } else if tx_key.send(AppEvent::Tick).is_err() {
                break;
            }
        });

        Self { rx, tx }
    }

    pub fn sender(&self) -> mpsc::Sender<AppEvent> {
        self.tx.clone()
    }

    pub fn next(&self) -> Result<AppEvent, mpsc::RecvError> {
        self.rx.recv()
    }
}
