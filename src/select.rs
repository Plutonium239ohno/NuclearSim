//! The Select tool: pick a region, then copy, cut, paste, move, turn,
//! mirror and line it up with the grid (see sim_core::selection).

use crate::{Game, snapshot};
use macroquad::prelude::*;
use sim_core::selection::{self, Clip, Kinds};
use sim_core::sim::Sim;
use sim_core::types::ids;

/// Something being carried with the mouse: a paste, or a selection being
/// moved.
pub struct Floating {
    pub clip: Clip,
    /// From the mouse to the clip's centre.
    pub grab: Vec2,
    /// A move: what was lifted and where from (Esc puts it back).
    pub origin: Option<(Clip, Vec2)>,
    /// Dropped when the button is let go (a drag), not on the next click.
    pub drag: bool,
}

#[derive(Default)]
pub struct SelState {
    /// The selected region (it hugs what's in it).
    pub region: Option<Rect>,
    pub kinds: Kinds,
    pub clip: Option<Clip>,
    pub floating: Option<Floating>,
    /// Corner of a selection being dragged out.
    pub start: Option<Vec2>,
    /// What the selection holds, for the panel: (region, when, text),
    /// refreshed twice a second (counting a big selection isn't free).
    pub summary: Option<(Rect, f64, String)>,
}

