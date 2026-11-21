//! The 30 wallpapers: 10 samurai, 10 anime cars and 10 anime scenes, from Wallhaven's public API
//! (anime category, safe-for-work only). They are downloaded on the user's PC for personal use;
//! nothing is redistributed. Every URL and file is checked before it is used.
use serde::{Deserialize, Serialize};

/// A style of wallpaper and the search that finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Collection {
    pub name: &'static str,
    pub query: &'static str,
}

pub const COLLECTIONS: [Collection; 3] = [
    Collection { name: "Samurai", query: "samurai" },
    Collection { name: "Anime cars", query: "car" },
    Collection { name: "Anime scenery", query: "landscape" },
];

/// Wallpapers per collection (3 × 10 = 30).
pub const PER_COLLECTION: usize = 10;
/// Largest file accepted.
pub const MAX_BYTES: usize = 25 * 1024 * 1024;

/// Search for one collection: anime category only (`categories=010`), safe for work only
/// (`purity=100`), the most-favourited first, at least 1920×1080, landscape screens.
pub fn search_url(c: &Collection) -> String {
    format!("https://wallhaven.cc/api/v1/search?q={}&categories=010&purity=100&sorting=toplist&order=desc&atleast=1920x1080&ratios=16x9,16x10", c.query)
}

/// One wallpaper as kept in the catalogue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wallpaper {
    pub id: String,
    pub collection: String,
    /// Full image, e.g. https://w.wallhaven.cc/full/x8/wallhaven-x8kq2e.jpg
    pub url: String,
    /// Small preview, e.g. https://th.wallhaven.cc/small/x8/x8kq2e.jpg
    pub thumb: String,
    /// Page on wallhaven.cc that credits the uploader.
    pub page: String,
    pub resolution: String,
}

impl Wallpaper {
    /// File name of the full image on disk.
    pub fn file_name(&self) -> String {
        let ext = if self.url.ends_with(".png") { "png" } else { "jpg" };
        format!("{}.{ext}", self.id)
    }
}

#[derive(Deserialize)]
struct SearchResponse { data: Vec<Item> }
#[derive(Deserialize)]
struct Item {
    id: String,
    path: String,
    url: String,
    #[serde(default)]
    purity: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    resolution: String,
    thumbs: Thumbs,
}
#[derive(Deserialize)]
struct Thumbs { small: String }

