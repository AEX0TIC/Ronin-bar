//! A background thread that reads CPU, memory, battery, network and volume once a second and
//! carries out the bar's actions, so the bar itself never waits on Windows.
use crate::sys;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub cpu: u8,
    pub ram: u8,
    pub battery: Option<(u8, bool)>,
    pub online: bool,
    /// (level 0..1, muted), None without a sound device.
    pub volume: Option<(f32, bool)>,
    pub fullscreen: bool,
}

pub enum Cmd { Volume(f32), Mute(bool), Lock, Sleep, Restart, ShutDown }

pub fn start() -> (Arc<Mutex<Stats>>, Sender<Cmd>) {
    let stats = Arc::new(Mutex::new(Stats::default()));
    let (tx, rx) = channel();
    let s = stats.clone();
    std::thread::spawn(move || run(s, rx));
    (stats, tx)
}

fn run(stats: Arc<Mutex<Stats>>, rx: Receiver<Cmd>) {
    sys::com_init();
    let net = sys::network_manager();
    let mut vol = sys::volume_endpoint();
    let mut last = sys::cpu_times();
    let mut tick = 0u32;
    loop {
        // Act on commands as they come; refresh the readings once a second.
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(c) => {
            match c {
                Cmd::Volume(v) => if let Some(e) = &vol { sys::volume_set(e, v); sys::mute_set(e, false) },
                Cmd::Mute(m) => if let Some(e) = &vol { sys::mute_set(e, m) },
                Cmd::Lock => sys::lock(),
                Cmd::Sleep => sys::sleep(),
                Cmd::Restart => sys::restart(),
                Cmd::ShutDown => sys::shut_down(),
            }
            if let (Some(e), Ok(mut s)) = (&vol, stats.lock()) { s.volume = sys::volume_get(e); }
            continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
        tick += 1;
        if !tick.is_multiple_of(10) { continue }
        if tick.is_multiple_of(100) || vol.is_none() { vol = sys::volume_endpoint(); }   // headphones plugged in or out
        let now = sys::cpu_times();
        let cpu = ronin_core::cpu_percent(last, now);
        last = now;
        let fresh = Stats {
            cpu, ram: sys::ram_percent(), battery: sys::battery(),
            online: net.as_ref().map(sys::online).unwrap_or(false),
            volume: vol.as_ref().and_then(sys::volume_get),
            fullscreen: sys::fullscreen_app_active(),
        };
        if let Ok(mut s) = stats.lock() { *s = fresh; }
    }
}
