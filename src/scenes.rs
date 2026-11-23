//! Preset scenes shared by the app and the headless probes.

use crate::sim::Sim;
use crate::types::ids;
use macroquad::math::{Rect, Vec2, vec2};
use macroquad::rand::gen_range;

/// Id of the first species with this name.
pub fn species(sim: &Sim, name: &str) -> Option<u8> {
    sim.types.iter().position(|t| t.name == name).map(|k| k as u8)
}

/// Fill a rectangle with a hexagonal lattice of `kind` at contact spacing
/// (the fluid rest density), skipping walls. Returns how many were added.
pub fn fill_rect(sim: &mut Sim, rect: Rect, kind: u8) -> usize {
    let r = sim.types[kind as usize].radius;
    let dx = 2.0 * r;
    let dy = dx * 0.866_025_4;
    let mut n = 0;
    let mut row = 0;
    let mut y = rect.y + r;
    while y <= rect.y + rect.h - r {
        let mut x = rect.x + r + if row % 2 == 1 { r } else { 0.0 };
        while x <= rect.x + rect.w - r {
            let p = vec2(x, y);
            if !sim.walls.blocked(p, r) && p.x >= r && p.y >= r && p.x <= sim.width - r && p.y <= sim.height - r {
                sim.add(p, Vec2::ZERO, kind);
                n += 1;
            }
            x += dx;
        }
        y += dy;
        row += 1;
    }
    n
}

fn liquid_config(sim: &mut Sim) {
    sim.cfg.uniform_gravity = true;
    sim.cfg.nuclear = false;
    sim.cfg.coulomb = false;
    sim.cfg.mutual_gravity = false;
    sim.cfg.rules = false;
    sim.cfg.drag = 0.05;
}

/// Classic dam break: a water column released into a basin with a few
/// obstacles, plus a pocket of oil.
pub fn dam_break(sim: &mut Sim) {
    sim.clear();
    sim.walls.clear();
    liquid_config(sim);
    let (w, h) = (sim.width, sim.height);
    let water = species(sim, "Water").unwrap_or(ids::FIRST_CUSTOM);
    let oil = species(sim, "Oil").unwrap_or(water);
    // Gate-less reservoir on the left, a ramp and a post in the basin.
    sim.walls.add(vec2(w * 0.55, h), vec2(w * 0.75, h * 0.82));
    sim.walls.add(vec2(w * 0.88, h), vec2(w * 0.88, h * 0.7));
    sim.walls.add(vec2(w * 0.40, h * 0.30), vec2(w * 0.52, h * 0.36));
    fill_rect(sim, Rect::new(0.0, h * 0.25, w * 0.28, h * 0.75), water);
    fill_rect(sim, Rect::new(w * 0.30, h * 0.05, w * 0.14, h * 0.20), oil);
}

/// A tank of water with blobs of oil mixed through it: the oil rises and
/// collects on top.
pub fn oil_and_water(sim: &mut Sim) {
    sim.clear();
    sim.walls.clear();
    liquid_config(sim);
    let (w, h) = (sim.width, sim.height);
    let water = species(sim, "Water").unwrap_or(ids::FIRST_CUSTOM);
    let oil = species(sim, "Oil").unwrap_or(water);
    let x0 = w * 0.3;
    let x1 = w * 0.7;
    sim.walls.add(vec2(x0, h * 0.35), vec2(x0, h));
    sim.walls.add(vec2(x1, h * 0.35), vec2(x1, h));
    let half = sim.walls.half;
    let inner = Rect::new(x0 + half, h * 0.45, x1 - x0 - 2.0 * half, h * 0.55);
    // Oil blobs first (they are bigger), then water fills around them.
    let mut blobs: Vec<(Vec2, f32)> = Vec::new();
    for _ in 0..400 {
        if blobs.len() == 14 {
            break;
        }
        let r = gen_range(18.0, 40.0);
        let c = vec2(gen_range(inner.x + r, inner.x + inner.w - r), gen_range(inner.y + inner.h * 0.3, inner.y + inner.h - r));
        if blobs.iter().all(|(q, rq)| q.distance(c) > r + rq + 8.0) {
            blobs.push((c, r));
        }
    }
    for &(c, r) in &blobs {
        fill_disc(sim, c, r, oil);
    }
    let start = sim.len();
    fill_rect(sim, inner, water);
    // Drop water that landed inside a blob.
    let ro = sim.types[oil as usize].radius;
    let rw = sim.types[water as usize].radius;
    let pos = sim.pos.clone();
    sim.retain(|i| i < start || blobs.iter().all(|(c, r)| pos[i].distance(*c) > r + ro + rw));
}

/// Hex-lattice disc of `kind`.
pub fn fill_disc(sim: &mut Sim, c: Vec2, radius: f32, kind: u8) {
    let r = sim.types[kind as usize].radius;
    let (dx, dy) = (2.0 * r, 2.0 * r * 0.866_025_4);
    let mut row = 0;
    let mut y = c.y - radius;
    while y <= c.y + radius {
        let mut x = c.x - radius + if row % 2 == 1 { r } else { 0.0 };
        while x <= c.x + radius {
            let p = vec2(x, y);
            if p.distance(c) <= radius && !sim.walls.blocked(p, r) {
                sim.add(p, Vec2::ZERO, kind);
            }
            x += dx;
        }
        y += dy;
        row += 1;
    }
}
