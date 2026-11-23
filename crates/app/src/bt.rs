//! Bluetooth devices for Ronin's Bluetooth popup, on a thread of their own: paired devices and
//! whether they are connected, connecting and disconnecting headphones and speakers (the same
//! way Windows' own panel does it, through the audio driver), searching for nearby devices,
//! pairing and forgetting. Only Windows' public device functions are used.
use ronin_core::bluetooth as rules;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{Interface, GUID, HSTRING};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationKind, DevicePairingResultStatus, DeviceUnpairingResultStatus};
use windows::Foundation::IReference;
use windows_collections::IIterable;

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// Windows' id for the device (used to pair and forget it).
    pub id: String,
    pub name: String,
    /// 12 hex digits, when Windows tells it.
    pub addr: Option<String>,
    pub connected: bool,
    /// Headphones, speakers and headsets: Ronin can connect and disconnect these.
    pub audio: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BtState {
    /// Paired devices, connected ones first (None until read).
    pub paired: Option<Vec<Device>>,
    /// Devices found by the last search, not paired yet.
    pub nearby: Option<Vec<Device>>,
    pub scanning: bool,
    pub busy: Option<String>,
    pub error: Option<String>,
}

pub enum BtCmd { Refresh, Scan, Connect(String), Disconnect(String), Pair(String), Forget(String) }

pub fn start(wake: eframe::egui::Context) -> (Arc<Mutex<BtState>>, Sender<BtCmd>) {
    let state = Arc::new(Mutex::new(BtState::default()));
    let (tx, rx) = channel();
    let s = state.clone();
    std::thread::spawn(move || run(s, rx, wake));
    (state, tx)
}

const CLASSIC: &str = "{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}";
const LOW_ENERGY: &str = "{bb7bb05e-5972-42b5-94fc-76eaa7084d49}";
const CONNECTED: &str = "System.Devices.Aep.IsConnected";
const ADDRESS: &str = "System.Devices.Aep.DeviceAddress";
/// Windows' Bluetooth audio commands (KSPROPSETID_BtAudio): 0 = reconnect, 1 = disconnect.
const BT_AUDIO: GUID = GUID::from_u128(0x7fa06c40_b8f6_4c7e_8556_e8c33a12e54d);

/// Bluetooth devices Windows knows, paired or not.
fn find(paired: bool) -> Option<Vec<Device>> {
    let aqs = HSTRING::from(format!(
        "(System.Devices.Aep.ProtocolId:=\"{CLASSIC}\" OR System.Devices.Aep.ProtocolId:=\"{LOW_ENERGY}\") AND System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#{}",
        if paired { "True" } else { "False" }));
    let props = IIterable::<HSTRING>::from(vec![HSTRING::from(CONNECTED), HSTRING::from(ADDRESS)]);
    let list = DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(&aqs, &props, DeviceInformationKind::AssociationEndpoint).ok()?.join().ok()?;
    let mut out: Vec<Device> = Vec::new();
    for d in list {
        let name = d.Name().map(|n| n.to_string()).unwrap_or_default();
        if !rules::worth_listing(&name) { continue }
        let p = d.Properties().ok();
        let get = |k: &str| p.as_ref().and_then(|p| p.Lookup(&HSTRING::from(k)).ok());
        let connected = get(CONNECTED).and_then(|v| v.cast::<IReference<bool>>().ok()).and_then(|r| r.Value().ok()).unwrap_or(false);
        let addr = get(ADDRESS).and_then(|v| v.cast::<IReference<HSTRING>>().ok()).and_then(|r| r.Value().ok()).and_then(|a| rules::address(&a.to_string()));
        let dev = Device { id: d.Id().map(|i| i.to_string()).unwrap_or_default(), name, addr, connected, audio: false };
        // A device with both classic and Low Energy shows up twice: keep one, connected first.
        match out.iter_mut().find(|o| o.name == dev.name) {
            Some(o) => { if dev.connected && !o.connected { *o = dev; } }
            None => out.push(dev),
        }
        if out.len() >= 60 { break }
    }
    out.sort_by_key(|d| (!d.connected, d.name.to_lowercase()));
    Some(out)
}

