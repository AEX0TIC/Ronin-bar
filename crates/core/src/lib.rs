//! Ronin Bar core: everything that needs no window or Windows API, so it is unit-tested on
//! any machine. No `unsafe` code is allowed in this crate.
#![forbid(unsafe_code)]

pub mod dock;
pub mod settings;
pub mod theme;
pub mod wallhaven;

/// CPU use in percent between two samples of Windows' (idle, kernel, user) time counters.
/// Kernel time includes idle time.
pub fn cpu_percent(prev: (u64, u64, u64), now: (u64, u64, u64)) -> u8 {
    let idle = now.0.saturating_sub(prev.0);
    let total = now.1.saturating_sub(prev.1) + now.2.saturating_sub(prev.2);
    match (idle * 100).checked_div(total) { Some(i) => 100 - i.min(100) as u8, None => 0 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cpu_percent() {
        assert_eq!(super::cpu_percent((0, 0, 0), (75, 80, 20)), 25);   // 100 ticks, 75 idle
        assert_eq!(super::cpu_percent((0, 0, 0), (0, 0, 0)), 0);       // no time passed
        assert_eq!(super::cpu_percent((10, 10, 10), (5, 5, 5)), 0);    // counters went backwards
        assert_eq!(super::cpu_percent((0, 0, 0), (0, 50, 50)), 100);
    }
}
