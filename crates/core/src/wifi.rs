//! Wi-Fi helpers for the network popup: signal bars, password rules, and the profile Windows
//! stores for a network (built here, with every value escaped, and only then handed to Windows).

/// How a network is secured, as far as Ronin Bar can connect to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    Open,
    WpaPsk,
    Wpa2Psk,
    Wpa3Sae,
    /// Company networks (802.1X) and anything else: connect through Windows' settings.
    Other,
}

/// Signal strength (0–100) as 0–4 bars.
pub fn bars(quality: u32) -> u8 {
    match quality { 0..=10 => 0, 11..=35 => 1, 36..=60 => 2, 61..=85 => 3, _ => 4 }
}

/// A network name Windows accepts: 1–32 bytes.
pub fn valid_ssid(ssid: &str) -> bool { !ssid.is_empty() && ssid.len() <= 32 }

/// A password for this kind of network: 8–63 characters, or exactly 64 hex digits.
pub fn valid_password(sec: Security, pw: &str) -> bool {
    match sec {
        Security::Open => pw.is_empty(),
        Security::WpaPsk | Security::Wpa2Psk | Security::Wpa3Sae =>
            (8..=63).contains(&pw.chars().count()) && !pw.chars().any(char::is_control)
                || (pw.len() == 64 && pw.chars().all(|c| c.is_ascii_hexdigit())),
        Security::Other => false,
    }
}

fn escape(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).map(|c| match c {
        '&' => "&amp;".to_string(), '<' => "&lt;".to_string(), '>' => "&gt;".to_string(),
        '"' => "&quot;".to_string(), '\'' => "&apos;".to_string(), c => c.to_string(),
    }).collect()
}

/// The WLAN profile XML for joining `ssid` (None if the name or password is not acceptable,
/// or the network needs Windows' own settings).
pub fn profile_xml(ssid: &str, sec: Security, password: &str) -> Option<String> {
    if !valid_ssid(ssid) || !valid_password(sec, password) { return None }
    let hex: String = ssid.bytes().map(|b| format!("{b:02X}")).collect();
    let (auth, enc) = match sec {
        Security::Open => ("open", "none"),
        Security::WpaPsk => ("WPAPSK", "TKIP"),
        Security::Wpa2Psk => ("WPA2PSK", "AES"),
        Security::Wpa3Sae => ("WPA3SAE", "AES"),
        Security::Other => return None,
    };
    let key = if sec == Security::Open { String::new() } else {
        let kind = if password.len() == 64 && password.chars().all(|c| c.is_ascii_hexdigit()) { "networkKey" } else { "passPhrase" };
        format!("<sharedKey><keyType>{kind}</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>", escape(password))
    };
    let name = escape(ssid);
    Some(format!(concat!(
        r#"<?xml version="1.0"?><WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">"#,
        "<name>{name}</name><SSIDConfig><SSID><hex>{hex}</hex><name>{name}</name></SSID></SSIDConfig>",
        "<connectionType>ESS</connectionType><connectionMode>auto</connectionMode><MSM><security>",
        "<authEncryption><authentication>{auth}</authentication><encryption>{enc}</encryption><useOneX>false</useOneX></authEncryption>",
        "{key}</security></MSM></WLANProfile>"),
        name = name, hex = hex, auth = auth, enc = enc, key = key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_and_rules() {
        assert_eq!((bars(0), bars(30), bars(50), bars(80), bars(100)), (0, 1, 2, 3, 4));
        assert!(valid_password(Security::Wpa2Psk, "12345678"));
        assert!(!valid_password(Security::Wpa2Psk, "1234567"));
        assert!(!valid_password(Security::Wpa2Psk, &"x".repeat(64)));
        assert!(valid_password(Security::Wpa2Psk, &"ab".repeat(32)));
        assert!(valid_password(Security::Open, ""));
        assert!(!valid_password(Security::Other, "whatever1"));
        assert!(!valid_ssid("") && !valid_ssid(&"a".repeat(33)) && valid_ssid("Home"));
    }

    #[test]
    fn profile_escapes_everything() {
        let x = profile_xml("Café <5G>", Security::Wpa2Psk, "p&ss\"word</keyMaterial>").unwrap();
        assert!(x.contains("<name>Café &lt;5G&gt;</name>"));
        assert!(x.contains("<hex>436166C3A9203C35473E</hex>"));
        assert!(x.contains("<keyMaterial>p&amp;ss&quot;word&lt;/keyMaterial&gt;</keyMaterial>"));
        assert_eq!(x.matches("<keyMaterial>").count(), 1, "the password cannot add elements");
        assert!(x.contains("<authentication>WPA2PSK</authentication><encryption>AES</encryption>"));
        let open = profile_xml("Cafe", Security::Open, "").unwrap();
        assert!(open.contains("<authentication>open</authentication>") && !open.contains("sharedKey"));
        assert!(profile_xml("Office", Security::Other, "secret123").is_none());
        assert!(profile_xml("Home", Security::Wpa3Sae, "short").is_none());
    }
}
