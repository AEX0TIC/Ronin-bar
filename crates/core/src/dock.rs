//! The dock: apps you pin to the bar (from the Start menu) and how they are named and searched.

/// At most this many pinned apps.
pub const MAX_PINNED: usize = 16;

/// Prefix of an installed app's id, as Windows' Applications folder (and Ronin's launcher) names it.
pub const APP_PREFIX: &str = "shell:AppsFolder\\";

/// An installed app's id: `shell:AppsFolder\<id>` with a plain id (Store apps look like
/// `Microsoft.WindowsCalculator_8wekyb3d8bbwe!App`, desktop apps like a path or a name).
pub fn is_app_id(s: &str) -> bool {
    s.strip_prefix(APP_PREFIX).is_some_and(|id| {
        !id.is_empty() && id.len() < 300 && !id.contains("..")
            && id.chars().all(|c| c.is_alphanumeric() || "._-!{}\\ ~()+,".contains(c))
    })
}

/// Only installed apps, Start-menu shortcuts and programs can be pinned (opened with Windows'
/// "open", never run through a command shell).
pub fn pinnable(path: &str) -> bool {
    if is_app_id(path) { return true }
    let p = path;
    let lower = p.to_ascii_lowercase();
    let absolute = p.len() > 3 && p.as_bytes()[1] == b':' && (p.as_bytes()[2] == b'\\' || p.as_bytes()[2] == b'/');
    absolute && p.len() < 400 && !p.contains("..") && !p.chars().any(|c| c.is_control() || "\"<>|?*".contains(c))
        && (lower.ends_with(".lnk") || lower.ends_with(".exe") || lower.ends_with(".url"))
}

/// "C:\…\Programs\Visual Studio Code.lnk" → "Visual Studio Code"; an app id → its last part
/// (the launcher shows apps by their real name).
pub fn display_name(path: &str) -> String {
    if let Some(id) = path.strip_prefix(APP_PREFIX) {
        let base = id.rsplit(['\\', '/']).next().unwrap_or(id);
        let base = base.split('!').next().unwrap_or(base);
        let base = base.split('_').next().unwrap_or(base);
        let base = base.rsplit('.').find(|p| !p.is_empty() && !p.eq_ignore_ascii_case("exe")).unwrap_or(base);
        return base.chars().filter(|c| !c.is_control()).take(60).collect();
    }
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
        assert!(pinnable(r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        assert!(pinnable(r"shell:AppsFolder\{6D809377-6AF0-444B-8957-A3773F02200E}\Brave\brave.exe"));
        assert_eq!(display_name(r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"), "WindowsCalculator");
        for bad in [r"shell:AppsFolder\", r"shell:AppsFolder\..\x", "shell:AppsFolder\\a;calc", "shell:AppsFolder\\a&b", "shell:Startup\\x", r"notepad.exe", r"C:\x\run.bat", r"C:\x\a.ps1", r"C:\x\..\cmd.exe", r#"C:\x\"a.exe"#, "C:\\x\\a.exe\n", r"\\server\share\a.exe", ""] {
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
