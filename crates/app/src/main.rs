//! Ronin Bar: a smart dock across the top of the screen: pinned apps, open windows, working
//! controls, anime wallpapers and five themes.
//!
//! Run it once and it installs itself into one folder (`%LOCALAPPDATA%\Programs\Ronin Bar`),
//! adds itself to Settings → Apps and the Start menu, and starts with Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)] mod bt;
#[cfg(windows)] mod dock;
#[cfg(windows)] mod download;
#[cfg(windows)] mod glyph;
#[cfg(windows)] mod install;
#[cfg(windows)] mod media;
#[cfg(windows)] mod net;
#[cfg(windows)] mod store;
#[cfg(windows)] mod sys;
#[cfg(windows)] mod ui;
#[cfg(windows)] mod update;
#[cfg(windows)] mod winrt;
#[cfg(windows)] mod worker;

#[cfg(not(windows))]
fn main() { eprintln!("Ronin Bar runs on Windows 10 and 11."); }

#[cfg(windows)]
fn main() {
    use std::sync::Arc;
    sys::dpi_aware();
    let store = Arc::new(store::Store { data: install::data_dir() });

    if std::env::args().any(|a| a == "--uninstall") {
        if !sys::ask("Uninstall Ronin Bar", "Remove Ronin Bar and its wallpapers, and put your old wallpaper and colours back?") { return }
        if let Some(bar) = sys::find_window(ui::TITLE) { sys::close_window(bar); std::thread::sleep(std::time::Duration::from_millis(1500)); }
        install::uninstall(&store.load_settings());
        sys::tell("Ronin Bar", "Ronin Bar has been removed.");
        install::remove_folder_after_exit();
        return;
    }

    if !install::running_installed() {
        let dir = install::install_dir();
        let q = format!("Install Ronin Bar for your Windows account?\n\nEverything goes into one folder:\n{}\n\nIt puts a smart dock at the top of the screen in place of the Windows taskbar (you can turn that off), and brings 80 anime wallpapers from wallhaven.cc. Uninstall any time from Settings → Apps.", dir.display());
        if !sys::ask("Install Ronin Bar", &q) { return }
        if let Some(bar) = sys::find_window(ui::TITLE) { sys::close_window(bar); std::thread::sleep(std::time::Duration::from_millis(1500)); }
        match install::install() {
            Ok(exe) => { let _ = std::process::Command::new(exe).spawn(); }
            Err(e) => sys::tell("Ronin Bar", &format!("Ronin Bar could not be installed: {e}")),
        }
        return;
    }

    // The program Windows starts is a small watchdog; the bar itself runs as its child (`--bar`).
    // If the bar ever ends unexpectedly (for example a graphics driver that drops it while the PC
    // sleeps), the watchdog starts it again within a second or two.
    if !std::env::args().any(|a| a == "--bar") { watch(&store); return }

    // Right after an update the old copy is still closing: wait for it (up to 15 s).
    let updated = std::env::args().any(|a| a == "--updated");
    let mut alone = sys::single_instance();
    for _ in 0..50 {
        if alone || !updated { break }
        std::thread::sleep(std::time::Duration::from_millis(300));
        alone = sys::single_instance();
    }
    if !alone { return }
    update::clean_up();
    // Whatever happens, never leave the PC without a taskbar.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| { sys::show_taskbar(None); sys::night_light(0); default_hook(info); }));

    let mut settings = store.load_settings();
    if settings.original.is_none() {
        // Remember how Windows looked before anything changes, so uninstall can put it back.
        settings.original = Some(sys::snapshot());
        sys::apply_theme(ronin_core::theme::by_id(&settings.theme));
        store.save_settings(&settings);
    }

    let (w, _, scale) = sys::primary_screen();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(ui::TITLE)
            .with_decorations(false).with_taskbar(false).with_always_on_top().with_resizable(false)
            .with_inner_size([w as f32 / scale, ui::BAR_HEIGHT]).with_position([0.0, 0.0])
            .with_icon(logo()),
        // The bar and its popups are separate windows drawn one after the other; waiting for the
        // screen's refresh after each would halve how fast they react.
        glow_options: eframe::egui_glow::GlowConfiguration { vsync: false, ..Default::default() },
        ..Default::default()
    };
    if eframe::run_native(ui::TITLE, options, Box::new(move |cc| Ok(Box::new(ui::Bar::new(cc, store, settings))))).is_err() {
        // The window or its drawing could not carry on: tell the watchdog to start the bar again.
        sys::show_taskbar(None);
        std::process::exit(3);
    }
}

/// Keeps the bar running. A bar that ends normally (Quit, Uninstall, an update, signing out)
/// exits with 0 and the watchdog ends too. Anything else, a crash included, starts it again after
/// a short pause that grows if it keeps failing; after 8 failures in 2 minutes it stops and leaves
/// Windows' own taskbar, so the PC is never left without one.
#[cfg(windows)]
fn watch(store: &store::Store) {
    use std::time::{Duration, Instant};
    // Right after an update the old watchdog is still closing: wait for it (up to 15 s).
    let updated = std::env::args().any(|a| a == "--updated");
    let mut alone = sys::single_watch();
    for _ in 0..50 {
        if alone || !updated { break }
        std::thread::sleep(Duration::from_millis(300));
        alone = sys::single_watch();
    }
    if !alone { return }
    let Ok(exe) = std::env::current_exe() else { return };
    let mut failures: Vec<Instant> = Vec::new();
    let mut first = true;
    loop {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--bar");
        if first && updated { cmd.arg("--updated"); }
        first = false;
        let Ok(mut child) = cmd.spawn() else { return };
        match child.wait() {
            Ok(status) if status.success() => return,
            _ => {}
        }
        // It ended unexpectedly: Windows' taskbar comes back until the bar is up again.
        sys::show_taskbar(store.load_settings().original.and_then(|o| o.taskbar_state));
        failures.retain(|t| t.elapsed() < Duration::from_secs(120));
        failures.push(Instant::now());
        if failures.len() > 8 { return }
        // 1 s, then 2, 4, 8… (at most 15 s): time for a screen that is still waking up.
        let pause = Duration::from_secs(1u64 << (failures.len() - 1).min(4)).min(Duration::from_secs(15));
        std::thread::sleep(pause);
    }
}

/// The Ronin logo for the window (the .exe itself gets it from build.rs).
#[cfg(windows)]
fn logo() -> eframe::egui::IconData {
    let img = image::load_from_memory_with_format(include_bytes!("../assets/ronin.png"), image::ImageFormat::Png)
        .map(|i| i.into_rgba8()).unwrap_or_else(|_| image::RgbaImage::new(1, 1));
    let (width, height) = img.dimensions();
    eframe::egui::IconData { rgba: img.into_raw(), width, height }
}
