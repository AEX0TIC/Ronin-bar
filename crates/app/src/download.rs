//! Downloads wallpaper previews from Wallhaven in the background, each full picture when it is
//! chosen, and draws the built-in Ronin wallpapers.
use crate::store::{write_atomic, Store};
use ronin_core::wallhaven::{self, Wallpaper, COLLECTIONS, MAX_BYTES, PER_COLLECTION};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub error: Option<String>,
}

pub fn agent() -> ureq::Agent {
    base().timeout_global(Some(Duration::from_secs(60))).build().into()
}

/// For the update download: quick to notice a dead connection, but never cuts off a slow one
/// that is still bringing data.
pub fn update_agent() -> ureq::Agent {
    base()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(15 * 60)))
        .build()
        .into()
}

fn base() -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    // Windows' own certificate store (the default, a bundled list, does not work with native-tls on Windows).
    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(true)
        .user_agent(concat!("RoninBar/", env!("CARGO_PKG_VERSION"), " (+https://github.com/AEX0TIC/Ronin-bar)"))
}

/// wallhaven.cc's search allows 45 requests a minute; one every 1.5 s stays well under that.
/// Only the background refresh of the collections is paced: a picture you click downloads at once.
const PAUSE: Duration = Duration::from_millis(1500);

/// Downloads `url` after waiting `pause` (zero for downloads you asked for), and waits it out
/// (up to 3 times) when the site says "too many requests" or is briefly unavailable.
fn get(agent: &ureq::Agent, url: &str, limit: u64, pause: Duration) -> Result<Vec<u8>, String> {
    let mut wait = Duration::from_secs(5);
    for attempt in 0.. {
        if !pause.is_zero() { std::thread::sleep(pause); }
        match agent.get(url).call() {
            Ok(mut r) => return r.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| format!("{url}: {e}")),
            Err(ureq::Error::StatusCode(code)) if (code == 429 || code >= 500) && attempt < 3 => {
                std::thread::sleep(wait);
                wait *= 2;
            }
            Err(e) => return Err(format!("{url}: {e}")),
        }
    }
    unreachable!()
}

/// Brings every collection up to date with Wallhaven's current favourites: previews of new ones are
/// downloaded, and ones that dropped out are removed (except `keep`, the wallpaper in use).
pub fn run(store: Arc<Store>, catalog: Arc<Mutex<Vec<Wallpaper>>>, progress: Arc<Mutex<Progress>>, keep: String, wake: eframe::egui::Context) {
    std::thread::spawn(move || {
        let set = |f: &dyn Fn(&mut Progress)| { if let Ok(mut p) = progress.lock() { f(&mut p); } wake.request_repaint(); };
        set(&|p| { p.running = true; p.error = None; p.done = 0; p.total = COLLECTIONS.len() * PER_COLLECTION; });
        let agent = agent();
        let mut errors = Vec::new();
        for c in COLLECTIONS {
            let found = match get(&agent, &wallhaven::search_url(&c), 2 * 1024 * 1024, PAUSE) {
                Ok(b) => wallhaven::parse_search(&String::from_utf8_lossy(&b), c.name),
                Err(e) => { errors.push(e); continue }
            };
            if found.is_empty() { continue }
            // Drop this collection's wallpapers that are no longer among the favourites.
            if let Ok(mut l) = catalog.lock() {
                let (gone, rest): (Vec<Wallpaper>, Vec<Wallpaper>) = l.drain(..)
                    .partition(|w| w.collection == c.name && w.id != keep && !found.iter().any(|f| f.id == w.id));
                *l = rest;
                for w in gone { let _ = std::fs::remove_file(store.thumb_file(&w)); let _ = std::fs::remove_file(store.wallpaper_file(&w)); }
                store.save_catalog(&l);
            }
            for w in found {
                let have = catalog.lock().map(|l| l.iter().any(|x| x.id == w.id)).unwrap_or(false);
                if !have {
                    match fetch_preview(&agent, &store, &w) {
                        Ok(()) => if let Ok(mut l) = catalog.lock() { l.push(w); store.save_catalog(&l); },
                        Err(e) => errors.push(e),
                    }
                }
                set(&|p| p.done += 1);
            }
        }
        let err = (!errors.is_empty()).then(|| format!("{} could not be downloaded (first: {})", errors.len(), errors[0]));
        set(&|p| { p.running = false; p.error = err.clone(); });
    });
}

