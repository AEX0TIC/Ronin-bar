//! Settings and the wallpaper catalogue on disk, written atomically (temp file, then rename).
use ronin_core::{settings::Settings, wallhaven::Wallpaper};
use std::path::{Path, PathBuf};

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(d) = path.parent() { std::fs::create_dir_all(d)?; }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub struct Store { pub data: PathBuf }

impl Store {
    pub fn settings_file(&self) -> PathBuf { self.data.join("settings.json") }
    pub fn catalog_file(&self) -> PathBuf { self.data.join("catalog.json") }
    pub fn wallpaper_file(&self, w: &Wallpaper) -> PathBuf { self.data.join("wallpapers").join(w.file_name()) }
    pub fn thumb_file(&self, w: &Wallpaper) -> PathBuf { self.data.join("thumbs").join(format!("{}.jpg", w.id)) }

    pub fn load_settings(&self) -> Settings {
        std::fs::read_to_string(self.settings_file()).map(|t| Settings::from_json(&t)).unwrap_or_default()
    }
    pub fn save_settings(&self, s: &Settings) { let _ = write_atomic(&self.settings_file(), s.to_json().as_bytes()); }

    /// Wallpapers whose preview is on disk (the full picture is downloaded when it is chosen).
    pub fn load_catalog(&self) -> Vec<Wallpaper> {
        let list: Vec<Wallpaper> = std::fs::read_to_string(self.catalog_file()).ok()
            .and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        list.into_iter().filter(|w| ronin_core::wallhaven::is_id(&w.id) && self.thumb_file(w).is_file()).collect()
    }
    /// Pictures from the GitHub packs that are already downloaded (picture and preview).
    pub fn load_packs(&self) -> Vec<Wallpaper> {
        ronin_core::github::all().into_iter().filter(|w| self.wallpaper_file(w).is_file() && self.thumb_file(w).is_file()).collect()
    }
    pub fn save_catalog(&self, list: &[Wallpaper]) {
        if let Ok(j) = serde_json::to_string_pretty(list) { let _ = write_atomic(&self.catalog_file(), j.as_bytes()); }
    }
}
