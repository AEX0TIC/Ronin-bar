//! Bluetooth helpers that need no Windows: device addresses and which devices are worth listing.

/// A Bluetooth address as 12 lowercase hex digits ("00:1A:7D:DA:71:13" → "001a7dda7113").
pub fn address(s: &str) -> Option<String> {
    let hex: String = s.chars().filter(|c| !matches!(c, ':' | '-' | '(' | ')' | ' ')).collect::<String>().to_ascii_lowercase();
    (hex.len() == 12 && hex.bytes().all(|b| b.is_ascii_hexdigit()) && hex != "000000000000").then_some(hex)
}

/// True if a Windows device id belongs to the device with this address (Windows writes the
/// address into the ids of a Bluetooth device's audio drivers).
pub fn id_has_address(device_id: &str, addr: &str) -> bool {
    addr.len() == 12 && device_id.to_ascii_lowercase().contains(addr)
}

/// Nearby devices without a real name (shown by Windows as their address, or blank) are left out.
pub fn worth_listing(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && address(n).is_none() && !n.eq_ignore_ascii_case("unknown") && n.chars().all(|c| !c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(address("00:1A:7D:DA:71:13").as_deref(), Some("001a7dda7113"));
        assert_eq!(address("(00-1a-7d-da-71-13)").as_deref(), Some("001a7dda7113"));
        assert_eq!(address("00:1A:7D"), None);
        assert_eq!(address("00:00:00:00:00:00"), None);
        assert_eq!(address("zz:1A:7D:DA:71:13"), None);
    }

    #[test]
    fn ids() {
        let id = r"{2}.\\?\bthenum#{0000110b-0000-1000-8000-00805f9b34fb}_vid&0001004c#7&2a6b&0&001A7DDA7113_C00000000#{6994ad04-93ef-11d0-a3cc-00a0c9223196}";
        assert!(id_has_address(id, "001a7dda7113"));
        assert!(!id_has_address(id, "001a7dda7114"));
        assert!(!id_has_address(id, ""));
    }

    #[test]
    fn names() {
        assert!(worth_listing("WH-1000XM4"));
        assert!(!worth_listing("  "));
        assert!(!worth_listing("00:1A:7D:DA:71:13"));
        assert!(!worth_listing("Unknown"));
    }
}