fn fetch_preview(agent: &ureq::Agent, store: &Store, w: &Wallpaper) -> Result<(), String> {
    // Previews come from Wallhaven's picture server, which is not rate-limited like its search.
    let thumb = get(agent, &w.thumb, 2 * 1024 * 1024, Duration::from_millis(150))?;
    if !wallhaven::is_image(&thumb) { return Err(format!("{}: preview is not an image", w.id)) }
    write_atomic(&store.thumb_file(w), &thumb).map_err(|e| e.to_string())
}

/// What a full-picture download ended with: the wallpaper id, and an error if it failed.
pub type Fetched = Arc<Mutex<Option<(String, Result<(), String>)>>>;

/// Downloads one wallpaper's full picture in the background, straight away, and wakes the bar
/// the moment it is in.
pub fn full(store: Arc<Store>, w: Wallpaper, done: Fetched, wake: eframe::egui::Context) {
    std::thread::spawn(move || {
        let r = (|| {
            let bytes = get(&agent(), &w.url, MAX_BYTES as u64 + 1, Duration::ZERO)?;
            if !wallhaven::is_image(&bytes) { return Err(format!("{}: not a JPEG/PNG image", w.id)) }
            write_atomic(&store.wallpaper_file(&w), &bytes).map_err(|e| e.to_string())
        })();
        if let Ok(mut d) = done.lock() { *d = Some((w.id, r)); }
        wake.request_repaint();
    });
}

/// The built-in Ronin wallpapers (one per theme), ready in `ready` as they are drawn (the current theme's first).
/// They are drawn at the screen's resolution once, then reused.
pub fn originals(store: Arc<Store>, screen: (u32, u32), first_theme: String, ready: Arc<Mutex<Vec<Wallpaper>>>, wake: eframe::egui::Context) {
    use ronin_core::{art, theme::THEMES};
    std::thread::spawn(move || {
        // Wallpapers drawn by an older version of the drawing code are drawn again.
        let marker = store.data.join("wallpapers").join("ronin-art.txt");
        let fresh = std::fs::read_to_string(&marker).ok().map(|v| v.trim() == art::ART_VERSION.to_string()).unwrap_or(false);
        let mut order: Vec<usize> = (0..THEMES.len()).collect();
        order.sort_by_key(|&i| THEMES[i].id != first_theme);
        for i in order {
            let t = &THEMES[i];
            let id = art::ORIGINAL_IDS[i];
            let w = Wallpaper {
                id: id.to_string(), collection: art::ORIGINALS_NAME.to_string(), url: format!("{id}.png"), thumb: String::new(),
                page: String::new(), resolution: format!("{}x{}", screen.0, screen.1),
            };
            let have = fresh && store.wallpaper_file(&w).is_file() && store.thumb_file(&w).is_file();
            if !have {
                let save = |path: std::path::PathBuf, wd: u32, ht: u32, fmt: image::ImageFormat| -> Option<()> {
                    let img = image::RgbImage::from_raw(wd, ht, art::wallpaper(t, wd, ht))?;
                    let tmp = path.with_extension("tmp");
                    std::fs::create_dir_all(path.parent()?).ok()?;
                    img.save_with_format(&tmp, fmt).ok()?;
                    std::fs::rename(&tmp, &path).ok()
                };
                if save(store.thumb_file(&w), 244, 138, image::ImageFormat::Jpeg).is_none()
                    || save(store.wallpaper_file(&w), screen.0, screen.1, image::ImageFormat::Png).is_none() { continue }
            }
            if let Ok(mut l) = ready.lock() { if !l.iter().any(|x| x.id == w.id) { l.push(w); l.sort_by(|a, b| a.id.cmp(&b.id)); } }
            wake.request_repaint();
        }
        if !fresh { let _ = write_atomic(&marker, art::ART_VERSION.to_string().as_bytes()); }
    });
}

