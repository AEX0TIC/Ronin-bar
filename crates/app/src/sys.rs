//! Everything that talks to Windows. Only Windows' own APIs are used: no helper programs.
use ronin_core::{settings::Original, theme::Theme};
use std::ffi::c_void;
use std::path::Path;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LUID, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::Networking::NetworkListManager::{INetworkListManager, NetworkListManager};
use windows::Win32::Security::{AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Power::{GetSystemPowerStatus, SetSuspendState, SYSTEM_POWER_STATUS};
use windows::Win32::System::Shutdown::{ExitWindowsEx, LockWorkStation, EWX_LOGOFF, EWX_POWEROFF, EWX_REBOOT, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, GetSystemTimes, OpenProcessToken};
use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LWIN};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABE_TOP, ABM_GETSTATE, ABM_NEW, ABM_REMOVE, ABM_SETPOS, ABM_SETSTATE, ABS_ALWAYSONTOP, ABS_AUTOHIDE, APPBARDATA};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::Win32::UI::WindowsAndMessaging::*;
use winreg::{enums::*, RegKey};

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const DWM: &str = r"Software\Microsoft\Windows\DWM";
const ACCENT: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() }

// ───── registry ─────
fn hkcu() -> RegKey { RegKey::predef(HKEY_CURRENT_USER) }
pub fn reg_dword(path: &str, name: &str) -> Option<u32> { hkcu().open_subkey(path).ok()?.get_value(name).ok() }
fn set_dword(path: &str, name: &str, v: u32) { if let Ok((k, _)) = hkcu().create_subkey(path) { let _ = k.set_value(name, &v); } }
fn restore_dword(path: &str, name: &str, v: Option<u32>) {
    match v {
        Some(v) => set_dword(path, name, v),
        None => if let Ok(k) = hkcu().open_subkey_with_flags(path, KEY_SET_VALUE) { let _ = k.delete_value(name); },
    }
}

// ───── one instance only ─────
/// True if this is the only Ronin Bar running. The mutex lives as long as the process.
pub fn single_instance() -> bool {
    unsafe {
        let h = CreateMutexW(None, true, w!("Local\\RoninBar.Bar"));
        h.is_ok() && windows::Win32::Foundation::GetLastError() != windows::Win32::Foundation::ERROR_ALREADY_EXISTS
    }
}

// ───── wallpaper & theme ─────
pub fn current_wallpaper() -> String {
    let mut buf = [0u16; 520];
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, Some(buf.as_mut_ptr() as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

pub fn set_wallpaper(path: &Path) -> bool {
    if let Ok((k, _)) = hkcu().create_subkey(r"Control Panel\Desktop") {
        let _ = k.set_value("WallpaperStyle", &"10");   // Fill
        let _ = k.set_value("TileWallpaper", &"0");
    }
    let p = wide(&path.display().to_string());
    unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, Some(p.as_ptr() as *mut c_void), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE).is_ok() }
}

fn broadcast_theme_change() {
    let s = wide("ImmersiveColorSet");
    unsafe { let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(s.as_ptr() as isize), SMTO_ABORTIFHUNG, 300, None); }
}

/// Windows dark/light mode and accent colour for this theme.
pub fn apply_theme(t: &Theme) {
    let light = u32::from(!t.dark);
    set_dword(PERSONALIZE, "AppsUseLightTheme", light);
    set_dword(PERSONALIZE, "SystemUsesLightTheme", light);
    let [r, g, b] = t.accent;
    let abgr = 0xff00_0000 | (u32::from(b) << 16) | (u32::from(g) << 8) | u32::from(r);
    set_dword(DWM, "AccentColor", abgr);
    set_dword(ACCENT, "AccentColorMenu", abgr);
    broadcast_theme_change();
}

/// What Windows looks like now (taken once, before Ronin Bar changes anything).
pub fn snapshot() -> Original {
    Original {
        wallpaper: current_wallpaper(),
        apps_use_light_theme: reg_dword(PERSONALIZE, "AppsUseLightTheme"),
        system_uses_light_theme: reg_dword(PERSONALIZE, "SystemUsesLightTheme"),
        accent_color: reg_dword(DWM, "AccentColor"),
        accent_color_menu: reg_dword(ACCENT, "AccentColorMenu"),
        taskbar_state: Some(taskbar_state()),
    }
}

