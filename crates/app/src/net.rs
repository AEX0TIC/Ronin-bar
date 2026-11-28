//! Wi-Fi and Bluetooth for the network popup, on a thread of their own: Windows' Native Wi-Fi
//! functions to scan, connect and disconnect, and its radio controls to switch Wi-Fi and
//! Bluetooth on and off. The bar only reads the shared state and sends commands.
use ronin_core::wifi::{self, Security};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::*;
use crate::winrt::{spawn, wait};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug)]
pub struct Network {
    pub ssid: String,
    pub quality: u32,
    pub security: Security,
    pub connected: bool,
    /// Windows already knows this network (it has a saved profile), so no password is needed.
    pub known: bool,
    pub profile: String,
}

#[derive(Clone, Debug, Default)]
pub struct NetState {
    /// No Wi-Fi adapter at all.
    pub no_wifi: bool,
    pub networks: Vec<Network>,
    pub scanning: bool,
    pub busy: Option<String>,
    pub error: Option<String>,
    pub wifi_on: Option<bool>,
    /// Set when a switch is clicked: until Windows has finished switching, the popup keeps
    /// showing what was clicked instead of an older reading.
    pub hold: Option<Instant>,
    /// The Wi-Fi network Windows is connected to right now, and its signal (0–100).
    pub connected: Option<(String, u32)>,
    pub bluetooth_on: Option<bool>,
    /// Mobile hotspot: on, off, or None when this PC cannot share its connection right now.
    pub hotspot_on: Option<bool>,
}

impl NetState {
    /// Airplane mode: every radio this PC has is off.
    pub fn airplane(&self) -> bool {
        (self.wifi_on.is_some() || self.bluetooth_on.is_some()) && self.wifi_on != Some(true) && self.bluetooth_on != Some(true)
    }
}

pub enum NetCmd {
    /// Re-read the list Windows already has (instant), without a new scan.
    Refresh,
    /// Windows says something changed: a scan finished (true), or a connection or radio changed.
    Changed(bool),
    /// A radio was switched on or off (from Ronin, Windows or a key on the keyboard).
    RadioChanged,
    Scan,
    Connect { ssid: String, password: String },
    Disconnect,
    Radio { bluetooth: bool, on: bool },
    /// Every radio off, or back on.
    Airplane(bool),
    Hotspot(bool),
}

/// Starts the thread. `wake` repaints the bar as soon as anything changes.
pub fn start(wake: eframe::egui::Context) -> (Arc<Mutex<NetState>>, Sender<NetCmd>) {
    let state = Arc::new(Mutex::new(NetState::default()));
    let (tx, rx) = channel();
    let (s, news) = (state.clone(), tx.clone());
    std::thread::spawn(move || run(s, rx, news, wake));
    (state, tx)
}

struct Wlan { h: HANDLE, iface: Option<GUID> }

impl Drop for Wlan {
    fn drop(&mut self) { unsafe { WlanCloseHandle(self.h, None); } }
}

impl Wlan {
    fn open() -> Option<Wlan> {
        let (mut ver, mut h) = (0u32, HANDLE::default());
        if unsafe { WlanOpenHandle(2, None, &mut ver, &mut h) } != 0 { return None }
        let mut w = Wlan { h, iface: None };
        unsafe {
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(h, None, &mut list) == 0 && !list.is_null() {
                if (*list).dwNumberOfItems > 0 { w.iface = Some((*list).InterfaceInfo[0].InterfaceGuid); }
                WlanFreeMemory(list as *const _);
            }
        }
        Some(w)
    }

    /// The network Windows is connected to right now, asked directly (not from the scan list).
    fn current(&self) -> Option<(String, u32)> {
        let g = self.iface.as_ref()?;
        unsafe {
            let (mut size, mut data) = (0u32, std::ptr::null_mut::<std::ffi::c_void>());
            if WlanQueryInterface(self.h, g, wlan_intf_opcode_current_connection, None, &mut size, &mut data, None) != 0 || data.is_null() { return None }
            let a = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
            let r = (a.isState == wlan_interface_state_connected).then(|| {
                let s = &a.wlanAssociationAttributes.dot11Ssid;
                let len = (s.uSSIDLength as usize).min(32);
                (String::from_utf8_lossy(&s.ucSSID[..len]).to_string(), a.wlanAssociationAttributes.wlanSignalQuality.min(100))
            });
            WlanFreeMemory(data as *const _);
            r
        }
    }

