pub mod app;
pub mod offline;
pub mod pages;
pub mod popups;
pub mod theme;

use std::sync::{Arc, Mutex};

/// Result slot for a background computation. Starting a new job discards the old one's result.
pub struct Job<T> {
    slot: Arc<Mutex<Option<(u64, T)>>>,
    generation: u64,
    pub running: bool,
}

impl<T: Send + 'static> Default for Job<T> {
    fn default() -> Self {
        Self { slot: Arc::new(Mutex::new(None)), generation: 0, running: false }
    }
}

impl<T: Send + 'static> Job<T> {
    pub fn start(&mut self, f: impl FnOnce() -> T + Send + 'static) {
        self.generation += 1;
        self.running = true;
        let generation = self.generation;
        let slot = self.slot.clone();
        std::thread::spawn(move || {
            let v = f();
            *slot.lock().unwrap() = Some((generation, v));
            crate::hub::repaint();
        });
    }

    /// Returns the finished result of the most recent job, once.
    pub fn poll(&mut self) -> Option<T> {
        let mut g = self.slot.lock().unwrap();
        match g.take() {
            Some((generation, v)) if generation == self.generation => {
                self.running = false;
                Some(v)
            }
            Some(_) => None,
            None => None,
        }
    }
}