/// Puts Windows back exactly as `snapshot` found it.
pub fn restore(o: &Original) {
    restore_dword(PERSONALIZE, "AppsUseLightTheme", o.apps_use_light_theme);
    restore_dword(PERSONALIZE, "SystemUsesLightTheme", o.system_uses_light_theme);
    restore_dword(DWM, "AccentColor", o.accent_color);
    restore_dword(ACCENT, "AccentColorMenu", o.accent_color_menu);
    broadcast_theme_change();
    show_taskbar(o.taskbar_state);
    let p = wide(&o.wallpaper);
    unsafe { let _ = SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, Some(p.as_ptr() as *mut c_void), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE); }
}

// ───── system stats ─────
fn filetime(f: windows::Win32::Foundation::FILETIME) -> u64 { (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime) }

/// (idle, kernel, user) CPU time counters; CPU % comes from the difference of two samples.
pub fn cpu_times() -> (u64, u64, u64) {
    let (mut i, mut k, mut u) = Default::default();
    unsafe { let _ = GetSystemTimes(Some(&mut i), Some(&mut k), Some(&mut u)); }
    (filetime(i), filetime(k), filetime(u))
}

/// Memory in use, percent.
pub fn ram_percent() -> u8 {
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe { let _ = GlobalMemoryStatusEx(&mut m); }
    m.dwMemoryLoad.min(100) as u8
}

/// (percent, charging), or None on PCs without a battery.
pub fn battery() -> Option<(u8, bool)> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).ok()?; }
    if s.BatteryFlag & 128 != 0 || s.BatteryLifePercent > 100 { return None }   // no battery / unknown
    Some((s.BatteryLifePercent, s.ACLineStatus == 1))
}

/// Initialises COM on the calling thread (the worker thread owns all COM objects).
pub fn com_init() { unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); } }

pub fn network_manager() -> Option<INetworkListManager> { unsafe { CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok() } }
pub fn online(n: &INetworkListManager) -> bool { unsafe { n.IsConnectedToInternet().map(|b| b.as_bool()).unwrap_or(false) } }

pub fn volume_endpoint() -> Option<IAudioEndpointVolume> {
    unsafe {
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let d = e.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}
pub fn volume_get(v: &IAudioEndpointVolume) -> Option<(f32, bool)> {
    unsafe { Some((v.GetMasterVolumeLevelScalar().ok()?, v.GetMute().ok()?.as_bool())) }
}
pub fn volume_set(v: &IAudioEndpointVolume, level: f32) { unsafe { let _ = v.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null()); } }
pub fn mute_set(v: &IAudioEndpointVolume, mute: bool) { unsafe { let _ = v.SetMute(mute, std::ptr::null()); } }

// ───── power ─────
pub fn lock() { unsafe { let _ = LockWorkStation(); } }
pub fn sleep() { unsafe { let _ = SetSuspendState(false, false, false); } }

fn enable_shutdown_privilege() {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).is_err() { return }
        let mut luid = LUID::default();
        if LookupPrivilegeValueW(None, w!("SeShutdownPrivilege"), &mut luid).is_ok() {
            let mut tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, ..Default::default() };
            tp.Privileges[0].Luid = luid;
            tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
            let _ = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
        }
        let _ = CloseHandle(token);
    }
}
pub fn restart() { enable_shutdown_privilege(); unsafe { let _ = ExitWindowsEx(EWX_REBOOT, SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_FLAG_PLANNED); } }
pub fn shut_down() { enable_shutdown_privilege(); unsafe { let _ = ExitWindowsEx(EWX_POWEROFF, SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_FLAG_PLANNED); } }

// ───── the bar window ─────
pub fn find_window(title: &str) -> Option<HWND> {
    let t = HSTRING::from(title);
    unsafe { FindWindowW(PCWSTR::null(), &t).ok().filter(|h| !h.is_invalid()) }
}

/// The primary monitor's size in physical pixels, and its DPI scale.
pub fn primary_screen() -> (i32, i32, f32) {
    unsafe {
        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        let dpi = windows::Win32::UI::HiDpi::GetDpiForSystem();
        (w, h, dpi as f32 / 96.0)
    }
}

fn appbar_data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: hwnd, uEdge: ABE_TOP, ..Default::default() }
}

/// Reserves a strip at the top of the primary screen for the bar, so maximised windows stay below
/// it (like a second taskbar), and moves the bar there. Returns false if Windows refused.
pub fn appbar_register(hwnd: HWND, height_px: i32) -> bool {
    unsafe {
        let mut d = appbar_data(hwnd);
        if SHAppBarMessage(ABM_NEW, &mut d) == 0 { return false }
        let (w, _, _) = primary_screen();
        // Always the very top of the screen. (Windows' suggested spot can be lower, e.g. while a bar
        // that was closed without saying goodbye still holds the top; the bar must not sit under it.)
        d.rc = RECT { left: 0, top: 0, right: w, bottom: height_px };
        SHAppBarMessage(ABM_SETPOS, &mut d);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, w, height_px, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    keep_top_free(height_px);
    true
}

fn work_area() -> RECT {
    let mut r = RECT::default();
    unsafe { let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut RECT as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)); }
    r
}

