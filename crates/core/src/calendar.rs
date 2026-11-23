//! Dates for the calendar popup: month lengths, weekdays and the month grid.

pub const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

pub fn is_leap(y: i32) -> bool { (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 }

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m { 1 | 3 | 5 | 7 | 8 | 10 | 12 => 31, 4 | 6 | 9 | 11 => 30, 2 if is_leap(y) => 29, 2 => 28, _ => 0 }
}

/// Day of the week, 0 = Sunday.
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    (y + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400) + T[(m.clamp(1, 12) - 1) as usize] + d as i32).rem_euclid(7) as u32
}

/// The month before or after (`step` months), as (year, month).
pub fn shift(y: i32, m: u32, step: i32) -> (i32, u32) {
    let i = y * 12 + (m.clamp(1, 12) as i32 - 1) + step;
    (i.div_euclid(12), i.rem_euclid(12) as u32 + 1)
}

/// Six weeks, Sunday first; `None` for days outside the month.
pub fn grid(y: i32, m: u32) -> [[Option<u32>; 7]; 6] {
    let mut g = [[None; 7]; 6];
    let first = weekday(y, m, 1) as usize;
    for d in 1..=days_in_month(y, m) {
        let i = first + d as usize - 1;
        g[i / 7][i % 7] = Some(d);
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_dates() {
        assert_eq!(weekday(2026, 11, 23), 1, "Monday 23 November 2026");
        assert_eq!(weekday(2000, 1, 1), 6, "Saturday");
        assert_eq!(weekday(2024, 2, 29), 4, "Thursday");
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 13), 0);
    }

    #[test]
    fn months_and_grid() {
        assert_eq!(shift(2026, 1, -1), (2025, 12));
        assert_eq!(shift(2026, 12, 1), (2027, 1));
        assert_eq!(shift(2026, 11, 14), (2028, 1));
        let g = grid(2026, 11);
        assert_eq!(g[0][0], Some(1), "1 Nov 2026 is a Sunday");
        assert_eq!(g[3][1], Some(23));
        assert_eq!(g.iter().flatten().flatten().count(), 30);
        assert_eq!(grid(2026, 2)[0][0], Some(1), "Feb 2026 also starts on Sunday");
    }
}
