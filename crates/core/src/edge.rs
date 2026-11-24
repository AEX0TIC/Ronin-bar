//! Which side of the screen the bar sits on, and where exactly it goes there. Plain maths, so it
//! is tested on any machine.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Edge {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

pub const EDGES: [Edge; 4] = [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right];

impl Edge {
    /// "top", "bottom", "left" or "right" (as saved in the settings); anything else is the top.
    pub fn parse(s: &str) -> Edge {
        match s { "bottom" => Edge::Bottom, "left" => Edge::Left, "right" => Edge::Right, _ => Edge::Top }
    }

    pub fn id(self) -> &'static str {
        match self { Edge::Top => "top", Edge::Bottom => "bottom", Edge::Left => "left", Edge::Right => "right" }
    }

    pub fn name(self) -> &'static str {
        match self { Edge::Top => "Top", Edge::Bottom => "Bottom", Edge::Left => "Left", Edge::Right => "Right" }
    }

    /// Left and right bars stand upright.
    pub fn vertical(self) -> bool { matches!(self, Edge::Left | Edge::Right) }

    /// The bar's rectangle (x, y, width, height) on a `sw` × `sh` screen, `thick` deep, slid
    /// `hide` off the screen (0 = fully shown, `thick` = fully hidden).
    pub fn strip(self, sw: i32, sh: i32, thick: i32, hide: i32) -> (i32, i32, i32, i32) {
        match self {
            Edge::Top => (0, -hide, sw, thick),
            Edge::Bottom => (0, sh - thick + hide, sw, thick),
            Edge::Left => (-hide, 0, thick, sh),
            Edge::Right => (sw - thick + hide, 0, thick, sh),
        }
    }

    /// Whether a screen point (x, y) touches this edge of a `sw` × `sh` screen.
    pub fn touches(self, sw: i32, sh: i32, x: i32, y: i32) -> bool {
        match self {
            Edge::Top => y <= 1,
            Edge::Bottom => y >= sh - 2,
            Edge::Left => x <= 1,
            Edge::Right => x >= sw - 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for e in EDGES { assert_eq!(Edge::parse(e.id()), e); }
        assert_eq!(Edge::parse("sideways"), Edge::Top);
        assert!(Edge::Left.vertical() && Edge::Right.vertical() && !Edge::Top.vertical() && !Edge::Bottom.vertical());
    }

    #[test]
    fn strips() {
        let (sw, sh) = (1920, 1080);
        assert_eq!(Edge::Top.strip(sw, sh, 45, 0), (0, 0, 1920, 45));
        assert_eq!(Edge::Top.strip(sw, sh, 45, 45), (0, -45, 1920, 45));
        assert_eq!(Edge::Bottom.strip(sw, sh, 45, 0), (0, 1035, 1920, 45));
        assert_eq!(Edge::Bottom.strip(sw, sh, 45, 45), (0, 1080, 1920, 45));
        assert_eq!(Edge::Left.strip(sw, sh, 60, 0), (0, 0, 60, 1080));
        assert_eq!(Edge::Right.strip(sw, sh, 60, 60), (1920, 0, 60, 1080));
    }

    #[test]
    fn touching() {
        assert!(Edge::Top.touches(1920, 1080, 500, 0));
        assert!(!Edge::Top.touches(1920, 1080, 500, 5));
        assert!(Edge::Bottom.touches(1920, 1080, 500, 1079));
        assert!(Edge::Left.touches(1920, 1080, 0, 500));
        assert!(Edge::Right.touches(1920, 1080, 1919, 500));
    }
}