/// Keeps the bar at the very top of the main screen, full width, if anything moved it.
pub fn pin_top(hwnd: HWND, height_px: i32) {
    let (w, _, _) = primary_screen();
    let mut r = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut r).is_ok() && (r.left != 0 || r.top != 0 || r.right != w || r.bottom != height_px) {
            let _ = SetWindowPos(hwnd, None, 0, 0, w, height_px, SWP_NOACTIVATE | SWP_NOZORDER);
        }
    }
}

/// Makes the work area of the main screen start exactly where the bar ends, so maximised windows
/// (with their title bar, tabs, and close / minimise / maximise buttons) sit right below the bar:
/// not under it, and without a gap. Returns true if it had to change anything.
pub fn keep_top_free(height_px: i32) -> bool {
    let mut r = work_area();
    if r.top == height_px || r.right <= r.left { return false }
    r.top = height_px;
    unsafe { let _ = SystemParametersInfoW(SPI_SETWORKAREA, 0, Some(&mut r as *mut RECT as *mut c_void), SPIF_SENDCHANGE); }
    true
}

/// True for a maximised window on the main screen whose top edge is not where the bar ends
/// (it was maximised before the bar took its place, so its title bar is under the bar or there is
/// a gap). Only once the work area itself is right, so fixing it can never loop.
pub fn misfit_maximized(raw: isize, height_px: i32) -> bool {
    if work_area().top != height_px { return false }
    unsafe {
        let h = HWND(raw as *mut c_void);
        let primary = MonitorFromWindow(GetDesktopWindow(), MONITOR_DEFAULTTOPRIMARY);
        if !IsZoomed(h).as_bool() || MonitorFromWindow(h, windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST) != primary { return false }
        // The visible frame (GetWindowRect includes invisible resize borders).
        let mut r = RECT::default();
        let visible = windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(h, windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut RECT as *mut c_void, std::mem::size_of::<RECT>() as u32).is_ok();
        if !visible { let _ = GetWindowRect(h, &mut r); r.top += 8; }
        (r.top - height_px).abs() > 2
    }
}

/// Maximises a window again so it fits the current work area.
pub fn refit(raw: isize) {
    let h = HWND(raw as *mut c_void);
    unsafe {
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        let _ = ShowWindow(h, SW_SHOWMAXIMIZED);
    }
}

/// Gives the top strip back to other windows (when the bar quits).
pub fn release_top(height_px: i32) {
    let mut r = work_area();
    if r.top != height_px { return }
    r.top = 0;
    unsafe { let _ = SystemParametersInfoW(SPI_SETWORKAREA, 0, Some(&mut r as *mut RECT as *mut c_void), SPIF_SENDCHANGE); }
}

// ───── the Windows taskbar ─────
/// Every Windows taskbar (one per screen).
fn taskbars() -> Vec<HWND> {
    let mut v = Vec::new();
    unsafe {
        if let Ok(h) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) { if !h.is_invalid() { v.push(h); } }
        let mut after: Option<HWND> = None;
        while let Ok(h) = FindWindowExW(None, after, w!("Shell_SecondaryTrayWnd"), PCWSTR::null()) {
            if h.is_invalid() || v.len() > 16 { break }
            v.push(h);
            after = Some(h);
        }
    }
    v
}

fn tray_data() -> Option<APPBARDATA> {
    let tray = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).ok()? };
    Some(APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: tray, ..Default::default() })
}

/// The taskbar's auto-hide / always-on-top state.
pub fn taskbar_state() -> u32 {
    tray_data().map(|mut d| unsafe { SHAppBarMessage(ABM_GETSTATE, &mut d) as u32 }).unwrap_or(ABS_ALWAYSONTOP)
}

fn set_taskbar_state(state: u32) {
    if let Some(mut d) = tray_data() {
        d.lParam = LPARAM(state as isize);
        unsafe { SHAppBarMessage(ABM_SETSTATE, &mut d); }
    }
}

static TASKBAR_HIDDEN: AtomicBool = AtomicBool::new(false);
static TASKBAR_BEFORE: AtomicU32 = AtomicU32::new(ABS_ALWAYSONTOP);

