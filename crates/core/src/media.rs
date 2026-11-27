//! Media playing anywhere on the PC (music, videos, films): the maths behind a smooth, in-sync
//! progress bar, time labels, and a readable name for the app that plays it. No Windows code
//! here, so it is tested on any machine.
use std::time::Duration;

/// Where playback is now, from where the app last said it was and how long ago it said so.
/// While playing, the position moves on with the clock (so the bar glides instead of jumping
/// between the app's reports); it never goes past the end.
pub fn position_now(reported: Duration, since_report: Duration, playing: bool, end: Duration) -> Duration {
    let p = if playing { reported + since_report } else { reported };
    if end.is_zero() { p } else { p.min(end) }
}

/// Windows writes moments as 100-nanosecond steps since 1 Jan 1601; this is how long ago that
/// moment was, given now in the same steps (0 if the clocks disagree).
pub fn ago(then_ticks: i64, now_ticks: i64) -> Duration {
    let d = now_ticks.saturating_sub(then_ticks);
    if d <= 0 { Duration::ZERO } else { Duration::from_nanos(d as u64 * 100) }
}

/// Now, in Windows' 100-nanosecond steps since 1601, from seconds since 1970.
pub fn ticks_from_unix(since_1970: Duration) -> i64 {
    const EPOCH_DIFF: i64 = 116_444_736_000_000_000; // 1601 → 1970
    EPOCH_DIFF + (since_1970.as_nanos() / 100) as i64
}

/// "3:07", or "1:02:09" for an hour or more.
pub fn time_label(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// A readable name for the app playing, from Windows' app id:
/// "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify" → "Spotify", "Brave" → "Brave",
/// "Chrome" / "MSEdge" → "Chrome" / "Edge", "C:\…\vlc.exe" → "VLC".
pub fn app_name(app_id: &str) -> String {
    let id = app_id.trim();
    let low = id.to_ascii_lowercase();
    for (key, name) in [("spotify", "Spotify"), ("msedge", "Edge"), ("chrome", "Chrome"), ("brave", "Brave"), ("firefox", "Firefox"),
                        ("vlc", "VLC"), ("zunemusic", "Media Player"), ("zunevideo", "Films & TV"), ("applemusic", "Apple Music"),
                        ("netflix", "Netflix"), ("opera", "Opera"), ("youtube", "YouTube")] {
        if low.contains(key) { return name.to_string() }
    }
    let last = id.rsplit(['\\', '/', '!']).next().unwrap_or(id);
    let last = last.strip_suffix(".exe").or_else(|| last.strip_suffix(".EXE")).unwrap_or(last);
    let last = last.rsplit('.').next().unwrap_or(last);
    let clean: String = last.chars().filter(|c| !c.is_control()).take(24).collect();
    if clean.is_empty() { "Media".to_string() } else { clean }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_moves_while_playing_only() {
        let s = Duration::from_secs;
        assert_eq!(position_now(s(30), s(2), true, s(200)), s(32));
        assert_eq!(position_now(s(30), s(2), false, s(200)), s(30));
        assert_eq!(position_now(s(199), s(5), true, s(200)), s(200));   // never past the end
        assert_eq!(position_now(s(10), s(5), true, Duration::ZERO), s(15)); // live stream: no end known
    }

    #[test]
    fn windows_time() {
        assert_eq!(ago(100, 100 + 10_000_000), Duration::from_secs(1));
        assert_eq!(ago(200, 100), Duration::ZERO);
        assert_eq!(ticks_from_unix(Duration::ZERO), 116_444_736_000_000_000);
    }

    #[test]
    fn labels() {
        assert_eq!(time_label(Duration::from_secs(187)), "3:07");
        assert_eq!(time_label(Duration::from_secs(3729)), "1:02:09");
        assert_eq!(time_label(Duration::ZERO), "0:00");
    }

    #[test]
    fn app_names() {
        assert_eq!(app_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "Spotify");
        assert_eq!(app_name("MSEdge"), "Edge");
        assert_eq!(app_name(r"C:\Program Files\VideoLAN\VLC\vlc.exe"), "VLC");
        assert_eq!(app_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "Media Player");
        assert_eq!(app_name("SomeVendor.CoolPlayer"), "CoolPlayer");
        assert_eq!(app_name(""), "Media");
    }
}