/// The audio driver controls of every Bluetooth headphone, speaker and headset (connected or
/// not), with the driver's device id, which carries the device's address.
fn audio_controls() -> Vec<(String, windows::Win32::Media::KernelStreaming::IKsControl)> {
    use windows::Win32::Media::Audio::{eAll, IDeviceTopology, IMMDeviceEnumerator, IPart, MMDeviceEnumerator, DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATE_UNPLUGGED};
    use windows::Win32::Media::KernelStreaming::IKsControl;
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};
    let mut out = Vec::new();
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return out };
        let Ok(all) = en.EnumAudioEndpoints(eAll, DEVICE_STATE(DEVICE_STATE_ACTIVE.0 | DEVICE_STATE_UNPLUGGED.0)) else { return out };
        for i in 0..all.GetCount().unwrap_or(0).min(64) {
            let Ok(dev) = all.Item(i) else { continue };
            let Ok(topo) = dev.Activate::<IDeviceTopology>(CLSCTX_ALL, None) else { continue };
            let Ok(part) = topo.GetConnector(0).and_then(|c| c.GetConnectedTo()).and_then(|o| o.cast::<IPart>()) else { continue };
            let Ok(p) = part.GetTopologyObject().and_then(|t| t.GetDeviceId()) else { continue };
            let id = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            let low = id.to_ascii_lowercase();
            if !low.contains("bthenum") && !low.contains("bthhfenum") && !low.contains("bthleenum") { continue }
            let mut raw: *mut std::ffi::c_void = std::ptr::null_mut();
            if part.Activate(CLSCTX_ALL.0, &IKsControl::IID, Some(&mut raw)).is_err() || raw.is_null() { continue }
            out.push((id, IKsControl::from_raw(raw)));
        }
    }
    out
}

/// Connects (or disconnects) a Bluetooth audio device by its address.
fn audio_connect(addr: &str, connect: bool) -> Result<(), String> {
    use windows::Win32::Media::KernelStreaming::{KSIDENTIFIER, KSIDENTIFIER_0, KSIDENTIFIER_0_0, KSPROPERTY_TYPE_GET};
    let controls: Vec<_> = audio_controls().into_iter().filter(|(id, _)| rules::id_has_address(id, addr)).collect();
    if controls.is_empty() { return Err("Windows has no sound driver for this device".into()) }
    let prop = KSIDENTIFIER { Anonymous: KSIDENTIFIER_0 { Anonymous: KSIDENTIFIER_0_0 { Set: BT_AUDIO, Id: if connect { 0 } else { 1 }, Flags: KSPROPERTY_TYPE_GET } } };
    let mut any = false;
    for (_, ks) in controls {
        let mut n = 0u32;
        // Stereo and hands-free are separate drivers; asking each is what Windows does too.
        any |= unsafe { ks.KsProperty(&prop, std::mem::size_of::<KSIDENTIFIER>() as u32, std::ptr::null_mut(), 0, &mut n) }.is_ok();
    }
    if any { Ok(()) } else { Err("The device did not answer".into()) }
}

fn pair(id: &str) -> Result<(), String> {
    let d = DeviceInformation::CreateFromIdAsync(&HSTRING::from(id)).and_then(|a| a.join()).map_err(|e| e.message())?;
    // Windows shows its own prompt if the device asks for a PIN or a confirmation.
    let r = d.Pairing().and_then(|p| p.PairAsync()).and_then(|a| a.join()).map_err(|e| e.message())?;
    match r.Status() {
        Ok(DevicePairingResultStatus::Paired) | Ok(DevicePairingResultStatus::AlreadyPaired) => Ok(()),
        Ok(s) => Err(format!("Pairing did not finish (code {}). Put the device in pairing mode and try again.", s.0)),
        Err(e) => Err(e.message()),
    }
}