impl SelState {
    pub fn summary(&mut self, sim: &Sim) -> Option<String> {
        let r = self.region?;
        let now = get_time();
        match &self.summary {
            Some((at, t, s)) if *at == r && now - t < 0.5 => Some(s.clone()),
            _ => {
                let s = selection::copy(sim, r, self.kinds).0.summary();
                self.summary = Some((r, now, s.clone()));
                Some(s)
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Op {
    Copy,
    Cut,
    Paste,
    Duplicate,
    Delete,
    /// Counter-clockwise on screen, radians.
    Rotate(f32),
    FlipX,
    FlipY,
    /// Move it so its corner sits on the grid.
    AlignGrid,
    /// Put every wall end and part corner on the grid.
    SnapPoints,
    SelectAll,
    /// Esc: drop a paste, put a move back, or deselect.
    Cancel,
}

fn grow(r: Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + 2.0 * by, r.h + 2.0 * by)
}

/// The region hugging what's selected in `r`, or None if it's empty.
fn hug(sim: &Sim, r: Rect, kinds: Kinds) -> Option<Rect> {
    let (clip, c) = selection::copy(sim, r, kinds);
    if clip.is_empty() {
        return None;
    }
    let b = clip.bounds();
    Some(grow(Rect::new(b.x + c.x, b.y + c.y, b.w, b.h), 4.0))
}

/// Where a floating clip goes for this mouse position.
pub fn anchor(g: &Game, f: &Floating, mouse: Vec2) -> Vec2 {
    let at = mouse + f.grab;
    if g.snap.grid_on { f.clip.grid_anchor(at, g.snap.grid) } else { at }
}

fn drop_floating(sim: &mut Sim, g: &mut Game, mouse: Vec2) {
    if let Some(f) = g.sel.floating.take() {
        let at = anchor(g, &f, mouse);
        g.sel.region = Some(selection::place(sim, &f.clip, at));
    }
}

/// Lift the selection out of the scene (for a move or a turn in place).
fn lift(sim: &mut Sim, g: &mut Game) -> Option<(Clip, Vec2)> {
    let r = g.sel.region?;
    let (clip, c) = selection::copy(sim, r, g.sel.kinds);
    if clip.is_empty() {
        g.sel.region = None;
        return None;
    }
    snapshot(sim, g);
    selection::delete(sim, r, g.sel.kinds);
    Some((clip, c))
}

/// Change the selection where it is (lift, change, put back).
fn in_place(sim: &mut Sim, g: &mut Game, f: impl FnOnce(&mut Clip, Vec2) -> Vec2) {
    if let Some((mut clip, c)) = lift(sim, g) {
        let at = f(&mut clip, c);
        g.sel.region = Some(selection::place(sim, &clip, at));
    }
}

pub fn op(sim: &mut Sim, g: &mut Game, op: Op, mouse: Vec2) {
    // A paste or move in progress is changed as it's carried.
    if let Some(f) = g.sel.floating.as_mut() {
        match op {
            Op::Rotate(a) => f.clip.rotate(a),
            Op::FlipX => f.clip.flip_x(),
            Op::FlipY => f.clip.flip_y(),
            Op::AlignGrid => {
                g.snap.grid_on = true;
                g.say("Grid on: it lands with its corner on the grid");
            }
            Op::SnapPoints => {
                let at = mouse + f.grab;
                f.clip.snap_points(at, g.snap.grid);
            }
            Op::Cancel | Op::Delete => {
                let f = g.sel.floating.take().unwrap();
                match f.origin {
                    Some((clip, at)) if op == Op::Cancel => {
                        g.sel.region = Some(selection::place(sim, &clip, at));
                        g.say("Move cancelled");
                    }
                    _ => g.say(if op == Op::Cancel { "Paste cancelled" } else { "Deleted" }),
                }
            }
            Op::Copy => g.sel.clip = Some(f.clip.clone()),
            _ => {}
        }
        return;
    }
    let grid = g.snap.grid;
    match op {
        Op::Copy | Op::Cut | Op::Duplicate => {
            let Some(r) = g.sel.region else {
                g.say("Select something first (Select tool, S: drag a box)");
                return;
            };
            let (clip, c) = selection::copy(sim, r, g.sel.kinds);
            if clip.is_empty() {
                g.say("Nothing selected");
                return;
            }
            let what = clip.summary();
            match op {
                Op::Copy => g.say(format!("Copied {what}")),
                Op::Cut => {
                    snapshot(sim, g);
                    selection::delete(sim, r, g.sel.kinds);
                    g.sel.region = None;
                    g.say(format!("Cut {what}"));
                }
                _ => {
                    // A copy to place, starting just beside the original.
                    snapshot(sim, g);
                    let off = vec2(grid.max(20.0), grid.max(20.0));
                    g.sel.floating = Some(Floating { clip: clip.clone(), grab: c + off - mouse, origin: None, drag: false });
                    g.say(format!("Duplicate of {what}: click to place it"));
                }
            }
            g.sel.clip = Some(clip);
        }
        Op::Paste => match g.sel.clip.clone() {
            Some(clip) => {
                snapshot(sim, g);
                g.tool = crate::Tool::Select;
                g.say(format!("Pasting {}: click to place (Shift-click to keep stamping), R/Q/E turn, H mirrors, Esc cancels", clip.summary()));
                g.sel.floating = Some(Floating { clip, grab: Vec2::ZERO, origin: None, drag: false });
            }
            None => g.say("The clipboard is empty: select something and copy it (Ctrl+C)"),
        },
        Op::Delete => {
            if let Some(r) = g.sel.region {
                snapshot(sim, g);
                if selection::delete(sim, r, g.sel.kinds) {
                    g.say("Deleted");
                }
                g.sel.region = None;
            }
        }
        Op::Rotate(a) => in_place(sim, g, |c, at| {
            c.rotate(a);
            at
        }),
        Op::FlipX => in_place(sim, g, |c, at| {
            c.flip_x();
            at
        }),
        Op::FlipY => in_place(sim, g, |c, at| {
            c.flip_y();
            at
        }),
        Op::AlignGrid => in_place(sim, g, |c, at| c.grid_anchor(at, grid)),
        Op::SnapPoints => in_place(sim, g, |c, at| {
            c.snap_points(at, grid);
            at
        }),
        Op::SelectAll => {
            let all = Rect::new(-10.0, -10.0, sim.width + 20.0, sim.height + 20.0);
            g.sel.region = hug(sim, all, g.sel.kinds);
            g.tool = crate::Tool::Select;
        }
        Op::Cancel => g.sel.region = None,
    }
}

/// The mouse with the Select tool (or while carrying something).
pub fn mouse(sim: &mut Sim, g: &mut Game, mouse: Vec2, pressed: bool, released: bool, shift: bool) {
    if let Some(f) = g.sel.floating.as_ref() {
        let drop = if f.drag { released } else { pressed };
        if drop {
            let stamp = shift && !f.drag;
            let clip = f.clip.clone();
            let grab = f.grab;
            drop_floating(sim, g, mouse);
            if stamp {
                // Shift-click: keep stamping copies.
                snapshot(sim, g);
                g.sel.floating = Some(Floating { clip, grab, origin: None, drag: false });
            }
        }
        return;
    }
    if pressed {
        match g.sel.region {
            Some(r) if r.contains(mouse) => {
                if let Some((clip, c)) = lift(sim, g) {
                    g.sel.floating = Some(Floating { clip: clip.clone(), grab: c - mouse, origin: Some((clip, c)), drag: true });
                }
            }
            _ => {
                g.sel.region = None;
                g.sel.start = Some(mouse);
            }
        }
    }
    if released {
        if let Some(a) = g.sel.start.take() {
            let r = Rect::new(a.x.min(mouse.x), a.y.min(mouse.y), (a.x - mouse.x).abs(), (a.y - mouse.y).abs());
            if r.w > 3.0 && r.h > 3.0 {
                g.sel.region = hug(sim, r, g.sel.kinds);
                if g.sel.region.is_none() {
                    g.say("Nothing in that box (see what the selection takes on the Build tab)");
                }
            }
        }
    }
}

/// Keyboard shortcuts. Ctrl+V works with any tool; the rest with Select.
pub fn keys(sim: &mut Sim, g: &mut Game, mouse: Vec2, ctrl: bool, shift: bool) -> bool {
    let mut used = false;
    let mut run = |o: Op, sim: &mut Sim, g: &mut Game| {
        op(sim, g, o, mouse);
        used = true;
    };
    if ctrl && is_key_pressed(KeyCode::V) {
        run(Op::Paste, sim, g);
    }
    let active = g.tool == crate::Tool::Select || g.sel.floating.is_some();
    if !active {
        return used;
    }
    let step = g.snap.angle_step.max(1.0).to_radians();
    let quarter = std::f32::consts::FRAC_PI_2;
    if ctrl {
        if is_key_pressed(KeyCode::C) { run(Op::Copy, sim, g); }
        if is_key_pressed(KeyCode::X) { run(Op::Cut, sim, g); }
        if is_key_pressed(KeyCode::D) { run(Op::Duplicate, sim, g); }
        if is_key_pressed(KeyCode::A) { run(Op::SelectAll, sim, g); }
    } else {
        if is_key_pressed(KeyCode::R) { run(Op::Rotate(if shift { quarter } else { -quarter }), sim, g); }
        if is_key_pressed(KeyCode::Q) { run(Op::Rotate(step), sim, g); }
        if is_key_pressed(KeyCode::E) { run(Op::Rotate(-step), sim, g); }
        if is_key_pressed(KeyCode::H) { run(if shift { Op::FlipY } else { Op::FlipX }, sim, g); }
        if is_key_pressed(KeyCode::Delete) || is_key_pressed(KeyCode::Backspace) { run(Op::Delete, sim, g); }
        if is_key_pressed(KeyCode::Escape) && (g.sel.floating.is_some() || g.sel.region.is_some()) {
            run(Op::Cancel, sim, g);
        }
    }
    used
}

/// The selection box, the box being dragged out, and what's being carried.
pub fn draw(sim: &Sim, g: &Game, mouse: Vec2) {
    let accent = Color::new(0.45, 0.75, 1.0, 0.9);
    if let Some(a) = g.sel.start {
        let r = Rect::new(a.x.min(mouse.x), a.y.min(mouse.y), (a.x - mouse.x).abs(), (a.y - mouse.y).abs());
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.45, 0.75, 1.0, 0.06));
        dashed_rect(r, accent);
    }
    if let (Some(r), None) = (g.sel.region, g.sel.floating.as_ref()) {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.45, 0.75, 1.0, 0.05));
        dashed_rect(r, accent);
        let tip = "drag to move   R turn   H mirror   Ctrl+C / X / D   Del";
        draw_text(tip, r.x, r.y - 5.0, 14.0, Color::new(0.6, 0.8, 1.0, 0.8));
    }
    if let Some(f) = g.sel.floating.as_ref() {
        let at = anchor(g, f, mouse);
        ghost(sim, &f.clip, at);
        let b = f.clip.bounds();
        dashed_rect(Rect::new(b.x + at.x - 4.0, b.y + at.y - 4.0, b.w + 8.0, b.h + 8.0), accent);
        let tip = if f.drag { "release to drop".to_string() } else { "click to place   Shift-click: stamp more   R / Q / E turn   H mirror   Esc".into() };
        draw_text(&tip, b.x + at.x - 4.0, b.y + at.y - 9.0, 14.0, Color::new(0.6, 0.8, 1.0, 0.85));
    }
}

