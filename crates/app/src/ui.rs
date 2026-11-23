//! The dock across the top of the screen and its control panel.
use crate::glyph::{self, Glyph};
use crate::{bt, dock, download, install, net, store::Store, sys, update, worker};
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
enum Tab { Wallpapers, Themes, Settings }

/// Ronin's own popups, opened from the bar in place of Windows' flyouts.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Popup { Launcher, Calendar, Sound, Power, Battery, Network, Bluetooth, Quick, Update }

/// The pages of the quick settings popup.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum QuickView { Main, Night, Access, Project }

/// What a quick settings tile does when clicked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum QAct { Airplane, Night, NightPage, Hotspot, HotspotSettings, Access, Project, Cast, Captions, Nearby, Battery, Snip }

/// How often to look for a new version (the first look is 20 s after start).
const UPDATE_EVERY: Duration = Duration::from_secs(6 * 3600);

/// Width of a quick settings tile, and the gap between tiles.
const TILE_W: f32 = 108.0;
const TILE_GAP: f32 = 14.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirm { Sleep, SignOut, Restart, ShutDown, Uninstall }

/// Installed apps (name, launch id), once read.
type AppList = Arc<Mutex<Option<Vec<(String, String)>>>>;

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
    /// Wi-Fi and Bluetooth (their own thread), the network whose password is being typed, and the
    /// last scan asked for.
    net: Arc<Mutex<net::NetState>>,
    net_tx: Sender<net::NetCmd>,
    joining: Option<(String, String)>,
    /// Bluetooth devices (their own thread).
    bt: Arc<Mutex<bt::BtState>>,
    bt_tx: Sender<bt::BtCmd>,
    net_scan_at: Instant,
    /// Every installed app (name, launch id), read in the background; when it was read; the
    /// launcher's search text.
    all_apps: AppList,
    all_apps_at: Instant,
    /// App ids of open windows the list was already read again for (once each).
    apps_asked: std::collections::HashSet<String>,
    search: String,
    /// The launcher's chosen row (↑ ↓) and whether hidden apps are shown.
    launcher_sel: usize,
    show_hidden: bool,
    /// The brightness and volume being dragged, until the slider is let go, and when brightness
    /// was last sent (the screen is asked at most a few times a second).
    bright_drag: Option<f32>,
    bright_sent: Instant,
    vol_drag: Option<f32>,
    /// The theme the colours were last set for.
    styled: &'static str,
    /// The popup window is made once and then only shown and hidden, so popups open instantly.
    popup_live: bool,
    popup_size: (f32, f32),
    /// Quick settings: the page shown, whether the open popup was reached from it (it then has a
    /// back button), a short message from the last action, and sticky keys as last read.
    quick_view: QuickView,
    from_quick: bool,
    quick_note: Option<String>,
    sticky: bool,
    /// Updates: what the update thread found, when it last looked, the version whose popup was
    /// already shown, and whether "install now" was asked for.
    upd: update::Shared,
    upd_at: Instant,
    upd_seen: Option<ronin_core::update::Version>,
    upd_install: bool,
    /// For background work that has to wake the bar.
    ctx: egui::Context,
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
}

