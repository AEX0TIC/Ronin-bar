//! The dock across the top of the screen and its control panel.
use crate::{dock, download, install, store::Store, sys, worker};
use eframe::egui::{self, Color32, RichText, Vec2};
use ronin_core::settings::{Settings, AUTO_CHANGE_CHOICES};
use ronin_core::theme::{self, Theme, THEMES};
use ronin_core::wallhaven::{self, Wallpaper, COLLECTIONS, PER_COLLECTION};
use std::collections::HashMap;
use std::sync::{mpsc::Sender, Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;

pub const TITLE: &str = "Ronin Bar";
/// Bar height in logical pixels.
pub const BAR_HEIGHT: f32 = 34.0;
/// Dock icon size and slot width in logical pixels.
const ICON: f32 = 20.0;
const SLOT: f32 = 30.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab { Apps, Wallpapers, Themes, Sound, Power, Settings }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirm { Sleep, Restart, ShutDown, Uninstall }

pub struct Bar {
    store: Arc<Store>,
    settings: Settings,
    catalog: Arc<Mutex<Vec<Wallpaper>>>,
    progress: Arc<Mutex<download::Progress>>,
    stats: Arc<Mutex<worker::Stats>>,
    tx: Sender<worker::Cmd>,
    hwnd: Option<HWND>,
    topmost: bool,
    panel: bool,
    tab: Tab,
    confirm: Option<Confirm>,
    last_change: Instant,
    screen_w: f32,
    /// Open app windows, refreshed a few times a second.
    wins: Vec<dock::Win>,
    wins_at: Instant,
    /// The app window that was in front before the bar was clicked.
    active: isize,
    /// Icons by file path (`None` = Windows had none).
    icons: HashMap<String, Option<egui::TextureHandle>>,
    /// Start-menu apps, read when the Apps tab is first opened.
    apps: Option<Vec<(String, String)>>,
    query: String,
}

fn c(rgb: [u8; 3]) -> Color32 { Color32::from_rgb(rgb[0], rgb[1], rgb[2]) }

fn visuals(t: &Theme) -> egui::Visuals {
    let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = c(t.bg);
    v.window_fill = c(t.bg);
    v.extreme_bg_color = c(t.surface);
    v.override_text_color = Some(c(t.fg));
    v.selection.bg_fill = c(t.accent);
    v.hyperlink_color = c(t.accent);
    for w in [&mut v.widgets.inactive, &mut v.widgets.noninteractive] { w.bg_fill = c(t.surface); w.weak_bg_fill = c(t.surface); }
    v.widgets.hovered.weak_bg_fill = c(t.accent).gamma_multiply(0.35);
    v.widgets.active.weak_bg_fill = c(t.accent).gamma_multiply(0.55);
    v
}

impl Bar {
    pub fn new(cc: &eframe::CreationContext<'_>, store: Arc<Store>, settings: Settings) -> Bar {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let (stats, tx) = worker::start();
        let catalog = Arc::new(Mutex::new(store.load_catalog()));
        let progress = Arc::new(Mutex::new(download::Progress::default()));
        if catalog.lock().map(|l| l.len()).unwrap_or(0) < COLLECTIONS.len() * PER_COLLECTION {
            download::run(store.clone(), catalog.clone(), progress.clone());
        }
        let (w, _, scale) = sys::primary_screen();
        Bar { store, settings, catalog, progress, stats, tx, hwnd: None, topmost: true, panel: false, tab: Tab::Wallpapers,
              confirm: None, last_change: Instant::now(), screen_w: w as f32 / scale,
              wins: Vec::new(), wins_at: Instant::now() - Duration::from_secs(5), active: 0, icons: HashMap::new(), apps: None, query: String::new() }
    }

    fn theme(&self) -> &'static Theme { theme::by_id(&self.settings.theme) }
    fn save(&self) { self.store.save_settings(&self.settings); }

    fn set_wallpaper(&mut self, id: &str) {
        let w = self.catalog.lock().ok().and_then(|l| l.iter().find(|w| w.id == id).cloned());
        if let Some(w) = w {
            if sys::set_wallpaper(&self.store.wallpaper_file(&w)) {
                self.settings.wallpaper = w.id;
                self.last_change = Instant::now();
                self.save();
            }
        }
    }

    fn step_wallpaper(&mut self, step: isize) {
        let id = self.catalog.lock().ok().and_then(|l| wallhaven::next_id(&l, &self.settings.wallpaper, step).map(str::to_string));
        if let Some(id) = id { self.set_wallpaper(&id); }
    }

    fn random_wallpaper(&mut self) {
        let n = self.catalog.lock().map(|l| l.len()).unwrap_or(0);
        if n > 1 {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1);
            self.step_wallpaper(1 + (nanos as usize % (n - 1)) as isize);
        }
    }

    fn set_theme(&mut self, t: &'static Theme) {
        self.settings.theme = t.id.to_string();
        sys::apply_theme(t);
        self.save();
    }

    /// First frame: reserve the strip at the top of the screen for the bar.
    fn dock(&mut self, ctx: &egui::Context) {
        if self.hwnd.is_some() { return }
        if let Some(h) = sys::find_window(TITLE) {
            let px = (BAR_HEIGHT * ctx.pixels_per_point()).round() as i32;
            sys::appbar_register(h, px);
            self.hwnd = Some(h);
        }
    }

    fn icon(&mut self, ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
        if let Some(t) = self.icons.get(path) { return t.clone() }
        let tex = dock::icon_rgba(path).map(|(w, h, px)| {
            ctx.load_texture(path, egui::ColorImage::from_rgba_unmultiplied([w, h], &px), egui::TextureOptions::LINEAR)
        });
        if self.icons.len() > 400 { self.icons.clear(); }
        self.icons.insert(path.to_string(), tex.clone());
        tex
    }

    fn pin(&mut self, path: &str) {
        let mut p = self.settings.pinned.clone();
        p.push(path.to_string());
        self.settings.pinned = ronin_core::dock::clean_pins(p);
        self.save();
    }

    fn unpin(&mut self, i: usize) {
        if i < self.settings.pinned.len() { self.settings.pinned.remove(i); self.save(); }
    }

    fn tick(&mut self) {
        if self.wins_at.elapsed() >= Duration::from_millis(400) {
            self.wins = dock::windows(&self.wins);
            self.wins_at = Instant::now();
        }
        let fg = dock::foreground();
        if self.wins.iter().any(|w| w.hwnd == fg) { self.active = fg; }
        // Step back while a fullscreen app or game is in front.
        let full = self.stats.lock().map(|s| s.fullscreen).unwrap_or(false);
        if let Some(h) = self.hwnd { if full == self.topmost { self.topmost = !full; sys::set_topmost(h, self.topmost); } }
        // A wallpaper once the first ones have downloaded.
        if self.settings.wallpaper.is_empty() {
            let first = self.catalog.lock().ok().and_then(|l| l.first().map(|w| w.id.clone()));
            if let Some(id) = first { self.set_wallpaper(&id); }
        }
        let m = self.settings.auto_change_minutes;
        if m > 0 && self.last_change.elapsed() >= Duration::from_secs(u64::from(m) * 60) { self.step_wallpaper(1); }
    }

    /// One dock slot: the icon (or first letter), an accent line under it when `lit`.
    fn slot(&mut self, ui: &mut egui::Ui, path: Option<&str>, name: &str, lit: bool, dim: bool) -> egui::Response {
        let t = self.theme();
        let tex = path.and_then(|p| self.icon(ui.ctx(), p));
        let (rect, r) = ui.allocate_exact_size(Vec2::new(SLOT, BAR_HEIGHT), egui::Sense::click());
        if r.hovered() { ui.painter().rect_filled(rect.shrink2(Vec2::new(1.0, 3.0)), 6.0, c(t.accent).gamma_multiply(0.25)); }
        let icon = egui::Rect::from_center_size(rect.center(), Vec2::splat(ICON));
        let tint = if dim { Color32::from_white_alpha(120) } else { Color32::WHITE };
        match tex {
            Some(tex) => { egui::Image::new((tex.id(), Vec2::splat(ICON))).tint(tint).paint_at(ui, icon); }
            None => {
                let letter: String = name.chars().next().unwrap_or('?').to_uppercase().collect();
                ui.painter().rect_filled(icon, 5.0, c(t.surface));
                ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, letter, egui::FontId::proportional(13.0), c(t.fg));
            }
        }
        if lit {
            let y = rect.bottom() - 3.0;
            ui.painter().line_segment([egui::pos2(rect.center().x - 7.0, y), egui::pos2(rect.center().x + 7.0, y)], egui::Stroke::new(2.0, c(t.accent)));
        }
        r.on_hover_text(name)
    }

    fn dock_items(&mut self, ui: &mut egui::Ui, room: f32) {
        let t = self.theme();
        let mut slots = (room / SLOT).floor().max(0.0) as usize;
        // Pinned apps: click opens, right-click manages them.
        let pinned = self.settings.pinned.clone();
        for p in pinned.iter().take(slots) {
            let name = ronin_core::dock::display_name(p);
            let r = self.slot(ui, Some(p), &name, false, false);
            if r.clicked() && !dock::launch(p) { self.panel = true; self.tab = Tab::Apps; }
            if r.secondary_clicked() { self.panel = true; self.tab = Tab::Apps; }
        }
        slots = slots.saturating_sub(pinned.len());
        if pinned.is_empty() && ui.add(egui::Button::new(RichText::new("+ apps").size(12.0).color(c(t.muted))).frame(false)).clicked() {
            self.panel = true;
            self.tab = Tab::Apps;
        }
        if !self.wins.is_empty() {
            let (r, _) = ui.allocate_exact_size(Vec2::new(9.0, BAR_HEIGHT), egui::Sense::hover());
            ui.painter().line_segment([egui::pos2(r.center().x, r.top() + 9.0), egui::pos2(r.center().x, r.bottom() - 9.0)], egui::Stroke::new(1.0, c(t.muted)));
        }
        // Open windows: click focuses (or minimises the one in front), middle-click closes, right-click pins.
        let wins = self.wins.clone();
        // Keep one slot for "+N" when they don't all fit.
        let shown = if wins.len() <= slots { wins.len() } else { slots.saturating_sub(1) };
        for win in wins.iter().take(shown) {
            let front = win.hwnd == self.active && !dock::is_minimized(win.hwnd);
            let r = self.slot(ui, win.exe.as_deref(), &win.title, front, dock::is_minimized(win.hwnd));
            if r.clicked() {
                if front { dock::minimize(win.hwnd); self.active = 0; } else { dock::focus(win.hwnd); self.active = win.hwnd; }
            }
            if r.middle_clicked() { dock::close(win.hwnd); }
            if r.secondary_clicked() {
                if let Some(exe) = &win.exe { if !self.settings.pinned.iter().any(|p| p.eq_ignore_ascii_case(exe)) { let e = exe.clone(); self.pin(&e); } }
            }
        }
        if wins.len() > shown { ui.label(RichText::new(format!("+{}", wins.len() - shown)).size(12.0).color(c(t.muted))); }
    }

    fn bar(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let s = self.stats.lock().map(|s| s.clone()).unwrap_or_default();
        let muted = c(t.muted);
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.horizontal_centered(|ui| {
            ui.add_space(8.0);
            let menu = ui.add(egui::Button::new(RichText::new("ronin").strong().color(c(t.accent))).frame(false));
            if menu.clicked() { self.panel = !self.panel; }
            ui.add_space(6.0);
            // Leave room on the right for the clock and the controls.
            let room = ui.available_width() - 520.0;
            self.dock_items(ui, room);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if ui.add(egui::Button::new(RichText::new("power").size(12.0)).frame(false)).clicked() { self.panel = true; self.tab = Tab::Power; }
                ui.add_space(10.0);
                let (wd, d, mo, h, mi) = sys::local_time();
                const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
                ui.label(RichText::new(format!("{:02}:{:02}", h, mi)).size(13.0).strong());
                ui.label(RichText::new(format!("{} {} {}", DAYS[wd as usize % 7], d, MONTHS[(mo as usize + 11) % 12])).size(12.0).color(muted));
                ui.add_space(12.0);
                let wall = ui.add(egui::Button::new(RichText::new("wall").size(12.0)).frame(false)).on_hover_text("Next wallpaper (right-click: all wallpapers)");
                if wall.clicked() { self.step_wallpaper(1); }
                if wall.secondary_clicked() { self.panel = true; self.tab = Tab::Wallpapers; }
                ui.add_space(10.0);
                if let Some((v, m)) = s.volume {
                    let text = if m { "muted".to_string() } else { format!("{:.0}%", v * 100.0) };
                    let r = ui.add(egui::Button::new(RichText::new(text).size(13.0)).frame(false))
                        .on_hover_text("Click: mute / unmute · Scroll: volume");
                    if r.clicked() { let _ = self.tx.send(worker::Cmd::Mute(!m)); }
                    if r.hovered() {
                        // One wheel notch = 5 %.
                        let dy: f32 = ui.input(|i| i.raw.events.iter().filter_map(|e| match e {
                            egui::Event::MouseWheel { delta, .. } => Some(delta.y), _ => None }).sum());
                        if dy != 0.0 { let _ = self.tx.send(worker::Cmd::Volume(v + dy.signum() * 0.05)); }
                    }
                    ui.label(RichText::new("vol").color(muted).size(12.0));
                    ui.add_space(10.0);
                }
                if let Some((b, charging)) = s.battery {
                    ui.label(RichText::new(format!("{b}%{}", if charging { " +" } else { "" })).size(13.0));
                    ui.label(RichText::new("bat").color(muted).size(12.0));
                    ui.add_space(10.0);
                }
                ui.label(RichText::new(if s.online { "up" } else { "down" }).size(13.0).color(if s.online { c(t.fg) } else { c(t.accent) }));
                ui.label(RichText::new("net").color(muted).size(12.0));
                ui.add_space(10.0);
                // right-to-left: value first, then label
                ui.label(RichText::new(format!("{}%", s.ram)).size(13.0));
                ui.label(RichText::new("ram").color(muted).size(12.0));
                ui.add_space(10.0);
                ui.label(RichText::new(format!("{}%", s.cpu)).size(13.0));
                ui.label(RichText::new("cpu").color(muted).size(12.0));
            });
        });
    }

    fn tab_apps(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let ctx = ui.ctx().clone();
        ui.label(RichText::new("Pin apps to the dock. Click opens · right-click an open window pins its program · middle-click closes a window.").color(c(t.muted)).size(12.0));
        ui.add_space(4.0);
        ui.label(RichText::new(format!("Pinned · {}/{}", self.settings.pinned.len(), ronin_core::dock::MAX_PINNED)).strong());
        let pinned = self.settings.pinned.clone();
        let mut action: Option<(usize, i8)> = None; // (index, -1 left / 1 right / 0 unpin)
        for (i, p) in pinned.iter().enumerate() {
            ui.horizontal(|ui| {
                match self.icon(&ctx, p) { Some(tex) => { ui.add(egui::Image::new((tex.id(), Vec2::splat(18.0)))); } None => { ui.add_space(22.0); } }
                ui.label(ronin_core::dock::display_name(p)).on_hover_text(p);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("unpin").clicked() { action = Some((i, 0)); }
                    if ui.add_enabled(i + 1 < pinned.len(), egui::Button::new("→").small()).clicked() { action = Some((i, 1)); }
                    if ui.add_enabled(i > 0, egui::Button::new("←").small()).clicked() { action = Some((i, -1)); }
                });
            });
        }
        if pinned.is_empty() { ui.label(RichText::new("Nothing pinned yet.").color(c(t.muted))); }
        match action {
            Some((i, 0)) => self.unpin(i),
            Some((i, d)) => { let j = (i as isize + d as isize) as usize; self.settings.pinned.swap(i, j); self.save(); }
            None => {}
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Start menu").strong());
            ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("search").desired_width(200.0));
            if ui.small_button("refresh").clicked() { self.apps = None; }
        });
        if self.apps.is_none() { self.apps = Some(dock::start_menu_apps()); }
        let list: Vec<(String, String)> = self.apps.as_ref().map(|a| a.iter().filter(|(n, _)| ronin_core::dock::matches(n, &self.query)).cloned().collect()).unwrap_or_default();
        let full = self.settings.pinned.len() >= ronin_core::dock::MAX_PINNED;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 24.0, list.len(), |ui, range| {
            for (name, path) in &list[range] {
                ui.horizontal(|ui| {
                    match self.icon(&ctx, path) { Some(tex) => { ui.add(egui::Image::new((tex.id(), Vec2::splat(18.0)))); } None => { ui.add_space(22.0); } }
                    ui.label(name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let is_pinned = self.settings.pinned.iter().any(|p| p.eq_ignore_ascii_case(path));
                        if is_pinned {
                            ui.label(RichText::new("pinned").color(c(t.muted)).size(12.0));
                        } else if ui.add_enabled(!full, egui::Button::new("pin").small()).clicked() {
                            let p = path.clone();
                            self.pin(&p);
                        }
                        if ui.small_button("open").clicked() { dock::launch(path); }
                    });
                });
            }
        });
        if list.is_empty() { ui.label(RichText::new("No Start-menu apps match.").color(c(t.muted))); }
    }

    fn panel(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        ui.horizontal(|ui| {
            for (label, tab) in [("Apps", Tab::Apps), ("Wallpapers", Tab::Wallpapers), ("Themes", Tab::Themes), ("Sound", Tab::Sound), ("Power", Tab::Power), ("Settings", Tab::Settings)] {
                if ui.selectable_label(self.tab == tab, label).clicked() { self.tab = tab; self.confirm = None; }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("close").clicked() { self.panel = false; }
            });
        });
        ui.separator();
        match self.tab {
            Tab::Apps => self.tab_apps(ui),
            Tab::Wallpapers => self.tab_wallpapers(ui),
            Tab::Themes => {
                ui.label(RichText::new("Colours the bar and sets Windows' dark/light mode and accent colour.").color(c(t.muted)));
                ui.add_space(6.0);
                for th in THEMES.iter() {
                    let selected = th.id == t.id;
                    let frame = egui::Frame::new().fill(c(th.bg)).corner_radius(8.0).inner_margin(10.0)
                        .stroke(egui::Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { c(th.accent) } else { c(th.surface) }));
                    let r = frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(th.name).color(c(th.fg)).strong());
                            ui.label(RichText::new(if th.dark { "dark" } else { "light" }).color(c(th.muted)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                for col in [th.accent, th.muted, th.surface] {
                                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                                    ui.painter().circle_filled(rect.center(), 7.0, c(col));
                                }
                            });
                        });
                    }).response.interact(egui::Sense::click());
                    if r.clicked() { self.set_theme(th); }
                    ui.add_space(4.0);
                }
            }
            Tab::Sound => {
                let s = self.stats.lock().map(|s| s.clone()).unwrap_or_default();
                match s.volume {
                    None => { ui.label("No sound device found."); }
                    Some((v, m)) => {
                        let mut pct = (v * 100.0).round();
                        ui.horizontal(|ui| {
                            ui.label("Volume");
                            if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%")).changed() { let _ = self.tx.send(worker::Cmd::Volume(pct / 100.0)); }
                        });
                        let mut mute = m;
                        if ui.checkbox(&mut mute, "Mute").changed() { let _ = self.tx.send(worker::Cmd::Mute(mute)); }
                    }
                }
            }
            Tab::Power => {
                ui.label(RichText::new("Unsaved work in other apps is lost on restart and shut down.").color(c(t.muted)));
                ui.add_space(6.0);
                if ui.button("Lock").clicked() { let _ = self.tx.send(worker::Cmd::Lock); }
                for (label, what) in [("Sleep", Confirm::Sleep), ("Restart", Confirm::Restart), ("Shut down", Confirm::ShutDown)] {
                    if ui.button(label).clicked() { self.confirm = Some(what); }
                }
                self.confirm_row(ui);
            }
            Tab::Settings => {
                let mut on = self.settings.start_with_windows;
                if ui.checkbox(&mut on, "Start with Windows").changed() {
                    self.settings.start_with_windows = on;
                    install::set_start_with_windows(on);
                    self.save();
                }
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Installed in {}", install::install_dir().display())).color(c(t.muted)).size(12.0));
                ui.label(RichText::new(format!("Ronin Bar {}", env!("CARGO_PKG_VERSION"))).color(c(t.muted)).size(12.0));
                ui.add_space(12.0);
                if ui.button("Quit the bar").clicked() { self.quit(ui.ctx()); }
                if ui.button("Uninstall Ronin Bar…").clicked() { self.confirm = Some(Confirm::Uninstall); }
                self.confirm_row(ui);
            }
        }
    }

    fn confirm_row(&mut self, ui: &mut egui::Ui) {
        let Some(what) = self.confirm else { return };
        let q = match what {
            Confirm::Sleep => "Put the PC to sleep now?",
            Confirm::Restart => "Restart the PC now?",
            Confirm::ShutDown => "Shut down the PC now?",
            Confirm::Uninstall => "Remove Ronin Bar and its wallpapers, and put your old wallpaper and colours back?",
        };
        ui.add_space(8.0);
        ui.label(RichText::new(q).strong());
        ui.horizontal(|ui| {
            if ui.button("Yes").clicked() {
                self.confirm = None;
                match what {
                    Confirm::Sleep => { let _ = self.tx.send(worker::Cmd::Sleep); }
                    Confirm::Restart => { let _ = self.tx.send(worker::Cmd::Restart); }
                    Confirm::ShutDown => { let _ = self.tx.send(worker::Cmd::ShutDown); }
                    Confirm::Uninstall => { install::uninstall(&self.settings); install::remove_folder_after_exit(); self.quit(ui.ctx()); }
                }
            }
            if ui.button("No").clicked() { self.confirm = None; }
        });
    }

    fn tab_wallpapers(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let p = self.progress.lock().map(|p| p.clone()).unwrap_or_default();
        let list = self.catalog.lock().map(|l| l.clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            if ui.button("previous").clicked() { self.step_wallpaper(-1); }
            if ui.button("next").clicked() { self.step_wallpaper(1); }
            if ui.button("random").clicked() { self.random_wallpaper(); }
            ui.label("change every");
            let label = |m: u32| if m == 0 { "never".to_string() } else if m < 60 { format!("{m} min") } else { format!("{} h", m / 60) };
            egui::ComboBox::from_id_salt("auto").selected_text(label(self.settings.auto_change_minutes)).show_ui(ui, |ui| {
                for m in AUTO_CHANGE_CHOICES {
                    if ui.selectable_value(&mut self.settings.auto_change_minutes, m, label(m)).changed() { self.last_change = Instant::now(); self.save(); }
                }
            });
        });
        if p.running {
            ui.label(RichText::new(format!("Downloading wallpapers from wallhaven.cc… {}/{}", p.done, p.total)).color(c(t.muted)));
        } else if let Some(e) = &p.error {
            ui.label(RichText::new(format!("Some downloads failed: {e}")).color(c(t.accent)).size(12.0));
        }
        if !p.running && list.len() < COLLECTIONS.len() * PER_COLLECTION && ui.button("Download missing wallpapers").clicked() {
            download::run(self.store.clone(), self.catalog.clone(), self.progress.clone());
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for col in COLLECTIONS {
                let items: Vec<&Wallpaper> = list.iter().filter(|w| w.collection == col.name).collect();
                ui.label(RichText::new(format!("{} · {}", col.name, items.len())).strong());
                ui.horizontal_wrapped(|ui| {
                    for w in items {
                        let uri = format!("file://{}", self.store.thumb_file(w).display());
                        let current = w.id == self.settings.wallpaper;
                        let img = egui::Image::new(uri).fit_to_exact_size(Vec2::new(122.0, 69.0)).corner_radius(6.0);
                        let r = ui.add(egui::Button::image(img).selected(current))
                            .on_hover_text(format!("{} · {}\n{}", w.collection, w.resolution, w.page));
                        if r.clicked() { let id = w.id.clone(); self.set_wallpaper(&id); }
                    }
                });
                ui.add_space(6.0);
            }
            ui.label(RichText::new("Wallpapers come from wallhaven.cc and belong to their artists; they are downloaded to this PC for personal use.").color(c(t.muted)).size(11.0));
        });
    }

    fn quit(&mut self, ctx: &egui::Context) {
        if let Some(h) = self.hwnd.take() { sys::appbar_remove(h); }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for Bar {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.set_visuals(visuals(self.theme()));
        ctx.request_repaint_after(Duration::from_millis(500));
        self.dock(&ctx);
        self.tick();
        egui::Frame::new().fill(c(self.theme().bg)).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            self.bar(ui);
        });
        if self.panel {
            let (w, h) = (440.0, 560.0);
            let pos = egui::pos2((self.screen_w - w - 8.0).max(0.0), BAR_HEIGHT + 6.0);
            let builder = egui::ViewportBuilder::default()
                .with_title("Ronin Bar")
                .with_decorations(false).with_taskbar(false).with_always_on_top().with_resizable(false)
                .with_inner_size([w, h]).with_position(pos);
            let close = ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("ronin-panel"), builder, |ui, _class| {
                egui::Frame::new().fill(c(self.theme().bg)).inner_margin(12.0).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.panel(ui);
                });
                ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            });
            if close { self.panel = false; }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(h) = self.hwnd.take() { sys::appbar_remove(h); }
    }
}
