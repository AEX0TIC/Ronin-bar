//! The dock: Start-menu apps, open windows, and their icons, using plain Windows APIs.
use ronin_core::dock;
use std::ffi::c_void;
use std::path::Path;
use windows::core::{w, BOOL, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, ShellExecuteW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::*;

/// A top-level app window, as the taskbar would show it.
#[derive(Clone, Debug)]
pub struct Win {
    pub hwnd: isize,
    pub title: String,
    pub exe: Option<String>,
    /// The installed app behind the window (`shell:AppsFolder\\…`), when Windows knows it. Store
    /// apps such as Settings all run inside one host program, so this is how their own name and
    /// icon are found.
    pub app: Option<String>,
}

impl Win {
    /// The program file, unless it is only a host for Store apps.
    pub fn own_exe(&self) -> Option<&str> {
        self.exe.as_deref().filter(|e| {
            let l = e.to_ascii_lowercase();
            !l.ends_with("\\applicationframehost.exe") && !l.contains("\\windowsapps\\")
        })
    }
    /// What to pin for this window: its program, or its installed app.
    pub fn pin_target(&self) -> Option<String> {
        self.own_exe().map(str::to_string).or_else(|| self.app.clone())
    }
}

fn h(hwnd: isize) -> HWND { HWND(hwnd as *mut c_void) }

/// Opens a pinned shortcut or program the way Explorer would (no shell, no arguments).
pub fn launch(path: &str) -> bool {
    if !dock::pinnable(path) || (!dock::is_app_id(path) && !Path::new(path).is_file()) { return false }
    let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(path), None, None, SW_SHOWNORMAL) };
    r.0 as isize > 32
}

fn exe_of(pid: u32) -> Option<String> {
    unsafe {
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(p);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)); }
    pid
}

/// The app id Windows gave a window (the taskbar groups windows by it).
fn window_app_id(hwnd: HWND) -> Option<String> {
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let v = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let p = PropVariantToStringAlloc(&v).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s.filter(|s| !s.trim().is_empty())
    }
}

/// The app id of a Store app's process.
fn process_app_id(pid: u32) -> Option<String> {
    use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    unsafe {
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let r = GetApplicationUserModelId(p, &mut len, Some(PWSTR(buf.as_mut_ptr())));
        let _ = CloseHandle(p);
        (r.0 == 0 && len > 1).then(|| String::from_utf16_lossy(&buf[..(len as usize - 1).min(buf.len())]))
    }
}

/// The program and installed app behind a window. For a Store app's frame, the app itself runs
/// in a child window of its own process.
fn identify(hwnd: HWND) -> (Option<String>, Option<String>) {
    let mut exe = exe_of(pid_of(hwnd));
    let mut id = window_app_id(hwnd);
    let host = exe.as_deref().map(|e| e.to_ascii_lowercase().ends_with("\\applicationframehost.exe")).unwrap_or(false);
    if host {
        if let Ok(core) = unsafe { FindWindowExW(Some(hwnd), None, w!("Windows.UI.Core.CoreWindow"), None) } {
            let pid = pid_of(core);
            if let Some(e) = exe_of(pid) { exe = Some(e); }
            if id.is_none() { id = process_app_id(pid); }
        }
    } else if id.is_none() && exe.as_deref().map(|e| e.to_ascii_lowercase().contains("\\windowsapps\\")).unwrap_or(false) {
        id = process_app_id(pid_of(hwnd));
    }
    let app = id.map(|i| format!("{}{i}", dock::APP_PREFIX)).filter(|a| dock::is_app_id(a));
    (exe, app)
}

/// The icon a window shows in its own title bar (for programs Windows has no file icon for).
pub fn window_icon(hwnd: isize) -> Option<(usize, usize, Vec<u8>)> {
    unsafe {
        let mut icon = 0usize;
        for kind in [1usize, 2, 0] {   // ICON_BIG, ICON_SMALL2, ICON_SMALL
            let mut r = 0usize;
            let ok = SendMessageTimeoutW(h(hwnd), WM_GETICON, WPARAM(kind), LPARAM(0), SMTO_ABORTIFHUNG | SMTO_BLOCK, 60, Some(&mut r));
            if ok.0 != 0 && r != 0 { icon = r; break }
        }
        if icon == 0 { icon = GetClassLongPtrW(h(hwnd), GCLP_HICON); }
        if icon == 0 { icon = GetClassLongPtrW(h(hwnd), GCLP_HICONSM); }
        // The window owns this icon; it is only read, never destroyed here.
        if icon == 0 { None } else { icon_pixels(HICON(icon as *mut c_void)) }
    }
}

fn is_app_window(hwnd: HWND, own_pid: u32) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() { return false }
        if GetWindow(hwnd, GW_OWNER).map(|o| !o.0.is_null()).unwrap_or(false) { return false }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 { return false }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4).is_ok() && cloaked != 0 { return false }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid != own_pid && GetWindowTextLengthW(hwnd) > 0
    }
}

unsafe extern "system" fn collect(hwnd: HWND, lp: LPARAM) -> BOOL {
    let list = unsafe { &mut *(lp.0 as *mut Vec<isize>) };
    list.push(hwnd.0 as isize);
    BOOL(1)
}