fn dashed_rect(r: Rect, c: Color) {
    let edges = [(vec2(r.x, r.y), vec2(r.x + r.w, r.y)), (vec2(r.x + r.w, r.y), vec2(r.x + r.w, r.y + r.h)), (vec2(r.x + r.w, r.y + r.h), vec2(r.x, r.y + r.h)), (vec2(r.x, r.y + r.h), vec2(r.x, r.y))];
    for (a, b) in edges {
        let len = a.distance(b);
        let d = (b - a).normalize_or_zero();
        let mut t = 0.0;
        while t < len {
            let e = (t + 6.0).min(len);
            draw_line(a.x + d.x * t, a.y + d.y * t, a.x + d.x * e, a.y + d.y * e, 1.0, c);
            t += 10.0;
        }
    }
}

/// A see-through preview of the clip with its centre at `at`.
fn ghost(sim: &Sim, c: &Clip, at: Vec2) {
    let wall = Color::new(0.75, 0.8, 0.95, 0.55);
    for s in &c.walls {
        let (a, b) = (s.a + at, s.b + at);
        draw_line(a.x, a.y, b.x, b.y, (s.half * 2.0).max(1.0), wall);
    }
    let rect = |r: Rect, col: Color| draw_rectangle_lines(r.x + at.x, r.y + at.y, r.w, r.h, 1.0, col);
    for f in &c.fields {
        rect(f.rect, Color::new(0.4, 0.9, 1.0, 0.6));
    }
    for d in &c.drains {
        rect(d.rect, Color::new(1.0, 0.45, 0.4, 0.6));
    }
    for p in &c.pumps {
        rect(p.rect, Color::new(0.6, 0.8, 1.0, 0.6));
        let m = p.rect.center() + at;
        draw_line(m.x, m.y, m.x + p.dir.x * 14.0, m.y + p.dir.y * 14.0, 2.0, Color::new(0.6, 0.8, 1.0, 0.7));
    }
    for d in &c.detectors {
        let col = Color::new(0.55, 1.0, 0.6, 0.6);
        match d.shape {
            sim_core::detector::DetShape::Box(r) => rect(r, col),
            sim_core::detector::DetShape::Line(a, b) => draw_line(a.x + at.x, a.y + at.y, b.x + at.x, b.y + at.y, 1.5, col),
        }
    }
    for r in &c.rods {
        draw_rectangle_lines(r.x - r.half_w + at.x, r.top + at.y, 2.0 * r.half_w, r.len, 1.0, Color::new(0.7, 0.72, 0.8, 0.6));
    }
    let spawn = Color::new(1.0, 0.82, 0.35, 0.8);
    for s in &c.spawners {
        let p = s.pos + at;
        draw_circle_lines(p.x, p.y, 5.0, 1.0, spawn);
        if s.vel.length() > 0.5 {
            let tip = p + s.vel.normalize() * 16.0;
            draw_line(p.x, p.y, tip.x, tip.y, 1.5, spawn);
        }
    }
    for p in c.sites.iter().map(|s| s.pos).chain(c.sources.iter().copied()) {
        draw_circle_lines(p.x + at.x, p.y + at.y, 4.0, 1.0, Color::new(1.0, 0.6, 0.3, 0.7));
    }
    for p in &c.particles {
        let q = p.pos + at;
        let r = if p.kind == ids::NUCLEUS { 3.0 } else { sim.types.get(p.kind as usize).map_or(1.5, |t| t.radius.max(1.0)) };
        draw_circle(q.x, q.y, r, Color::new(0.85, 0.9, 1.0, 0.45));
    }
}
