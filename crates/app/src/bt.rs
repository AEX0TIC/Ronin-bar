//! Bluetooth devices for Ronin's Bluetooth popup. Windows reports every change to the device
//! list as it happens (a device added, connected, disconnected, removed), the same way its own
//! Settings page is kept up to date, so nothing is asked again and again and nothing waits.
//! Connecting and disconnecting headphones and speakers goes through their audio driver, as
//! Windows' own panel does; pairing and forgetting use Windows' public pairing functions.
use ronin_core::bluetooth as rules;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{Interface, Ref, GUID, HSTRING};
use windows::Devices::Enumeration::{
    DeviceInformation, DeviceInformationKind, DeviceInformationUpdate, DevicePairingResultStatus, DeviceUnpairingResultStatus, DeviceWatcher,
};
use windows::Foundation::{IReference, TypedEventHandler};
use windows_collections::IIterable;
use crate::winrt::{spawn, wait};

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
    /// Paired devices, connected ones first (None until Windows has listed them).
    pub paired: Option<Vec<Device>>,
    /// Devices found by the current or last search, not paired yet.
    pub nearby: Option<Vec<Device>>,
    pub scanning: bool,
    pub busy: Option<String>,
    pub error: Option<String>,
}

pub enum BtCmd { Scan, StopScan, Connect(String), Disconnect(String), Pair(String), Forget(String), /* internal: */ CheckAudio }

pub fn start(wake: eframe::egui::Context) -> (Arc<Mutex<BtState>>, Sender<BtCmd>) {
    let state = Arc::new(Mutex::new(BtState::default()));
    let (tx, rx) = channel();
    let (s, again) = (state.clone(), tx.clone());
    std::thread::spawn(move || run(s, rx, again, wake));
    (state, tx)
}

const CLASSIC: &str = "{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}";
const LOW_ENERGY: &str = "{bb7bb05e-5972-42b5-94fc-76eaa7084d49}";
const CONNECTED: &str = "System.Devices.Aep.IsConnected";
const ADDRESS: &str = "System.Devices.Aep.DeviceAddress";
/// Windows' Bluetooth audio commands (KSPROPSETID_BtAudio): 0 = reconnect, 1 = disconnect.
const BT_AUDIO: GUID = GUID::from_u128(0x7fa06c40_b8f6_4c7e_8556_e8c33a12e54d);
/// How long a search for new devices runs.
const SEARCH: Duration = Duration::from_secs(25);

type Shared = Arc<Mutex<BtState>>;

fn prop<T: windows::core::RuntimeType + 'static>(p: &windows_collections::IMapView<HSTRING, windows::core::IInspectable>, key: &str) -> Option<T> {
    p.Lookup(&HSTRING::from(key)).ok()?.cast::<IReference<T>>().ok()?.Value().ok()
}

fn device(d: &DeviceInformation) -> Option<Device> {
    let name = d.Name().ok()?.to_string();
    if !rules::worth_listing(&name) { return None }
    let p = d.Properties().ok();
    let connected = p.as_ref().and_then(|p| prop::<bool>(p, CONNECTED)).unwrap_or(false);
    let addr = p.as_ref().and_then(|p| prop::<HSTRING>(p, ADDRESS)).and_then(|a| rules::address(&a.to_string()));
    Some(Device { id: d.Id().ok()?.to_string(), name, addr, connected, audio: false })
}

fn list(s: &mut BtState, paired: bool) -> &mut Vec<Device> {
    if paired { s.paired.get_or_insert_with(Vec::new) } else { s.nearby.get_or_insert_with(Vec::new) }
}

fn tidy(l: &mut [Device]) { l.sort_by_key(|d| (!d.connected, d.name.to_lowercase())); }

