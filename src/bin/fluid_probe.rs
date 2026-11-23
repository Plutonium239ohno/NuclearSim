//! Headless checks for the liquid solver.
//! Reports: kinetic energy (does it settle?), compression (mean nearest-
//! neighbour distance vs. the diameter), escapes through walls, and for the
//! oil/water tank, whether the lighter oil ends up on top.

use particle_sim::scenes;
use particle_sim::sim::Sim;
use std::time::Instant;

fn stats(sim: &Sim, label: &str, frame: usize) {
    let mut ke = 0.0f64;
    let mut nn_sum = 0.0f64;
    let mut cnt = 0usize;
    let n = sim.len();
    for i in 0..n {
        let t = &sim.types[sim.kind[i] as usize];
        ke += 0.5 * t.mass as f64 * sim.vel[i].length_squared() as f64;
        if i % 7 == 0 {
            let mut best = f32::MAX;
            for j in 0..n {
                if j != i {
                    best = best.min(sim.pos[i].distance(sim.pos[j]));
                }
            }
            nn_sum += (best / (2.0 * t.radius)) as f64;
            cnt += 1;
        }
    }
    println!("{label} f{frame:4}  n {n}  KE/n {:.1}  nn/diam {:.3}", ke / n as f64, nn_sum / cnt as f64);
}

fn main() {
    let dt = 1.0 / 60.0;
    for scene in ["dam", "oilwater"] {
        let mut sim = Sim::new(1600.0, 900.0);
        if scene == "dam" { scenes::dam_break(&mut sim) } else { scenes::oil_and_water(&mut sim) }
        if let Ok(c) = std::env::var("COH") { for t in sim.types.iter_mut() { if t.fluid { t.cohesion = c.parse().unwrap(); } } }
        if let Ok(c) = std::env::var("ITERS") { sim.cfg.iterations = c.parse().unwrap(); }
        if let Ok(c) = std::env::var("SUB") { sim.cfg.substeps = c.parse().unwrap(); }
        if let Ok(c) = std::env::var("OIL_R") { let k = scenes::species(&sim, "Oil").unwrap(); sim.types[k as usize].radius = c.parse().unwrap(); }
        if let Ok(c) = std::env::var("OIL_V") { let k = scenes::species(&sim, "Oil").unwrap(); sim.types[k as usize].viscosity = c.parse().unwrap(); }
        if std::env::var("JIT").is_ok() { for p in sim.pos.iter_mut() { *p += macroquad::math::vec2(macroquad::rand::gen_range(-0.8, 0.8), macroquad::rand::gen_range(-0.8, 0.8)); } }
        if std::env::var("ONLY_OW").is_ok() && scene == "dam" { continue; }
        let n0 = sim.len();
        let t0 = Instant::now();
        let frames: usize = std::env::var("FRAMES").map(|f| f.parse().unwrap()).unwrap_or(600);
        let mut acc = [0.0f32; 5];
        for f in 0..frames {
            sim.step(dt, None);
            for k in 0..5 { acc[k] += sim.timings.sub[k] / frames as f32; }
            if f % 120 == 119 {
                stats(&sim, scene, f + 1);
                let oil = scenes::species(&sim, "Oil").unwrap();
                let (mut yo, mut no) = (0.0, 0);
                for i in 0..sim.len() { if sim.kind[i] == oil { yo += sim.pos[i].y; no += 1; } }
                println!("    oil y {:.0}", yo / no as f32);
            }
        }
        let ms = t0.elapsed().as_secs_f32() * 1000.0 / frames as f32;
        println!("{scene}: {n0} particles, {ms:.2} ms/frame  (sort/short/integ/pos/vel {:.2?})", acc);
        let oil = scenes::species(&sim, "Oil").unwrap();
        let (mut yo, mut no, mut yw, mut nw) = (0.0, 0, 0.0, 0);
        for i in 0..sim.len() {
            if sim.kind[i] == oil { yo += sim.pos[i].y; no += 1; } else { yw += sim.pos[i].y; nw += 1; }
        }
        println!("  mean y: oil {:.0}  water {:.0}  (smaller = higher)", yo / no as f32, yw / nw as f32);
        if scene == "oilwater" {
            let x0 = 1600.0 * 0.3;
            let x1 = 1600.0 * 0.7;
            let out = sim.pos.iter().filter(|p| p.y > 900.0 * 0.4 && (p.x < x0 || p.x > x1)).count();
            println!("  leaked out of tank: {out}");
        }
    }
}