/// Pictures downloaded per click on a category's "get" button.
pub const BATCH: usize = 24;

/// How far a GitHub pack's download is.
#[derive(Clone, Debug, Default)]
pub struct PackProgress {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub error: Option<String>,
}
pub type PackStates = Arc<Mutex<std::collections::HashMap<String, PackProgress>>>;

/// Pictures of a category downloaded at the same time.
const WORKERS: usize = 4;

/// Downloads the next `BATCH` pictures of a GitHub category that are not here yet (pinned files,
/// checked to be real images), 4 at a time, and makes their previews here; each joins `ready`
/// and shows in the panel the moment it is in.
pub fn pack(store: Arc<Store>, pack: &'static ronin_core::github::Pack, ready: Arc<Mutex<Vec<Wallpaper>>>, states: PackStates, wake: eframe::egui::Context) {
    std::thread::spawn(move || {
        let list: Vec<Wallpaper> = ronin_core::github::wallpapers(pack).into_iter()
            .filter(|w| !(store.wallpaper_file(w).is_file() && store.thumb_file(w).is_file()))
            .take(BATCH).collect();
        let set = |f: &dyn Fn(&mut PackProgress)| {
            if let Ok(mut m) = states.lock() { f(m.entry(pack.name.to_string()).or_default()); }
            wake.request_repaint();
        };
        set(&|p| { p.running = true; p.done = 0; p.total = list.len(); p.error = None; });
        let queue = Arc::new(Mutex::new(list));
        let errors = Arc::new(Mutex::new(Vec::<String>::new()));
        std::thread::scope(|sc| {
            for _ in 0..WORKERS {
                let (queue, errors, store, ready, set) = (queue.clone(), errors.clone(), &store, &ready, &set);
                sc.spawn(move || {
                    let agent = agent();
                    // The lock is let go inside `and_then`, so the four downloads really run side by side.
                    while let Some(w) = queue.lock().ok().and_then(|mut q| q.pop()) {
                        let r = (|| -> Result<(), String> {
                            // GitHub serves these files without a rate limit: no waiting between them.
                            let bytes = get(&agent, &w.url, MAX_BYTES as u64 + 1, Duration::ZERO)?;
                            if !wallhaven::is_image(&bytes) { return Err(format!("{}: not a JPEG/PNG image", w.id)) }
                            let img = image::load_from_memory(&bytes).map_err(|e| format!("{}: {e}", w.id))?;
                            let thumb = img.thumbnail(320, 180).into_rgb8();
                            let tmp = store.thumb_file(&w).with_extension("tmp");
                            std::fs::create_dir_all(tmp.parent().ok_or("no folder")?).map_err(|e| e.to_string())?;
                            thumb.save_with_format(&tmp, image::ImageFormat::Jpeg).map_err(|e| e.to_string())?;
                            write_atomic(&store.wallpaper_file(&w), &bytes).map_err(|e| e.to_string())?;
                            std::fs::rename(&tmp, store.thumb_file(&w)).map_err(|e| e.to_string())
                        })();
                        match r {
                            Ok(()) => if let Ok(mut l) = ready.lock() { if !l.iter().any(|x| x.id == w.id) { l.push(w); } },
                            Err(e) => if let Ok(mut l) = errors.lock() { l.push(e); },
                        }
                        set(&|p| p.done += 1);
                    }
                });
            }
        });
        let errors = errors.lock().map(|e| e.clone()).unwrap_or_default();
        let err = (!errors.is_empty()).then(|| format!("{} could not be downloaded (first: {})", errors.len(), errors[0]));
        set(&|p| { p.running = false; p.error = err.clone(); });
    });
}