    /// Asks Windows to tell this program whenever Wi-Fi changes (scan finished, connected,
    /// disconnected, radio on or off). The callback only passes the news on to the network thread.
    fn listen(&self, tx: Sender<NetCmd>) {
        unsafe extern "system" fn told(n: *mut L2_NOTIFICATION_DATA, ctx: *mut std::ffi::c_void) {
            if n.is_null() || ctx.is_null() { return }
            let (n, tx) = unsafe { (&*n, &*(ctx as *const Sender<NetCmd>)) };
            let scan_done = n.NotificationSource == WLAN_NOTIFICATION_SOURCE_ACM
                && (n.NotificationCode == wlan_notification_acm_scan_complete.0 as u32 || n.NotificationCode == wlan_notification_acm_scan_fail.0 as u32);
            let _ = tx.send(NetCmd::Changed(scan_done));
        }
        // Kept for the life of the program, like the registration itself.
        let ctx = Box::into_raw(Box::new(tx)) as *const std::ffi::c_void;
        unsafe { WlanRegisterNotification(self.h, WLAN_NOTIFICATION_SOURCE_ACM | WLAN_NOTIFICATION_SOURCE_MSM, true, Some(told), Some(ctx), None, None); }
    }

    fn scan(&self) { if let Some(g) = &self.iface { unsafe { WlanScan(self.h, g, None, None, None); } } }

    fn networks(&self) -> Vec<Network> {
        let Some(g) = &self.iface else { return Vec::new() };
        let mut out: Vec<Network> = Vec::new();
        unsafe {
            let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
            if WlanGetAvailableNetworkList(self.h, g, 0, None, &mut list) != 0 || list.is_null() { return out }
            let n = (*list).dwNumberOfItems as usize;
            let items = std::slice::from_raw_parts((*list).Network.as_ptr(), n);
            for a in items {
                let len = (a.dot11Ssid.uSSIDLength as usize).min(32);
                let ssid = String::from_utf8_lossy(&a.dot11Ssid.ucSSID[..len]).to_string();
                if ssid.is_empty() || ssid.chars().any(char::is_control) { continue }   // hidden networks
                let security = match a.dot11DefaultAuthAlgorithm {
                    DOT11_AUTH_ALGO_80211_OPEN if !a.bSecurityEnabled.as_bool() => Security::Open,
                    DOT11_AUTH_ALGO_WPA_PSK => Security::WpaPsk,
                    DOT11_AUTH_ALGO_RSNA_PSK => Security::Wpa2Psk,
                    DOT11_AUTH_ALGO_WPA3_SAE => Security::Wpa3Sae,
                    _ => Security::Other,
                };
                let pn = a.strProfileName.iter().position(|&c| c == 0).unwrap_or(256);
                let net = Network {
                    ssid, quality: a.wlanSignalQuality.min(100), security,
                    connected: a.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                    known: a.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0,
                    profile: String::from_utf16_lossy(&a.strProfileName[..pn]),
                };
                // One entry per name: the connected / saved one, else the strongest.
                match out.iter_mut().find(|o| o.ssid == net.ssid) {
                    Some(o) => {
                        let better = (net.connected, net.known, net.quality) > (o.connected, o.known, o.quality);
                        if better { *o = net; }
                    }
                    None => out.push(net),
                }
            }
            WlanFreeMemory(list as *const _);
        }
        // Which one is connected comes from Windows' own connection, not from the scan list,
        // which can lag behind.
        if let Some((ssid, q)) = self.current() {
            for n in &mut out { n.connected = n.ssid == ssid; }
            if !out.iter().any(|n| n.connected) {
                out.push(Network { ssid, quality: q, security: Security::Other, connected: true, known: true, profile: String::new() });
            }
        } else {
            for n in &mut out { n.connected = false; }
        }
        out.sort_by_key(|n| std::cmp::Reverse((n.connected, n.known, n.quality)));
        out
    }

    fn connect_profile(&self, profile: &str) -> Result<(), String> {
        let g = self.iface.as_ref().ok_or("No Wi-Fi adapter")?;
        let name = HSTRING::from(profile);
        let p = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(name.as_ptr()),
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        match unsafe { WlanConnect(self.h, g, &p, None) } { 0 => Ok(()), e => Err(format!("Windows could not connect (error {e})")) }
    }

