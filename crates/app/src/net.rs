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
    pub bluetooth_on: Option<bool>,
}

pub enum NetCmd {
    /// Re-read the list Windows already has (instant), without a new scan.
    Refresh,
    Scan,
    Connect { ssid: String, password: String },
    Disconnect,
    Radio { bluetooth: bool, on: bool },
}

/// Starts the thread. `wake` repaints the bar as soon as anything changes.
pub fn start(wake: eframe::egui::Context) -> (Arc<Mutex<NetState>>, Sender<NetCmd>) {
    let state = Arc::new(Mutex::new(NetState::default()));
    let (tx, rx) = channel();
    let s = state.clone();
    std::thread::spawn(move || run(s, rx, wake));
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

/// Wi-Fi and Bluetooth radios: on, off, or None when there is none (or access was refused).
fn radios() -> (Option<bool>, Option<bool>) {
    let Ok(list) = Radio::GetRadiosAsync().and_then(|a| a.join()) else { return (None, None) };
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
    let _ = Radio::RequestAccessAsync().and_then(|a| a.join());
    let list = Radio::GetRadiosAsync().and_then(|a| a.join()).map_err(|e| e.message())?;
    let kind = if bluetooth { RadioKind::Bluetooth } else { RadioKind::WiFi };
    for r in list {
        if r.Kind().ok() == Some(kind) {
            let state = if on { RadioState::On } else { RadioState::Off };
            r.SetStateAsync(state).and_then(|a| a.join()).map_err(|e| e.message())?;
        }
    }
    Ok(())
}

fn run(state: Arc<Mutex<NetState>>, rx: Receiver<NetCmd>, wake: eframe::egui::Context) {
    crate::sys::com_init_mta();
    let wlan = Wlan::open();
    let set = |f: &dyn Fn(&mut NetState)| { if let Ok(mut s) = state.lock() { f(&mut s); } wake.request_repaint(); };
    set(&|s| s.no_wifi = wlan.as_ref().and_then(|w| w.iface).is_none());
    let list = |wlan: &Option<Wlan>| {
        let nets = wlan.as_ref().map(|w| w.networks()).unwrap_or_default();
        set(&|s| s.networks = nets.clone());
    };
    let radios_now = || { let (w, b) = radios(); set(&|s| { s.wifi_on = w; s.bluetooth_on = b; }); };
    list(&wlan);
    radios_now();
    let mut last_scan = Instant::now() - Duration::from_secs(60);
    // Times to read the list again: a scan's results and a new connection arrive a moment later,
    // so the list is read a few times instead of the thread sleeping blind.
    let mut later: Vec<Instant> = Vec::new();
    let mut connecting: Option<(String, Instant)> = None;
    loop {
        let wait = later.iter().min().map(|t| t.saturating_duration_since(Instant::now())).unwrap_or(Duration::from_secs(5));
        let cmd = match rx.recv_timeout(wait.max(Duration::from_millis(30))) {
            Ok(c) => Some(c),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let soon = |ms: &[u64]| ms.iter().map(|m| Instant::now() + Duration::from_millis(*m)).collect::<Vec<_>>();
        match cmd {
            Some(NetCmd::Refresh) => { list(&wlan); radios_now(); }
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
                let r = set_radio(bluetooth, on);
                set(&|s| s.error = r.clone().err().map(|e| format!("Could not switch {}: {e}", if bluetooth { "Bluetooth" } else { "Wi-Fi" })));
                radios_now();
                later.extend(soon(&[800, 2500]));
            }
            None => {}
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