/// Hides the Windows taskbar while the bar runs: auto-hide frees the bottom of the screen, and
/// the taskbar windows are hidden. Called again now and then, because Explorer can bring it back.
/// `before` is the state to put back afterwards.
pub fn hide_taskbar(before: u32) {
    if !TASKBAR_HIDDEN.swap(true, Ordering::SeqCst) {
        TASKBAR_BEFORE.store(before, Ordering::SeqCst);
        set_taskbar_state(ABS_AUTOHIDE);
    }
    for t in taskbars() { unsafe { if IsWindowVisible(t).as_bool() { let _ = ShowWindow(t, SW_HIDE); } } }
}

/// Brings the Windows taskbar back as it was. Safe to call any number of times.
pub fn show_taskbar(state: Option<u32>) {
    let before = TASKBAR_BEFORE.load(Ordering::SeqCst);
    let was_hidden = TASKBAR_HIDDEN.swap(false, Ordering::SeqCst);
    for t in taskbars() { unsafe { let _ = ShowWindow(t, SW_SHOWNA); } }
    match state {
        Some(s) => set_taskbar_state(s),
        None if was_hidden => set_taskbar_state(before),
        None => {}
    }
}

// ───── Windows shortcuts ─────
/// Presses Win (+ `key`), e.g. Win for Start, Win+A for quick settings, Win+N for notifications.
pub fn press_win(key: Option<u16>) {
    let k = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() }, ..Default::default() } },
    };
    let mut seq = vec![k(VK_LWIN, false)];
    if let Some(c) = key { seq.push(k(VIRTUAL_KEY(c), false)); seq.push(k(VIRTUAL_KEY(c), true)); }
    seq.push(k(VK_LWIN, true));
    unsafe { SendInput(&seq, std::mem::size_of::<INPUT>() as i32); }
}

/// Opens one of a fixed set of Windows settings pages (the "more settings" links in Ronin's popups).
pub fn open_settings(page: &str) {
    const PAGES: [&str; 7] = ["sound", "network-wifi", "network-status", "bluetooth", "display", "batterysaver", "powersleep"];
    if !PAGES.contains(&page) { return }
    let uri = HSTRING::from(format!("ms-settings:{page}"));
    unsafe { let _ = windows::Win32::UI::Shell::ShellExecuteW(None, w!("open"), &uri, None, None, SW_SHOWNORMAL); }
}

/// Signs out of Windows (open apps are asked to close).
pub fn sign_out() { unsafe { let _ = ExitWindowsEx(EWX_LOGOFF, SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_FLAG_PLANNED); } }

/// Opens Task Manager.
pub fn task_manager() {
    unsafe { let _ = windows::Win32::UI::Shell::ShellExecuteW(None, w!("open"), w!("taskmgr.exe"), None, None, SW_SHOWNORMAL); }
}

pub fn appbar_remove(hwnd: HWND) {
    unsafe { let mut d = appbar_data(hwnd); SHAppBarMessage(ABM_REMOVE, &mut d); }
}

/// True while a fullscreen app or game covers the primary screen (the bar then steps back).
pub fn fullscreen_app_active() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == GetDesktopWindow() || fg == GetShellWindow() { return false }
        // A maximised window is not fullscreen, even when the taskbar is hidden and it fills the screen.
        if IsZoomed(fg).as_bool() { return false }
        let ex = GetWindowLongW(fg, GWL_STYLE) as u32;
        if ex & WS_CAPTION.0 == WS_CAPTION.0 { return false }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() { return false }
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTOPRIMARY), &mut mi).as_bool() { return false }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

/// Puts the bar above or below other windows.
pub fn set_topmost(hwnd: HWND, on: bool) {
    let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
    unsafe { let _ = SetWindowPos(hwnd, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE); }
}

pub fn close_window(hwnd: HWND) { unsafe { let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)); } }

/// A plain Windows question box. Returns true for Yes/OK.
pub fn ask(title: &str, text: &str) -> bool {
    let (t, m) = (HSTRING::from(title), HSTRING::from(text));
    unsafe { MessageBoxW(None, &m, &t, MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND) == IDYES }
}
pub fn tell(title: &str, text: &str) {
    let (t, m) = (HSTRING::from(title), HSTRING::from(text));
    unsafe { MessageBoxW(None, &m, &t, MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND); }
}

/// Local date and time: (weekday 0=Sunday, day, month 1-12, hour, minute).
/// Today's date: (year, month, day).
pub fn today() -> (i32, u32, u32) {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (i32::from(t.wYear), u32::from(t.wMonth), u32::from(t.wDay))
}

pub fn local_time() -> (u16, u16, u16, u16, u16) {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (t.wDayOfWeek, t.wDay, t.wMonth, t.wHour, t.wMinute)
}
