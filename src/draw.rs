//! Precise drawing: wall shapes (line, rectangle, circle, polygon), grid
//! snapping and angle snapping, and the readouts shown while drawing.

use macroquad::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Freehand,
    Line,
    Rect,
    Circle,
    Polygon,
}

pub const SHAPES: [(Shape, &str); 5] = [
    (Shape::Freehand, "Freehand"),
    (Shape::Line, "Line"),
    (Shape::Rect, "Rectangle"),
    (Shape::Circle, "Circle"),
    (Shape::Polygon, "Polygon"),
];

pub struct Snap {
    /// Snap points to the grid (G).
    pub grid_on: bool,
    /// Grid spacing (px).
    pub grid: f32,
    /// Angle step (degrees) used while Ctrl is held.
    pub angle_step: f32,
}

impl Default for Snap {
    fn default() -> Self {
        Self { grid_on: false, grid: 20.0, angle_step: 15.0 }
    }
}

impl Snap {
    /// p on the grid (if the grid is on).
    pub fn point(&self, p: Vec2) -> Vec2 {
        if self.grid_on && self.grid > 0.5 { (p / self.grid).round() * self.grid } else { p }
    }

    /// `d` turned to the nearest multiple of the angle step (same length).
    pub fn angle(&self, d: Vec2) -> Vec2 {
        let len = d.length();
        if len < 1e-6 {
            return d;
        }
        let step = self.angle_step.max(1.0).to_radians();
        let a = (d.y.atan2(d.x) / step).round() * step;
        Vec2::from_angle(a) * len
    }

    /// End point b from a: snapped to the angle step (Ctrl), else to the grid.
    pub fn end(&self, a: Vec2, b: Vec2, ctrl: bool) -> Vec2 {
        if ctrl { a + self.angle(b - a) } else { self.point(b) }
    }
}

/// Angle of d in degrees, counter-clockwise from the +x axis as seen on the
/// screen (y points down there).
pub fn screen_angle(d: Vec2) -> f32 {
    let a = (-d.y).atan2(d.x).to_degrees();
    if a < 0.0 { a + 360.0 } else { a }
}

/// Wall segments of a shape dragged from a to b. Rect: corners a, b
/// (Ctrl: a square). Circle: centre a, through b, `sides` sides.
pub fn segments(shape: Shape, a: Vec2, b: Vec2, sides: u32, ctrl: bool) -> Vec<(Vec2, Vec2)> {
    match shape {
        Shape::Line => vec![(a, b)],
        Shape::Rect => {
            let b = if ctrl {
                let s = (b.x - a.x).abs().max((b.y - a.y).abs());
                vec2(a.x + s * (b.x - a.x).signum(), a.y + s * (b.y - a.y).signum())
            } else {
                b
            };
            let (c, d) = (vec2(b.x, a.y), vec2(a.x, b.y));
            vec![(a, c), (c, b), (b, d), (d, a)]
        }
        Shape::Circle => {
            let r = a.distance(b);
            let n = sides.max(3);
            let a0 = (b - a).y.atan2((b - a).x);
            let pts: Vec<Vec2> = (0..n).map(|k| a + Vec2::from_angle(a0 + k as f32 * std::f32::consts::TAU / n as f32) * r).collect();
            (0..n as usize).map(|k| (pts[k], pts[(k + 1) % n as usize])).collect()
        }
        Shape::Freehand | Shape::Polygon => Vec::new(),
    }
}

/// What to show next to the cursor while dragging a shape.
pub fn readout(shape: Shape, a: Vec2, b: Vec2, sides: u32, ctrl: bool) -> String {
    match shape {
        Shape::Rect => {
            let s = segments(Shape::Rect, a, b, 4, ctrl);
            let (w, h) = ((s[0].1.x - s[0].0.x).abs(), (s[1].1.y - s[1].0.y).abs());
            format!("{w:.0} × {h:.0} px")
        }
        Shape::Circle => format!("r {:.0} px   {} sides", a.distance(b), sides.max(3)),
        _ => format!("{:.0} px   {:.0}°", a.distance(b), screen_angle(b - a)),
    }
}

/// Faint grid over the world (main view: world = screen).
pub fn draw_grid(snap: &Snap, w: f32, h: f32, left: f32) {
    if !snap.grid_on || snap.grid < 4.0 {
        return;
    }
    let c = Color::new(1.0, 1.0, 1.0, 0.05);
    let mut x = (left / snap.grid).ceil() * snap.grid;
    while x < w {
        draw_line(x, 0.0, x, h, 1.0, c);
        x += snap.grid;
    }
    let mut y = 0.0;
    while y < h {
        draw_line(left, y, w, y, 1.0, c);
        y += snap.grid;
    }
}

/// Preview of segments being drawn (thick, translucent).
pub fn preview(segs: &[(Vec2, Vec2)], thickness: f32) {
    for &(a, b) in segs {
        draw_line(a.x, a.y, b.x, b.y, thickness.max(1.0), Color::new(0.6, 0.65, 0.8, 0.5));
    }
}

/// Text next to the cursor (screen coordinates).
pub fn label(text: &str, at: Vec2) {
    draw_text(text, at.x + 14.0, at.y + 22.0, 18.0, Color::new(0.85, 0.9, 1.0, 0.95));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_snap_keeps_length() {
        let s = Snap::default();
        let d = s.angle(vec2(100.0, 9.0)); // ~5° -> 0°
        assert!((d - vec2(100.4, 0.0)).length() < 0.1, "{d:?}");
        let d = s.angle(vec2(70.0, -72.0)); // ~45.8° up -> 45°
        assert!((screen_angle(d) - 45.0).abs() < 1e-3 && (d.length() - vec2(70.0, -72.0).length()).abs() < 1e-3);
    }

    #[test]
    fn grid_snap() {
        let s = Snap { grid_on: true, grid: 20.0, ..Snap::default() };
        assert_eq!(s.point(vec2(31.0, 49.0)), vec2(40.0, 40.0));
        let off = Snap::default();
        assert_eq!(off.point(vec2(31.0, 49.0)), vec2(31.0, 49.0));
    }

    #[test]
    fn shapes() {
        let r = segments(Shape::Rect, vec2(0.0, 0.0), vec2(30.0, -10.0), 0, true);
        assert_eq!(r.len(), 4);
        assert_eq!(r[1].1, vec2(30.0, -30.0)); // Ctrl: a square
        let c = segments(Shape::Circle, vec2(100.0, 100.0), vec2(150.0, 100.0), 6, false);
        assert_eq!(c.len(), 6);
        assert!(c.iter().all(|(a, _)| (a.distance(vec2(100.0, 100.0)) - 50.0).abs() < 1e-3));
        assert_eq!(c[0].0, vec2(150.0, 100.0)); // starts where the drag ended
        assert!((c[5].1 - c[0].0).length() < 1e-3); // closed
    }
}
