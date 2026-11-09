use std::sync::mpsc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event as CEvent, KeyEvent};

#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    Tick,
    Error(String),
}

pub struct EventHandler {
    rx: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64) -> Self {
        let (tx, rx) = mpsc::channel();
        let tick_rate = Duration::from_millis(tick_rate_ms);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);

        let worker = thread::spawn(move || {
            let mut last_tick = std::time::Instant::now();
            while !worker_stop.load(Ordering::Relaxed) {
                let timeout = tick_rate
                    .checked_sub(last_tick.elapsed())
                    .unwrap_or(Duration::ZERO);

                let polled = match event::poll(timeout) {
                    Ok(polled) => polled,
                    Err(error) => {
                        let _ = tx.send(Event::Error(format!(
                            "Terminal event polling failed: {error}"
                        )));
                        break;
                    }
                };
                if polled {
                    match event::read() {
                        Ok(CEvent::Key(key)) => {
                            if tx.send(Event::Key(key)).is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            let _ = tx
                                .send(Event::Error(format!("Terminal event read failed: {error}")));
                            break;
                        }
                    }
                }

                if last_tick.elapsed() >= tick_rate {
                    if tx.send(Event::Tick).is_err() {
                        break;
                    }
                    last_tick = std::time::Instant::now();
                }
            }
        });

        Self {
            rx,
            stop,
            worker: Some(worker),
        }
    }

    pub fn next(&self) -> Result<Event, mpsc::RecvError> {
        self.rx.recv()
    }
}

impl Drop for EventHandler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