/// Starts a watcher: Windows calls back for every device added, changed or removed.
fn watch(state: &Shared, wake: &eframe::egui::Context, tx: &Sender<BtCmd>, paired: bool) -> Option<DeviceWatcher> {
    let aqs = HSTRING::from(format!(
        "(System.Devices.Aep.ProtocolId:=\"{CLASSIC}\" OR System.Devices.Aep.ProtocolId:=\"{LOW_ENERGY}\") AND System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#{}",
        if paired { "True" } else { "False" }));
    let props = IIterable::<HSTRING>::from(vec![HSTRING::from(CONNECTED), HSTRING::from(ADDRESS)]);
    let w = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(&aqs, &props, DeviceInformationKind::AssociationEndpoint).ok()?;
    let (st, wk, t) = (state.clone(), wake.clone(), tx.clone());
    let _ = w.Added(&TypedEventHandler::<DeviceWatcher, DeviceInformation>::new(move |_, d: Ref<DeviceInformation>| {
        if let Some(dev) = d.ok().ok().and_then(device) {
            if let Ok(mut s) = st.lock() {
                let l = list(&mut s, paired);
                // A device with both classic and Low Energy shows up twice: keep one.
                match l.iter_mut().find(|o| o.name == dev.name) {
                    Some(o) => { if dev.connected && !o.connected { let audio = o.audio; *o = Device { audio, ..dev }; } }
                    None => l.push(dev),
                }
                tidy(l);
            }
            wk.request_repaint();
            if paired { let _ = t.send(BtCmd::CheckAudio); }
        }
        Ok(())
    }));
    let (st, wk) = (state.clone(), wake.clone());
    let _ = w.Updated(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(move |_, u: Ref<DeviceInformationUpdate>| {
        if let Ok(u) = u.ok() {
            let id = u.Id().map(|i| i.to_string()).unwrap_or_default();
            let on = u.Properties().ok().and_then(|p| prop::<bool>(&p, CONNECTED));
            if let (Some(on), Ok(mut s)) = (on, st.lock()) {
                let l = list(&mut s, paired);
                if let Some(d) = l.iter_mut().find(|d| d.id == id) { d.connected = on; }
                tidy(l);
                // A connect or disconnect asked from the popup is done once Windows reports it.
                if s.busy.is_some() { s.busy = None; }
            }
            wk.request_repaint();
        }
        Ok(())
    }));
    let (st, wk) = (state.clone(), wake.clone());
    let _ = w.Removed(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(move |_, u: Ref<DeviceInformationUpdate>| {
        if let Ok(u) = u.ok() {
            let id = u.Id().map(|i| i.to_string()).unwrap_or_default();
            if let Ok(mut s) = st.lock() { list(&mut s, paired).retain(|d| d.id != id); }
            wk.request_repaint();
        }
        Ok(())
    }));
    let (st, wk) = (state.clone(), wake.clone());
    let _ = w.EnumerationCompleted(&TypedEventHandler::<DeviceWatcher, windows::core::IInspectable>::new(move |_, _| {
        if let Ok(mut s) = st.lock() {
            list(&mut s, paired);   // an empty list now means "none", not "still reading"
            if !paired { s.scanning = false; }
        }
        wk.request_repaint();
        Ok(())
    }));
    w.Start().ok()?;
    Some(w)
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
    let d = wait(DeviceInformation::CreateFromIdAsync(&HSTRING::from(id)), Duration::from_secs(8))?;
    // Windows shows its own prompt if the device asks for a PIN or a confirmation.
    let r = wait(d.Pairing().and_then(|p| p.PairAsync()), Duration::from_secs(60))?;
    match r.Status() {
        Ok(DevicePairingResultStatus::Paired) | Ok(DevicePairingResultStatus::AlreadyPaired) => Ok(()),
        Ok(s) => Err(format!("Pairing did not finish (code {}). Put the device in pairing mode and try again.", s.0)),
        Err(e) => Err(e.message()),
    }
}

fn forget(id: &str) -> Result<(), String> {
    let d = wait(DeviceInformation::CreateFromIdAsync(&HSTRING::from(id)), Duration::from_secs(8))?;
    let r = wait(d.Pairing().and_then(|p| p.UnpairAsync()), Duration::from_secs(15))?;
    match r.Status() {
        Ok(DeviceUnpairingResultStatus::Unpaired) | Ok(DeviceUnpairingResultStatus::AlreadyUnpaired) => Ok(()),
        _ => Err("Windows could not remove the device".into()),
    }
}