fn forget(id: &str) -> Result<(), String> {
    let d = DeviceInformation::CreateFromIdAsync(&HSTRING::from(id)).and_then(|a| a.join()).map_err(|e| e.message())?;
    let r = d.Pairing().and_then(|p| p.UnpairAsync()).and_then(|a| a.join()).map_err(|e| e.message())?;
    match r.Status() {
        Ok(DeviceUnpairingResultStatus::Unpaired) | Ok(DeviceUnpairingResultStatus::AlreadyUnpaired) => Ok(()),
        _ => Err("Windows could not remove the device".into()),
    }
}

enum Kind { Connect, Other }

fn run(state: Arc<Mutex<BtState>>, rx: Receiver<BtCmd>, wake: eframe::egui::Context) {
    crate::sys::com_init_mta();
    let set = |f: &dyn Fn(&mut BtState)| { if let Ok(mut s) = state.lock() { f(&mut s); } wake.request_repaint(); };
    let refresh = || {
        let audio: Vec<String> = audio_controls().into_iter().map(|(id, _)| id).collect();
        let mut list = find(true).unwrap_or_default();
        for d in &mut list { d.audio = d.addr.as_deref().map(|a| audio.iter().any(|id| rules::id_has_address(id, a))).unwrap_or(false); }
        set(&|s| s.paired = Some(list.clone()));
    };
    let mut later: Vec<Instant> = Vec::new();
    loop {
        let wait = later.iter().min().map(|t| t.saturating_duration_since(Instant::now())).unwrap_or(Duration::from_secs(3600));
        let cmd = match rx.recv_timeout(wait.max(Duration::from_millis(30))) {
            Ok(c) => Some(c),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let cmd_kind = match &cmd { Some(BtCmd::Connect(_)) => Kind::Connect, _ => Kind::Other };
        let soon = |ms: &[u64]| ms.iter().map(|m| Instant::now() + Duration::from_millis(*m)).collect::<Vec<_>>();
        match cmd {
            Some(BtCmd::Refresh) => { set(&|s| s.error = None); refresh(); }
            Some(BtCmd::Scan) => {
                if state.lock().map(|s| s.scanning).unwrap_or(true) { continue }
                set(&|s| { s.scanning = true; s.error = None; });
                // Searching takes several seconds; it runs beside this thread so the buttons keep working.
                let (st, wk) = (state.clone(), wake.clone());
                std::thread::spawn(move || {
                    crate::sys::com_init_mta();
                    let found = find(false).unwrap_or_default();
                    if let Ok(mut s) = st.lock() { s.nearby = Some(found); s.scanning = false; }
                    wk.request_repaint();
                });
            }
            Some(BtCmd::Connect(addr)) | Some(BtCmd::Disconnect(addr)) => {
                let on = matches!(cmd_kind, Kind::Connect);
                set(&|s| { s.busy = Some(if on { "Connecting…".into() } else { "Disconnecting…".into() }); s.error = None; });
                let r = audio_connect(&addr, on);
                set(&|s| { s.busy = None; s.error = r.clone().err(); });
                later.extend(soon(&[700, 2000, 4000, 7000]));
            }
            Some(BtCmd::Pair(id)) => {
                set(&|s| { s.busy = Some("Pairing… (accept on the device if it asks)".into()); s.error = None; });
                let r = pair(&id);
                set(&|s| { s.busy = None; s.error = r.clone().err(); if r.is_ok() { if let Some(n) = s.nearby.as_mut() { n.retain(|d| d.id != id); } } });
                later.extend(soon(&[300, 2500, 6000]));
            }
            Some(BtCmd::Forget(id)) => {
                let r = forget(&id);
                set(&|s| { s.error = r.clone().err(); if r.is_ok() { if let Some(p) = s.paired.as_mut() { p.retain(|d| d.id != id); } } });
                later.extend(soon(&[500]));
            }
            None => {}
        }
        if later.iter().any(|t| *t <= Instant::now()) {
            later.retain(|t| *t > Instant::now());
            refresh();
        }
    }
}