    /// Saves a profile for a new network (password included) and connects with it.
    fn join(&self, ssid: &str, sec: Security, password: &str) -> Result<(), String> {
        let g = self.iface.as_ref().ok_or("No Wi-Fi adapter")?;
        let xml = wifi::profile_xml(ssid, sec, password).ok_or("That password does not fit this network")?;
        let mut reason = 0u32;
        let r = unsafe { WlanSetProfile(self.h, g, 0, &HSTRING::from(xml), PCWSTR::null(), true, None, &mut reason) };
        if r != 0 { return Err(format!("Windows did not accept the network (error {r}, reason {reason})")) }
        self.connect_profile(ssid)
    }

    fn disconnect(&self) { if let Some(g) = &self.iface { unsafe { WlanDisconnect(self.h, g, None); } } }
}

/// The PC's radios, found once and then kept: reading or switching them is then immediate.
static RADIOS: std::sync::OnceLock<Mutex<Vec<Radio>>> = std::sync::OnceLock::new();

fn radio_list() -> Vec<Radio> {
    let cached = RADIOS.get().and_then(|m| m.lock().ok().map(|l| l.clone())).unwrap_or_default();
    if !cached.is_empty() { return cached }
    let found: Vec<Radio> = wait(Radio::GetRadiosAsync(), Duration::from_secs(4)).map(|l| l.into_iter().collect()).unwrap_or_default();
    if !found.is_empty() { if let Ok(mut m) = RADIOS.get_or_init(Default::default).lock() { if m.is_empty() { *m = found.clone(); } } }
    found
}

/// Wi-Fi and Bluetooth radios: on, off, or None when there is none (or access was refused).
fn radios() -> (Option<bool>, Option<bool>) {
    let list = radio_list();
    if list.is_empty() { return (None, None) }
    let (mut wifi, mut bt) = (None, None);
    for r in list {
        let on = r.State().map(|s| s == RadioState::On).ok();
        match r.Kind() {
            Ok(RadioKind::WiFi) => wifi = wifi.or(on),
            Ok(RadioKind::Bluetooth) => bt = bt.or(on),
            _ => {}
        }
    }
    (wifi, bt)
}

fn set_radio(bluetooth: bool, on: bool) -> Result<(), String> {
    static ACCESS: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ACCESS.get_or_init(|| { let _ = wait(Radio::RequestAccessAsync(), Duration::from_secs(4)); });
    let list = radio_list();
    if list.is_empty() { return Err("Windows did not find the radio".into()) }
    let kind = if bluetooth { RadioKind::Bluetooth } else { RadioKind::WiFi };
    for r in list {
        if r.Kind().ok() == Some(kind) {
            let state = if on { RadioState::On } else { RadioState::Off };
            wait(r.SetStateAsync(state), Duration::from_secs(8))?;
        }
    }
    Ok(())
}

/// Mobile hotspot for the current internet connection: on, off, or None when it cannot be shared.
fn hotspot() -> Option<bool> {
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Networking::NetworkOperators::{NetworkOperatorTetheringManager, TetheringOperationalState};
    let profile = NetworkInformation::GetInternetConnectionProfile().ok()?;
    let m = NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile).ok()?;
    match m.TetheringOperationalState().ok()? {
        TetheringOperationalState::On => Some(true),
        TetheringOperationalState::Off | TetheringOperationalState::InTransition => Some(false),
        _ => None,
    }
}

fn set_hotspot(on: bool) -> Result<(), String> {
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Networking::NetworkOperators::{NetworkOperatorTetheringManager, TetheringOperationStatus};
    let profile = NetworkInformation::GetInternetConnectionProfile().map_err(|_| "No internet connection to share".to_string())?;
    let m = NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile).map_err(|e| e.message())?;
    let op = if on { m.StartTetheringAsync() } else { m.StopTetheringAsync() };
    let r = wait(op, Duration::from_secs(25))?;
    match r.Status() {
        Ok(TetheringOperationStatus::Success) => Ok(()),
        _ => Err(r.AdditionalErrorMessage().map(|m| m.to_string()).ok().filter(|m| !m.is_empty()).unwrap_or_else(|| "Windows refused".to_string())),
    }
}

