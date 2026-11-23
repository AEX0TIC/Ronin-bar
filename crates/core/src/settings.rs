//! Settings, saved as JSON in `<install folder>\data\settings.json`.
//!
//! A bad or missing value never stops the app: every field falls back to its default.
use serde::{Deserialize, Serialize};

/// What Windows looked like before Ronin Bar changed it, so uninstall can put it back.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Original {
    pub wallpaper: String,
    pub apps_use_light_theme: Option<u32>,
    pub system_uses_light_theme: Option<u32>,
    pub accent_color: Option<u32>,
    pub accent_color_menu: Option<u32>,
    /// The Windows taskbar's auto-hide state (ABM_GETSTATE) before Ronin Bar hid it.
    #[serde(default)]
    pub taskbar_state: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    /// Id of the current wallpaper: a wallhaven id, or `ronin1`…`ronin5` for the built-in ones.
    pub wallpaper: String,
    /// Change the wallpaper every N minutes (0 = never).
    pub auto_change_minutes: u32,
    pub start_with_windows: bool,
    /// Apps pinned to the dock: Start-menu shortcuts or programs.
    pub pinned: Vec<String>,
    /// Hide the Windows taskbar while Ronin Bar runs (it comes back when the bar quits).
    pub hide_taskbar: bool,
    /// Ronin's night light (a warmer screen), and how strong it is in percent.
    pub night_light: bool,
    pub night_strength: u8,
    /// Filled once, the first time Ronin Bar changes anything.
    pub original: Option<Original>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { theme: "samurai".into(), wallpaper: String::new(), auto_change_minutes: 0, start_with_windows: true, pinned: Vec::new(), hide_taskbar: true, night_light: false, night_strength: crate::night::DEFAULT_STRENGTH, original: None }
    }
}

/// Allowed auto-change intervals in minutes.
pub const AUTO_CHANGE_CHOICES: [u32; 5] = [0, 15, 30, 60, 180];

impl Settings {
    /// Parses settings; anything unreadable becomes the default, and bad values are corrected.
    pub fn from_json(text: &str) -> Settings {
        let mut s: Settings = serde_json::from_str(text).unwrap_or_default();
        if !crate::theme::THEMES.iter().any(|t| t.id == s.theme) { s.theme = Settings::default().theme; }
        if !AUTO_CHANGE_CHOICES.contains(&s.auto_change_minutes) { s.auto_change_minutes = 0; }
        if !crate::wallhaven::is_id(&s.wallpaper) { s.wallpaper.clear(); }
        if !(10..=100).contains(&s.night_strength) { s.night_strength = crate::night::DEFAULT_STRENGTH; }
        s.pinned = crate::dock::clean_pins(std::mem::take(&mut s.pinned));
        s
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_values_become_defaults() {
        let s = Settings::from_json(r#"{"theme":"nope","auto_change_minutes":7,"wallpaper":"../x","start_with_windows":false,"pinned":["C:\\a\\x.exe","evil.bat"]}"#);
        assert_eq!(s.pinned, vec![r"C:\a\x.exe".to_string()]);
        assert_eq!(s.theme, "samurai");
        assert_eq!(s.auto_change_minutes, 0);
        assert_eq!(s.wallpaper, "");
        assert!(!s.start_with_windows);
        assert_eq!(Settings::from_json("not json"), Settings::default());
        assert_eq!(Settings::from_json("{}"), Settings::default());
    }

    #[test]
    fn round_trip() {
        let mut s = Settings { theme: "matcha".into(), wallpaper: "x8kq2e".into(), auto_change_minutes: 30, ..Default::default() };
        s.original = Some(Original { wallpaper: r"C:\Users\me\Pictures\a.jpg".into(), accent_color: Some(0xff00_00ff), ..Default::default() });
        assert_eq!(Settings::from_json(&s.to_json()), s);
    }
}
