//! Wallpapers from GitHub: the community collection
//! [dharmx/walls](https://github.com/dharmx/walls), sorted into Ronin Bar's own categories and
//! pinned to one commit so the files can never change under the app. Downloaded on the user's PC for personal use when a pack is chosen;
//! nothing is redistributed. The pictures belong to their artists.
use crate::wallhaven::Wallpaper;

pub const REPO: &str = "dharmx/walls";
/// The commit every file is read from.
pub const COMMIT: &str = "e8d8a42e4fbecb2cf9619995edbc7ce19610bbd8";

/// A category of pictures (paths inside the repository).
#[derive(Debug)]
pub struct Pack {
    pub name: &'static str,
    pub files: Vec<&'static str>,
}

/// Every usable picture of the collection, one per line: `category<TAB>path`. Logos, icon sets,
/// patterns and pictures too small or not widescreen are left out.
const LIST: &str = include_str!("walls.txt");

/// The categories, in the order they are listed.
pub fn packs() -> &'static [Pack] {
    static PACKS: std::sync::OnceLock<Vec<Pack>> = std::sync::OnceLock::new();
    PACKS.get_or_init(|| {
        let mut v: Vec<Pack> = Vec::new();
        for line in LIST.lines() {
            let Some((name, file)) = line.split_once('\t') else { continue };
            if !safe_path(file) { continue }
            match v.iter_mut().find(|p| p.name == name) {
                Some(p) => p.files.push(file),
                None => v.push(Pack { name, files: vec![file] }),
            }
        }
        v
    })
}

/// A plain picture path inside the repository.
fn safe_path(f: &str) -> bool {
    let l = f.to_ascii_lowercase();
    !f.is_empty() && !f.contains("..") && !f.starts_with('/')
        && f.chars().all(|c| c.is_ascii_alphanumeric() || "_-./'".contains(c))
        && (l.ends_with(".jpg") || l.ends_with(".jpeg") || l.ends_with(".png"))
}

/// A short id for a file (letters and digits, starting with "g"), stable across versions.
pub fn id(file: &str) -> String {
    // FNV-1a, 64 bit, written in base 36.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in file.bytes() { h ^= u64::from(b); h = h.wrapping_mul(0x0100_0000_01b3); }
    let mut s = String::from("g");
    for _ in 0..10 { s.push(char::from_digit((h % 36) as u32, 36).unwrap_or('0')); h /= 36; }
    s
}

/// The category's pictures as wallpapers (previews are made from the picture itself).
pub fn wallpapers(p: &Pack) -> Vec<Wallpaper> {
    p.files.iter().map(|f| Wallpaper {
        id: id(f),
        collection: p.name.to_string(),
        url: format!("https://raw.githubusercontent.com/{REPO}/{COMMIT}/{}", f.replace('\'', "%27")),
        thumb: String::new(),
        page: String::new(),
        resolution: String::new(),
    }).collect()
}

/// Every wallpaper of every pack.
pub fn all() -> Vec<Wallpaper> { packs().iter().flat_map(wallpapers).collect() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_is_a_safe_pinned_picture() {
        assert_eq!(COMMIT.len(), 40);
        assert!(COMMIT.chars().all(|c| c.is_ascii_hexdigit()));
        let lines = LIST.lines().filter(|l| !l.is_empty()).count();
        let files: usize = packs().iter().map(|p| p.files.len()).sum();
        assert_eq!(files, lines, "every listed line must be used");
        assert!(packs().len() >= 10 && files >= 1000);
        for p in packs() { assert!(p.files.len() >= 20, "{} is too small", p.name); }
        for bad in ["../x.jpg", "/a.jpg", "a/b.gif", "a/b c.jpg", "a/\"b.jpg", ""] { assert!(!safe_path(bad), "{bad}"); }
        assert!(safe_path("anime/a_man's_head.png"));
    }

    #[test]
    fn ids_are_unique_and_urls_are_pinned() {
        let all = all();
        let mut ids: Vec<&str> = all.iter().map(|w| w.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "ids must be unique");
        for w in &all {
            assert!(crate::wallhaven::is_id(&w.id), "{}", w.id);
            assert!(crate::art::original_theme(&w.id).is_none());
            assert!(w.url.starts_with("https://raw.githubusercontent.com/dharmx/walls/e8d8a42e"));
            assert!(!w.url.contains('\'') && !w.url.contains(' '));
        }
        assert_eq!(id("anime/x.jpg"), id("anime/x.jpg"));
    }
}
