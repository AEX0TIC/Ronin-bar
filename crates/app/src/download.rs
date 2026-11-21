//! Downloads the 30 wallpapers (and small previews) from Wallhaven in the background.
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
    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent("RoninBar/0.1 (+https://github.com/AEX0TIC/Ronin-bar)")
        .build()
        .into()
}

fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut r = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    r.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| format!("{url}: {e}"))
}

/// Fetches whatever is missing from the 30, adding each finished wallpaper to `catalog`.
pub fn run(store: Arc<Store>, catalog: Arc<Mutex<Vec<Wallpaper>>>, progress: Arc<Mutex<Progress>>) {
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
            for w in found {
                let have = catalog.lock().map(|l| l.iter().any(|x| x.id == w.id)).unwrap_or(false);
                if !have {
                    match fetch_one(&agent, &store, &w) {
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

fn fetch_one(agent: &ureq::Agent, store: &Store, w: &Wallpaper) -> Result<(), String> {
    let full = get(agent, &w.url, MAX_BYTES as u64 + 1)?;
    if !wallhaven::is_image(&full) { return Err(format!("{}: not a JPEG/PNG image", w.id)) }
    let thumb = get(agent, &w.thumb, 2 * 1024 * 1024)?;
    if !wallhaven::is_image(&thumb) { return Err(format!("{}: preview is not an image", w.id)) }
    write_atomic(&store.thumb_file(w), &thumb).map_err(|e| e.to_string())?;
    write_atomic(&store.wallpaper_file(w), &full).map_err(|e| e.to_string())?;
    Ok(())
}