/// Open app windows in taskbar order, without the bar's own windows. `exe` is filled from `known` when the
/// window was seen before, so a process is opened only once per window.
pub fn windows(known: &[Win]) -> Vec<Win> {
    let mut all: Vec<isize> = Vec::new();
    unsafe { let _ = EnumWindows(Some(collect), LPARAM(&mut all as *mut Vec<isize> as isize)); }
    let own = unsafe { GetCurrentProcessId() };
    let mut out = Vec::new();
    for raw in all {
        let hwnd = h(raw);
        if !is_app_window(hwnd, own) { continue }
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) }.max(0) as usize;
        let title = String::from_utf16_lossy(&buf[..n]);
        let (exe, app) = match known.iter().find(|k| k.hwnd == raw) { Some(k) => (k.exe.clone(), k.app.clone()), None => identify(hwnd) };
        out.push(Win { hwnd: raw, title, exe, app });
    }
    out
}

pub fn foreground() -> isize { unsafe { GetForegroundWindow().0 as isize } }
pub fn is_minimized(hwnd: isize) -> bool { unsafe { IsIconic(h(hwnd)).as_bool() } }

/// Brings a window to the front, restoring it if it was minimised.
pub fn focus(hwnd: isize) {
    unsafe {
        if IsIconic(h(hwnd)).as_bool() { let _ = ShowWindow(h(hwnd), SW_RESTORE); }
        let _ = SetForegroundWindow(h(hwnd));
    }
}
pub fn minimize(hwnd: isize) { unsafe { let _ = ShowWindow(h(hwnd), SW_MINIMIZE); } }
/// Asks the window to close, like its X button (the app may still ask to save).
pub fn close(hwnd: isize) { unsafe { let _ = PostMessageW(Some(h(hwnd)), WM_CLOSE, WPARAM(0), LPARAM(0)); } }

/// Every installed app, as Windows' Applications folder lists them (Store apps too), with its
/// name and launch id (`shell:AppsFolder\\…`), sorted by name. Call on a thread with COM.
pub fn all_apps() -> Vec<(String, String)> {
    use windows::Win32::UI::Shell::{IEnumShellItems, IShellItem, SHGetKnownFolderItem, BHID_EnumItems, FOLDERID_AppsFolder, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING};
    let mut out: Vec<(String, String)> = Vec::new();
    unsafe {
        let Ok(folder) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) else { return out };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return out };
        loop {
            let mut one = [None];
            let mut n = 0u32;
            if items.Next(&mut one, Some(&mut n)).is_err() || n == 0 { break }
            let Some(item) = one[0].take() else { break };
            let text = |k| item.GetDisplayName(k).ok().map(|p| {
                let s = p.to_string().unwrap_or_default();
                windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
                s
            });
            let (Some(name), Some(id)) = (text(SIGDN_NORMALDISPLAY), text(SIGDN_PARENTRELATIVEPARSING)) else { continue };
            let target = format!("{}{id}", dock::APP_PREFIX);
            if name.trim().is_empty() || dock::is_clutter(&name) || ronin_core::launcher::is_junk(&name, &id) || !dock::is_app_id(&target) { continue }
            if !out.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) { out.push((name, target)); }
            if out.len() > 2000 { break }
        }
    }
    // Programs installed without a Start-menu entry, from Windows' own list of installed software.
    let known: Vec<String> = out.iter().map(|(n, _)| ronin_core::launcher::simple_name(n)).collect();
    let ids: Vec<String> = out.iter().map(|(_, id)| id.to_ascii_lowercase()).collect();
    for (name, exe) in installed_programs() {
        let simple = ronin_core::launcher::simple_name(&name);
        let file = exe.rsplit('\\').next().unwrap_or(&exe).to_ascii_lowercase();
        if simple.is_empty() || known.contains(&simple) || ids.iter().any(|i| i.ends_with(&format!("\\{file}"))) { continue }
        if out.iter().any(|(_, p)| p.eq_ignore_ascii_case(&exe)) { continue }
        out.push((name, exe));
    }
    out.sort_by_key(|(n, _)| n.to_lowercase());
    out
}

