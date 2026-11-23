//! A background thread that reads CPU, memory, battery, network and volume once a second and
//! carries out the bar's actions, so the bar itself never waits on Windows.
use crate::sys;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub cpu: u8,
    pub ram: u8,
    pub battery: Option<(u8, bool)>,
    pub online: bool,
    /// (level 0..1, muted), None without a sound device.
    pub volume: Option<(f32, bool)>,
    pub fullscreen: bool,
    /// Minutes of battery left, and battery saver on.
    pub battery_left: Option<u32>,
    pub saver: bool,
    /// Built-in screen brightness in percent (None when Windows cannot set it).
    pub brightness: Option<u8>,
}

/// `Night(0)` turns the night light off; any other value is its strength.
pub enum Cmd { Volume(f32), Mute(bool), Brightness(u8), Night(u8), Lock, Sleep, SignOut, Restart, ShutDown }

/// Starts the thread. `wake` repaints the bar whenever a reading changes, so it never shows stale values.
pub fn start(wake: eframe::egui::Context) -> (Arc<Mutex<Stats>>, Sender<Cmd>) {
    let stats = Arc::new(Mutex::new(Stats::default()));
    let (tx, rx) = channel();
    let s = stats.clone();
    std::thread::spawn(move || run(s, rx, wake));
    (stats, tx)
}

fn run(stats: Arc<Mutex<Stats>>, rx: Receiver<Cmd>, wake: eframe::egui::Context) {
    sys::com_init();
    let net = sys::network_manager();
    let mut vol = sys::volume_endpoint();
    let wmi = sys::wmi();
    let mut brightness = wmi.as_ref().and_then(sys::brightness_get);
    let mut last = sys::cpu_times();
    let mut tick = 0u32;
    // Windows resets the screen colours after sleep, a lock or a new display, so the night light
    // is put back every few seconds while it is on.
    let mut night = 0u8;
    loop {
        // Act on commands as they come; refresh the readings once a second.
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(c) => {
            match c {
                Cmd::Brightness(b) => if let Some(w) = &wmi {
                    // Only the newest level counts when several arrive while the screen is busy.
                    let mut b = b;
                    let mut rest = Vec::new();
                    while let Ok(next) = rx.try_recv() { match next { Cmd::Brightness(n) => b = n, other => rest.push(other) } }
                    sys::brightness_set(w, b);
                    brightness = Some(b.min(100));
                    if let Ok(mut s) = stats.lock() { s.brightness = brightness; }
                    for c in rest { act(c, &vol); }
                },
                Cmd::Night(n) => { night = n; sys::night_light(n); }
                other => act(other, &vol),
            }
            if let (Some(e), Ok(mut s)) = (&vol, stats.lock()) { s.volume = sys::volume_get(e); }
            wake.request_repaint();
            continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
        tick += 1;
        // Volume is cheap to read: often, so volume keys and other apps show up at once.
        if tick.is_multiple_of(2) {
            let v = vol.as_ref().and_then(sys::volume_get);
            if let Ok(mut s) = stats.lock() { if s.volume != v { s.volume = v; wake.request_repaint(); } }
        }
        if !tick.is_multiple_of(10) { continue }
        if tick.is_multiple_of(100) || vol.is_none() { vol = sys::volume_endpoint(); }   // headphones plugged in or out
        if night > 0 && tick.is_multiple_of(50) { sys::night_light(night); }
        if tick.is_multiple_of(30) { brightness = wmi.as_ref().and_then(sys::brightness_get); }   // changed by keys or Windows
        let now = sys::cpu_times();
        let cpu = ronin_core::cpu_percent(last, now);
        last = now;
        let fresh = Stats {
            cpu, ram: sys::ram_percent(), battery: sys::battery(),
            online: net.as_ref().map(sys::online).unwrap_or(false),
            volume: vol.as_ref().and_then(sys::volume_get),
            fullscreen: sys::fullscreen_app_active(),
            battery_left: sys::battery_detail().0, saver: sys::battery_detail().1, brightness,
        };
        if let Ok(mut s) = stats.lock() {
            if *s != fresh { *s = fresh; wake.request_repaint(); }
        }
    }
}

fn act(c: Cmd, vol: &Option<sys::Volume>) {
    match c {
        Cmd::Volume(v) => if let Some(e) = vol { sys::volume_set(e, v); sys::mute_set(e, false) },
        Cmd::Mute(m) => if let Some(e) = vol { sys::mute_set(e, m) },
        Cmd::Brightness(_) | Cmd::Night(_) => {}
        Cmd::Lock => sys::lock(),
        Cmd::Sleep => sys::sleep(),
        Cmd::SignOut => sys::sign_out(),
        Cmd::Restart => sys::restart(),
        Cmd::ShutDown => sys::shut_down(),
    }
}