fn run(state: Shared, rx: Receiver<BtCmd>, again: Sender<BtCmd>, wake: eframe::egui::Context) {
    crate::sys::com_init_mta();
    let set = |f: &dyn Fn(&mut BtState)| { if let Ok(mut s) = state.lock() { f(&mut s); } wake.request_repaint(); };
    // Paired devices are watched for as long as the bar runs (Windows' Settings does the same).
    let paired = watch(&state, &wake, &again, true);
    if paired.is_none() { set(&|s| { s.paired = Some(Vec::new()); s.error = Some("Windows did not list your Bluetooth devices.".into()); }); }
    let mut search: Option<(DeviceWatcher, Instant)> = None;
    let mut audio_at = crate::sys::ago(Duration::from_secs(60));
    loop {
        let wait_for = search.as_ref().map(|(_, at)| (*at + SEARCH).saturating_duration_since(Instant::now())).unwrap_or(Duration::from_secs(3600));
        let cmd = match rx.recv_timeout(wait_for.max(Duration::from_millis(50))) {
            Ok(c) => Some(c),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        };
        match cmd {
            Some(BtCmd::Scan) => {
                if let Some((w, _)) = search.take() { let _ = w.Stop(); }
                set(&|s| { s.nearby = Some(Vec::new()); s.scanning = true; s.error = None; });
                search = watch(&state, &wake, &again, false).map(|w| (w, Instant::now()));
                if search.is_none() { set(&|s| { s.scanning = false; s.error = Some("Windows could not start a search.".into()); }); }
            }
            Some(BtCmd::StopScan) => {
                if let Some((w, _)) = search.take() { let _ = w.Stop(); }
                set(&|s| s.scanning = false);
            }
            Some(BtCmd::CheckAudio) => {
                // Which paired devices are headphones or speakers (a burst of new devices needs one look).
                if audio_at.elapsed() < Duration::from_millis(800) { continue }
                audio_at = Instant::now();
                let ids: Vec<String> = audio_controls().into_iter().map(|(id, _)| id).collect();
                set(&|s| if let Some(l) = s.paired.as_mut() {
                    for d in l.iter_mut() { d.audio = d.addr.as_deref().map(|a| ids.iter().any(|id| rules::id_has_address(id, a))).unwrap_or(false); }
                });
            }
            Some(c @ (BtCmd::Connect(_) | BtCmd::Disconnect(_))) => {
                let (addr, on) = match c { BtCmd::Connect(a) => (a, true), BtCmd::Disconnect(a) => (a, false), _ => continue };
                let (st, wk) = (state.clone(), wake.clone());
                spawn(move || {
                    let r = audio_connect(&addr, on);
                    if let Ok(mut s) = st.lock() {
                        if let Err(e) = r {
                            s.busy = None;
                            s.error = Some(e);
                            // Put back what Windows really has.
                            if let Some(d) = s.paired.as_mut().and_then(|l| l.iter_mut().find(|d| d.addr.as_deref() == Some(addr.as_str()))) { d.connected = !on; }
                        }
                    }
                    wk.request_repaint();
                    // Windows' report clears "Connecting…"; if none comes, clear it anyway.
                    std::thread::sleep(Duration::from_secs(8));
                    if let Ok(mut s) = st.lock() { s.busy = None; }
                    wk.request_repaint();
                });
            }
            Some(BtCmd::Pair(id)) => {
                set(&|s| { s.busy = Some("Pairing… (accept on the device if it asks)".into()); s.error = None; });
                // Pairing waits for the person (a PIN or a button on the device): its own thread.
                let (st, wk) = (state.clone(), wake.clone());
                spawn(move || {
                    let r = pair(&id);
                    if let Ok(mut s) = st.lock() {
                        s.busy = None;
                        s.error = r.clone().err();
                        if r.is_ok() { if let Some(n) = s.nearby.as_mut() { n.retain(|d| d.id != id); } }
                    }
                    wk.request_repaint();
                });
            }
            Some(BtCmd::Forget(id)) => {
                let (st, wk) = (state.clone(), wake.clone());
                spawn(move || {
                    let r = forget(&id);
                    if let Ok(mut s) = st.lock() { s.error = r.err(); }
                    wk.request_repaint();
                });
            }
            None => {}
        }
        // A search ends by itself after a while.
        if search.as_ref().map(|(_, at)| at.elapsed() >= SEARCH).unwrap_or(false) {
            if let Some((w, _)) = search.take() { let _ = w.Stop(); }
            set(&|s| s.scanning = false);
        }
        let _ = &paired;
    }
}
