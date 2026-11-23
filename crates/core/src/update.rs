//! Updates: reads the newest release on GitHub and decides whether it is newer than this copy.
//! Only this repository's own release files are ever accepted, and only with their published
//! SHA-256; the app checks the download against it before replacing itself.

/// GitHub's description of the newest published release (no drafts, no pre-releases).
pub const LATEST_URL: &str = "https://api.github.com/repos/AEX0TIC/Ronin-bar/releases/latest";
/// The release page people can read in a browser.
pub const RELEASES_PAGE: &str = "https://github.com/AEX0TIC/Ronin-bar/releases/latest";
/// Every release file lives under this address.
const DOWNLOAD_PREFIX: &str = "https://github.com/AEX0TIC/Ronin-bar/releases/download/";
/// A real Ronin Bar is a few MB; anything far bigger is refused.
pub const MAX_EXE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// `0.5.0` or `v0.5.0`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut it = s.split('.').map(|p| if !p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit()) { p.parse().ok() } else { None });
        let v = Version(it.next()??, it.next()??, it.next()??);
        it.next().is_none().then_some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}.{}.{}", self.0, self.1, self.2) }
}

/// A release that can be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The program and its SHA-256 file.
    pub exe_url: String,
    pub sha_url: String,
    pub exe_name: String,
    /// Release notes as written on GitHub (shown as plain text).
    pub notes: String,
}

/// The file name every release uses for the program.
pub fn exe_name(v: Version) -> String { format!("Ronin-Bar-{v}.exe") }

/// Reads GitHub's answer. Returns the release only if it carries both of its files, at the
/// addresses this repository publishes them under.
pub fn parse_release(json: &str) -> Option<Release> {
    let j: serde_json::Value = serde_json::from_str(json).ok()?;
    if j["draft"].as_bool().unwrap_or(false) || j["prerelease"].as_bool().unwrap_or(false) { return None }
    let tag = j["tag_name"].as_str()?;
    let version = Version::parse(tag)?;
    let name = exe_name(version);
    let expect = |file: &str| format!("{DOWNLOAD_PREFIX}v{version}/{file}");
    let mut exe_url = None;
    let mut sha_url = None;
    for a in j["assets"].as_array()? {
        let (Some(n), Some(u)) = (a["name"].as_str(), a["browser_download_url"].as_str()) else { continue };
        if n == name && u == expect(&name) { exe_url = Some(u.to_string()); }
        let sha = format!("{name}.sha256");
        if n == sha && u == expect(&sha) { sha_url = Some(u.to_string()); }
    }
    let notes: String = j["body"].as_str().unwrap_or("").chars().filter(|c| !c.is_control() || *c == '\n').take(2000).collect();
    Some(Release { version, exe_url: exe_url?, sha_url: sha_url?, exe_name: name, notes })
}

/// True if `latest` is newer than the running `current` version.
pub fn is_newer(current: &str, latest: Version) -> bool {
    Version::parse(current).map(|c| latest > c).unwrap_or(false)
}

/// The SHA-256 from a release's `.sha256` file (`<64 hex digits>  <file name>`), lowercase.
/// The file name, when present, must be the program's.
pub fn parse_sha256(text: &str, file: &str) -> Option<String> {
    let mut parts = text.split_whitespace();
    let hash = parts.next()?.to_ascii_lowercase();
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) { return None }
    match parts.next() {
        Some(n) if n.trim_start_matches('*') != file => None,
        _ => Some(hash),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(Version::parse("v0.5.0"), Some(Version(0, 5, 0)));
        assert_eq!(Version::parse("1.10.3"), Some(Version(1, 10, 3)));
        for bad in ["", "v1", "1.2", "1.2.3.4", "1.2.x", "1..3", "-1.2.3", "1.2.3-beta"] { assert_eq!(Version::parse(bad), None, "{bad}"); }
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert!(is_newer("0.4.0", Version(0, 5, 0)));
        assert!(!is_newer("0.5.0", Version(0, 5, 0)));
        assert!(!is_newer("0.6.0", Version(0, 5, 9)));
    }

    fn release_json(tag: &str, base: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","draft":false,"prerelease":false,"body":"New things\r\nmore","assets":[
            {{"name":"Ronin-Bar-0.5.0.exe","browser_download_url":"{base}/v0.5.0/Ronin-Bar-0.5.0.exe"}},
            {{"name":"Ronin-Bar-0.5.0.exe.sha256","browser_download_url":"{base}/v0.5.0/Ronin-Bar-0.5.0.exe.sha256"}}]}}"#)
    }

    #[test]
    fn reads_a_release() {
        let r = parse_release(&release_json("v0.5.0", "https://github.com/AEX0TIC/Ronin-bar/releases/download")).unwrap();
        assert_eq!(r.version, Version(0, 5, 0));
        assert_eq!(r.exe_name, "Ronin-Bar-0.5.0.exe");
        assert!(r.exe_url.ends_with("/v0.5.0/Ronin-Bar-0.5.0.exe") && r.sha_url.ends_with(".sha256"));
        assert_eq!(r.notes, "New things\nmore");
    }

    #[test]
    fn refuses_files_from_anywhere_else() {
        assert_eq!(parse_release(&release_json("v0.5.0", "https://evil.example/AEX0TIC/Ronin-bar/releases/download")), None);
        assert_eq!(parse_release(&release_json("v0.5.0", "https://github.com/someone/Ronin-bar/releases/download")), None);
        // The tag must match the files.
        assert_eq!(parse_release(&release_json("v0.6.0", "https://github.com/AEX0TIC/Ronin-bar/releases/download")), None);
        assert_eq!(parse_release("not json"), None);
        let draft = release_json("v0.5.0", "https://github.com/AEX0TIC/Ronin-bar/releases/download").replace("\"draft\":false", "\"draft\":true");
        assert_eq!(parse_release(&draft), None);
    }

    #[test]
    fn checksums() {
        let h = "AB".repeat(32);
        assert_eq!(parse_sha256(&format!("{h}  Ronin-Bar-0.5.0.exe"), "Ronin-Bar-0.5.0.exe"), Some("ab".repeat(32)));
        assert_eq!(parse_sha256(&h, "x.exe"), Some("ab".repeat(32)));
        assert_eq!(parse_sha256(&format!("{h}  other.exe"), "Ronin-Bar-0.5.0.exe"), None);
        assert_eq!(parse_sha256("abc  Ronin-Bar-0.5.0.exe", "Ronin-Bar-0.5.0.exe"), None);
        assert_eq!(parse_sha256(&"zz".repeat(32), "x"), None);
    }
}
