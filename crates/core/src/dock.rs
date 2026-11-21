//! The dock: apps you pin to the bar (from the Start menu) and how they are named and searched.

/// At most this many pinned apps.
pub const MAX_PINNED: usize = 16;

/// Only Start-menu shortcuts and programs can be pinned (opened with Windows' "open", never run
/// through a shell).
pub fn pinnable(path: &str) -> bool {
    let p = path;
    let lower = p.to_ascii_lowercase();
    let absolute = p.len() > 3 && p.as_bytes()[1] == b':' && (p.as_bytes()[2] == b'\\' || p.as_bytes()[2] == b'/');
    absolute && p.len() < 400 && !p.contains("..") && !p.chars().any(|c| c.is_control() || "\"<>|?*".contains(c))
        && (lower.ends_with(".lnk") || lower.ends_with(".exe") || lower.ends_with(".url"))
}

/// "C:\…\Programs\Visual Studio Code.lnk" → "Visual Studio Code".
pub fn display_name(path: &str) -> String {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    stem.chars().filter(|c| !c.is_control()).take(60).collect()
}

/// Keeps valid, distinct pins, at most `MAX_PINNED`, in order.
pub fn clean_pins(pins: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in pins {
        if pinnable(&p) && !out.iter().any(|o| o.eq_ignore_ascii_case(&p)) && out.len() < MAX_PINNED { out.push(p); }
    }
    out
}

/// Start-menu entries whose name contains every word of `query` (case-insensitive).
pub fn matches(name: &str, query: &str) -> bool {
    let n = name.to_lowercase();
    query.to_lowercase().split_whitespace().all(|w| n.contains(w))
}

/// Start-menu shortcuts nobody wants in a dock.
pub fn is_clutter(name: &str) -> bool {
    let n = name.to_lowercase();
    ["uninstall", "readme", "release notes", "help", "documentation", "license", "website"].iter().any(|w| n.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_shortcuts_and_programs() {
        assert!(pinnable(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Firefox.lnk"));
        assert!(pinnable(r"C:\Windows\notepad.exe"));
        assert!(pinnable(r"D:\Games\Steam.url"));
        for bad in [r"notepad.exe", r"C:\x\run.bat", r"C:\x\a.ps1", r"C:\x\..\cmd.exe", r#"C:\x\"a.exe"#, "C:\\x\\a.exe\n", r"\\server\share\a.exe", ""] {
            assert!(!pinnable(bad), "{bad:?}");
        }
    }

    #[test]
    fn names_search_and_cleaning() {
        assert_eq!(display_name(r"C:\x\Programs\Visual Studio Code.lnk"), "Visual Studio Code");
        assert_eq!(display_name("C:/x/steam.url"), "steam");
        assert!(matches("Visual Studio Code", "studio vis"));
        assert!(!matches("Firefox", "chrome"));
        assert!(matches("Anything", ""));
        assert!(is_clutter("Uninstall Firefox") && !is_clutter("Firefox"));
        let many: Vec<String> = (0..30).map(|i| format!(r"C:\a\{i}.exe")).collect();
        assert_eq!(clean_pins(many).len(), MAX_PINNED);
        assert_eq!(clean_pins(vec![r"C:\a\x.exe".into(), r"c:\A\X.EXE".into(), "bad".into()]), vec![r"C:\a\x.exe".to_string()]);
    }
}
