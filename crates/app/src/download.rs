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

fn agent() -> ureq::Agent {
    // Windows' own certificate store (the default, a bundled list, does not work with native-tls on Windows).
    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent("RoninBar/0.2 (+https://github.com/AEX0TIC/Ronin-bar)")
        .build()
        .into()
}

/// wallhaven.cc allows 45 requests a minute; one every 1.5 s stays well under that.
const PAUSE: Duration = Duration::from_millis(1500);

/// Downloads `url`, pacing requests and waiting it out (up to 3 times) when the site says
/// "too many requests" or is briefly unavailable.
fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut wait = Duration::from_secs(15);
    for attempt in 0.. {
        std::thread::sleep(PAUSE);
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
pub fn run(store: Arc<Store>, catalog: Arc<Mutex<Vec<Wallpaper>>>, progress: Arc<Mutex<Progress>>, keep: String) {
    std::thread::spawn(move || {
        let set = |f: &dyn Fn(&mut Progress)| { if let Ok(mut p) = progress.lock() { f(&mut p); } };
        set(&|p| { p.running = true; p.error = None; p.done = 0; p.total = COLLECTIONS.len() * PER_COLLECTION; });
        let agent = agent();
        let mut errors = Vec::new();
        for c in COLLECTIONS {
            let found = match get(&agent, &wallhaven::search_url(&c), 2 * 1024 * 1024) {
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
    let thumb = get(agent, &w.thumb, 2 * 1024 * 1024)?;
    if !wallhaven::is_image(&thumb) { return Err(format!("{}: preview is not an image", w.id)) }
    write_atomic(&store.thumb_file(w), &thumb).map_err(|e| e.to_string())
}

/// What a full-picture download ended with: the wallpaper id, and an error if it failed.
pub type Fetched = Arc<Mutex<Option<(String, Result<(), String>)>>>;

/// Downloads one wallpaper's full picture in the background.
pub fn full(store: Arc<Store>, w: Wallpaper, done: Fetched) {
    std::thread::spawn(move || {
        let r = (|| {
            let bytes = get(&agent(), &w.url, MAX_BYTES as u64 + 1)?;
            if !wallhaven::is_image(&bytes) { return Err(format!("{}: not a JPEG/PNG image", w.id)) }
            write_atomic(&store.wallpaper_file(&w), &bytes).map_err(|e| e.to_string())
        })();
        if let Ok(mut d) = done.lock() { *d = Some((w.id, r)); }
    });
}

/// The built-in Ronin wallpapers (one per theme), ready in `ready` as they are drawn (the current theme's first).
/// They are drawn at the screen's resolution once, then reused.
pub fn originals(store: Arc<Store>, screen: (u32, u32), first_theme: String, ready: Arc<Mutex<Vec<Wallpaper>>>) {
    use ronin_core::{art, theme::THEMES};
    std::thread::spawn(move || {
        let mut order: Vec<usize> = (0..THEMES.len()).collect();
        order.sort_by_key(|&i| THEMES[i].id != first_theme);
        for i in order {
            let t = &THEMES[i];
            let id = art::ORIGINAL_IDS[i];
            let w = Wallpaper {
                id: id.to_string(), collection: art::ORIGINALS_NAME.to_string(), url: format!("{id}.png"), thumb: String::new(),
                page: String::new(), resolution: format!("{}x{}", screen.0, screen.1),
            };
            let have = store.wallpaper_file(&w).is_file() && store.thumb_file(&w).is_file();
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
        }
    });
}
