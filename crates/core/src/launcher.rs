//! The launcher's rules: which entries are real apps (not helpers, links or documents), how
//! search results are ranked, and the apps used most.

/// Entries Windows lists among apps that are not apps you launch: uninstallers, manuals, web
/// links, driver helpers, setup and registration tools.
pub fn is_junk(name: &str, target: &str) -> bool {
    let n = name.to_lowercase();
    const WORDS: &[&str] = &[
        "uninstall", "readme", "read me", "release notes", "help", "documentation", "manual", "user guide",
        "license", "licence", "website", "web site", "homepage", "home page", "changelog", "what's new",
        "add a new", "virtual network adapter", "registration", "repair", "setup", "installer",
        "configuration tool", "check for updates", "safe mode", "support center", "eula", "faq",
        // Command lines and Windows' admin and developer tools.
        "command prompt", "powershell", "native tools", "cross tools", "developer prompt", "verifier",
        "administration assistant", "administrative tools", "windows tools", "defragment", "disk cleanup",
        "event viewer", "system configuration", "system information", "registry editor", "task scheduler",
        "performance monitor", "resource monitor", "recovery drive", "odbc", "iscsi", "memory diagnostic",
        "print management", "component services", "computer management", "local security policy",
        "steps recorder", "debuggable package", "debugger", "profiler", "sdk tools", "symbol",
    ];
    if WORDS.iter().any(|w| n.contains(w)) { return true }
    // A target that is a document, a web page, a script or an admin console.
    let t = target.to_lowercase();
    const ENDS: [&str; 22] = [
        ".chm", ".hlp", ".txt", ".pdf", ".htm", ".html", ".rtf", ".doc", ".docx", ".md", ".ini", ".log",
        ".xml", ".bat", ".cmd", ".ps1", ".vbs", ".js", ".msc", ".cpl", ".reg", ".msi",
    ];
    if ENDS.iter().any(|e| t.ends_with(e)) { return true }
    // Web links, except game launchers (Steam, Epic and the like use links too).
    t.ends_with(".url") && (n.contains("web") || n.contains("online") || n.contains("site"))
}

/// Windows' own folder ids (System32, its 32-bit twin, and Windows) as Windows' Applications
/// folder writes them at the start of a program's id.
const WINDOWS_FOLDERS: [&str; 3] = ["{1ac14e77-02e7-4e5d-b744-2eb1ae5198b7}", "{d65231b0-b2f1-4857-a4ce-a8e7c6ea7d27}", "{f38bf404-1d43-42f2-9305-67de0b28fc23}"];
/// The few programs inside Windows' own folders that people do open.
const WINDOWS_KEEP: [&str; 4] = ["notepad.exe", "mspaint.exe", "calc.exe", "snippingtool.exe"];

/// A program from Windows' own system folders (cmd, consoles, diagnostics), not an app.
pub fn is_system_tool(id: &str) -> bool {
    let l = id.to_ascii_lowercase();
    let l = l.strip_prefix("shell:appsfolder\\").unwrap_or(&l);
    let inside = WINDOWS_FOLDERS.iter().any(|f| l.starts_with(f))
        || l.starts_with(r"c:\windows\system32\") || l.starts_with(r"c:\windows\syswow64\");
    inside && !WINDOWS_KEEP.iter().any(|k| l.ends_with(k))
}

/// A desktop `.url` that starts a program (a game launcher's own link such as steam:// or
/// com.epicgames.launcher://), not a web page or a file.
pub fn is_launcher_link(url: &str) -> bool {
    let Some((scheme, rest)) = url.trim().split_once(':') else { return false };
    let s = scheme.to_ascii_lowercase();
    !rest.is_empty() && s.len() >= 2 && s.len() <= 40
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
        && !["http", "https", "ftp", "file", "mailto", "javascript", "data", "ms-settings", "about"].contains(&s.as_str())
}

/// Same app under two names ("7-Zip 23.01 (x64)" and "7-Zip"): lowercase, without versions,
/// bitness or punctuation.
pub fn simple_name(name: &str) -> String {
    let n = name.to_lowercase().replace("(x64)", "").replace("(x86)", "").replace("(64-bit)", "").replace("(32-bit)", "").replace("64-bit", "").replace("x64", "");
    n.split_whitespace()
        .filter(|w| { let d = w.strip_prefix('v').unwrap_or(w); d.is_empty() || !d.chars().all(|c| c.is_ascii_digit() || c == '.') })
        .collect::<Vec<_>>().join(" ")
        .chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect::<String>().trim().to_string()
}

/// How well `name` fits what was typed: lower is better, None when it does not fit at all.
/// Starts with it, then a word starts with it, then initials ("vsc" → Visual Studio Code), then
/// every typed word somewhere in the name, then the letters in order.
pub fn rank(name: &str, query: &str) -> Option<u32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() { return Some(0) }
    let n = name.to_lowercase();
    let words: Vec<&str> = n.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    if n.starts_with(&q) { return Some(0) }
    if words.iter().any(|w| w.starts_with(&q)) { return Some(1) }
    let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
    if q.len() >= 2 && initials.starts_with(&q) { return Some(2) }
    if q.split_whitespace().all(|w| n.contains(w)) { return Some(3) }
    let mut it = n.chars();
    if q.len() >= 3 && q.chars().filter(|c| !c.is_whitespace()).all(|c| it.any(|x| x == c)) { return Some(4) }
    None
}

