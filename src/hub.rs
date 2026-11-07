//! Global state shared between worker threads and the UI thread.
//! Replaces the static C# events (`OnSyncBegin`, `OnSyncUpdate`, ...) and `Dispatcher.Invoke`.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicI32, Ordering};

use crate::workshop::model::Entry;

#[derive(Clone, Debug, Default)]
pub struct SyncState {
    pub entry: Option<Entry>,
    pub progress: i32,
    pub status: String,
}

/// Things worker threads ask the UI to do.
pub enum UiEvent {
    Message { title: String, message: String },
    Error(String),
    ScriptedPackage(Box<Entry>),
    SyncFinished,
    LibraryChanged,
    GameExited,
    /// The workshop folder was moved; carries the new setting value (empty = default).
    WorkshopMoved(String),
    /// A worker started something long-running and wants a progress popup for it.
    #[cfg_attr(windows, allow(dead_code))]
    Progress { title: String, message: String, handle: crate::actions::ProgressHandle },
}

struct Hub {
    ctx: Mutex<Option<egui::Context>>,
    sync: Mutex<Option<SyncState>>,
    events: Mutex<VecDeque<UiEvent>>,
}

fn hub() -> &'static Hub {
    static HUB: OnceLock<Hub> = OnceLock::new();
    HUB.get_or_init(|| Hub { ctx: Mutex::new(None), sync: Mutex::new(None), events: Mutex::new(VecDeque::new()) })
}

static BUSY: AtomicI32 = AtomicI32::new(0);

pub fn set_ctx(ctx: egui::Context) {
    *hub().ctx.lock().unwrap() = Some(ctx);
}

pub fn repaint() {
    if let Some(ctx) = hub().ctx.lock().unwrap().as_ref() {
        ctx.request_repaint();
    }
}

/// Sends a viewport command (minimize, focus, ...) to the main window from any thread.
#[cfg_attr(windows, allow(dead_code))]
pub fn viewport_cmd(cmd: egui::ViewportCommand) {
    if let Some(ctx) = hub().ctx.lock().unwrap().as_ref() {
        ctx.send_viewport_cmd(cmd);
        ctx.request_repaint();
    }
}

pub fn send(ev: UiEvent) {
    hub().events.lock().unwrap().push_back(ev);
    repaint();
}

pub fn drain() -> Vec<UiEvent> {
    hub().events.lock().unwrap().drain(..).collect()
}

pub fn sync_begin(entry: &Entry) {
    *hub().sync.lock().unwrap() =
        Some(SyncState { entry: Some(entry.clone()), progress: 0, status: "Preparing".into() });
    repaint();
}

pub fn sync_update(progress: i32, status: &str) {
    if let Some(s) = hub().sync.lock().unwrap().as_mut() {
        s.progress = progress;
        s.status = status.to_string();
    }
    repaint();
}

pub fn sync_end() {
    let was = hub().sync.lock().unwrap().take();
    if was.is_some() {
        send(UiEvent::SyncFinished);
    }
}

pub fn sync_state() -> Option<SyncState> {
    hub().sync.lock().unwrap().clone()
}

/// RAII guard that marks the launcher as busy (UI disabled), like `MainWindow.OnSyncBegin/End`.
pub struct Busy;

impl Busy {
    pub fn new() -> Self {
        BUSY.fetch_add(1, Ordering::SeqCst);
        repaint();
        Busy
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.fetch_sub(1, Ordering::SeqCst);
        repaint();
    }
}

pub fn is_busy() -> bool {
    BUSY.load(Ordering::SeqCst) > 0 || sync_state().is_some()
}

pub fn error_text(e: &anyhow::Error) -> String {
    format!("{e:#}")
}
