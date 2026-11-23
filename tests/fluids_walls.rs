use macroquad::math::vec2;
use particle_sim::scenes;
use particle_sim::sim::Sim;
use particle_sim::types::ids;

fn mean_y(sim: &Sim, kind: u8) -> f32 {
    let ys: Vec<f32> = (0..sim.len()).filter(|&i| sim.kind[i] == kind).map(|i| sim.pos[i].y).collect();
    ys.iter().sum::<f32>() / ys.len() as f32
}

#[test]
fn dam_break_stays_sane() {
    let mut sim = Sim::new(1200.0, 700.0);
    scenes::dam_break(&mut sim);
    let n = sim.len();
    for _ in 0..240 {
        sim.step(1.0 / 60.0, None);
    }
    assert_eq!(sim.len(), n);
    assert!(sim.pos.iter().all(|p| p.is_finite() && p.x >= 0.0 && p.y >= 0.0 && p.x <= 1200.0 && p.y <= 700.0));
    let max_v = sim.vel.iter().map(|v| v.length()).fold(0.0, f32::max);
    assert!(max_v < sim.cfg.max_speed + 1.0);
}

#[test]
fn water_keeps_its_volume() {
    // A settled column must not collapse: nearest neighbours stay near one diameter.
    let mut sim = Sim::new(400.0, 600.0);
    sim.cfg.uniform_gravity = true;
    sim.cfg.coulomb = false;
    let water = scenes::species(&sim, "Water").unwrap();
    let n = scenes::fill_rect(&mut sim, macroquad::math::Rect::new(0.0, 200.0, 400.0, 400.0), water);
    for _ in 0..300 {
        sim.step(1.0 / 60.0, None);
    }
    let top = sim.pos.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    // Started at y=200; allow a few percent of compression.
    assert!(top < 200.0 + 0.08 * 400.0, "column collapsed: top at {top} ({n} particles)");
}

#[test]
fn oil_rises_through_water() {
    let mut sim = Sim::new(1600.0, 900.0);
    scenes::oil_and_water(&mut sim);
    let oil = scenes::species(&sim, "Oil").unwrap();
    let water = scenes::species(&sim, "Water").unwrap();
    for _ in 0..480 {
        sim.step(1.0 / 60.0, None);
    }
    assert!(mean_y(&sim, oil) < mean_y(&sim, water) - 100.0, "oil should float");
}

#[test]
fn walls_stop_fast_particles() {
    let mut sim = Sim::new(800.0, 400.0);
    sim.cfg.coulomb = false;
    sim.walls.add(vec2(400.0, 0.0), vec2(400.0, 400.0));
    // Includes electrons, which pass through other particles but not walls.
    for k in 0..40 {
        let kind = if k % 2 == 0 { ids::NEUTRON } else { ids::ELECTRON };
        sim.add(vec2(100.0 + k as f32 * 5.0, 10.0 + k as f32 * 9.0), vec2(1500.0, 37.0), kind);
    }
    for _ in 0..120 {
        sim.step(1.0 / 60.0, None);
        assert!(sim.pos.iter().all(|p| p.x < 400.0), "a particle tunnelled through the wall");
    }
}