/// Most-used apps first, at most `n`, from (app, times opened).
pub fn most_used(counts: &[(String, u32)], n: usize) -> Vec<String> {
    let mut v: Vec<&(String, u32)> = counts.iter().filter(|(_, c)| *c > 0).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter().take(n).map(|(a, _)| a.clone()).collect()
}

/// Counts one more launch of `app`, keeping the list small.
pub fn count_launch(counts: &mut Vec<(String, u32)>, app: &str) {
    match counts.iter_mut().find(|(a, _)| a == app) {
        Some((_, c)) => *c = c.saturating_add(1),
        None => counts.push((app.to_string(), 1)),
    }
    if counts.len() > 200 {
        counts.sort_by_key(|b| std::cmp::Reverse(b.1));
        counts.truncate(150);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junk() {
        for (n, t) in [("Add a new TAP-Windows6 virtual network adapter", r"x\addtap.bat"), ("Acer Product Registration", "x.exe"),
                       ("Uninstall Firefox", "u.exe"), ("Python Manuals", r"x\python.chm"), ("Visit our Website", r"x\site.url"),
                       ("Readme", r"x\readme.txt"), ("Computer Management", r"x\compmgmt.msc")] {
            assert!(is_junk(n, t), "{n}");
        }
        for (n, t) in [("Visual Studio Code", r"x\Code.exe"), ("Counter-Strike 2", r"x\Counter-Strike 2.url"),
                       ("Calculator", "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"), ("Brave", r"x\brave.exe")] {
            assert!(!is_junk(n, t), "{n}");
        }
    }

    #[test]
    fn system_tools_and_links() {
        assert!(is_system_tool(r"shell:AppsFolder\{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\cmd.exe"));
        assert!(is_system_tool(r"C:\Windows\System32\dfrgui.exe"));
        assert!(!is_system_tool(r"shell:AppsFolder\{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\notepad.exe"));
        assert!(!is_system_tool(r"C:\Program Files\Blackmagic Design\DaVinci Resolve\Resolve.exe"));
        for bad in ["Developer Command Prompt for VS 2019", "Windows PowerShell ISE", "Application Verifier (X64)", "Defragment and Optimize Drives", "MinGW Installer"] {
            assert!(is_junk(bad, "x.exe"), "{bad}");
        }
        assert!(is_launcher_link("steam://rungameid/2669320"));
        assert!(is_launcher_link("com.epicgames.launcher://apps/Fortnite?action=launch"));
        for bad in ["https://example.com", "http://x", "file:///C:/a.pdf", "mailto:a@b.c", "javascript:alert(1)", "", "nocolon"] {
            assert!(!is_launcher_link(bad), "{bad}");
        }
    }

    #[test]
    fn names() {
        assert_eq!(simple_name("7-Zip 23.01 (x64)"), "7zip");
        assert_eq!(simple_name("VLC media player"), "vlc media player");
        assert_eq!(simple_name("Notepad++ v8.6"), "notepad");
    }

    #[test]
    fn ranking() {
        assert_eq!(rank("Visual Studio Code", "vis"), Some(0));
        assert_eq!(rank("Visual Studio Code", "code"), Some(1));
        assert_eq!(rank("Visual Studio Code", "vsc"), Some(2));
        assert_eq!(rank("Visual Studio Code", "studio vis"), Some(3));
        assert_eq!(rank("Adobe Photoshop", "phs"), Some(4));
        assert_eq!(rank("Firefox", "chrome"), None);
        assert_eq!(rank("Anything", "  "), Some(0));
    }

    #[test]
    fn usage() {
        let mut c = Vec::new();
        for _ in 0..3 { count_launch(&mut c, "a"); }
        count_launch(&mut c, "b");
        assert_eq!(most_used(&c, 5), vec!["a".to_string(), "b".to_string()]);
        for i in 0..300 { count_launch(&mut c, &format!("x{i}")); }
        assert!(c.len() <= 200 && c.iter().any(|(a, _)| a == "a"));
    }
}