/// Asks Windows to report every radio being switched on or off (by Ronin, by Windows' own
/// settings, or by a key on the keyboard).
fn watch_radios(tx: Sender<NetCmd>) {
    spawn(move || {
        // The same radios are kept for reading and switching, and stay alive so their reports keep coming.
        for r in radio_list() {
            let t = tx.clone();
            let h = windows::Foundation::TypedEventHandler::<Radio, windows::core::IInspectable>::new(move |_, _| { let _ = t.send(NetCmd::RadioChanged); Ok(()) });
            let _ = r.StateChanged(&h);
        }
        let _ = tx.send(NetCmd::RadioChanged);
    });
}

fn run(state: Arc<Mutex<NetState>>, rx: Receiver<NetCmd>, news: Sender<NetCmd>, wake: eframe::egui::Context) {
    crate::sys::com_init_mta();
    let wlan = Wlan::open();
    if let Some(w) = &wlan { w.listen(news.clone()); }
    watch_radios(news);
    let set = |f: &dyn Fn(&mut NetState)| { if let Ok(mut s) = state.lock() { f(&mut s); } wake.request_repaint(); };
    set(&|s| s.no_wifi = wlan.as_ref().and_then(|w| w.iface).is_none());
    let list = |wlan: &Option<Wlan>| {
        let nets = wlan.as_ref().map(|w| w.networks()).unwrap_or_default();
        let now = nets.iter().find(|n| n.connected).map(|n| (n.ssid.clone(), n.quality));
        set(&|s| { s.networks = nets.clone(); s.connected = now.clone(); });
    };
    // Radio and hotspot state are read on a thread of their own (one at a time): those Windows
    // calls can be slow, and the Wi-Fi list must never wait for them.
    let reading = Arc::new(AtomicBool::new(false));
    let radios_now = || {
        if reading.swap(true, Ordering::SeqCst) { return }
        let (st, wk, busy) = (state.clone(), wake.clone(), reading.clone());
        spawn(move || {
            let (w, b) = radios();
            if let Ok(mut s) = st.lock() {
                // A switch still in progress keeps what was clicked.
                if s.hold.map(|t| t.elapsed() > Duration::from_secs(6)).unwrap_or(true) { s.wifi_on = w; s.bluetooth_on = b; s.hold = None; }
            }
            wk.request_repaint();
            let hs = hotspot();
            if let Ok(mut s) = st.lock() { s.hotspot_on = hs; }
            wk.request_repaint();
            busy.store(false, Ordering::SeqCst);
        });
    };
    // Switching radios or the hotspot runs beside this thread too; the popup already shows the
    // new state, and the real one is read back when Windows has finished.
    let switch = |what: &'static str, job: Box<dyn FnOnce() -> Result<(), String> + Send>| {
        let (st, wk, busy) = (state.clone(), wake.clone(), reading.clone());
        spawn(move || {
            let r = job();
            // Switching is done: show what Windows really has now (instant: the radios are kept).
            let (w, b) = radios();
            if let Ok(mut s) = st.lock() { s.error = r.err().map(|e| format!("{what}: {e}")); s.hold = None; s.wifi_on = w; s.bluetooth_on = b; }
            wk.request_repaint();
            if !busy.swap(true, Ordering::SeqCst) {
                let hs = hotspot();
                if let Ok(mut s) = st.lock() { s.hotspot_on = hs; }
                busy.store(false, Ordering::SeqCst);
            }
            wk.request_repaint();
        });
    };
    // Which radios were on before airplane mode, to turn the same ones back on.
    let mut before_airplane = (true, false);
    list(&wlan);
    radios_now();
    let mut last_scan = crate::sys::ago(Duration::from_secs(60));
    // Times to read the list again: a scan's results and a new connection arrive a moment later,
    // so the list is read a few times instead of the thread sleeping blind.
    let mut later: Vec<Instant> = Vec::new();
    let mut connecting: Option<(String, Instant)> = None;
    loop {
        let wait = later.iter().min().map(|t| t.saturating_duration_since(Instant::now())).unwrap_or(Duration::from_secs(10));
        let cmd = match rx.recv_timeout(wait.max(Duration::from_millis(30))) {
            Ok(c) => Some(c),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let soon = |ms: &[u64]| ms.iter().map(|m| Instant::now() + Duration::from_millis(*m)).collect::<Vec<_>>();
        match cmd {
            Some(NetCmd::Refresh) => { set(&|s| s.error = None); list(&wlan); radios_now(); }
            Some(NetCmd::Changed(scan_done)) => {
                if scan_done { set(&|s| s.scanning = false); }
                // News often comes in bursts: read once, a moment after the first.
                if !later.iter().any(|t| t.saturating_duration_since(Instant::now()) < Duration::from_millis(250)) { later.extend(soon(&[150])); }
            }
            Some(NetCmd::RadioChanged) => { radios_now(); later.extend(soon(&[400])); }
            Some(NetCmd::Scan) => {
                // Windows allows a fresh scan every few seconds.
                if last_scan.elapsed() > Duration::from_secs(4) {
                    if let Some(w) = &wlan { w.scan(); set(&|s| s.scanning = true); }
                    last_scan = Instant::now();
                    later.extend(soon(&[700, 1600, 3200]));
                }
            }
            Some(NetCmd::Connect { ssid, password }) => {
                set(&|s| { s.busy = Some(format!("Connecting to {ssid}…")); s.error = None; });
                let known = state.lock().ok().and_then(|s| s.networks.iter().find(|n| n.ssid == ssid).cloned());
                let r = match (&wlan, known) {
                    (Some(w), Some(n)) if n.known && password.is_empty() => w.connect_profile(&n.profile),
                    (Some(w), Some(n)) => w.join(&ssid, n.security, &password),
                    _ => Err("That network is out of reach".to_string()),
                };
                match r {
                    Ok(()) => { connecting = Some((ssid, Instant::now())); later.extend(soon(&[500, 1200, 2000, 3000, 4500, 6500, 9000])); }
                    Err(e) => set(&|s| { s.busy = None; s.error = Some(e.clone()); }),
                }
            }
            Some(NetCmd::Disconnect) => {
                if let Some(w) = &wlan { w.disconnect(); }
                // Shown at once; Windows confirms a moment later.
                set(&|s| s.networks.iter_mut().for_each(|n| n.connected = false));
                later.extend(soon(&[600, 1500]));
            }
            Some(NetCmd::Radio { bluetooth, on }) => {
                switch(if bluetooth { "Could not switch Bluetooth" } else { "Could not switch Wi-Fi" }, Box::new(move || set_radio(bluetooth, on)));
                if !bluetooth { later.extend(soon(&[1500, 3500])); }
            }
            Some(NetCmd::Airplane(on)) => {
                let (w, b) = state.lock().map(|s| (s.wifi_on, s.bluetooth_on)).unwrap_or((None, None));
                if on { before_airplane = (w == Some(true), b == Some(true)); }
                let (w0, b0, has_bt) = (before_airplane.0, before_airplane.1, b.is_some());
                switch("Could not switch airplane mode", Box::new(move || if on {
                    set_radio(false, false).and(if has_bt { set_radio(true, false) } else { Ok(()) })
                } else {
                    set_radio(false, w0 || !b0).and(if b0 { set_radio(true, true) } else { Ok(()) })
                }));
                later.extend(soon(&[1500, 3500]));
            }
            Some(NetCmd::Hotspot(on)) => {
                switch("Mobile hotspot", Box::new(move || set_hotspot(on)));
            }
            // Nothing happened for a while: a quick check anyway (cheap; no scan).
            None => if later.is_empty() { list(&wlan); },
        }
        // Read the list again when one of the planned times has come.
        if later.iter().any(|t| *t <= Instant::now()) {
            later.retain(|t| *t > Instant::now());
            list(&wlan);
            if later.is_empty() { set(&|s| s.scanning = false); }
            if let Some((ssid, at)) = connecting.clone() {
                let done = state.lock().map(|s| s.networks.iter().any(|n| n.ssid == ssid && n.connected)).unwrap_or(false);
                if done { connecting = None; set(&|s| s.busy = None); }
                else if at.elapsed() > Duration::from_secs(10) {
                    connecting = None;
                    set(&|s| { s.busy = None; s.error = Some(format!("Could not connect to {ssid}. Check the password and try again.")); });
                }
            }
        }
    }
}
