//! Install and uninstall. Everything lives in one folder:
//!
//! ```text
//! %LOCALAPPDATA%\Programs\Ronin Bar\
//!   ronin-bar.exe
//!   data\settings.json, data\catalog.json, data\wallpapers\, data\thumbs\
//! ```
//! Outside it, only: the Settings → Apps entry, a Start-menu shortcut and (if on) the
//! start-with-Windows entry. Uninstall removes all of them and puts the wallpaper and colours back.
use crate::sys;
use ronin_core::settings::Settings;
use std::path::{Path, PathBuf};
use winreg::{enums::*, RegKey};

pub const APP_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\RoninBar";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "Ronin Bar";
const EXE: &str = "ronin-bar.exe";

pub fn install_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"));
    base.join("Programs").join("Ronin Bar")
}
pub fn installed_exe() -> PathBuf { install_dir().join(EXE) }
pub fn data_dir() -> PathBuf { install_dir().join("data") }

fn same(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) { (Ok(x), Ok(y)) => x == y, _ => false }
}
/// True when this program is the installed copy.
pub fn running_installed() -> bool {
    std::env::current_exe().map(|e| same(&e, &installed_exe())).unwrap_or(false)
}

fn start_menu_shortcut() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join(r"Microsoft\Windows\Start Menu\Programs\Ronin Bar.lnk"))
}

/// Copies this program into the install folder and registers it. Returns the installed exe.
pub fn install() -> std::io::Result<PathBuf> {
    let dir = install_dir();
    std::fs::create_dir_all(dir.join("data"))?;
    let target = installed_exe();
    std::fs::copy(std::env::current_exe()?, &target)?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (k, _) = hkcu.create_subkey(APP_KEY)?;
    let exe = target.display().to_string();
    k.set_value("DisplayName", &"Ronin Bar")?;
    k.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))?;
    k.set_value("Publisher", &"AEX0TIC")?;
    k.set_value("DisplayIcon", &exe)?;
    k.set_value("InstallLocation", &dir.display().to_string())?;
    k.set_value("UninstallString", &format!("\"{exe}\" --uninstall"))?;
    k.set_value("URLInfoAbout", &"https://github.com/AEX0TIC/Ronin-bar")?;
    k.set_value("NoModify", &1u32)?;
    k.set_value("NoRepair", &1u32)?;
    if let Some(lnk) = start_menu_shortcut() { let _ = make_shortcut(&lnk, &target); }
    set_start_with_windows(true);
    Ok(target)
}

/// Keeps the version shown in Settings → Apps current after an update.
pub fn register_version() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(k) = hkcu.open_subkey_with_flags(APP_KEY, KEY_SET_VALUE) { let _ = k.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION")); }
}

pub fn set_start_with_windows(on: bool) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((k, _)) = hkcu.create_subkey(RUN_KEY) {
        if on { let _ = k.set_value(RUN_NAME, &format!("\"{}\"", installed_exe().display())); } else { let _ = k.delete_value(RUN_NAME); }
    }
}

/// Removes everything Ronin Bar added and restores the wallpaper and colours.
/// The folder itself goes with `remove_folder_after_exit`.
pub fn uninstall(settings: &Settings) {
    if let Some(o) = &settings.original { sys::restore(o); }
    set_start_with_windows(false);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(APP_KEY);
    if let Some(lnk) = start_menu_shortcut() { let _ = std::fs::remove_file(lnk); }
}

/// Deletes the install folder 2 seconds after this program exits (a running program's own file is
/// locked). Call it last, right before exiting.
pub fn remove_folder_after_exit() {
    let dir = install_dir();
    // Only ever the install folder itself, quoted, after a 2-second pause; cmd.exe is part of Windows.
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd.exe")
        .arg("/c")
        .raw_arg(format!("timeout /t 2 /nobreak >nul & rmdir /s /q \"{}\"", dir.display()))
        .creation_flags(0x0800_0000)   // CREATE_NO_WINDOW
        .spawn();
}

fn make_shortcut(lnk: &Path, target: &Path) -> windows::core::Result<()> {
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    sys::com_init();
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(target.as_os_str()))?;
        link.SetDescription(&HSTRING::from("Ronin Bar: top bar, anime wallpapers and themes"))?;
        link.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
        link.cast::<IPersistFile>()?.Save(&HSTRING::from(lnk.as_os_str()), true)?;
    }
    Ok(())
}