fn popup_size(p: Popup) -> (f32, f32) {
    match p { Popup::Launcher => (440.0, 600.0), Popup::Calendar => (330.0, 392.0), Popup::Sound => (330.0, 178.0), Popup::Power => (300.0, 330.0), Popup::Battery => (330.0, 280.0), Popup::Network => (360.0, 470.0), Popup::Bluetooth => (360.0, 470.0), Popup::Quick => (3.0 * TILE_W + 2.0 * TILE_GAP + 32.0, 520.0), Popup::Update => (400.0, 430.0) }
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
        let (stats, tx) = worker::start(cc.egui_ctx.clone());
        let (net, net_tx) = net::start(cc.egui_ctx.clone());
        let (bt, bt_tx) = bt::start(cc.egui_ctx.clone());
        let catalog = Arc::new(Mutex::new(store.load_catalog()));
        let originals = Arc::new(Mutex::new(Vec::new()));
        let packs = Arc::new(Mutex::new(store.load_packs()));
        let progress = Arc::new(Mutex::new(download::Progress::default()));
        let (w, h, scale) = sys::primary_screen();
        download::originals(store.clone(), (w.max(16) as u32, h.max(16) as u32), settings.theme.clone(), originals.clone());
        if settings.night_light { let _ = tx.send(worker::Cmd::Night(settings.night_strength)); }
        if catalog.lock().map(|l| l.len()).unwrap_or(0) < COLLECTIONS.len() * PER_COLLECTION {
            download::run(store.clone(), catalog.clone(), progress.clone(), settings.wallpaper.clone());
        }
        let long_ago = Instant::now() - Duration::from_secs(5);
        Bar { store, settings, catalog, originals, packs, pack_states: Default::default(), progress, fetching: None, fetched: Arc::new(Mutex::new(None)), fetch_error: None, stats, tx, hwnd: None, bar_px: 0, topmost: true, panel: false, popup: None, popup_x: 0.0, popup_at: Instant::now(),
              popup_closed: None, cal: { let (y, m, _) = sys::today(); (y, m) }, bright_drag: None, bright_sent: Instant::now(), vol_drag: None, styled: "", popup_live: false, popup_size: (330.0, 392.0), quick_view: QuickView::Main, from_quick: false, quick_note: None, sticky: false, upd: Default::default(), upd_at: Instant::now() - UPDATE_EVERY + Duration::from_secs(20), upd_seen: None, upd_install: false, ctx: cc.egui_ctx.clone(), all_apps: Arc::new(Mutex::new(None)), all_apps_at: Instant::now() - Duration::from_secs(3600), apps_asked: Default::default(), search: String::new(), launcher_sel: 0, show_hidden: false, net, net_tx, bt, bt_tx, joining: None, net_scan_at: Instant::now() - Duration::from_secs(60), tab: Tab::Wallpapers,
              confirm: None, last_change: Instant::now(), screen_w: w as f32 / scale, wins: Vec::new(), wins_at: long_ago, guard_at: long_ago,
              active: 0, refitted: HashMap::new(), icons: HashMap::new(), thumbs: HashMap::new() }
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
        self.from_quick = false;
        self.opened(p);
    }

    /// Shows another popup in the same window (from a quick settings tile); it gets a back button.
    fn go(&mut self, p: Popup) {
        self.popup = Some(p);
        self.from_quick = true;
        self.opened(p);
    }

    /// Fresh data for a popup that has just appeared.
    fn opened(&mut self, p: Popup) {
        if matches!(p, Popup::Network | Popup::Quick) { let _ = self.net_tx.send(net::NetCmd::Refresh); }
        if p == Popup::Bluetooth { let _ = self.bt_tx.send(bt::BtCmd::CheckAudio); }
        if p == Popup::Network { self.net_scan_at = Instant::now() - Duration::from_secs(60); }
        if p == Popup::Quick { self.quick_view = QuickView::Main; self.quick_note = None; self.sticky = sys::sticky_keys(); }
        if p == Popup::Launcher { self.search.clear(); self.launcher_sel = 0; self.show_hidden = false; self.load_apps(); }
    }

    fn close_popup(&mut self) {
        if let Some(p) = self.popup.take() { self.popup_closed = Some((p, Instant::now())); }
        let _ = self.bt_tx.send(bt::BtCmd::StopScan);
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
            let img = if pins::is_app_id(path) { egui::ColorImage::from_rgba_premultiplied([w, h], &px) } else { egui::ColorImage::from_rgba_unmultiplied([w, h], &px) };
            ctx.load_texture(path, img, egui::TextureOptions::LINEAR)
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
            self.popup_live = true;   // make the popup window now, hidden, so the first click is instant
            self.load_apps();         // the list of installed apps also names and draws open windows
            if self.settings.hide_taskbar { self.hide_taskbar(); }
        }
    }

    /// Follows the screen's scale and size while the bar runs (Settings → Display → Scale, a
    /// monitor plugged in or out): the bar's height in pixels, its reserved strip, and where the
    /// popups and the panel open.
    fn follow_screen(&mut self, ctx: &egui::Context) {
        let Some(h) = self.hwnd else { return };
        let ppp = ctx.pixels_per_point();
        let (w, _, _) = sys::primary_screen();
        let sw = w as f32 / ppp;
        if (sw - self.screen_w).abs() > 0.5 { self.screen_w = sw; }
        let px = (BAR_HEIGHT * ppp).round() as i32;
        if px != self.bar_px && px > 0 {
            self.bar_px = px;
            sys::appbar_resize(h, px);
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
        self.updates(full);
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

    /// An open window's icon: its program's, else its installed app's (Store apps), else the icon
    /// the window draws in its own title bar.
    ///
    /// The installed app comes first when Windows lists it: many apps run inside another
    /// program (Netflix and other web apps inside Edge or Chrome, Store apps inside a host), and
    /// only the app id tells them apart.
    fn win_icon(&mut self, ctx: &egui::Context, w: &dock::Win) -> Option<egui::TextureHandle> {
        if let Some(a) = w.app.as_deref().filter(|a| self.listed_app(a).is_some()) {
            if let Some(t) = self.icon(ctx, a) { return Some(t) }
        }
        // An app id Windows' list does not have yet (a web app just installed): read the list
        // again (at most once a minute), and use the program's icon meanwhile.
        let loaded = self.all_apps.lock().map(|l| l.is_some()).unwrap_or(false);
        if let (Some(a), true) = (&w.app, loaded) {
            if self.apps_asked.insert(a.to_ascii_lowercase()) { self.all_apps_at = Instant::now() - Duration::from_secs(3600); self.load_apps(); }
        }
        if let Some(t) = w.own_exe().and_then(|e| self.icon(ctx, e)) { return Some(t) }
        if let Some(t) = w.app.as_deref().and_then(|a| self.icon(ctx, a)) { return Some(t) }
        let key = format!("window:{}", w.hwnd);
        if let Some(t) = self.icons.get(&key) { return t.clone() }
        let tex = dock::window_icon(w.hwnd).map(|(iw, ih, px)| ctx.load_texture(&key, egui::ColorImage::from_rgba_unmultiplied([iw, ih], &px), egui::TextureOptions::LINEAR));
        self.icons.insert(key, tex.clone());
        tex
    }

    /// The name of an installed app, if Windows' list of apps has this id.
    fn listed_app(&self, id: &str) -> Option<String> {
        self.all_apps.lock().ok()?.as_ref()?.iter().find(|(_, a)| a.eq_ignore_ascii_case(id)).map(|(n, _)| n.clone())
    }

    /// What to pin for an open window: its installed app when Windows lists it, else its program.
    fn win_pin(&self, w: &dock::Win) -> Option<String> {
        w.app.clone().filter(|a| self.listed_app(a).is_some()).or_else(|| w.pin_target())
    }

    /// One dock slot: the app's icon (or its first letter) and a marker when it is in front.
    fn slot(&mut self, ui: &mut egui::Ui, tex: Option<egui::TextureHandle>, name: &str, front: bool, dim: bool) -> egui::Response {
        let t = self.theme();
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
        // Pinned apps: click opens, right-click unpins.
        let pinned = self.settings.pinned.clone();
        let mut unpin: Option<usize> = None;
        for (i, p) in pinned.iter().take(slots).enumerate() {
            let name = self.app_name(p);
            let tex = self.icon(ui.ctx(), p);
            let r = self.slot(ui, tex, &name, false, false).on_hover_text(format!("{name}\nright-click to unpin"));
            if r.clicked() && !dock::launch(p) { self.toggle_popup(Popup::Launcher, 40.0); }
            if r.secondary_clicked() { unpin = Some(i); }
        }
        if let Some(i) = unpin { self.unpin(i); }
        slots = slots.saturating_sub(pinned.len());
        if pinned.is_empty() {
            let r = self.chip(ui, Some("pin apps".into()), false, |p, g| glyph::draw(p, g, Glyph::Apps, c(t.muted)));
            if r.on_hover_text("Open the Ronin launcher and click Pin next to an app").clicked() { self.toggle_popup(Popup::Launcher, 40.0); }
            slots = slots.saturating_sub(3);
        }
        if !self.wins.is_empty() && slots > 0 { self.divider(ui); }
        // Open windows: click focuses (or minimises the one in front), middle-click closes, right-click pins.
        let wins = self.wins.clone();
        let shown = if wins.len() <= slots { wins.len() } else { slots.saturating_sub(1) };
        for win in wins.iter().take(shown) {
            let min = dock::is_minimized(win.hwnd);
            let front = win.hwnd == self.active && !min;
            let tex = self.win_icon(&ui.ctx().clone(), win);
            let app = win.app.as_deref().and_then(|a| self.listed_app(a));
            let tip = match &app { Some(n) if !win.title.contains(n.as_str()) => format!("{n}\n{}", win.title), _ => win.title.clone() };
            let r = self.slot(ui, tex, app.as_deref().unwrap_or(&win.title), front, min).on_hover_text(tip);
            if r.clicked() {
                if front { dock::minimize(win.hwnd); self.active = 0; } else { dock::focus(win.hwnd); self.active = win.hwnd; }
            }
            if r.middle_clicked() { dock::close(win.hwnd); }
            if r.secondary_clicked() {
                if let Some(target) = self.win_pin(win) { if !self.settings.pinned.iter().any(|p| p.eq_ignore_ascii_case(&target)) { self.pin(&target); } }
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
            if m.clicked() { self.toggle_popup(Popup::Launcher, mr.center().x); }
            if m.secondary_clicked() { self.popup = None; self.panel = !self.panel; }
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
                let (ls, lt, lw) = (lit(Tab::Settings), lit(Tab::Themes), lit(Tab::Wallpapers));
                let pw = self.chip(ui, None, self.popup == Some(Popup::Power), |p, g| glyph::draw(p, g, Glyph::Power, accent));
                if pw.clicked() { self.confirm = None; self.toggle_popup(Popup::Power, pw.rect.center().x); }
                self.divider(ui);
                if self.chip(ui, None, ls, |p, g| glyph::draw(p, g, Glyph::Gear, fg)).clicked() { self.open(Tab::Settings); }
                // Shown only while a new version is waiting.
                let newer = self.upd.lock().ok().and_then(|u| u.latest.as_ref().map(|r| r.version));
                if let Some(v) = newer {
                    let r = self.chip(ui, None, self.popup == Some(Popup::Update), |p, g| glyph::draw(p, g, Glyph::Update, accent));
                    if r.on_hover_text(format!("Ronin Bar {v} is available")).clicked() { self.toggle_popup(Popup::Update, self.screen_w - 210.0); }
                }
                if self.chip(ui, None, lt, |p, g| glyph::draw(p, g, Glyph::Theme, fg)).clicked() { self.open(Tab::Themes); }
                let wall = self.chip(ui, None, lw, |p, g| glyph::draw(p, g, Glyph::Wallpaper, fg));
                if wall.clicked() { self.open(Tab::Wallpapers); }
                if wall.secondary_clicked() { self.step_wallpaper(1); }
                self.divider(ui);
                if let Some((v, m)) = s.volume {
                    let text = if m { "mute".to_string() } else { format!("{:.0}", v * 100.0) };
                    let r = self.chip(ui, Some(text), self.popup == Some(Popup::Sound), |p, g| glyph::volume(p, g, v, m, fg));
                    if r.clicked() { self.toggle_popup(Popup::Sound, r.rect.center().x); }
                    if r.secondary_clicked() || r.middle_clicked() { self.set_mute(!m); }
                    if r.hovered() {
                        // One wheel notch = 5 %.
                        let dy: f32 = ui.input(|i| i.raw.events.iter().filter_map(|e| match e {
                            egui::Event::MouseWheel { delta, .. } => Some(delta.y), _ => None }).sum());
                        if dy != 0.0 { self.set_volume(v + dy.signum() * 0.05); }
                    }
                }
                if let Some((b, charging)) = s.battery {
                    let r = self.chip(ui, Some(format!("{b}%")), self.popup == Some(Popup::Battery), |p, g| glyph::battery(p, g, b, charging, fg, accent));
                    if r.clicked() { self.toggle_popup(Popup::Battery, r.rect.center().x); }
                }
                // Quick settings: airplane mode, night light, hotspot and the rest.
                let r = self.chip(ui, None, self.popup == Some(Popup::Quick), |p, g| glyph::draw(p, g, Glyph::Toggles, fg)).on_hover_text("Quick settings");
                if r.clicked() { self.toggle_popup(Popup::Quick, r.rect.center().x); }
                // Bluetooth and Wi-Fi, each with its own popup.
                let (wifi_on, bt_on, wifi_net) = self.net.lock().map(|n| (n.wifi_on, n.bluetooth_on, n.connected.clone())).unwrap_or((None, None, None));
                if let Some(on) = bt_on {
                    let r = self.chip(ui, None, self.popup == Some(Popup::Bluetooth), |p, g| glyph::quick(p, g.shrink(1.0), glyph::Quick::Bluetooth, if on { fg } else { muted })).on_hover_text(if on { "Bluetooth on" } else { "Bluetooth off" });
                    if r.clicked() { self.toggle_popup(Popup::Bluetooth, r.rect.center().x); }
                }
                let tip = match &wifi_net { Some((n, q)) => format!("Wi-Fi: {n} ({q}%)"), None if wifi_on == Some(false) => "Wi-Fi is off".into(), None if s.online => "Connected by cable".into(), None => "Not connected".into() };
                let r = self.chip(ui, None, self.popup == Some(Popup::Network), |p, g| glyph::network(p, g, wifi_net.is_some() || s.online, fg, muted)).on_hover_text(tip);
                if r.clicked() { self.joining = None; self.toggle_popup(Popup::Network, r.rect.center().x); }
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
            if mute.clicked() { self.set_mute(!m); }
            ui.label(RichText::new("Sound").strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(if m { "muted".to_string() } else { format!("{:.0}%", v * 100.0) }).size(15.0));
            });
        });
        ui.add_space(8.0);
        ui.spacing_mut().slider_width = ui.available_width();
        self.volume_slider(ui, v);
        ui.add_space(6.0);
        let mut mute = m;
        if ui.checkbox(&mut mute, "Mute").changed() { self.set_mute(mute); }
        self.settings_link(ui, "Sound settings", "sound");
    }

    /// Volume and mute, shown at once (the worker thread confirms a moment later).
    fn set_volume(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        if let Ok(mut s) = self.stats.lock() { s.volume = Some((v, false)); }
        let _ = self.tx.send(worker::Cmd::Volume(v));
    }
    fn set_mute(&mut self, m: bool) {
        if let Ok(mut s) = self.stats.lock() { if let Some((_, mute)) = s.volume.as_mut() { *mute = m; } }
        let _ = self.tx.send(worker::Cmd::Mute(m));
    }
    /// A volume slider that follows the mouse exactly while dragged.
    fn volume_slider(&mut self, ui: &mut egui::Ui, v: f32) -> egui::Response {
        let mut pct = self.vol_drag.unwrap_or((v * 100.0).round());
        let r = ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).show_value(false));
        if r.changed() { self.vol_drag = Some(pct); self.set_volume(pct / 100.0); }
        if !r.dragged() { self.vol_drag = None; }
        r
    }
    /// A brightness slider: follows the mouse, and the screen follows a few times a second.
    fn brightness_slider(&mut self, ui: &mut egui::Ui, br: u8) -> egui::Response {
        let mut v = self.bright_drag.unwrap_or(f32::from(br));
        let r = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).show_value(false));
        if r.changed() { self.bright_drag = Some(v); }
        let send = (r.changed() && (!r.dragged() || self.bright_sent.elapsed() > Duration::from_millis(120))) || r.drag_stopped();
        if send { let _ = self.tx.send(worker::Cmd::Brightness(v.round() as u8)); self.bright_sent = Instant::now(); }
        if !r.dragged() && !r.changed() { self.bright_drag = None; }
        r
    }

    /// Reads the installed apps in the background (again if the list is over a minute old, so new
    /// installs show up).
    fn load_apps(&mut self) {
        if self.all_apps_at.elapsed() < Duration::from_secs(60) { return }
        self.all_apps_at = Instant::now();
        let slot = self.all_apps.clone();
        std::thread::spawn(move || {
            sys::com_init();
            let list = dock::all_apps();
            if let Ok(mut s) = slot.lock() { *s = Some(list); }
        });
    }

    /// A pinned app's name: its real name for installed apps, else from its file name.
    fn app_name(&self, pin: &str) -> String {
        if pins::is_app_id(pin) {
            if let Some(n) = self.all_apps.lock().ok().and_then(|l| l.as_ref().and_then(|l| l.iter().find(|(_, id)| id == pin).map(|(n, _)| n.clone()))) { return n }
        }
        pins::display_name(pin)
    }

    /// Opens an app from the launcher and remembers it for "Most used".
    fn launch_app(&mut self, id: &str) {
        if dock::launch(id) {
            ronin_core::launcher::count_launch(&mut self.settings.launch_counts, id);
            self.save();
        }
        self.close_popup();
    }

    /// A row of big app icons (pinned or most used); returns what was clicked.
    fn icon_row(&mut self, ui: &mut egui::Ui, apps: &[String], can_unpin: bool) -> (Option<String>, Option<String>) {
        let t = self.theme();
        let ctx = ui.ctx().clone();
        let (mut launch, mut unpin) = (None, None);
        ui.horizontal_wrapped(|ui| {
            for p in apps {
                let name = self.app_name(p);
                let (rect, r) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::click());
                if r.hovered() { ui.painter().rect_filled(rect, 10.0, c(t.surface)); }
                let icon = Rect::from_center_size(rect.center() - vec2(0.0, 8.0), Vec2::splat(30.0));
                match self.icon(&ctx, p) {
                    Some(tex) => { egui::Image::new((tex.id(), Vec2::splat(30.0))).paint_at(ui, icon); }
                    None => { ui.painter().rect_filled(icon, 7.0, c(t.surface)); }
                }
                let short: String = name.chars().take(10).collect();
                ui.painter().text(egui::pos2(rect.center().x, rect.bottom() - 9.0), egui::Align2::CENTER_CENTER, short, FontId::proportional(10.5), c(t.fg));
                if can_unpin {
                    // A small × in the corner unpins (so does a right-click).
                    let x_rect = Rect::from_center_size(rect.right_top() + vec2(-9.0, 9.0), Vec2::splat(16.0));
                    let x = ui.interact(x_rect, ui.scope_id().with("unpin").with(p.as_str()), Sense::click());
                    if r.hovered() || x.hovered() {
                        ui.painter().circle_filled(x_rect.center(), 8.0, if x.hovered() { c(t.accent) } else { c(t.surface) });
                        let k = x_rect.shrink(4.5);
                        let st = Stroke::new(1.4, c(t.fg));
                        ui.painter().line_segment([k.left_top(), k.right_bottom()], st);
                        ui.painter().line_segment([k.right_top(), k.left_bottom()], st);
                    }
                    if x.on_hover_text("Unpin").clicked() || r.secondary_clicked() { unpin = Some(p.clone()); continue }
                }
                if r.on_hover_text(&name).clicked() { launch = Some(p.clone()); }
            }
        });
        (launch, unpin)
    }

    /// The Ronin launcher: pinned and most-used apps, then every app, with search. ↑ ↓ choose,
    /// Enter opens, right-click for pin, hide and show in folder.
    fn pop_launcher(&mut self, ui: &mut egui::Ui) {
        use ronin_core::launcher;
        let t = self.theme();
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
            glyph::mark(ui.painter(), r, c(t.accent), c(t.fg));
            ui.label(RichText::new("R O N I N").strong().size(15.0));
        });
        ui.add_space(6.0);
        let before = self.search.clone();
        let field = ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search apps  ·  arrow keys to choose, Enter to open").desired_width(f32::INFINITY).margin(vec2(10.0, 8.0)));
        if self.popup_at.elapsed() < Duration::from_millis(300) { field.request_focus(); }
        if self.search != before { self.launcher_sel = 0; }
        let (up, down, enter) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::Enter)));
        ui.add_space(8.0);
        let apps = self.all_apps.lock().ok().and_then(|l| l.clone());
        let mut launch: Option<String> = None;
        let mut pin: Option<String> = None;
        let mut unpin: Option<String> = None;
        let mut hide: Option<(String, bool)> = None;
        let searching = !self.search.trim().is_empty();
        if !searching {
            if !self.settings.pinned.is_empty() {
                ui.label(RichText::new("PINNED").color(c(t.accent)).size(11.0));
                let pinned: Vec<String> = self.settings.pinned.iter().take(12).cloned().collect();
                let (l, u) = self.icon_row(ui, &pinned, true);
                launch = launch.or(l); unpin = unpin.or(u);
                ui.add_space(4.0);
            }
            // Most used: apps you open often that are not pinned yet.
            let installed = apps.as_ref().map(|l| l.iter().map(|(_, id)| id.clone()).collect::<Vec<_>>()).unwrap_or_default();
            let used: Vec<String> = launcher::most_used(&self.settings.launch_counts, 24).into_iter()
                .filter(|a| !self.settings.pinned.contains(a) && installed.contains(a) && !self.settings.hidden_apps.contains(a)).take(6).collect();
            if !used.is_empty() {
                ui.label(RichText::new("MOST USED").color(c(t.accent)).size(11.0));
                let (l, _) = self.icon_row(ui, &used, false);
                launch = launch.or(l);
                ui.add_space(4.0);
            }
        }
        let hidden_n = self.settings.hidden_apps.len();
        ui.horizontal(|ui| {
            ui.label(RichText::new(if searching { "RESULTS" } else { "ALL APPS" }).color(c(t.accent)).size(11.0));
            if hidden_n > 0 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.show_hidden { format!("hide {hidden_n} hidden") } else { format!("show {hidden_n} hidden") };
                    if ui.add(egui::Button::new(RichText::new(label).size(11.0).color(c(t.muted))).frame(false)).clicked() { self.show_hidden = !self.show_hidden; }
                });
            }
        });
        match apps {
            None => { ui.label(RichText::new("Reading your apps…").color(c(t.muted))); }
            Some(list) => {
                let mut found: Vec<(u32, &(String, String))> = list.iter()
                    .filter(|(_, id)| self.show_hidden || !self.settings.hidden_apps.contains(id))
                    .filter_map(|a| launcher::rank(&a.0, &self.search).map(|r| (r, a)))
                    .collect();
                if searching { found.sort_by_key(|(r, (n, _))| (*r, n.to_lowercase())); }
                let n = found.len();
                if n > 0 {
                    if down { self.launcher_sel = (self.launcher_sel + 1).min(n - 1); }
                    if up { self.launcher_sel = self.launcher_sel.saturating_sub(1); }
                    self.launcher_sel = self.launcher_sel.min(n - 1);
                    if enter && (searching || self.launcher_sel > 0) { launch = Some(found[self.launcher_sel].1 .1.clone()); }
                }
                let sel = self.launcher_sel;
                let row_h = 30.0;
                let footer = 44.0;
                let mut area = egui::ScrollArea::vertical().max_height((ui.available_height() - footer).max(80.0)).auto_shrink([false, false]);
                // Keep the chosen app in view while ↑ ↓ move through the list.
                if up || down { area = area.vertical_scroll_offset(((sel as f32) - 4.0).max(0.0) * (row_h + ui.spacing().item_spacing.y)); }
                area.show_rows(ui, row_h, n, |ui, range| {
                    for k in range {
                        let (_, (name, id)) = found[k];
                        let is_hidden = self.settings.hidden_apps.contains(id);
                        let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                        if k == sel && (searching || up || down || self.launcher_sel > 0) { ui.painter().rect_filled(rect, 8.0, c(t.accent).gamma_multiply(0.22)); }
                        else if r.hovered() { ui.painter().rect_filled(rect, 8.0, c(t.surface)); }
                        let icon = Rect::from_center_size(egui::pos2(rect.left() + 18.0, rect.center().y), Vec2::splat(22.0));
                        if let Some(tex) = self.icon(&ctx, id) { egui::Image::new((tex.id(), Vec2::splat(22.0))).tint(if is_hidden { Color32::from_white_alpha(110) } else { Color32::WHITE }).paint_at(ui, icon); }
                        ui.painter().text(egui::pos2(rect.left() + 38.0, rect.center().y), egui::Align2::LEFT_CENTER, name, FontId::proportional(13.5), if is_hidden { c(t.muted) } else { c(t.fg) });
                        let pinned = self.settings.pinned.iter().any(|p| p == id);
                        // Pin / Unpin button at the end of the row (always shown for pinned apps).
                        let pill = Rect::from_min_max(egui::pos2(rect.right() - 66.0, rect.top() + 4.0), egui::pos2(rect.right() - 4.0, rect.bottom() - 4.0));
                        let b = ui.interact(pill, ui.scope_id().with("pin").with(id.as_str()), Sense::click());
                        if r.hovered() || b.hovered() || pinned {
                            let fill = if b.hovered() { c(t.accent) } else if pinned { c(t.accent).gamma_multiply(0.25) } else { c(t.surface).gamma_multiply(1.6) };
                            ui.painter().rect_filled(pill, 11.0, fill);
                            let ink = if b.hovered() { c(t.bg) } else { c(t.fg) };
                            ui.painter().text(pill.center(), egui::Align2::CENTER_CENTER, if pinned { "Unpin" } else { "Pin" }, FontId::proportional(12.0), ink);
                        }
                        if b.clicked() { if pinned { unpin = Some(id.clone()) } else { pin = Some(id.clone()) } }
                        else if r.clicked() { launch = Some(id.clone()); }
                        // Right-click: everything you can do with the app.
                        r.context_menu(|ui| {
                            if ui.button(if pinned { "Unpin from the bar" } else { "Pin to the bar" }).clicked() {
                                if pinned { unpin = Some(id.clone()) } else { pin = Some(id.clone()) }
                                ui.close();
                            }
                            if ui.button(if is_hidden { "Show in the launcher again" } else { "Hide from the launcher" }).clicked() { hide = Some((id.clone(), !is_hidden)); ui.close(); }
                            if !pins::is_app_id(id) && ui.button("Show in folder").clicked() { dock::show_in_folder(id); ui.close(); }
                        });
                    }
                });
                if n == 0 { ui.label(RichText::new(if searching { "No app matches." } else { "No apps to show." }).color(c(t.muted))); }
            }
        }
        if let Some(p) = pin { self.pin(&p); }
        if let Some(p) = unpin { if let Some(i) = self.settings.pinned.iter().position(|q| *q == p) { self.unpin(i); } }
        if let Some((id, on)) = hide {
            if on { if !self.settings.hidden_apps.contains(&id) { self.settings.hidden_apps.push(id); } } else { self.settings.hidden_apps.retain(|a| *a != id); }
            self.save();
        }
        if let Some(id) = launch { self.launch_app(&id); return }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Right-click an app to pin, hide or find it").color(c(t.muted)).size(11.0));
            });
        });
    }

    /// An on/off switch.
    fn switch(&self, ui: &mut egui::Ui, on: bool) -> egui::Response {
        let t = self.theme();
        let (r, resp) = ui.allocate_exact_size(vec2(38.0, 20.0), Sense::click());
        ui.painter().rect_filled(r, 10.0, if on { c(t.accent) } else { c(t.surface) });
        let x = if on { r.right() - 10.0 } else { r.left() + 10.0 };
        ui.painter().circle_filled(egui::pos2(x, r.center().y), 7.0, if on { c(t.bg) } else { c(t.muted) });
        resp
    }

    /// A small "back" arrow at the start of a popup's title row.
    fn back_button(&self, ui: &mut egui::Ui) -> bool {
        let t = self.theme();
        let (r, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
        if resp.hovered() { ui.painter().rect_filled(r, 7.0, c(t.surface)); }
        glyph::quick(ui.painter(), r.shrink(5.0), glyph::Quick::Back, c(t.fg));
        resp.clicked()
    }

    /// One quick settings tile: a rounded button (lit in the accent colour when on) with its name
    /// underneath. A tile with a page behind it shows an arrow; when it is also a switch, the arrow
    /// is a separate part of the tile. Returns clicks on the tile and on its arrow.
    fn qtile(&self, ui: &mut egui::Ui, g: glyph::Quick, label: &str, on: Option<bool>, more: bool) -> (bool, bool) {
        let t = self.theme();
        let (rect, _) = ui.allocate_exact_size(vec2(TILE_W, 76.0), Sense::hover());
        let bx = Rect::from_min_size(rect.min, vec2(TILE_W, 50.0));
        let lit = on == Some(true);
        let split = more && on.is_some();
        let cut = bx.right() - 34.0;
        let main_r = if split { Rect::from_min_max(bx.min, pos2(cut, bx.bottom())) } else { bx };
        let id = ui.scope_id().with("ronin-tile").with(label);
        let main = ui.interact(main_r, id.with("main"), Sense::click());
        let arrow = split.then(|| ui.interact(Rect::from_min_max(pos2(cut, bx.top()), bx.max), id.with("more"), Sense::click()));
        let p = ui.painter();
        let base = if lit { c(t.accent) } else { c(t.surface) };
        p.rect_filled(bx, 10.0, base);
        let glow = Color32::from_white_alpha(if t.dark { 18 } else { 40 });
        if main.hovered() { p.rect_filled(main_r, 10.0, glow); }
        if let Some(a) = &arrow { if a.hovered() { p.rect_filled(Rect::from_min_max(pos2(cut, bx.top()), bx.max), 10.0, glow); } }
        let ink = if lit { c(t.bg) } else { c(t.fg) };
        if split {
            p.line_segment([pos2(cut, bx.top() + 9.0), pos2(cut, bx.bottom() - 9.0)], Stroke::new(1.0, if lit { c(t.bg).gamma_multiply(0.5) } else { c(t.bg) }));
            glyph::quick(p, Rect::from_center_size(main_r.center(), Vec2::splat(20.0)), g, ink);
            glyph::quick(p, Rect::from_center_size(pos2(cut + 17.0, bx.center().y), Vec2::splat(16.0)), glyph::Quick::More, ink);
        } else if more {
            glyph::quick(p, Rect::from_center_size(bx.center() - vec2(8.0, 0.0), Vec2::splat(20.0)), g, ink);
            glyph::quick(p, Rect::from_center_size(pos2(bx.right() - 18.0, bx.center().y), Vec2::splat(14.0)), glyph::Quick::More, ink);
        } else {
            glyph::quick(p, Rect::from_center_size(bx.center(), Vec2::splat(20.0)), g, ink);
        }
        let mut name: String = label.chars().take(17).collect();
        if label.chars().count() > 17 { name.push('…'); }
        p.text(pos2(bx.center().x, bx.bottom() + 14.0), egui::Align2::CENTER_CENTER, name, FontId::proportional(12.0), c(t.fg));
        let main_click = main.on_hover_text(label).clicked();
        (main_click, arrow.map(|a| a.clicked()).unwrap_or(main_click && more && !split))
    }

    /// Ronin's quick settings: Wi-Fi, Bluetooth, airplane mode, night light and more, then
    /// brightness and volume, all without leaving the bar.
    fn pop_quick(&mut self, ui: &mut egui::Ui) {
        match self.quick_view {
            QuickView::Main => self.quick_main(ui),
            QuickView::Night => self.quick_night(ui),
            QuickView::Access => self.quick_access(ui),
            QuickView::Project => self.quick_project(ui),
        }
    }

    fn quick_main(&mut self, ui: &mut egui::Ui) {
        use glyph::Quick as Q;
        let t = self.theme();
        let st = self.net.lock().map(|s| s.clone()).unwrap_or_default();
        let s = self.stats.lock().map(|s| s.clone()).unwrap_or_default();
        let radios_known = st.wifi_on.is_some() || st.bluetooth_on.is_some();
        // (icon, name, on/off if it is a switch, has a page, click, arrow click)
        let mut tiles: Vec<(Q, String, Option<bool>, bool, QAct, QAct)> = vec![
            (Q::Airplane, "Airplane mode".into(), radios_known.then(|| st.airplane()), false, QAct::Airplane, QAct::Airplane),
            (Q::Night, "Night light".into(), Some(self.settings.night_light), true, QAct::Night, QAct::NightPage),
            (Q::Hotspot, "Mobile hotspot".into(), st.hotspot_on, st.hotspot_on.is_some(), if st.hotspot_on.is_some() { QAct::Hotspot } else { QAct::HotspotSettings }, QAct::HotspotSettings),
            (Q::Access, "Accessibility".into(), None, true, QAct::Access, QAct::Access),
            (Q::Project, "Project".into(), None, true, QAct::Project, QAct::Project),
            (Q::Cast, "Cast".into(), None, false, QAct::Cast, QAct::Cast),
        ];
        if sys::has_tool(sys::Tool::Captions) { tiles.push((Q::Captions, "Live captions".into(), None, false, QAct::Captions, QAct::Captions)); }
        tiles.push((Q::Share, "Nearby sharing".into(), None, false, QAct::Nearby, QAct::Nearby));
        tiles.push((Q::Battery, if s.battery.is_some() { "Power & battery".into() } else { "Power".into() }, None, true, QAct::Battery, QAct::Battery));
        tiles.push((Q::Snip, "Screen snip".into(), None, false, QAct::Snip, QAct::Snip));

        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            glyph::mark(ui.painter(), r, c(t.accent), c(t.fg));
            ui.label(RichText::new("Quick settings").strong().size(15.0));
        });
        ui.add_space(8.0);
        let mut act: Option<QAct> = None;
        for row in tiles.chunks(3) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = TILE_GAP;
                for (g, name, on, more, a, b) in row {
                    let (main, arrow) = self.qtile(ui, *g, name, *on, *more);
                    if arrow && *more { act = Some(*b) } else if main { act = Some(*a) }
                }
            });
        }
        if let Some(e) = st.error.as_ref().or(self.quick_note.as_ref()) { ui.label(RichText::new(e).color(c(t.accent)).size(12.0)); }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(6.0);
        let w = ui.available_width();
        ui.spacing_mut().slider_width = w - 40.0;
        if let Some(br) = s.brightness {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                glyph::quick(ui.painter(), r.shrink(3.0), Q::Sun, c(t.fg));
                self.brightness_slider(ui, br);
            });
            ui.add_space(4.0);
        }
        if let Some((v, m)) = s.volume {
            ui.horizontal(|ui| {
                let (r, mute) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                if mute.hovered() { ui.painter().rect_filled(r, 7.0, c(t.surface)); }
                glyph::volume(ui.painter(), r.shrink(3.0), v, m, c(if m { t.muted } else { t.fg }));
                if mute.on_hover_text(if m { "Sound on" } else { "Mute" }).clicked() { self.set_mute(!m); }
                self.volume_slider(ui, v);
            });
        }
        // Battery on the left, Ronin settings and power on the right.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                if let Some((b, charging)) = s.battery {
                    let (r, resp) = ui.allocate_exact_size(vec2(80.0, 28.0), Sense::click());
                    if resp.hovered() { ui.painter().rect_filled(r, 7.0, c(t.surface)); }
                    glyph::battery(ui.painter(), Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), Vec2::splat(22.0)), b, charging, c(t.fg), c(t.accent));
                    ui.painter().text(pos2(r.left() + 32.0, r.center().y), egui::Align2::LEFT_CENTER, format!("{b}%"), FontId::proportional(13.0), c(t.fg));
                    if resp.clicked() { act = Some(QAct::Battery); }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (r, pw) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                    if pw.hovered() { ui.painter().rect_filled(r, 7.0, c(t.surface)); }
                    glyph::draw(ui.painter(), r.shrink(5.0), Glyph::Power, c(t.accent));
                    if pw.on_hover_text("Power").clicked() { self.confirm = None; self.go(Popup::Power); }
                    let (r, gear) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                    if gear.hovered() { ui.painter().rect_filled(r, 7.0, c(t.surface)); }
                    glyph::draw(ui.painter(), r.shrink(5.0), Glyph::Gear, c(t.fg));
                    if gear.on_hover_text("Ronin settings").clicked() { self.open(Tab::Settings); }
                });
            });
        });
        if let Some(a) = act { self.quick_act(a, &st); }
    }

    fn quick_act(&mut self, a: QAct, st: &net::NetState) {
        self.quick_note = None;
        match a {
            QAct::Airplane => {
                let on = !st.airplane();
                if let Ok(mut s) = self.net.lock() {
                    s.hold = Some(Instant::now());
                    if on { s.wifi_on = s.wifi_on.map(|_| false); s.bluetooth_on = s.bluetooth_on.map(|_| false); } else { s.wifi_on = Some(true); }
                }
                let _ = self.net_tx.send(net::NetCmd::Airplane(on));
            }
            QAct::Night => self.set_night(!self.settings.night_light),
            QAct::NightPage => self.quick_view = QuickView::Night,
            QAct::Hotspot => {
                let on = st.hotspot_on != Some(true);
                if let Ok(mut s) = self.net.lock() { s.hotspot_on = Some(on); }
                let _ = self.net_tx.send(net::NetCmd::Hotspot(on));
            }
            QAct::HotspotSettings => sys::open_settings("network-mobilehotspot"),
            QAct::Access => { self.sticky = sys::sticky_keys(); self.quick_view = QuickView::Access; }
            QAct::Project => self.quick_view = QuickView::Project,
            QAct::Cast => { sys::open_uri(sys::Uri::Cast); self.close_popup(); }
            QAct::Captions => { sys::toggle_tool(sys::Tool::Captions); self.close_popup(); }
            QAct::Nearby => sys::open_settings("crossdevice"),
            QAct::Battery => self.go(Popup::Battery),
            QAct::Snip => { self.close_popup(); sys::open_uri(sys::Uri::Snip); }
        }
    }

    fn set_night(&mut self, on: bool) {
        self.settings.night_light = on;
        self.save();
        let _ = self.tx.send(worker::Cmd::Night(if on { self.settings.night_strength } else { 0 }));
    }

    /// The title row of a quick settings page, with a back arrow to the tiles.
    fn quick_header(&mut self, ui: &mut egui::Ui, title: &str) {
        let mut back = false;
        ui.horizontal(|ui| { back = self.back_button(ui); ui.label(RichText::new(title).strong().size(15.0)); });
        if back { self.quick_view = QuickView::Main; self.quick_note = None; }
        ui.add_space(10.0);
    }

    /// A row with a name on the left and a switch on the right.
    fn switch_row(&self, ui: &mut egui::Ui, label: &str, on: bool) -> bool {
        let mut clicked = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).size(14.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { clicked = self.switch(ui, on).clicked(); });
        });
        ui.add_space(6.0);
        clicked
    }

    fn quick_night(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        self.quick_header(ui, "Night light");
        ui.label(RichText::new("Warmer colours that are easier on the eyes at night.").color(c(t.muted)));
        ui.add_space(10.0);
        if self.switch_row(ui, "Night light", self.settings.night_light) { self.set_night(!self.settings.night_light); }
        ui.label(RichText::new("Strength").size(14.0));
        let mut v = f32::from(self.settings.night_strength);
        ui.spacing_mut().slider_width = ui.available_width();
        let r = ui.add(egui::Slider::new(&mut v, 10.0..=100.0).show_value(false));
        if r.changed() {
            self.settings.night_strength = v.round() as u8;
            if self.settings.night_light { let _ = self.tx.send(worker::Cmd::Night(self.settings.night_strength)); }
        }
        if r.drag_stopped() || (r.changed() && !r.dragged()) { self.save(); }
        ui.horizontal(|ui| {
            ui.label(RichText::new("cooler").color(c(t.muted)).size(11.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.label(RichText::new("warmer").color(c(t.muted)).size(11.0)); });
        });
    }

    fn quick_access(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        self.quick_header(ui, "Accessibility");
        if sys::has_tool(sys::Tool::Magnifier) && self.switch_row(ui, "Magnifier", sys::tool_running(sys::Tool::Magnifier)) { sys::toggle_tool(sys::Tool::Magnifier); }
        if sys::has_tool(sys::Tool::Keyboard) && self.switch_row(ui, "On-screen keyboard", sys::tool_running(sys::Tool::Keyboard)) { sys::toggle_tool(sys::Tool::Keyboard); }
        if self.switch_row(ui, "Sticky keys", self.sticky) { sys::set_sticky_keys(!self.sticky); self.sticky = sys::sticky_keys(); }
        for (tool, name) in [(sys::Tool::Narrator, "Narrator"), (sys::Tool::Captions, "Live captions")] {
            if !sys::has_tool(tool) { continue }
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.small_button("start").clicked() { sys::toggle_tool(tool); } });
            });
            ui.add_space(6.0);
        }
        ui.label(RichText::new("Narrator stops with Ctrl + Windows + Enter.").color(c(t.muted)).size(11.0));
        self.settings_link(ui, "Accessibility settings", "easeofaccess");
    }

    fn quick_project(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        self.quick_header(ui, "Project");
        ui.label(RichText::new("How to use a second screen.").color(c(t.muted)));
        ui.add_space(8.0);
        let w = ui.available_width();
        for (label, mode) in [("PC screen only", sys::Screens::PcOnly), ("Duplicate", sys::Screens::Duplicate), ("Extend", sys::Screens::Extend), ("Second screen only", sys::Screens::SecondOnly)] {
            if ui.add_sized([w, 38.0], egui::Button::new(RichText::new(label).size(14.0))).clicked() {
                self.quick_note = Some(if sys::set_screens(mode) { format!("{label}: done") } else { "Windows could not switch. Is a second screen connected?".into() });
            }
        }
        if let Some(n) = &self.quick_note { ui.add_space(6.0); ui.label(RichText::new(n).color(c(t.accent)).size(12.0)); }
    }

    /// Switches Wi-Fi or Bluetooth, showing the new state at once.
    fn radio(&mut self, bluetooth: bool, on: bool) {
        if let Ok(mut s) = self.net.lock() { s.hold = Some(Instant::now()); if bluetooth { s.bluetooth_on = Some(on) } else { s.wifi_on = Some(on) } }
        if bluetooth && !on { let _ = self.bt_tx.send(bt::BtCmd::StopScan); }
        let _ = self.net_tx.send(net::NetCmd::Radio { bluetooth, on });
    }

    fn pop_network(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        if self.net_scan_at.elapsed() > Duration::from_secs(10) {
            let _ = self.net_tx.send(net::NetCmd::Scan);
            self.net_scan_at = Instant::now();
        }
        let st = self.net.lock().map(|s| s.clone()).unwrap_or_default();
        let online = self.stats.lock().map(|s| s.online).unwrap_or(false);
        let mut back = false;
        ui.horizontal(|ui| {
            if self.from_quick { back = self.back_button(ui); }
            ui.label(RichText::new("Wi-Fi").strong().size(15.0));
            if st.scanning && st.networks.len() < 2 { ui.label(RichText::new("searching…").color(c(t.muted)).size(12.0)); }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(on) = st.wifi_on {
                    if self.switch(ui, on).clicked() { self.radio(false, !on); }
                }
            });
        });
        if back { self.popup = Some(Popup::Quick); return }
        if st.no_wifi {
            ui.label(RichText::new(if online { "No Wi-Fi here; connected by cable." } else { "No Wi-Fi adapter, and not connected." }).color(c(t.muted)));
        }
        if let Some(b) = &st.busy { ui.label(RichText::new(b).color(c(t.muted)).size(12.0)); }
        if let Some(e) = &st.error { ui.label(RichText::new(e).color(c(t.accent)).size(12.0)); }
        ui.add_space(4.0);
        let mut action: Option<net::NetCmd> = None;
        let mut open_join: Option<String> = None;
        egui::ScrollArea::vertical().max_height(340.0).auto_shrink([false, true]).show(ui, |ui| {
            if st.wifi_on == Some(false) { ui.label(RichText::new("Wi-Fi is off.").color(c(t.muted))); return }
            for n in &st.networks {
                ui.horizontal(|ui| {
                    // Signal bars.
                    let (r, _) = ui.allocate_exact_size(vec2(18.0, 16.0), Sense::hover());
                    let lit = ronin_core::wifi::bars(n.quality);
                    for i in 0..4u8 {
                        let h = 4.0 + f32::from(i) * 3.5;
                        let bar = Rect::from_min_size(egui::pos2(r.left() + f32::from(i) * 4.5, r.bottom() - h), vec2(3.0, h));
                        ui.painter().rect_filled(bar, 1.0, if i < lit.max(1) { c(t.fg) } else { c(t.surface) });
                    }
                    let name = RichText::new(&n.ssid).strong();
                    ui.label(if n.connected { name.color(c(t.accent)) } else { name });
                    if n.security != ronin_core::wifi::Security::Open { ui.label(RichText::new("secured").color(c(t.muted)).size(11.0)); }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if n.connected {
                            if ui.small_button("disconnect").clicked() { action = Some(net::NetCmd::Disconnect); }
                        } else if n.security == ronin_core::wifi::Security::Other && !n.known {
                            if ui.small_button("in Windows").clicked() { sys::open_settings("network-wifi"); }
                        } else if ui.small_button("connect").clicked() {
                            if n.known || n.security == ronin_core::wifi::Security::Open {
                                action = Some(net::NetCmd::Connect { ssid: n.ssid.clone(), password: String::new() });
                            } else {
                                open_join = Some(n.ssid.clone());
                            }
                        }
                    });
                });
                // Password box under a new secured network.
                if self.joining.as_ref().map(|(s, _)| s == &n.ssid).unwrap_or(false) {
                    let mut go = false;
                    ui.horizontal(|ui| {
                        if let Some((_, pw)) = self.joining.as_mut() {
                            let r = ui.add(egui::TextEdit::singleline(pw).password(true).hint_text("password").desired_width(200.0));
                            r.request_focus();
                            go = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        }
                        if ui.small_button("join").clicked() { go = true; }
                        if ui.small_button("cancel").clicked() { self.joining = None; }
                    });
                    if go {
                        if let Some((ssid, pw)) = self.joining.take() {
                            if ronin_core::wifi::valid_password(n.security, &pw) { action = Some(net::NetCmd::Connect { ssid, password: pw }); }
                            else { self.joining = Some((ssid, pw)); }
                        }
                    }
                }
                ui.add_space(2.0);
            }
            if st.networks.is_empty() && !st.no_wifi { ui.label(RichText::new("Looking for networks…").color(c(t.muted))); }
        });
        if let Some(ssid) = open_join { self.joining = Some((ssid, String::new())); }
        if let Some(a) = action { let _ = self.net_tx.send(a); }
        self.settings_link(ui, "Network settings", "network-status");
    }

    /// Ronin's Bluetooth popup, laid out like the Wi-Fi one: on/off, your devices with connect /
    /// disconnect, and a search for new ones to pair.
    fn pop_bluetooth(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let radio = self.net.lock().map(|s| s.bluetooth_on).unwrap_or(None);
        let st = self.bt.lock().map(|s| s.clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Bluetooth").strong().size(15.0));
            if st.scanning && radio == Some(true) { ui.label(RichText::new("searching…").color(c(t.muted)).size(12.0)); }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(on) = radio { if self.switch(ui, on).clicked() { self.radio(true, !on); } }
            });
        });
        if let Some(b) = &st.busy { ui.label(RichText::new(b).color(c(t.muted)).size(12.0)); }
        if let Some(e) = &st.error { ui.label(RichText::new(e).color(c(t.accent)).size(12.0)); }
        ui.add_space(4.0);
        let mut cmd: Option<bt::BtCmd> = None;
        match radio {
            None => { ui.label(RichText::new("This PC has no Bluetooth.").color(c(t.muted))); }
            Some(false) => { ui.label(RichText::new("Bluetooth is off.").color(c(t.muted))); }
            Some(true) => {
                egui::ScrollArea::vertical().max_height(330.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.label(RichText::new("YOUR DEVICES").color(c(t.accent)).size(11.0));
                    match &st.paired {
                        None => { ui.label(RichText::new("Reading your devices…").color(c(t.muted))); }
                        Some(l) if l.is_empty() => { ui.label(RichText::new("No devices paired yet. Search below to add one.").color(c(t.muted))); }
                        Some(l) => for d in l {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                                glyph::quick(ui.painter(), r, glyph::Quick::Bluetooth, if d.connected { c(t.accent) } else { c(t.muted) });
                                let name = RichText::new(&d.name).strong();
                                ui.label(if d.connected { name.color(c(t.accent)) } else { name });
                                if d.connected { ui.label(RichText::new("connected").color(c(t.muted)).size(11.0)); }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.add(egui::Button::new(RichText::new("forget").size(11.0).color(c(t.muted))).frame(false)).on_hover_text("Remove this device from the PC").clicked() { cmd = Some(bt::BtCmd::Forget(d.id.clone())); }
                                    match (&d.addr, d.audio, d.connected) {
                                        (Some(a), true, true) => if ui.small_button("disconnect").clicked() { cmd = Some(bt::BtCmd::Disconnect(a.clone())); },
                                        (Some(a), true, false) => if ui.small_button("connect").clicked() { cmd = Some(bt::BtCmd::Connect(a.clone())); },
                                        // Mice, keyboards and phones connect by themselves when they are switched on.
                                        _ => if !d.connected { ui.label(RichText::new("turn it on to connect").color(c(t.muted)).size(11.0)); },
                                    }
                                });
                            });
                            ui.add_space(2.0);
                        },
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("NEW DEVICES").color(c(t.accent)).size(11.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(!st.scanning, egui::Button::new(if st.nearby.is_some() { "search again" } else { "search" }).small()).clicked() { cmd = Some(bt::BtCmd::Scan); }
                        });
                    });
                    match &st.nearby {
                        None if !st.scanning => { ui.label(RichText::new("Put your device in pairing mode, then search.").color(c(t.muted)).size(12.0)); }
                        None => { ui.label(RichText::new("Looking for devices nearby…").color(c(t.muted)).size(12.0)); }
                        Some(l) if l.is_empty() && !st.scanning => { ui.label(RichText::new("Nothing found. Is the device in pairing mode?").color(c(t.muted)).size(12.0)); }
                        Some(l) => for d in l {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                                glyph::quick(ui.painter(), r, glyph::Quick::Bluetooth, c(t.muted));
                                ui.label(&d.name);
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("pair").clicked() { cmd = Some(bt::BtCmd::Pair(d.id.clone())); }
                                });
                            });
                        },
                    }
                });
            }
        }
        if let Some(c) = cmd {
            // Shown at once; the device confirms a moment later.
            if let Ok(mut s) = self.bt.lock() {
                match &c {
                    bt::BtCmd::Connect(a) | bt::BtCmd::Disconnect(a) => {
                        let on = matches!(c, bt::BtCmd::Connect(_));
                        s.busy = Some(if on { "Connecting…".into() } else { "Disconnecting…".into() });
                        s.error = None;
                        if let Some(d) = s.paired.as_mut().and_then(|l| l.iter_mut().find(|d| d.addr.as_deref() == Some(a.as_str()))) { d.connected = on; }
                    }
                    bt::BtCmd::Scan => s.scanning = true,
                    _ => {}
                }
            }
            let _ = self.bt_tx.send(c);
        }
        self.settings_link(ui, "Bluetooth settings", "bluetooth");
    }

    fn pop_battery(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let s = self.stats.lock().map(|s| s.clone()).unwrap_or_default();
        if self.from_quick {
            let mut back = false;
            ui.horizontal(|ui| { back = self.back_button(ui); ui.label(RichText::new("Power & battery").strong().size(15.0)); });
            if back { self.popup = Some(Popup::Quick); return }
            ui.add_space(6.0);
        }
        if let Some((b, charging)) = s.battery {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(44.0, 30.0), Sense::hover());
                glyph::battery(ui.painter(), r, b, charging, c(t.fg), c(t.accent));
                ui.label(RichText::new(format!("{b}%")).size(30.0).strong());
            });
            let status = match (charging, s.battery_left) {
                (true, _) if b >= 100 => "Plugged in, fully charged".to_string(),
                (true, _) => "Charging".to_string(),
                (false, Some(m)) if m >= 60 => format!("On battery · about {} h {} min left", m / 60, m % 60),
                (false, Some(m)) => format!("On battery · about {m} min left"),
                (false, None) => "On battery".to_string(),
            };
            ui.label(RichText::new(status).color(c(t.muted)));
            if s.saver { ui.label(RichText::new("Battery saver is on").color(c(t.accent)).size(12.0)); }
        } else {
            ui.label(RichText::new("No battery: this PC runs on mains power.").color(c(t.muted)));
        }
        ui.add_space(12.0);
        match s.brightness {
            Some(br) => {
                ui.label(RichText::new(format!("Screen brightness  {:.0}%", self.bright_drag.unwrap_or(f32::from(br)))).strong());
                ui.spacing_mut().slider_width = ui.available_width();
                self.brightness_slider(ui, br);
            }
            None => { ui.label(RichText::new("Brightness of this screen is set on the monitor itself.").color(c(t.muted)).size(12.0)); }
        }
        self.settings_link(ui, "Power & battery settings", "batterysaver");
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
        // Each page has its own button on the bar (picture, half circle, gear), so the panel shows
        // just that page, with its name, instead of a row of tabs repeating those buttons.
        ui.add_space(6.0);
        let title = match self.tab { Tab::Wallpapers => "Wallpapers", Tab::Themes => "Themes", Tab::Settings => "Settings" };
        ui.label(RichText::new(title).strong().size(17.0).color(c(t.accent)));
        ui.separator();
        match self.tab {
            Tab::Wallpapers => self.tab_wallpapers(ui),
            Tab::Themes => self.tab_themes(ui),
            Tab::Settings => {
                let mut on = self.settings.hide_taskbar;
                if ui.checkbox(&mut on, "Replace the Windows taskbar").changed() {
                    self.settings.hide_taskbar = on;
                    self.save();
                    if on { self.hide_taskbar(); } else { self.show_taskbar(); }
                }
                ui.label(RichText::new("Hides the Windows taskbar while Ronin Bar runs; it comes back when you quit or uninstall. The Ronin mark opens your apps, the clock the calendar, and the icons on the right Wi-Fi, Bluetooth, quick settings, battery and sound.").color(c(t.muted)).size(12.0));
                ui.add_space(6.0);
                let mut start = self.settings.start_with_windows;
                if ui.checkbox(&mut start, "Start with Windows").changed() {
                    self.settings.start_with_windows = start;
                    install::set_start_with_windows(start);
                    self.save();
                }
                ui.add_space(12.0);
                ui.label(RichText::new("UPDATES").color(c(t.accent)).size(11.0));
                ui.horizontal(|ui| {
                    ui.label(format!("Ronin Bar {}", env!("CARGO_PKG_VERSION")));
                    if ui.button("Check for updates").clicked() { self.upd_at = Instant::now(); update::check(self.upd.clone(), self.ctx.clone()); }
                });
                self.update_status(ui);
                self.update_switches(ui);
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

    /// The popup window. It is made once (hidden until needed) and reused by every popup, so a
    /// click on the bar shows it at once instead of building a new window each time.
    fn popup_window(&mut self, ctx: &egui::Context) {
        if let Some(p) = self.popup { self.popup_size = popup_size(p); }
        let (w, h) = self.popup_size;
        let x = (self.popup_x - w / 2.0).clamp(8.0, (self.screen_w - w - 8.0).max(8.0));
        let builder = egui::ViewportBuilder::default()
            .with_title(POPUP_TITLE)
            .with_decorations(false).with_taskbar(false).with_always_on_top().with_resizable(false)
            .with_visible(self.popup.is_some())
            .with_inner_size([w, h]).with_position(egui::pos2(x, BAR_HEIGHT + 6.0));
        let id = egui::ViewportId::from_hash_of("ronin-popup");
        self.popup_live = true;
        let fresh = self.popup.is_some() && self.popup_at.elapsed() < Duration::from_millis(120);
        let close = ctx.show_viewport_immediate(id, builder, |ui, _class| {
            let t = self.theme();
            let Some(p) = self.popup else {
                ui.painter().rect_filled(ui.max_rect(), 0.0, c(t.bg));
                return false
            };
            egui::Frame::new().fill(c(t.bg)).stroke(Stroke::new(1.0, c(t.accent).gamma_multiply(0.5))).inner_margin(16.0).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                match p { Popup::Launcher => self.pop_launcher(ui), Popup::Calendar => self.pop_calendar(ui), Popup::Sound => self.pop_sound(ui), Popup::Power => self.pop_power(ui), Popup::Battery => self.pop_battery(ui), Popup::Network => self.pop_network(ui), Popup::Bluetooth => self.pop_bluetooth(ui), Popup::Quick => self.pop_quick(ui), Popup::Update => self.pop_update(ui) }
            });
            // Closes on Escape, or when you click anywhere else, like a desktop panel's menus.
            ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)
                || (i.viewport().focused == Some(false) && !fresh && self.popup_at.elapsed() > Duration::from_millis(400)))
        });
        if fresh { ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus); }
        if close && self.popup.is_some() { self.close_popup(); }
    }

    /// Looks for new versions now and then; shows the popup once per new version, downloads it,
    /// and installs it when asked to (or by itself, if that is switched on).
    fn updates(&mut self, fullscreen: bool) {
        if self.upd_at.elapsed() >= UPDATE_EVERY {
            self.upd_at = Instant::now();
            update::check(self.upd.clone(), self.ctx.clone());
        }
        let (latest, ready) = match self.upd.lock() { Ok(u) => (u.latest.as_ref().map(|r| r.version), u.ready.clone()), Err(_) => return };
        let Some(v) = latest else { return };
        if self.upd_seen != Some(v) {
            self.upd_seen = Some(v);
            if self.settings.auto_update { self.upd_install = true; update::fetch(self.upd.clone(), self.ctx.clone()); }
            else if self.settings.update_popup && self.popup.is_none() && !self.panel && !fullscreen { self.toggle_popup(Popup::Update, self.screen_w - 210.0); }
        }
        if let Some(path) = ready {
            if self.upd_install && !fullscreen {
                self.upd_install = false;
                match update::apply(&path) {
                    Ok(()) => self.quit(),
                    Err(e) => if let Ok(mut u) = self.upd.lock() { u.error = Some(e); u.ready = None; },
                }
            }
        }
    }

    fn install_update(&mut self) {
        self.upd_install = true;
        update::fetch(self.upd.clone(), self.ctx.clone());
    }

    /// The update switches, in the update popup and in Settings.
    fn update_switches(&mut self, ui: &mut egui::Ui) {
        let mut pop = self.settings.update_popup;
        if ui.checkbox(&mut pop, "Tell me when a new version is out").changed() { self.settings.update_popup = pop; self.save(); }
        let mut auto = self.settings.auto_update;
        if ui.checkbox(&mut auto, "Install new versions by themselves").changed() { self.settings.auto_update = auto; self.save(); }
    }

    /// Where an update stands, with the button to install it.
    fn update_status(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let u = self.upd.lock().map(|u| u.clone()).unwrap_or_default();
        if let Some((got, total)) = u.downloading {
            let frac = if total > 0 { got as f32 / total as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(frac).desired_width(ui.available_width()).text(format!("Downloading… {:.1} MB", got as f32 / 1_048_576.0)));
        } else if u.ready.is_some() {
            ui.label(RichText::new("Installing…").color(c(t.accent)));
        } else if let Some(r) = &u.latest {
            if ui.button(RichText::new(format!("Install Ronin Bar {}", r.version)).strong()).clicked() { self.install_update(); }
        } else if u.checking {
            ui.label(RichText::new("Looking for a new version…").color(c(t.muted)));
        } else if u.up_to_date {
            ui.label(RichText::new("You have the newest version.").color(c(t.muted)));
        }
        if let Some(e) = &u.error { ui.label(RichText::new(e).color(c(t.accent)).size(12.0)); }
    }

    fn pop_update(&mut self, ui: &mut egui::Ui) {
        let t = self.theme();
        let rel = self.upd.lock().ok().and_then(|u| u.latest.clone());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
            glyph::mark(ui.painter(), r, c(t.accent), c(t.fg));
            ui.label(RichText::new("New version available").strong().size(15.0));
        });
        ui.add_space(8.0);
        let Some(rel) = rel else {
            ui.label(RichText::new(format!("You have Ronin Bar {}, the newest version.", env!("CARGO_PKG_VERSION"))).color(c(t.muted)));
            return
        };
        ui.label(RichText::new(format!("Ronin Bar {} is out. You have {}.", rel.version, env!("CARGO_PKG_VERSION"))).size(14.0));
        ui.add_space(4.0);
        ui.label(RichText::new("Installing replaces this version and removes it; your settings, pins and wallpapers stay. The bar restarts in a second.").color(c(t.muted)).size(12.0));
        // Release notes, without the download instructions every release repeats.
        let notes: Vec<String> = rel.notes.lines().map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---") && !l.starts_with("Download") && !l.starts_with("Uninstall") && !l.starts_with("SHA-256") && !l.starts_with("Updated build") && !l.starts_with("Already running"))
            .map(|l| match l.strip_prefix("- ") { Some(b) => format!("•  {b}"), None => l.replace("**", "") })
            .collect();
        if !notes.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("WHAT'S NEW").color(c(t.accent)).size(11.0));
            egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| { for l in notes { ui.label(RichText::new(l).size(12.0)); } });
        }
        ui.add_space(8.0);
        self.update_status(ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Later").clicked() { self.close_popup(); }
            if ui.add(egui::Button::new(RichText::new("What's new").color(c(t.muted))).frame(false)).clicked() { sys::open_releases_page(); }
        });
        ui.add_space(6.0);
        self.update_switches(ui);
    }

    /// Gives back the top strip and the taskbar.
    fn release(&mut self) {
        if let Some(h) = self.hwnd.take() {
            sys::appbar_remove(h);
            sys::release_top(self.bar_px);
        }
        self.show_taskbar();
        if self.settings.night_light { sys::night_light(0); }
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
        if self.styled != self.theme().id { ctx.set_visuals(visuals(self.theme())); self.styled = self.theme().id; }
        // Background threads wake the bar when something changes; this only keeps the clock going.
        ctx.request_repaint_after(Duration::from_millis(if self.popup.is_some() { 250 } else { 1000 }));
        self.dock(&ctx);
        if self.guard_at.elapsed() >= Duration::from_secs(1) { self.follow_screen(&ctx); }
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
        if self.popup.is_some() || self.popup_live { self.popup_window(&ctx); }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) { self.release(); }
}
