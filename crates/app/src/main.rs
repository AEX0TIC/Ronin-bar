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

    // Right after an update or a restart the old copy is still closing: wait for it (up to 15 s).
    let updated = std::env::args().any(|a| a == "--updated");
    let restarts = restart_count();
    let mut alone = sys::single_instance();
    for _ in 0..50 {
        if alone || !(updated || restarts > 0) { break }
        std::thread::sleep(std::time::Duration::from_millis(300));
        alone = sys::single_instance();
    }
    if !alone { return }
    // Coming back from a crash right after the PC woke up: give the graphics driver a moment.
    if restarts > 0 { std::thread::sleep(std::time::Duration::from_secs(2)); }
    let started = std::time::Instant::now();
    update::clean_up();
    // Whatever happens, never leave the PC without a taskbar, and come back by itself.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        sys::show_taskbar(None); sys::night_light(0);
        default_hook(info);
        restart(started, restarts);
    }));

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
    // After sleep (or a driver update) Windows can reset the graphics card, and the bar's OpenGL
    // drawing stops working: eframe then gives up with an error, or panics while switching to a
    // popup's window. Neither can be recovered in place, so the bar starts itself again.
    if eframe::run_native(ui::TITLE, options, Box::new(move |cc| Ok(Box::new(ui::Bar::new(cc, store, settings))))).is_err() {
        sys::show_taskbar(None);
        restart(started, restarts);
    }
}

/// How many times in a row the bar has restarted itself (`--restarted N`; 0 for a normal start).
#[cfg(windows)]
fn restart_count() -> u32 {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == "--restarted").and_then(|i| args.get(i + 1)?.parse().ok()).unwrap_or(0)
}

/// Starts a fresh copy of the bar (which waits for this one to quit). A bar that keeps failing
/// within a minute of starting stops after a few tries rather than restarting forever.
#[cfg(windows)]
fn restart(started: std::time::Instant, restarts: u32) {
    let next = if started.elapsed() >= std::time::Duration::from_secs(60) { 1 } else { restarts + 1 };
    if next > 5 { return }
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--restarted").arg(next.to_string()).spawn();
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
