//! Ronin Bar: a smart dock across the top of the screen: pinned apps, open windows, working
//! controls, anime wallpapers and five themes.
//!
//! Run it once and it installs itself into one folder (`%LOCALAPPDATA%\Programs\Ronin Bar`),
//! adds itself to Settings → Apps and the Start menu, and starts with Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)] mod dock;
#[cfg(windows)] mod download;
#[cfg(windows)] mod glyph;
#[cfg(windows)] mod install;
#[cfg(windows)] mod net;
#[cfg(windows)] mod store;
#[cfg(windows)] mod sys;
#[cfg(windows)] mod ui;
#[cfg(windows)] mod worker;

#[cfg(not(windows))]
fn main() { eprintln!("Ronin Bar runs on Windows 10 and 11."); }

#[cfg(windows)]
fn main() {
    use std::sync::Arc;
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

    if !sys::single_instance() { return }
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
    let _ = eframe::run_native(ui::TITLE, options, Box::new(move |cc| Ok(Box::new(ui::Bar::new(cc, store, settings)))));
}

/// The Ronin logo for the window (the .exe itself gets it from build.rs).
#[cfg(windows)]
fn logo() -> eframe::egui::IconData {
    let img = image::load_from_memory_with_format(include_bytes!("../assets/ronin.png"), image::ImageFormat::Png)
        .map(|i| i.into_rgba8()).unwrap_or_else(|_| image::RgbaImage::new(1, 1));
    let (width, height) = img.dimensions();
    eframe::egui::IconData { rgba: img.into_raw(), width, height }
}
