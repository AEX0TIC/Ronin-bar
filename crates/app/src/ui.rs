//! The dock across the top of the screen and its control panel.
use crate::glyph::{self, Glyph};
use crate::{dock, download, install, store::Store, sys, worker};
use eframe::egui::{self, pos2, vec2, Color32, FontId, Rect, RichText, Sense, Stroke, Vec2};
use ronin_core::settings::{Settings, AUTO_CHANGE_CHOICES};
use ronin_core::theme::{self, Theme, THEMES};
use ronin_core::wallhaven::{self, Wallpaper, COLLECTIONS, PER_COLLECTION};
use ronin_core::{art, dock as pins, github};
use std::collections::HashMap;
use std::sync::{mpsc::Sender, Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;

pub const TITLE: &str = "Ronin Bar";
const PANEL_TITLE: &str = "Ronin Bar · control";
const POPUP_TITLE: &str = "Ronin Bar · popup";
/// Bar height in logical pixels.
pub const BAR_HEIGHT: f32 = 36.0;
/// Dock icon size and slot width in logical pixels.
const ICON: f32 = 20.0;
const SLOT: f32 = 34.0;
const CLOCK_W: f32 = 170.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab { Apps, Wallpapers, Themes, Sound, Power, Settings }

/// Ronin's own popups, opened from the bar in place of Windows' flyouts.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Popup { Calendar, Sound, Power }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirm { Sleep, SignOut, Restart, ShutDown, Uninstall }

pub struct Bar {
    store: Arc<Store>,
    settings: Settings,
    catalog: Arc<Mutex<Vec<Wallpaper>>>,
    originals: Arc<Mutex<Vec<Wallpaper>>>,
    /// Downloaded pictures from the GitHub packs, and how each pack's download is going.
    packs: Arc<Mutex<Vec<Wallpaper>>>,
    pack_states: download::PackStates,
    progress: Arc<Mutex<download::Progress>>,
    /// The wallpaper whose full picture is downloading, and where the download reports back.
    fetching: Option<String>,
    fetched: download::Fetched,
    fetch_error: Option<String>,
    stats: Arc<Mutex<worker::Stats>>,
    tx: Sender<worker::Cmd>,
    hwnd: Option<HWND>,
    bar_px: i32,
    topmost: bool,
    panel: bool,
    /// The open popup, the bar position it hangs from, when it opened, and the last one closed.
    popup: Option<Popup>,
    popup_x: f32,
    popup_at: Instant,
    popup_closed: Option<(Popup, Instant)>,
    /// The month shown in the calendar.
    cal: (i32, u32),
    tab: Tab,
    confirm: Option<Confirm>,
    last_change: Instant,
    screen_w: f32,
    /// Open app windows, refreshed a few times a second.
    wins: Vec<dock::Win>,
    wins_at: Instant,
    /// Taskbar and work-area check, about once a second.
    guard_at: Instant,
    /// The app window that was in front before the bar was clicked.
    active: isize,
    /// Maximised windows re-fitted below the bar, and when.
    refitted: HashMap<isize, Instant>,
    /// Icons by file path (`None` = Windows had none).
    icons: HashMap<String, Option<egui::TextureHandle>>,
    /// Wallpaper previews by wallpaper id (`None` = the file could not be read).
    thumbs: HashMap<String, Option<egui::TextureHandle>>,
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
    v.faint_bg_color = c(t.surface);
    v.override_text_color = Some(c(t.fg));
    v.selection.bg_fill = c(t.accent).gamma_multiply(0.8);
    v.selection.stroke = Stroke::new(1.0, c(t.fg));
    v.hyperlink_color = c(t.accent);
    v.window_stroke = Stroke::new(1.0, c(t.surface));
    for w in [&mut v.widgets.inactive, &mut v.widgets.noninteractive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = 8.into();
        w.bg_stroke = Stroke::NONE;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c(t.surface));
    for w in [&mut v.widgets.inactive, &mut v.widgets.noninteractive] { w.bg_fill = c(t.surface); w.weak_bg_fill = c(t.surface); }
    v.widgets.hovered.weak_bg_fill = c(t.accent).gamma_multiply(0.30);
    v.widgets.hovered.bg_fill = c(t.accent).gamma_multiply(0.30);
    v.widgets.active.weak_bg_fill = c(t.accent).gamma_multiply(0.55);
    v.widgets.active.bg_fill = c(t.accent).gamma_multiply(0.55);
    v
}

impl Bar {
    pub fn new(cc: &eframe::CreationContext<'_>, store: Arc<Store>, settings: Settings) -> Bar {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        cc.egui_ctx.all_styles_mut(|s| { s.spacing.button_padding = vec2(10.0, 5.0); s.spacing.item_spacing = vec2(8.0, 6.0); });
        let (stats, tx) = worker::start();
        let catalog = Arc::new(Mutex::new(store.load_catalog()));
        let originals = Arc::new(Mutex::new(Vec::new()));
        let packs = Arc::new(Mutex::new(store.load_packs()));
        let progress = Arc::new(Mutex::new(download::Progress::default()));
        let (w, h, scale) = sys::primary_screen();
        download::originals(store.clone(), (w.max(16) as u32, h.max(16) as u32), settings.theme.clone(), originals.clone());
        if catalog.lock().map(|l| l.len()).unwrap_or(0) < COLLECTIONS.len() * PER_COLLECTION {
            download::run(store.clone(), catalog.clone(), progress.clone(), settings.wallpaper.clone());
        }
        let long_ago = Instant::now() - Duration::from_secs(5);
        Bar { store, settings, catalog, originals, packs, pack_states: Default::default(), progress, fetching: None, fetched: Arc::new(Mutex::new(None)), fetch_error: None, stats, tx, hwnd: None, bar_px: 0, topmost: true, panel: false, popup: None, popup_x: 0.0, popup_at: Instant::now(),
              popup_closed: None, cal: { let (y, m, _) = sys::today(); (y, m) }, tab: Tab::Apps,
              confirm: None, last_change: Instant::now(), screen_w: w as f32 / scale, wins: Vec::new(), wins_at: long_ago, guard_at: long_ago,
              active: 0, refitted: HashMap::new(), icons: HashMap::new(), thumbs: HashMap::new(), apps: None, query: String::new() }
    }

    fn theme(&self) -> &'static Theme { theme::by_id(&self.settings.theme) }
    fn save(&self) { self.store.save_settings(&self.settings); }
    fn open(&mut self, tab: Tab) {
        self.popup = None;
        if self.panel && self.tab == tab { self.panel = false } else { self.panel = true; self.tab = tab; self.confirm = None; }
    }

    /// Opens a popup under the bar item at `x`, or closes it if it is open. (A click on the bar
    /// first takes focus from an open popup, which closes it; that click must not reopen it.)
    fn toggle_popup(&mut self, p: Popup, x: f32) {
        let just_closed = matches!(self.popup_closed, Some((q, at)) if q == p && at.elapsed() < Duration::from_millis(400));
        if self.popup == Some(p) || just_closed { self.close_popup(); self.popup_closed = None; return }
        self.panel = false;
        self.popup = Some(p);
        self.popup_x = x;
        self.popup_at = Instant::now();
        if p == Popup::Calendar { let (y, m, _) = sys::today(); self.cal = (y, m); }
    }

    fn close_popup(&mut self) {
        if let Some(p) = self.popup.take() { self.popup_closed = Some((p, Instant::now())); }
    }

    /// Built-in wallpapers first, then wallhaven's, then the GitHub packs'.
    fn walls(&self) -> Vec<Wallpaper> {
        let mut l = self.originals.lock().map(|l| l.clone()).unwrap_or_default();
        l.extend(self.catalog.lock().map(|l| l.clone()).unwrap_or_default());
        l.extend(self.packs.lock().map(|l| l.clone()).unwrap_or_default());
        l
    }

    /// Sets a wallpaper; one whose full picture is not here yet is downloaded first, then set.
    fn set_wallpaper(&mut self, id: &str) {
        let Some(w) = self.walls().into_iter().find(|w| w.id == id) else { return };
        let file = self.store.wallpaper_file(&w);
        if file.is_file() {
            if sys::set_wallpaper(&file) {
                self.settings.wallpaper = w.id;
                self.last_change = Instant::now();
                self.save();
            }
        } else if !w.url.is_empty() && art::original_theme(&w.id).is_none() {
            self.last_change = Instant::now();
            self.fetch_error = None;
            self.fetching = Some(w.id.clone());
            download::full(self.store.clone(), w, self.fetched.clone());
        }
    }

    fn step_wallpaper(&mut self, step: isize) {
        let list = self.walls();
        if let Some(id) = wallhaven::next_id(&list, &self.settings.wallpaper, step).map(str::to_string) { self.set_wallpaper(&id); }
    }

    fn random_wallpaper(&mut self) {
        let n = self.walls().len();
        if n > 1 {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1);
            self.step_wallpaper(1 + (nanos as usize % (n - 1)) as isize);
        }
    }

    fn set_theme(&mut self, t: &'static Theme) {
        self.settings.theme = t.id.to_string();
        sys::apply_theme(t);
        self.save();
        // A built-in wallpaper follows the theme.
        if art::original_theme(&self.settings.wallpaper).is_some() { self.set_wallpaper(art::original_for(t.id)); }
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

    /// One wallpaper in the panel. Its preview is only read from disk while it is on screen, so
    /// categories with hundreds of pictures stay light.
    fn tile(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, w: &Wallpaper) -> egui::Response {
        let t = self.theme();
        let size = Vec2::new(122.0, 69.0);
        let (rect, r) = ui.allocate_exact_size(size + vec2(10.0, 10.0), Sense::click());
        if ui.is_rect_visible(rect) {
            let current = w.id == self.settings.wallpaper;
            let frame = if current { c(t.accent).gamma_multiply(0.55) } else if r.hovered() { c(t.surface) } else { c(t.surface).gamma_multiply(0.6) };
            ui.painter().rect_filled(rect.shrink(1.0), 10.0, frame);
            let inner = Rect::from_center_size(rect.center(), size);
            match self.thumb(ctx, w) {
                Some(tex) => { egui::Image::new((tex.id(), size)).corner_radius(8.0).paint_at(ui, inner); }
                None => { ui.painter().text(inner.center(), egui::Align2::CENTER_CENTER, "loading", FontId::proportional(11.0), c(t.muted)); }
            }
        }
        let tip = if art::original_theme(&w.id).is_some() { format!("{} · drawn by Ronin Bar", w.collection) }
            else if w.page.is_empty() { w.collection.clone() }
            else if w.resolution.is_empty() { format!("{}\n{}", w.collection, w.page) }
            else { format!("{} · {}\n{}", w.collection, w.resolution, w.page) };
        r.on_hover_text(tip)
    }

    /// A wallpaper's preview, read straight from disk (no background loader that can stall).
    /// Missing files are not remembered, so a preview appears as soon as its download finishes.
    fn thumb(&mut self, ctx: &egui::Context, w: &Wallpaper) -> Option<egui::TextureHandle> {
        if let Some(t) = self.thumbs.get(&w.id) { return t.clone() }
        // Keep memory small: previews scrolled away are read again when they come back.
        if self.thumbs.len() >= 240 { self.thumbs.clear(); }
        let path = self.store.thumb_file(w);
        if !path.is_file() { return None }
        let tex = image::open(&path).ok().map(|img| {
            let img = img.thumbnail(320, 180).into_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            ctx.load_texture(format!("thumb-{}", w.id), egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
        });
        self.thumbs.insert(w.id.clone(), tex.clone());
        tex
    }

    fn pin(&mut self, path: &str) {
        let mut p = self.settings.pinned.clone();
        p.push(path.to_string());
        self.settings.pinned = pins::clean_pins(p);
        self.save();
    }

    fn unpin(&mut self, i: usize) {
        if i < self.settings.pinned.len() { self.settings.pinned.remove(i); self.save(); }
    }

    fn hide_taskbar(&mut self) {
        let now = sys::taskbar_state();
        let mut before = now;
        if let Some(o) = self.settings.original.as_mut() {
            match o.taskbar_state { Some(s) => before = s, None => { o.taskbar_state = Some(now); self.save(); } }
        }
        sys::hide_taskbar(before);
    }

    fn show_taskbar(&self) { sys::show_taskbar(self.settings.original.as_ref().and_then(|o| o.taskbar_state)); }

    /// First frame: reserve the strip at the top of the screen, and take over from the taskbar.
    fn dock(&mut self, ctx: &egui::Context) {
        if self.hwnd.is_some() { return }
        if let Some(h) = sys::find_window(TITLE) {
            self.bar_px = (BAR_HEIGHT * ctx.pixels_per_point()).round() as i32;
            sys::appbar_register(h, self.bar_px);
            self.hwnd = Some(h);
            if self.settings.hide_taskbar { self.hide_taskbar(); }
        }
    }

    fn tick(&mut self) {
        let done = self.fetched.lock().ok().and_then(|mut d| d.take());
        if let Some((id, r)) = done {
            // Only set it if it is still the one asked for last.
            if self.fetching.as_deref() == Some(id.as_str()) {
                self.fetching = None;
                match r { Ok(()) => self.set_wallpaper(&id), Err(e) => self.fetch_error = Some(e) }
            }
        }
        if self.wins_at.elapsed() >= Duration::from_millis(400) {
            self.wins = dock::windows(&self.wins);
            self.wins_at = Instant::now();
        }
        let fg = dock::foreground();
        if self.wins.iter().any(|w| w.hwnd == fg) { self.active = fg; }
        // Step back while a fullscreen app or game is in front.
        let full = self.stats.lock().map(|s| s.fullscreen).unwrap_or(false);
        if let Some(h) = self.hwnd { if full == self.topmost { self.topmost = !full; sys::set_topmost(h, self.topmost); } }
        // Explorer can bring the taskbar back or reset the work area (e.g. after it restarts).
        if self.hwnd.is_some() && self.guard_at.elapsed() >= Duration::from_secs(1) {
            self.guard_at = Instant::now();
            if !full {
                sys::keep_top_free(self.bar_px);
                if self.settings.hide_taskbar { self.hide_taskbar(); }
                if let Some(h) = self.hwnd { sys::pin_top(h, self.bar_px); }
                // Each window is fixed at most once a minute, so an app that insists on its own
                // position never flickers.
                self.refitted.retain(|_, t| t.elapsed() < Duration::from_secs(60));
                for w in &self.wins {
                    if !self.refitted.contains_key(&w.hwnd) && sys::misfit_maximized(w.hwnd, self.bar_px) {
                        sys::refit(w.hwnd);
                        self.refitted.insert(w.hwnd, Instant::now());
                    }
                }
            }
        }
        // The theme's own wallpaper as soon as it is drawn, on first start.
        if self.settings.wallpaper.is_empty() {
            let id = art::original_for(&self.settings.theme);
            if self.walls().iter().any(|w| w.id == id) { self.set_wallpaper(id); }
        }
        let m = self.settings.auto_change_minutes;
        if m > 0 && self.last_change.elapsed() >= Duration::from_secs(u64::from(m) * 60) { self.step_wallpaper(1); }
    }

    // ───── the bar ─────

    /// A clickable bar item: a glyph and optional text, with a soft pill on hover.
    fn chip(&self, ui: &mut egui::Ui, text: Option<String>, lit: bool, draw: impl FnOnce(&egui::Painter, Rect)) -> egui::Response {
        let t = self.theme();
        let galley = text.map(|s| ui.painter().layout_no_wrap(s, FontId::proportional(12.5), c(t.fg)));
        let (pad, gw, gap) = (8.0, 16.0, 5.0);
        let w = pad * 2.0 + gw + galley.as_ref().map(|g| gap + g.size().x).unwrap_or(0.0);
        let (rect, r) = ui.allocate_exact_size(vec2(w, BAR_HEIGHT), Sense::click());
        let pill = rect.shrink2(vec2(1.0, 6.0));
        if r.hovered() || lit { ui.painter().rect_filled(pill, 9.0, c(t.surface)); }
        if lit { ui.painter().rect_stroke(pill, 9.0, Stroke::new(1.0, c(t.accent).gamma_multiply(0.7)), egui::StrokeKind::Inside); }
        let g = Rect::from_center_size(pos2(rect.left() + pad + gw / 2.0, rect.center().y), Vec2::splat(gw));
        draw(ui.painter(), g);
        if let Some(gal) = galley { ui.painter().galley(pos2(g.right() + gap, rect.center().y - gal.size().y / 2.0), gal, c(t.fg)); }
        r
    }

    fn divider(&self, ui: &mut egui::Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(9.0, BAR_HEIGHT), Sense::hover());
        ui.painter().line_segment([pos2(r.center().x, r.top() + 11.0), pos2(r.center().x, r.bottom() - 11.0)], Stroke::new(1.0, c(self.theme().muted).gamma_multiply(0.45)));
    }

    /// One dock slot: the app's icon (or its first letter) and a marker when it is in front.
    fn slot(&mut self, ui: &mut egui::Ui, path: Option<&str>, name: &str, front: bool, dim: bool) -> egui::Response {
        let t = self.theme();
        let tex = path.and_then(|p| self.icon(ui.ctx(), p));
        let (rect, r) = ui.allocate_exact_size(Vec2::new(SLOT, BAR_HEIGHT), Sense::click());
        if r.hovered() || front { ui.painter().rect_filled(rect.shrink2(Vec2::new(2.0, 4.0)), 8.0, c(t.accent).gamma_multiply(if front { 0.18 } else { 0.28 })); }
        let icon = Rect::from_center_size(rect.center() - vec2(0.0, 1.0), Vec2::splat(ICON));
        let tint = if dim { Color32::from_white_alpha(110) } else { Color32::WHITE };
        match tex {
            Some(tex) => { egui::Image::new((tex.id(), Vec2::splat(ICON))).tint(tint).paint_at(ui, icon); }
            None => {
                let letter: String = name.chars().next().unwrap_or('?').to_uppercase().collect();
                ui.painter().rect_filled(icon, 6.0, c(t.surface));
                ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, letter, FontId::proportional(12.0), c(t.fg));
            }
        }
        if front {
            ui.painter().rect_filled(Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 4.5), vec2(12.0, 2.5)), 1.5, c(t.accent));
        }
        r
    }

    fn dock_items(&mut self, ui: &mut egui::Ui, room: f32) {
        let t = self.theme();
        let mut slots = (room / SLOT).floor().max(0.0) as usize;
        // Pinned apps: click opens, right-click manages them.
        let pinned = self.settings.pinned.clone();
        for p in pinned.iter().take(slots) {
            let r = self.slot(ui, Some(p), &pins::display_name(p), false, false);
            if r.clicked() && !dock::launch(p) { self.open(Tab::Apps); }
            if r.secondary_clicked() { self.open(Tab::Apps); }
        }
        slots = slots.saturating_sub(pinned.len());
        if pinned.is_empty() {
            let r = self.chip(ui, Some("pin apps".into()), false, |p, g| glyph::draw(p, g, Glyph::Apps, c(t.muted)));
            if r.clicked() { self.open(Tab::Apps); }
            slots = slots.saturating_sub(3);
        }
        if !self.wins.is_empty() && slots > 0 { self.divider(ui); }
        // Open windows: click focuses (or minimises the one in front), middle-click closes, right-click pins.
        let wins = self.wins.clone();
        let shown = if wins.len() <= slots { wins.len() } else { slots.saturating_sub(1) };
        for win in wins.iter().take(shown) {
            let min = dock::is_minimized(win.hwnd);
            let front = win.hwnd == self.active && !min;
            let r = self.slot(ui, win.exe.as_deref(), &win.title, front, min);
            if r.clicked() {
                if front { dock::minimize(win.hwnd); self.active = 0; } else { dock::focus(win.hwnd); self.active = win.hwnd; }
            }
            if r.middle_clicked() { dock::close(win.hwnd); }
            if r.secondary_clicked() {
                if let Some(exe) = &win.exe { if !self.settings.pinned.iter().any(|p| p.eq_ignore_ascii_case(exe)) { let e = exe.clone(); self.pin(&e); } }
            }
        }
        if wins.len() > shown {
            ui.label(RichText::new(format!("+{}", wins.len() - shown)).size(12.0).color(c(t.muted)));
        }
    }

    fn bar(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let s = self.stats.lock().map(|s| s.clone()).unwrap_or_default();
        let (fg, muted, accent) = (c(t.fg), c(t.muted), c(t.accent));
        let full = ui.max_rect();
        ui.spacing_mut().item_spacing.x = 2.0;

        // Clock in the middle: click for notifications and the calendar.
        let clock = Rect::from_center_size(full.center(), vec2(CLOCK_W, BAR_HEIGHT));
        let cr = ui.interact(clock, ui.scope_id().with("clock"), Sense::click());
        {
            let (wd, d, mo, h, mi) = sys::local_time();
            const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            let p = ui.painter();
            if cr.hovered() { p.rect_filled(clock.shrink2(vec2(10.0, 6.0)), 9.0, c(t.surface)); }
            let time = p.layout_no_wrap(format!("{h:02}:{mi:02}"), FontId::proportional(14.0), fg);
            let date = p.layout_no_wrap(format!("{}  {} {}", DAYS[wd as usize % 7], d, MONTHS[(mo as usize + 11) % 12]).to_uppercase(), FontId::proportional(11.0), muted);
            let total = time.size().x + 10.0 + date.size().x;
            let x0 = clock.center().x - total / 2.0;
            let (tw, th, dh) = (time.size().x, time.size().y, date.size().y);
            p.galley(pos2(x0, clock.center().y - th / 2.0), time, fg);
            p.circle_filled(pos2(x0 + tw + 5.0, clock.center().y), 1.6, accent);
            p.galley(pos2(x0 + tw + 10.0, clock.center().y - dh / 2.0 + 0.5), date, muted);
        }
        if cr.clicked() { self.toggle_popup(Popup::Calendar, clock.center().x); }

        ui.horizontal_centered(|ui| {
            ui.add_space(6.0);
            // The Ronin mark is the Start button; right-click opens Ronin's own panel.
            let (mr, m) = ui.allocate_exact_size(vec2(40.0, BAR_HEIGHT), Sense::click());
            if m.hovered() { ui.painter().rect_filled(mr.shrink2(vec2(2.0, 4.0)), 9.0, c(t.surface)); }
            glyph::mark(ui.painter(), Rect::from_center_size(mr.center(), Vec2::splat(30.0)), accent, fg);
            if m.clicked() { sys::press_win(None); }
            if m.secondary_clicked() { self.panel = !self.panel; }
            ui.add_space(4.0);

            // The dock, on a soft island, up to the clock.
            let island = ui.painter().add(egui::Shape::Noop);
            let x0 = ui.cursor().min.x;
            ui.add_space(4.0);
            let room = clock.left() - x0 - 24.0;
            self.dock_items(ui, room);
            ui.add_space(4.0);
            let x1 = ui.cursor().min.x;
            if x1 - x0 > 10.0 {
                let r = Rect::from_min_max(pos2(x0, full.top() + 3.0), pos2(x1, full.bottom() - 3.0));
                ui.painter().set(island, egui::Shape::rect_filled(r, 11.0, c(t.surface).gamma_multiply(0.55)));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                let lit = |tab: Tab| self.panel && self.tab == tab;
                let (lp, ls, la, lt, lw, lv) = (lit(Tab::Power), lit(Tab::Settings), lit(Tab::Apps), lit(Tab::Themes), lit(Tab::Wallpapers), lit(Tab::Sound));
                let pw = self.chip(ui, None, lp || self.popup == Some(Popup::Power), |p, g| glyph::draw(p, g, Glyph::Power, accent));
                if pw.clicked() { self.confirm = None; self.toggle_popup(Popup::Power, pw.rect.center().x); }
                self.divider(ui);
                if self.chip(ui, None, ls, |p, g| glyph::draw(p, g, Glyph::Gear, fg)).clicked() { self.open(Tab::Settings); }
                if self.chip(ui, None, la, |p, g| glyph::draw(p, g, Glyph::Apps, fg)).clicked() { self.open(Tab::Apps); }
                if self.chip(ui, None, lt, |p, g| glyph::draw(p, g, Glyph::Theme, fg)).clicked() { self.open(Tab::Themes); }
                let wall = self.chip(ui, None, lw, |p, g| glyph::draw(p, g, Glyph::Wallpaper, fg));
                if wall.clicked() { self.open(Tab::Wallpapers); }
                if wall.secondary_clicked() { self.step_wallpaper(1); }
                self.divider(ui);
                if let Some((v, m)) = s.volume {
                    let text = if m { "mute".to_string() } else { format!("{:.0}", v * 100.0) };
                    let r = self.chip(ui, Some(text), lv || self.popup == Some(Popup::Sound), |p, g| glyph::volume(p, g, v, m, fg));
                    if r.clicked() { self.toggle_popup(Popup::Sound, r.rect.center().x); }
                    if r.secondary_clicked() || r.middle_clicked() { let _ = self.tx.send(worker::Cmd::Mute(!m)); }
                    if r.hovered() {
                        // One wheel notch = 5 %.
                        let dy: f32 = ui.input(|i| i.raw.events.iter().filter_map(|e| match e {
                            egui::Event::MouseWheel { delta, .. } => Some(delta.y), _ => None }).sum());
                        if dy != 0.0 { let _ = self.tx.send(worker::Cmd::Volume(v + dy.signum() * 0.05)); }
                    }
                }
                if let Some((b, charging)) = s.battery {
                    if self.chip(ui, Some(format!("{b}%")), false, |p, g| glyph::battery(p, g, b, charging, fg, accent)).clicked() { sys::press_win(Some(b'A' as u16)); }
                }
                if self.chip(ui, None, false, |p, g| glyph::network(p, g, s.online, fg, muted)).clicked() { sys::press_win(Some(b'A' as u16)); }
                self.divider(ui);
                if self.chip(ui, Some(format!("{}%", s.ram)), false, |p, g| glyph::draw(p, g, Glyph::Ram, muted)).clicked() { sys::task_manager(); }
                if self.chip(ui, Some(format!("{}%", s.cpu)), false, |p, g| glyph::draw(p, g, Glyph::Cpu, muted)).clicked() { sys::task_manager(); }
            });
        });

        // A fine accent line along the bottom edge.
        let y = full.bottom() - 0.5;
        ui.painter().line_segment([pos2(full.left(), y), pos2(full.right(), y)], Stroke::new(1.0, accent.gamma_multiply(0.45)));
    }

    // ───── popups ─────

    fn pop_calendar(&mut self, ui: &mut egui::Ui) {
        use ronin_core::calendar::{grid, shift, weekday, MONTHS, WEEKDAYS};
        let t = self.theme();
        let (ty, tm, td) = sys::today();
        let (_, _, _, h, mi) = sys::local_time();
        ui.label(RichText::new(format!("{h:02}:{mi:02}")).size(34.0).strong());
        ui.label(RichText::new(format!("{}, {} {} {}", WEEKDAYS[weekday(ty, tm, td) as usize], td, MONTHS[(tm as usize + 11) % 12], ty)).color(c(t.muted)));
        ui.add_space(10.0);
        let (y, m) = self.cal;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} {}", MONTHS[(m as usize + 11) % 12], y)).strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(">").clicked() { self.cal = shift(y, m, 1); }
                if (y, m) != (ty, tm) && ui.small_button("today").clicked() { self.cal = (ty, tm); }
                if ui.small_button("<").clicked() { self.cal = shift(y, m, -1); }
            });
        });
        ui.add_space(6.0);
        let cell = vec2((ui.available_width() / 7.0).floor(), 34.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for d in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] {
                let (r, _) = ui.allocate_exact_size(vec2(cell.x, 22.0), Sense::hover());
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, d, FontId::proportional(11.5), c(t.muted));
            }
        });
        for week in grid(y, m) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, day) in week.iter().enumerate() {
                    let (r, _) = ui.allocate_exact_size(cell, Sense::hover());
                    let Some(d) = day else { continue };
                    let today = (y, m, *d) == (ty, tm, td);
                    if today { ui.painter().circle_filled(r.center(), 14.0, c(t.accent)); }
                    let col = if today { c(t.bg) } else if i == 0 || i == 6 { c(t.muted) } else { c(t.fg) };
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, d.to_string(), FontId::proportional(13.0), col);
                }
            });
        }
    }

    /// A "more in Windows settings" link at the bottom of a popup.
    fn settings_link(&self, ui: &mut egui::Ui, label: &str, page: &str) {
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if ui.add(egui::Button::new(RichText::new(label).size(12.0).color(c(self.theme().muted))).frame(false)).clicked() { sys::open_settings(page); }
        });
    }

    fn pop_sound(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let s = self.stats.lock().map(|s| s.clone()).unwrap_or_default();
        let Some((v, m)) = s.volume else { ui.label("No sound device found."); return };
        ui.horizontal(|ui| {
            let (r, mute) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
            if mute.hovered() { ui.painter().rect_filled(r, 8.0, c(t.surface)); }
            glyph::volume(ui.painter(), r.shrink(3.0), v, m, c(if m { t.muted } else { t.fg }));
            if mute.clicked() { let _ = self.tx.send(worker::Cmd::Mute(!m)); }
            ui.label(RichText::new("Sound").strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(if m { "muted".to_string() } else { format!("{:.0}%", v * 100.0) }).size(15.0));
            });
        });
        ui.add_space(8.0);
        let mut pct = (v * 100.0).round();
        ui.spacing_mut().slider_width = ui.available_width();
        if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).show_value(false)).changed() { let _ = self.tx.send(worker::Cmd::Volume(pct / 100.0)); }
        ui.add_space(6.0);
        let mut mute = m;
        if ui.checkbox(&mut mute, "Mute").changed() { let _ = self.tx.send(worker::Cmd::Mute(mute)); }
        self.settings_link(ui, "Sound settings", "sound");
    }

    fn pop_power(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Power").strong().size(15.0));
        ui.add_space(8.0);
        let w = ui.available_width();
        let big = |ui: &mut egui::Ui, label: &str| ui.add_sized([w, 32.0], egui::Button::new(RichText::new(label).size(14.0)));
        if big(ui, "Lock").clicked() { let _ = self.tx.send(worker::Cmd::Lock); self.close_popup(); }
        for (label, what) in [("Sleep", Confirm::Sleep), ("Sign out", Confirm::SignOut), ("Restart", Confirm::Restart), ("Shut down", Confirm::ShutDown)] {
            if big(ui, label).clicked() { self.confirm = Some(what); }
        }
        self.confirm_row(ui);
    }

    // ───── the control panel ─────

    fn panel(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
            glyph::mark(ui.painter(), r, c(t.accent), c(t.fg));
            ui.label(RichText::new("R O N I N").strong().size(15.0));
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(c(t.muted)).size(11.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Drawn, not a font character, so it shows on every PC.
                let (r, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                if resp.hovered() { ui.painter().rect_filled(r, 7.0, c(t.accent).gamma_multiply(0.35)); }
                let k = r.shrink(8.0);
                let st = Stroke::new(1.6, c(t.fg));
                ui.painter().line_segment([k.left_top(), k.right_bottom()], st);
                ui.painter().line_segment([k.right_top(), k.left_bottom()], st);
                if resp.clicked() { self.panel = false; }
            });
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (label, tab) in [("Apps", Tab::Apps), ("Wallpapers", Tab::Wallpapers), ("Themes", Tab::Themes), ("Sound", Tab::Sound), ("Power", Tab::Power), ("Settings", Tab::Settings)] {
                if ui.selectable_label(self.tab == tab, label).clicked() { self.tab = tab; self.confirm = None; }
            }
        });
        ui.separator();
        match self.tab {
            Tab::Apps => self.tab_apps(ui),
            Tab::Wallpapers => self.tab_wallpapers(ui),
            Tab::Themes => self.tab_themes(ui),
            Tab::Sound => {
                let s = self.stats.lock().map(|s| s.clone()).unwrap_or_default();
                match s.volume {
                    None => { ui.label("No sound device found."); }
                    Some((v, m)) => {
                        let mut pct = (v * 100.0).round();
                        ui.horizontal(|ui| {
                            ui.label("Volume");
                            ui.spacing_mut().slider_width = 300.0;
                            if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%")).changed() { let _ = self.tx.send(worker::Cmd::Volume(pct / 100.0)); }
                        });
                        let mut mute = m;
                        if ui.checkbox(&mut mute, "Mute").changed() { let _ = self.tx.send(worker::Cmd::Mute(mute)); }
                        ui.add_space(8.0);
                        ui.label(RichText::new("On the bar: click the speaker to mute, scroll over it to change the volume.").color(c(t.muted)).size(12.0));
                    }
                }
            }
            Tab::Power => {
                ui.label(RichText::new("Unsaved work in other apps is lost on restart and shut down.").color(c(t.muted)));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Lock").clicked() { let _ = self.tx.send(worker::Cmd::Lock); }
                    for (label, what) in [("Sleep", Confirm::Sleep), ("Restart", Confirm::Restart), ("Shut down", Confirm::ShutDown)] {
                        if ui.button(label).clicked() { self.confirm = Some(what); }
                    }
                });
                self.confirm_row(ui);
            }
            Tab::Settings => {
                let mut on = self.settings.hide_taskbar;
                if ui.checkbox(&mut on, "Replace the Windows taskbar").changed() {
                    self.settings.hide_taskbar = on;
                    self.save();
                    if on { self.hide_taskbar(); } else { self.show_taskbar(); }
                }
                ui.label(RichText::new("Hides the Windows taskbar while Ronin Bar runs; it comes back when you quit or uninstall. The Ronin mark opens Start, the clock opens notifications, network and battery open quick settings.").color(c(t.muted)).size(12.0));
                ui.add_space(6.0);
                let mut start = self.settings.start_with_windows;
                if ui.checkbox(&mut start, "Start with Windows").changed() {
                    self.settings.start_with_windows = start;
                    install::set_start_with_windows(start);
                    self.save();
                }
                ui.add_space(10.0);
                ui.label(RichText::new(format!("Installed in {}", install::install_dir().display())).color(c(t.muted)).size(12.0));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Quit the bar").clicked() { self.quit(); }
                    if ui.button("Uninstall Ronin Bar…").clicked() { self.confirm = Some(Confirm::Uninstall); }
                });
                self.confirm_row(ui);
            }
        }
    }

    fn tab_themes(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        ui.label(RichText::new("Colours the bar, sets Windows' dark or light mode and accent colour. A built-in wallpaper changes with it.").color(c(t.muted)).size(12.0));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for th in THEMES.iter() {
            let selected = th.id == t.id;
            let frame = egui::Frame::new().fill(c(th.bg)).corner_radius(12.0).inner_margin(egui::Margin::symmetric(14, 12))
                .stroke(Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { c(th.accent) } else { c(th.surface) }));
            let r = frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (mr, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                    glyph::mark(ui.painter(), mr, c(th.accent), c(th.fg));
                    ui.label(RichText::new(th.name).color(c(th.fg)).strong().size(14.0));
                    ui.label(RichText::new(if th.dark { "dark" } else { "light" }).color(c(th.muted)).size(12.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for col in [th.accent, th.muted, th.fg, th.surface] {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                            ui.painter().circle_filled(rect.center(), 6.0, c(col));
                        }
                    });
                });
            }).response.interact(Sense::click());
            if r.clicked() { self.set_theme(th); }
            ui.add_space(4.0);
        }
        });
    }

    fn confirm_row(&mut self, ui: &mut egui::Ui) {
        let Some(what) = self.confirm else { return };
        let q = match what {
            Confirm::Sleep => "Put the PC to sleep now?",
            Confirm::SignOut => "Sign out of Windows now?",
            Confirm::Restart => "Restart the PC now?",
            Confirm::ShutDown => "Shut down the PC now?",
            Confirm::Uninstall => "Remove Ronin Bar and its wallpapers, bring the taskbar back, and put your old wallpaper and colours back?",
        };
        ui.add_space(8.0);
        ui.label(RichText::new(q).strong());
        ui.horizontal(|ui| {
            if ui.button("Yes").clicked() {
                self.confirm = None;
                match what {
                    Confirm::Sleep => { let _ = self.tx.send(worker::Cmd::Sleep); }
                    Confirm::SignOut => { let _ = self.tx.send(worker::Cmd::SignOut); }
                    Confirm::Restart => { let _ = self.tx.send(worker::Cmd::Restart); }
                    Confirm::ShutDown => { let _ = self.tx.send(worker::Cmd::ShutDown); }
                    Confirm::Uninstall => {
                        self.release();
                        install::uninstall(&self.settings);
                        install::remove_folder_after_exit();
                        // Not `quit`: settings must not be written back into the folder being removed.
                        std::process::exit(0);
                    }
                }
            }
            if ui.button("No").clicked() { self.confirm = None; }
        });
    }

    fn tab_apps(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let ctx = ui.ctx().clone();
        ui.label(RichText::new("Click an icon to open it · click the window in front to minimise it · middle-click closes a window · right-click an open window to pin its program.").color(c(t.muted)).size(12.0));
        ui.add_space(4.0);
        ui.label(RichText::new(format!("Pinned  {}/{}", self.settings.pinned.len(), pins::MAX_PINNED)).strong());
        let pinned = self.settings.pinned.clone();
        let mut action: Option<(usize, i8)> = None; // (index, -1 left / 1 right / 0 unpin)
        for (i, p) in pinned.iter().enumerate() {
            ui.horizontal(|ui| {
                match self.icon(&ctx, p) { Some(tex) => { ui.add(egui::Image::new((tex.id(), Vec2::splat(18.0)))); } None => { ui.add_space(22.0); } }
                ui.label(pins::display_name(p)).on_hover_text(p);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("unpin").clicked() { action = Some((i, 0)); }
                    if ui.add_enabled(i + 1 < pinned.len(), egui::Button::new(">").small()).clicked() { action = Some((i, 1)); }
                    if ui.add_enabled(i > 0, egui::Button::new("<").small()).clicked() { action = Some((i, -1)); }
                });
            });
        }
        if pinned.is_empty() { ui.label(RichText::new("Nothing pinned yet: pick apps below.").color(c(t.muted))); }
        match action {
            Some((i, 0)) => self.unpin(i),
            Some((i, d)) => { let j = (i as isize + d as isize) as usize; self.settings.pinned.swap(i, j); self.save(); }
            None => {}
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Start menu").strong());
            ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("search").desired_width(220.0));
            if ui.small_button("refresh").clicked() { self.apps = None; }
        });
        if self.apps.is_none() { self.apps = Some(dock::start_menu_apps()); }
        let list: Vec<(String, String)> = self.apps.as_ref().map(|a| a.iter().filter(|(n, _)| pins::matches(n, &self.query)).cloned().collect()).unwrap_or_default();
        let full = self.settings.pinned.len() >= pins::MAX_PINNED;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 26.0, list.len(), |ui, range| {
            for (name, path) in &list[range] {
                ui.horizontal(|ui| {
                    match self.icon(&ctx, path) { Some(tex) => { ui.add(egui::Image::new((tex.id(), Vec2::splat(18.0)))); } None => { ui.add_space(22.0); } }
                    ui.label(name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.settings.pinned.iter().any(|p| p.eq_ignore_ascii_case(path)) {
                            ui.label(RichText::new("pinned").color(c(t.accent)).size(12.0));
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

    fn tab_wallpapers(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let p = self.progress.lock().map(|p| p.clone()).unwrap_or_default();
        let list = self.walls();
        let downloaded = self.catalog.lock().map(|l| l.len()).unwrap_or(0);
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
            ui.label(RichText::new(format!("Getting previews of the most-loved anime wallpapers from wallhaven.cc…  {}/{}", p.done, p.total)).color(c(t.muted)).size(12.0));
        } else if p.error.is_some() && downloaded < COLLECTIONS.len() * PER_COLLECTION {
            ui.label(RichText::new(format!("{} of {} anime wallpapers ready. Check the internet connection and try again.", downloaded, COLLECTIONS.len() * PER_COLLECTION)).color(c(t.accent)).size(12.0))
                .on_hover_text(p.error.clone().unwrap_or_default());
        }
        if self.fetching.is_some() {
            ui.label(RichText::new("Downloading the full picture…").color(c(t.muted)).size(12.0));
        } else if let Some(e) = &self.fetch_error {
            ui.label(RichText::new("That wallpaper could not be downloaded. Try again in a minute.").color(c(t.accent)).size(12.0)).on_hover_text(e.clone());
        }
        if !p.running && downloaded < COLLECTIONS.len() * PER_COLLECTION && ui.button("Get the anime wallpapers").clicked() {
            download::run(self.store.clone(), self.catalog.clone(), self.progress.clone(), self.settings.wallpaper.clone());
        }
        ui.add_space(4.0);
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let names: Vec<&str> = std::iter::once(art::ORIGINALS_NAME).chain(COLLECTIONS.iter().map(|c| c.name))
                .chain(github::packs().iter().map(|p| p.name)).collect();
            for name in names {
                if name == COLLECTIONS[0].name { ui.add_space(4.0); ui.label(RichText::new("FROM WALLHAVEN.CC").color(c(t.accent)).size(11.0)); }
                let pack = github::packs().iter().find(|p| p.name == name);
                if pack.is_some() && name == github::packs()[0].name {
                    ui.add_space(4.0);
                    ui.label(RichText::new("FROM GITHUB").color(c(t.accent)).size(11.0));
                }
                let items: Vec<&Wallpaper> = list.iter().filter(|w| w.collection == name).collect();
                let title = match pack {
                    Some(p) => format!("{name}  ·  {}/{}", items.len(), p.files.len()),
                    None => format!("{name}  ·  {}", items.len()),
                };
                // The built-in and wallhaven sets start open; the big GitHub categories start closed.
                egui::CollapsingHeader::new(RichText::new(title).strong()).id_salt(name).default_open(pack.is_none()).show(ui, |ui| {
                    if let Some(p) = pack {
                        let st = self.pack_states.lock().ok().and_then(|m| m.get(p.name).cloned()).unwrap_or_default();
                        ui.horizontal(|ui| {
                            if st.running {
                                ui.label(RichText::new(format!("downloading {}/{}", st.done, st.total)).color(c(t.muted)).size(12.0));
                            } else if items.len() < p.files.len() {
                                let next = (p.files.len() - items.len()).min(download::BATCH);
                                let label = if items.is_empty() { format!("get {next} (about {} MB)", next * 2) } else { format!("get {next} more (about {} MB)", next * 2) };
                                if ui.small_button(label).clicked() {
                                    download::pack(self.store.clone(), p, self.packs.clone(), self.pack_states.clone());
                                }
                                if let Some(e) = st.error { ui.label(RichText::new("some failed, try again").color(c(t.accent)).size(12.0)).on_hover_text(e); }
                            }
                        });
                    }
                    ui.horizontal_wrapped(|ui| {
                        for w in items {
                            let r = self.tile(ui, &ctx, w);
                            if r.clicked() { let id = w.id.clone(); self.set_wallpaper(&id); }
                        }
                    });
                });
                ui.add_space(4.0);
            }
            ui.label(RichText::new("Wallpapers from wallhaven.cc and GitHub belong to their artists; they are downloaded to this PC for personal use only. The Ronin originals are drawn by Ronin Bar itself.").color(c(t.muted)).size(11.0));
        });
    }

    /// Gives back the top strip and the taskbar.
    fn release(&mut self) {
        if let Some(h) = self.hwnd.take() {
            sys::appbar_remove(h);
            sys::release_top(self.bar_px);
        }
        self.show_taskbar();
    }

    /// Ends Ronin Bar right away (also from the panel, which is a window of its own): gives back the
    /// top strip and the taskbar first, so nothing is left behind.
    fn quit(&mut self) -> ! {
        self.release();
        self.save();
        std::process::exit(0)
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
            let (w, h) = (580.0, 640.0);
            let pos = egui::pos2((self.screen_w - w - 10.0).max(0.0), BAR_HEIGHT + 8.0);
            let builder = egui::ViewportBuilder::default()
                .with_title(PANEL_TITLE)
                .with_decorations(false).with_taskbar(false).with_always_on_top().with_resizable(false)
                .with_inner_size([w, h]).with_position(pos);
            let close = ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("ronin-panel"), builder, |ui, _class| {
                let t = self.theme();
                egui::Frame::new().fill(c(t.bg)).stroke(Stroke::new(1.0, c(t.accent).gamma_multiply(0.5))).inner_margin(16.0).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.panel(ui);
                });
                ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            });
            if close { self.panel = false; }
        }
        if let Some(p) = self.popup {
            let (w, h) = match p { Popup::Calendar => (330.0, 392.0), Popup::Sound => (330.0, 178.0), Popup::Power => (300.0, 330.0) };
            let x = (self.popup_x - w / 2.0).clamp(8.0, (self.screen_w - w - 8.0).max(8.0));
            let builder = egui::ViewportBuilder::default()
                .with_title(POPUP_TITLE)
                .with_decorations(false).with_taskbar(false).with_always_on_top().with_resizable(false)
                .with_inner_size([w, h]).with_position(egui::pos2(x, BAR_HEIGHT + 6.0));
            let id = egui::ViewportId::from_hash_of(("ronin-popup", p));
            let fresh = self.popup_at.elapsed() < Duration::from_millis(120);
            let close = ctx.show_viewport_immediate(id, builder, |ui, _class| {
                let t = self.theme();
                egui::Frame::new().fill(c(t.bg)).stroke(Stroke::new(1.0, c(t.accent).gamma_multiply(0.5))).inner_margin(16.0).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    match p { Popup::Calendar => self.pop_calendar(ui), Popup::Sound => self.pop_sound(ui), Popup::Power => self.pop_power(ui) }
                });
                // Closes on Escape, or when you click anywhere else, like a desktop panel's menus.
                ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)
                    || (i.viewport().focused == Some(false) && !fresh && self.popup_at.elapsed() > Duration::from_millis(400)))
            });
            if fresh { ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus); }
            if close { self.close_popup(); }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) { self.release(); }
}
