//! Updates from GitHub, inside the app: it looks for a new release now and then, downloads the
//! new program, checks it against the release's SHA-256, puts it in place of the old one, starts
//! it and removes the old copy. Only this repository's release files are accepted.
use crate::{download, install};
use ronin_core::update::{self, Release};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub struct State {
    pub checking: bool,
    /// A newer release, once found.
    pub latest: Option<Release>,
    /// True after a check that found nothing newer.
    pub up_to_date: bool,
    /// Bytes downloaded and expected, while the new version downloads.
    pub downloading: Option<(u64, u64)>,
    /// The checked new program, ready to put in place.
    pub ready: Option<PathBuf>,
    pub error: Option<String>,
}

pub type Shared = Arc<Mutex<State>>;

fn set(s: &Shared, wake: &eframe::egui::Context, f: impl FnOnce(&mut State)) {
    if let Ok(mut st) = s.lock() { f(&mut st); }
    wake.request_repaint();
}

/// Where the new program waits until it is installed.
fn update_dir() -> PathBuf { install::data_dir().join("update") }
/// The previous program, renamed out of the way while it is still running.
fn old_exe() -> PathBuf { install::install_dir().join("ronin-bar.old.exe") }

/// Looks for a newer release in the background.
pub fn check(s: Shared, wake: eframe::egui::Context) {
    if s.lock().map(|st| st.checking || st.downloading.is_some()).unwrap_or(true) { return }
    set(&s, &wake, |st| { st.checking = true; st.error = None; });
    std::thread::spawn(move || {
        let agent = download::agent();
        let r = agent.get(update::LATEST_URL).header("Accept", "application/vnd.github+json").call()
            .map_err(|e| format!("Could not reach GitHub: {e}"))
            .and_then(|mut r| r.body_mut().with_config().limit(1024 * 1024).read_to_string().map_err(|e| e.to_string()));
        set(&s, &wake, |st| {
            st.checking = false;
            match r {
                Ok(json) => match update::parse_release(&json) {
                    Some(rel) if update::is_newer(env!("CARGO_PKG_VERSION"), rel.version) => { st.latest = Some(rel); st.up_to_date = false; }
                    Some(_) => { st.latest = None; st.up_to_date = true; }
                    None => st.error = Some("The newest release has no Ronin Bar program yet.".into()),
                },
                Err(e) => st.error = Some(e),
            }
        });
    });
}

/// Downloads the new program and checks its SHA-256 in the background.
pub fn fetch(s: Shared, wake: eframe::egui::Context) {
    let Some(rel) = s.lock().ok().and_then(|st| if st.downloading.is_some() || st.ready.is_some() { None } else { st.latest.clone() }) else { return };
    set(&s, &wake, |st| { st.downloading = Some((0, 0)); st.error = None; });
    std::thread::spawn(move || {
        let r = fetch_now(&rel, &s, &wake);
        set(&s, &wake, |st| {
            st.downloading = None;
            match r { Ok(p) => st.ready = Some(p), Err(e) => st.error = Some(e) }
        });
    });
}

fn fetch_now(rel: &Release, s: &Shared, wake: &eframe::egui::Context) -> Result<PathBuf, String> {
    let agent = download::agent();
    let sha_text = agent.get(&rel.sha_url).call().map_err(|e| format!("Download failed: {e}"))?
        .body_mut().with_config().limit(4096).read_to_string().map_err(|e| e.to_string())?;
    let want = update::parse_sha256(&sha_text, &rel.exe_name).ok_or("The release's SHA-256 file is not valid")?;
    let mut resp = agent.get(&rel.exe_url).call().map_err(|e| format!("Download failed: {e}"))?;
    let total = resp.body().content_length().unwrap_or(0);
    let mut reader = resp.body_mut().with_config().limit(update::MAX_EXE_BYTES).reader();
    let (mut data, mut buf) = (Vec::with_capacity(total.min(update::MAX_EXE_BYTES) as usize), vec![0u8; 64 * 1024]);
    let mut shown = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 { break }
        data.extend_from_slice(&buf[..n]);
        let got = data.len() as u64;
        // Progress about every quarter megabyte.
        if got - shown >= 256 * 1024 { shown = got; set(s, wake, |st| st.downloading = Some((got, total))); }
    }
    let have: String = Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
    if have != want { return Err("The download does not match the release's SHA-256, so it was not installed.".into()) }
    let dir = update_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&rel.exe_name);
    crate::store::write_atomic(&path, &data).map_err(|e| format!("Could not save the update: {e}"))?;
    Ok(path)
}

/// Puts the checked new program in place of this one and starts it. The caller then quits; the
/// new copy removes the old one when it starts.
pub fn apply(new: &std::path::Path) -> Result<(), String> {
    let exe = install::installed_exe();
    let old = old_exe();
    let _ = std::fs::remove_file(&old);
    // A running program cannot be overwritten, but it can be renamed.
    std::fs::rename(&exe, &old).map_err(|e| format!("Could not move the old version: {e}"))?;
    if let Err(e) = std::fs::copy(new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("Could not install the new version: {e}"));
    }
    std::process::Command::new(&exe).arg("--updated").spawn().map_err(|e| {
        // Put the old version back so the bar keeps working.
        let _ = std::fs::remove_file(&exe);
        let _ = std::fs::rename(&old, &exe);
        format!("Could not start the new version: {e}")
    })?;
    Ok(())
}

/// After an update (and on every start): removes the old program and the downloaded file, and
/// shows the new version number in Settings → Apps. The old program may still be closing, so
/// this retries for a few seconds.
pub fn clean_up() {
    install::register_version();
    std::thread::spawn(|| {
        for _ in 0..20 {
            let old = old_exe();
            if !old.exists() || std::fs::remove_file(&old).is_ok() { break }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let _ = std::fs::remove_dir_all(update_dir());
    });
}
