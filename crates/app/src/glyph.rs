//! Small line icons for the bar, drawn with egui's painter so they stay sharp at any scaling.
use eframe::egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph { Gear, Power, Cpu, Ram, Wallpaper, Theme, Apps }

fn arc(p: &Painter, c: Pos2, r: f32, a0: f32, a1: f32, s: Stroke) {
    let n = ((a1 - a0).abs() * r / 2.0).ceil().max(6.0) as usize;
    let pts: Vec<Pos2> = (0..=n).map(|i| { let a = a0 + (a1 - a0) * i as f32 / n as f32; pos2(c.x + r * a.cos(), c.y - r * a.sin()) }).collect();
    p.add(Shape::line(pts, s));
}

/// The Ronin mark: an open ensō (no master) cut by a katana through the gap.
pub fn mark(p: &Painter, r: Rect, ring: Color32, blade: Color32) {
    let c = r.center();
    let rad = r.width().min(r.height()) * 0.36;
    let (a0, a1) = (78f32.to_radians(), (78f32 + 318.0).to_radians());
    let n = 64;
    for i in 0..n {
        let t0 = i as f32 / n as f32;
        let t1 = (i + 1) as f32 / n as f32;
        let w = rad * (0.10 + 0.30 * (1.0 - t0).powf(0.55) * (t0 * 10.0 + 0.35).min(1.0));
        let (b0, b1) = (a0 + (a1 - a0) * t0, a0 + (a1 - a0) * t1);
        p.line_segment([pos2(c.x + rad * b0.cos(), c.y - rad * b0.sin()), pos2(c.x + rad * b1.cos(), c.y - rad * b1.sin())], Stroke::new(w, ring));
    }
    let ang = 54f32.to_radians();
    let u = vec2(ang.cos(), -ang.sin());
    let s = rad / 300.0;
    p.line_segment([c - u * 170.0 * s, c + u * 420.0 * s], Stroke::new((rad * 0.09).max(1.2), blade));
    p.line_segment([c - u * 410.0 * s, c - u * 186.0 * s], Stroke::new((rad * 0.13).max(1.6), ring));
    let n2 = vec2(-u.y, u.x);
    p.line_segment([c - u * 176.0 * s - n2 * 46.0 * s, c - u * 176.0 * s + n2 * 46.0 * s], Stroke::new((rad * 0.06).max(1.0), blade));
}

pub fn draw(p: &Painter, r: Rect, g: Glyph, col: Color32) {
    let c = r.center();
    let u = r.width().min(r.height()) / 16.0; // drawn on a 16-unit grid
    let s = Stroke::new(1.35 * u.max(0.9), col);
    match g {
        Glyph::Gear => {
            p.circle_stroke(c, 4.2 * u, s);
            p.circle_stroke(c, 1.6 * u, s);
            for i in 0..8 {
                let a = i as f32 * TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * 4.6 * u, c + d * 6.6 * u], Stroke::new(2.0 * u, col));
            }
        }
        Glyph::Power => {
            arc(p, c + vec2(0.0, 0.6 * u), 5.4 * u, PI / 2.0 + 0.75, PI / 2.0 + TAU - 0.75, s);
            p.line_segment([c + vec2(0.0, -6.4 * u), c + vec2(0.0, -0.6 * u)], s);
        }
        Glyph::Cpu => {
            let b = Rect::from_center_size(c, vec2(9.0 * u, 9.0 * u));
            p.rect_stroke(b, 1.5 * u, s, eframe::egui::StrokeKind::Middle);
            p.rect_filled(Rect::from_center_size(c, vec2(3.4 * u, 3.4 * u)), 0.6 * u, col);
            for k in [-2.6f32, 0.0, 2.6] {
                for (a, d) in [(vec2(k, -4.5), vec2(0.0, -2.0)), (vec2(k, 4.5), vec2(0.0, 2.0)), (vec2(-4.5, k), vec2(-2.0, 0.0)), (vec2(4.5, k), vec2(2.0, 0.0))] {
                    p.line_segment([c + a * u, c + (a + d) * u], Stroke::new(1.0 * u.max(0.9), col));
                }
            }
        }
        Glyph::Ram => {
            let b = Rect::from_center_size(c - vec2(0.0, 1.0 * u), vec2(13.0 * u, 6.0 * u));
            p.rect_stroke(b, 1.0 * u, s, eframe::egui::StrokeKind::Middle);
            for k in [-3.8f32, -1.3, 1.3, 3.8] { p.rect_filled(Rect::from_center_size(c + vec2(k, -1.0) * u, vec2(1.5 * u, 2.6 * u)), 0.3 * u, col); }
            for k in [-5.0f32, -3.0, -1.0, 1.0, 3.0, 5.0] { p.line_segment([c + vec2(k, 2.2) * u, c + vec2(k, 4.0) * u], Stroke::new(1.0 * u.max(0.9), col)); }
        }
        Glyph::Wallpaper => {
            let b = Rect::from_center_size(c, vec2(13.0 * u, 10.0 * u));
            p.rect_stroke(b, 1.6 * u, s, eframe::egui::StrokeKind::Middle);
            p.add(Shape::line(vec![c + vec2(-5.5, 4.0) * u, c + vec2(-1.5, -0.5) * u, c + vec2(1.0, 2.0) * u, c + vec2(3.0, 0.2) * u, c + vec2(5.5, 3.2) * u], s));
            p.circle_filled(c + vec2(2.8, -2.4) * u, 1.3 * u, col);
        }
        Glyph::Theme => {
            p.circle_stroke(c, 5.6 * u, s);
            let pts: Vec<Pos2> = (0..=24).map(|i| { let a = PI / 4.0 + PI * i as f32 / 24.0; c + vec2(a.cos(), -a.sin()) * 5.6 * u }).collect();
            p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
        }
        Glyph::Apps => {
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                p.rect_stroke(Rect::from_center_size(c + vec2(x, y) * 3.2 * u, vec2(4.4 * u, 4.4 * u)), 1.2 * u, s, eframe::egui::StrokeKind::Middle);
            }
        }
    }
}