/// Installed software as Windows lists it in Settings → Apps (your account's and the PC's),
/// each with its program file. Only entries that name a program that really exists are used;
/// uninstallers, setup tools, updates and parts of Windows are left out.
fn installed_programs() -> Vec<(String, String)> {
    use winreg::{enums::*, RegKey};
    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    const KEY32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut out: Vec<(String, String)> = Vec::new();
    for (root, path) in [(HKEY_CURRENT_USER, KEY), (HKEY_LOCAL_MACHINE, KEY), (HKEY_LOCAL_MACHINE, KEY32)] {
        let Ok(k) = RegKey::predef(root).open_subkey(path) else { continue };
        for sub in k.enum_keys().flatten().take(2000) {
            let Ok(e) = k.open_subkey(&sub) else { continue };
            let name: String = e.get_value("DisplayName").unwrap_or_default();
            let name = name.trim().to_string();
            if name.is_empty() || name.len() > 120 { continue }
            // Parts of Windows, updates and add-ons are not apps of their own.
            if e.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 { continue }
            if e.get_value::<String, _>("ParentKeyName").is_ok() || e.get_value::<String, _>("ReleaseType").is_ok() { continue }
            let icon: String = e.get_value("DisplayIcon").unwrap_or_default();
            // "C:\x\app.exe,0" or "\"C:\x\app.exe\"" → C:\x\app.exe
            let exe = icon.trim().trim_matches('"').rsplit_once(',').map(|(p, i)| if i.trim().parse::<i32>().is_ok() { p } else { icon.trim() }).unwrap_or(icon.trim()).trim_matches('"').trim().to_string();
            let file = exe.rsplit('\\').next().unwrap_or("").to_ascii_lowercase();
            if !file.ends_with(".exe") || ["unins", "uninst", "setup", "install", "update", "helper", "crash", "elevat"].iter().any(|w| file.contains(w)) { continue }
            if !dock::pinnable(&exe) || !Path::new(&exe).is_file() { continue }
            if dock::is_clutter(&name) || ronin_core::launcher::is_junk(&name, &exe) { continue }
            if !out.iter().any(|(n, p)| n.eq_ignore_ascii_case(&name) || p.eq_ignore_ascii_case(&exe)) { out.push((name, exe)); }
        }
    }
    out
}

/// Shows a program in File Explorer, selected.
pub fn show_in_folder(path: &str) {
    if !dock::pinnable(path) || dock::is_app_id(path) || !Path::new(path).exists() { return }
    let args = HSTRING::from(format!("/select,\"{path}\""));
    unsafe { let _ = ShellExecuteW(None, w!("open"), w!("explorer.exe"), &args, None, SW_SHOWNORMAL); }
}

/// An installed app's icon (from Windows' Applications folder), as premultiplied RGBA.
fn app_icon(id: &str) -> Option<(usize, usize, Vec<u8>)> {
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};
    unsafe {
        let f: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(id), None).ok()?;
        let bmp = f.GetImage(windows::Win32::Foundation::SIZE { cx: 48, cy: 48 }, SIIGBF_ICONONLY).ok()?;
        let px = bitmap_pixels(bmp);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        px
    }
}

/// The pixels of a 32-bit bitmap, top row first, as RGBA.
unsafe fn bitmap_pixels(bmp: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<(usize, usize, Vec<u8>)> {
    let mut bm = BITMAP::default();
    if unsafe { GetObjectW(HGDIOBJ(bmp.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut c_void)) } == 0 { return None }
    let (wd, ht) = (bm.bmWidth.clamp(1, 256) as usize, bm.bmHeight.clamp(1, 256) as usize);
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: wd as i32, biHeight: -(ht as i32),
            biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; wd * ht * 4];
    let dc = unsafe { CreateCompatibleDC(None) };
    let lines = unsafe { GetDIBits(dc, bmp, 0, ht as u32, Some(px.as_mut_ptr() as *mut c_void), &mut bmi, DIB_RGB_COLORS) };
    unsafe { let _ = DeleteDC(dc); }
    if lines == 0 { return None }
    let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
    for p in px.chunks_exact_mut(4) { p.swap(0, 2); if !has_alpha { p[3] = 255; } }
    Some((wd, ht, px))
}

/// The icon Explorer shows for a file or installed app, as RGBA pixels.
pub fn icon_rgba(path: &str) -> Option<(usize, usize, Vec<u8>)> {
    if dock::is_app_id(path) { return app_icon(path) }
    unsafe {
        let mut info = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(&HSTRING::from(path), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON);
        if ok == 0 || info.hIcon.is_invalid() { return None }
        let px = icon_pixels(info.hIcon);
        let _ = DestroyIcon(info.hIcon);
        px
    }
}

unsafe fn icon_pixels(icon: HICON) -> Option<(usize, usize, Vec<u8>)> {
    let mut ii = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut ii).ok()? };
    let result = (|| {
        if ii.hbmColor.is_invalid() { return None }
        let mut bm = BITMAP::default();
        if unsafe { GetObjectW(HGDIOBJ(ii.hbmColor.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut c_void)) } == 0 { return None }
        let (wd, ht) = (bm.bmWidth.clamp(1, 256) as usize, bm.bmHeight.clamp(1, 256) as usize);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: wd as i32, biHeight: -(ht as i32),
                biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; wd * ht * 4];
        let dc = unsafe { CreateCompatibleDC(None) };
        let lines = unsafe { GetDIBits(dc, ii.hbmColor, 0, ht as u32, Some(px.as_mut_ptr() as *mut c_void), &mut bmi, DIB_RGB_COLORS) };
        unsafe { let _ = DeleteDC(dc); }
        if lines == 0 { return None }
        let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2); // BGRA → RGBA
            if !has_alpha { p[3] = 255; }
        }
        Some((wd, ht, px))
    })();
    unsafe {
        if !ii.hbmColor.is_invalid() { let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0)); }
        if !ii.hbmMask.is_invalid() { let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0)); }
    }
    result
}