/// Wallhaven ids are short lowercase letters and digits.
pub fn is_id(s: &str) -> bool {
    (4..=12).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn ok_url(u: &str, host: &str, id: &str) -> bool {
    u.starts_with(&format!("https://{host}/")) && u.contains(id) && !u.contains("..")
        && u.bytes().all(|b| b.is_ascii_alphanumeric() || b"/:._-".contains(&b))
}

/// The safe-for-work anime wallpapers in one search response, at most `PER_COLLECTION`.
/// Anything with an unexpected id, host, file type, category or purity is skipped.
pub fn parse_search(json: &str, collection: &str) -> Vec<Wallpaper> {
    let Ok(r) = serde_json::from_str::<SearchResponse>(json) else { return Vec::new() };
    r.data.into_iter().filter(|i| {
        is_id(&i.id) && i.purity == "sfw" && i.category == "anime"
            && ok_url(&i.path, "w.wallhaven.cc", &i.id) && (i.path.ends_with(".jpg") || i.path.ends_with(".png"))
            && ok_url(&i.thumbs.small, "th.wallhaven.cc", &i.id)
            && i.url == format!("https://wallhaven.cc/w/{}", i.id)
    }).take(PER_COLLECTION).map(|i| Wallpaper {
        id: i.id, collection: collection.to_string(), url: i.path, thumb: i.thumbs.small, page: i.url,
        resolution: i.resolution.chars().filter(|c| c.is_ascii_digit() || *c == 'x').take(12).collect(),
    }).collect()
}

/// True only for a real JPEG or PNG within the size limit (checked by content, not by name).
pub fn is_image(bytes: &[u8]) -> bool {
    bytes.len() <= MAX_BYTES && (bytes.starts_with(&[0xFF, 0xD8, 0xFF]) || bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
}

/// The next wallpaper id after `current` (wrapping), or the first.
pub fn next_id<'a>(list: &'a [Wallpaper], current: &str, step: isize) -> Option<&'a str> {
    if list.is_empty() { return None }
    let n = list.len() as isize;
    let i = list.iter().position(|w| w.id == current).map(|i| i as isize).unwrap_or(-1);
    let j = if i < 0 { 0 } else { (i + step).rem_euclid(n) };
    Some(&list[j as usize].id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"data":[
      {"id":"x8kq2e","url":"https://wallhaven.cc/w/x8kq2e","purity":"sfw","category":"anime","resolution":"3840x2160",
       "path":"https://w.wallhaven.cc/full/x8/wallhaven-x8kq2e.jpg","thumbs":{"small":"https://th.wallhaven.cc/small/x8/x8kq2e.jpg"}},
      {"id":"nsfw01","url":"https://wallhaven.cc/w/nsfw01","purity":"sketchy","category":"anime","resolution":"1920x1080",
       "path":"https://w.wallhaven.cc/full/ns/wallhaven-nsfw01.jpg","thumbs":{"small":"https://th.wallhaven.cc/small/ns/nsfw01.jpg"}},
      {"id":"evil01","url":"https://wallhaven.cc/w/evil01","purity":"sfw","category":"anime","resolution":"1920x1080",
       "path":"https://evil.example/full/wallhaven-evil01.jpg","thumbs":{"small":"https://th.wallhaven.cc/small/ev/evil01.jpg"}},
      {"id":"gif001","url":"https://wallhaven.cc/w/gif001","purity":"sfw","category":"anime","resolution":"1920x1080",
       "path":"https://w.wallhaven.cc/full/gi/wallhaven-gif001.gif","thumbs":{"small":"https://th.wallhaven.cc/small/gi/gif001.jpg"}},
      {"id":"pe0ple","url":"https://wallhaven.cc/w/pe0ple","purity":"sfw","category":"people","resolution":"1920x1080",
       "path":"https://w.wallhaven.cc/full/pe/wallhaven-pe0ple.jpg","thumbs":{"small":"https://th.wallhaven.cc/small/pe/pe0ple.jpg"}},
      {"id":"../../x","url":"https://wallhaven.cc/w/x","purity":"sfw","category":"anime","resolution":"1x1",
       "path":"https://w.wallhaven.cc/full/../../x.jpg","thumbs":{"small":"https://th.wallhaven.cc/small/x.jpg"}},
      {"id":"pngok1","url":"https://wallhaven.cc/w/pngok1","purity":"sfw","category":"anime","resolution":"2560x1440<script>",
       "path":"https://w.wallhaven.cc/full/pn/wallhaven-pngok1.png","thumbs":{"small":"https://th.wallhaven.cc/small/pn/pngok1.jpg"}}
    ],"meta":{"current_page":1}}"#;

    #[test]
    fn keeps_only_safe_anime_images_from_wallhaven() {
        let w = parse_search(SAMPLE, "Samurai");
        let ids: Vec<_> = w.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["x8kq2e", "pngok1"]);
        assert_eq!(w[0].file_name(), "x8kq2e.jpg");
        assert_eq!(w[1].file_name(), "pngok1.png");
        assert_eq!(w[1].resolution, "2560x1440");
        assert_eq!(w[0].collection, "Samurai");
        assert!(parse_search("garbage", "x").is_empty());
        assert!(parse_search(r#"{"data":[]}"#, "x").is_empty());
    }

    #[test]
    fn searches_are_anime_sfw_only() {
        assert_eq!(COLLECTIONS.len() * PER_COLLECTION, 30);
        for c in COLLECTIONS {
            let u = search_url(&c);
            assert!(u.starts_with("https://wallhaven.cc/api/v1/search?"));
            assert!(u.contains("categories=010") && u.contains("purity=100"));
        }
    }

    #[test]
    fn images_checked_by_content() {
        assert!(is_image(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]));
        assert!(is_image(b"\x89PNG\r\n\x1a\nrest"));
        assert!(!is_image(b"<html>not an image</html>"));
        assert!(!is_image(b"MZ\x90\x00"));
        assert!(!is_image(&vec![0xFF; MAX_BYTES + 1]));
    }

    #[test]
    fn ids_and_cycling() {
        assert!(is_id("x8kq2e"));
        for bad in ["", "abc", "X8KQ2E", "../x", "a b c d", "abcdefghijklm"] { assert!(!is_id(bad), "{bad}"); }
        let list = parse_search(SAMPLE, "s");
        assert_eq!(next_id(&list, "x8kq2e", 1), Some("pngok1"));
        assert_eq!(next_id(&list, "pngok1", 1), Some("x8kq2e"));
        assert_eq!(next_id(&list, "x8kq2e", -1), Some("pngok1"));
        assert_eq!(next_id(&list, "none", 1), Some("x8kq2e"));
        assert_eq!(next_id(&[], "x", 1), None);
    }
}