/// Speaker with 0–2 sound waves, or a cross when muted.
pub fn volume(p: &Painter, r: Rect, level: f32, muted: bool, col: Color32) {
    let c = r.center() - vec2(2.0, 0.0) * (r.height() / 16.0);
    let u = r.width().min(r.height()) / 16.0;
    let s = Stroke::new(1.35 * u.max(0.9), col);
    let body = vec![c + vec2(-5.0, -2.2) * u, c + vec2(-2.4, -2.2) * u, c + vec2(1.0, -5.2) * u, c + vec2(1.0, 5.2) * u, c + vec2(-2.4, 2.2) * u, c + vec2(-5.0, 2.2) * u];
    p.add(Shape::closed_line(body, s));
    if muted {
        p.line_segment([c + vec2(3.6, -2.4) * u, c + vec2(8.2, 2.4) * u], s);
        p.line_segment([c + vec2(3.6, 2.4) * u, c + vec2(8.2, -2.4) * u], s);
    } else {
        if level > 0.01 { arc(p, c + vec2(1.0, 0.0) * u, 3.4 * u, -0.8, 0.8, s); }
        if level > 0.5 { arc(p, c + vec2(1.0, 0.0) * u, 6.4 * u, -0.8, 0.8, s); }
    }
}

/// Wi-Fi style arcs; dimmed with a slash when offline.
pub fn network(p: &Painter, r: Rect, online: bool, col: Color32, off: Color32) {
    let u = r.width().min(r.height()) / 16.0;
    let c = r.center() + vec2(0.0, 4.6 * u);
    let k = if online { col } else { off };
    let s = Stroke::new(1.4 * u.max(0.9), k);
    p.circle_filled(c, 1.3 * u, k);
    for rad in [4.0f32, 7.4, 10.6] { arc(p, c, rad * u, PI / 4.0, 3.0 * PI / 4.0, s); }
    if !online { p.line_segment([c + vec2(-6.5, -10.5) * u, c + vec2(6.5, 1.5) * u], Stroke::new(1.6 * u, col)); }
}

/// Battery outline with its charge, and a bolt while charging.
pub fn battery(p: &Painter, r: Rect, pct: u8, charging: bool, col: Color32, low: Color32) {
    let u = r.width().min(r.height()) / 16.0;
    let c = r.center();
    let b = Rect::from_center_size(c - vec2(0.8, 0.0) * u, vec2(13.0 * u, 7.6 * u));
    p.rect_stroke(b, 1.8 * u, Stroke::new(1.3 * u.max(0.9), col), eframe::egui::StrokeKind::Middle);
    p.rect_filled(Rect::from_center_size(pos2(b.right() + 1.2 * u, c.y), vec2(1.4 * u, 3.2 * u)), 0.5 * u, col);
    let inner = b.shrink(1.9 * u);
    let w = inner.width() * (pct.min(100) as f32 / 100.0);
    let fill = if pct <= 15 && !charging { low } else { col };
    p.rect_filled(Rect::from_min_size(inner.min, vec2(w.max(0.8 * u), inner.height())), 0.8 * u, fill);
    if charging {
        let bolt = vec![c + vec2(0.4, -5.6) * u, c + vec2(-2.6, 0.6) * u, c + vec2(-0.2, 0.6) * u, c + vec2(-1.0, 5.6) * u, c + vec2(2.2, -0.8) * u, c + vec2(-0.2, -0.8) * u];
        p.add(Shape::closed_line(bolt, Stroke::new(1.0 * u, Color32::BLACK)));
    }
}
