mod ui;

use macroquad::prelude::*;
use macroquad::ui::root_ui;
use particle_sim::sim::{MouseForce, Sim};
use particle_sim::types::ids;
use particle_sim::scenes;
use particle_sim::render::{self, Renderer};
use std::time::Instant;
use ui::{Perf, Tool, UiState};

fn conf() -> Conf {
    Conf {
        window_title: "Particle Sim".to_owned(),
        window_width: 1600,
        window_height: 900,
        window_resizable: true,
        high_dpi: false,
        platform: miniquad::conf::Platform {
            // PARTICLE_SIM_X11=1 forces the X11 backend (XWayland).
            linux_backend: if std::env::var("PARTICLE_SIM_X11").is_ok() {
                miniquad::conf::LinuxBackend::X11Only
            } else {
                miniquad::conf::LinuxBackend::WaylandWithX11Fallback
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Clear and fill with a proton/neutron/electron plasma, nuclear mode on.
fn load_nuclear_soup(sim: &mut Sim, count: usize) {
    sim.clear();
    let saved: Vec<bool> = sim.types.iter().map(|t| t.in_random_spawn).collect();
    for (k, t) in sim.types.iter_mut().enumerate() {
        t.in_random_spawn = matches!(k as u8, ids::NEUTRON | ids::PROTON | ids::ELECTRON);
    }
    sim.spawn_random(count);
    for (t, s) in sim.types.iter_mut().zip(saved) {
        t.in_random_spawn = s;
    }
    sim.cfg.nuclear = true;
    sim.cfg.coulomb = true;
    sim.cfg.uniform_gravity = false;
}

#[macroquad::main(conf)]
async fn main() {
    rand::srand(miniquad::date::now() as u64);
    let mut renderer = Renderer::new();
    let skin = particle_sim::skin::dark_skin();
    root_ui().push_skin(&skin);

    let mut sim = Sim::new(screen_width(), screen_height());
    sim.cfg.rules = true;
    {
        let types = sim.types.clone();
        sim.rules.randomize_where(|k| types[k].custom && !types[k].fluid);
    }
    sim.spawn_random(3000);

    let mut st = UiState::new(&sim);
    let mut perf = Perf { step_ms: 0.0, far_ms: 0.0, rules_ms: 0.0 };

    // Dev automation: PARTICLE_SIM_SHOT=<file.png> renders ~4 s (optionally of
    // the nuclear soup with PARTICLE_SIM_SOUP=1), saves the window and exits.
    let auto_shot = std::env::var("PARTICLE_SIM_SHOT").ok();
    if std::env::var("PARTICLE_SIM_SOUP").is_ok() {
        load_nuclear_soup(&mut sim, 6000);
    }
    // PARTICLE_SIM_SCENE=dam|oil|soup, PARTICLE_SIM_VIEW=speed|element|trails.
    match std::env::var("PARTICLE_SIM_SCENE").as_deref() {
        Ok("dam") => scenes::dam_break(&mut sim),
        Ok("oil") => scenes::oil_and_water(&mut sim),
        Ok("soup") => load_nuclear_soup(&mut sim, 6000),
        _ => {}
    }
    match std::env::var("PARTICLE_SIM_VIEW").as_deref() {
        Ok("speed") => st.view.mode = render::ViewMode::Speed,
        Ok("element") => st.view.mode = render::ViewMode::Element,
        Ok("trails") => st.view.trails = true,
        Ok("dots") => st.view.smooth_fluid = false,
        _ => {}
    }
    // Wall tool: last committed point of the current stroke, and where a
    // Shift-drag straight line started.
    let mut stroke: Option<Vec2> = None;
    let mut line_start: Option<Vec2> = None;
    let mut frame: u64 = 0;
    let mut shot_request: Option<String> = None;

    loop {
        frame += 1;
        if is_key_pressed(KeyCode::F12) {
            shot_request = Some(format!("screenshot-{}.png", miniquad::date::now() as u64));
        }
        if let Some(p) = &auto_shot {
            if frame == 240 && std::env::var("PARTICLE_SIM_HOLD").is_err() {
                shot_request = Some(p.clone());
            }
        }

        sim.width = screen_width();
        sim.height = screen_height();
        let paintable: Vec<u8> = (0..sim.types.len() as u8).filter(|&k| !sim.types[k as usize].ghost).collect();

        // ---- Keyboard ----
        if is_key_pressed(KeyCode::Tab) { st.panel_open = !st.panel_open; }
        if is_key_pressed(KeyCode::Space) { st.paused = !st.paused; }
        if is_key_pressed(KeyCode::G) { sim.cfg.uniform_gravity = !sim.cfg.uniform_gravity; }
        if is_key_pressed(KeyCode::M) { sim.cfg.mutual_gravity = !sim.cfg.mutual_gravity; }
        if is_key_pressed(KeyCode::Q) { sim.cfg.coulomb = !sim.cfg.coulomb; }
        if is_key_pressed(KeyCode::T) { sim.cfg.rules = !sim.cfg.rules; }
        if is_key_pressed(KeyCode::K) { sim.cfg.collisions = !sim.cfg.collisions; }
        if is_key_pressed(KeyCode::N) { sim.cfg.nuclear = !sim.cfg.nuclear; }
        if is_key_pressed(KeyCode::R) {
            let types = sim.types.clone();
            sim.rules.randomize_where(|k| types[k].custom && !types[k].fluid);
        }
        if is_key_pressed(KeyCode::C) { sim.clear(); }
        if is_key_pressed(KeyCode::B) { st.tool = Tool::Paint; }
        if is_key_pressed(KeyCode::W) { st.tool = Tool::Wall; }
        if is_key_pressed(KeyCode::E) { st.tool = Tool::Erase; }
        if is_key_pressed(KeyCode::V) {
            let all = render::ViewMode::ALL;
            let k = all.iter().position(|&m| m == st.view.mode).unwrap_or(0);
            st.view.mode = all[(k + 1) % all.len()];
        }
        if is_key_pressed(KeyCode::L) { st.view.trails = !st.view.trails; }
        let digit_keys = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4,
                          KeyCode::Key5, KeyCode::Key6, KeyCode::Key7, KeyCode::Key8, KeyCode::Key9];
        for (i, k) in digit_keys.iter().enumerate() {
            if i < paintable.len() && is_key_pressed(*k) { st.brush_kind = paintable[i]; }
        }

        // ---- Mouse ----
        let mp: Vec2 = mouse_position().into();
        let over_ui = root_ui().is_mouse_over(mp);
        let lmb = !over_ui && is_mouse_button_down(MouseButton::Left);
        match st.tool {
            Tool::Paint => {
                if lmb && (st.brush_kind as usize) < sim.types.len() {
                    sim.spawn_in_circle(mp, st.brush_radius, st.brush_rate as usize, st.brush_kind);
                }
            }
            Tool::Wall => {
                sim.walls.half = st.wall_thickness * 0.5;
                let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
                if lmb && is_mouse_button_pressed(MouseButton::Left) {
                    stroke = Some(mp);
                    line_start = Some(mp);
                }
                if lmb && !shift {
                    // Freehand: commit a segment every few pixels of travel.
                    if let Some(last) = stroke {
                        if last.distance(mp) > 8.0 {
                            sim.walls.add(last, mp);
                            stroke = Some(mp);
                        }
                    }
                    line_start = stroke;
                }
                if !is_mouse_button_down(MouseButton::Left) {
                    if let (Some(a), true) = (line_start, shift) {
                        if a.distance(mp) > 2.0 {
                            sim.walls.add(a, mp);
                        }
                    } else if let Some(last) = stroke {
                        if last.distance(mp) > 1.0 {
                            sim.walls.add(last, mp);
                        }
                    }
                    stroke = None;
                    line_start = None;
                }
            }
            Tool::Erase => {
                if lmb {
                    sim.walls.erase_near(mp, st.brush_radius);
                }
            }
        }
        let mouse = if !over_ui && is_mouse_button_down(MouseButton::Right) {
            let sign = if is_key_down(KeyCode::LeftShift) { -1.0 } else { 1.0 };
            Some(MouseForce { pos: mp, strength: sign * st.mouse_strength, radius: st.mouse_radius })
        } else {
            None
        };

        // ---- Simulate ----
        sim.cfg.substeps = st.substeps.round().max(1.0) as u32;
        sim.cfg.iterations = st.iterations.round().max(1.0) as u32;
        if !st.paused {
            let dt = get_frame_time().clamp(1.0 / 240.0, 1.0 / 30.0);
            let t0 = Instant::now();
            sim.step(dt, mouse);
            let ms = t0.elapsed().as_secs_f32() * 1000.0;
            perf.step_ms = perf.step_ms * 0.9 + ms * 0.1;
            perf.far_ms = perf.far_ms * 0.9 + sim.timings.far * 0.1;
            perf.rules_ms = perf.rules_ms * 0.9 + sim.timings.rules * 0.1;
        }

        // ---- Render ----
        renderer.draw(&sim, &st.view);
        if let (Some(a), true, Tool::Wall) = (line_start, is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift), st.tool) {
            draw_line(a.x, a.y, mp.x, mp.y, st.wall_thickness, Color::new(0.6, 0.65, 0.8, 0.5));
        }
        if !over_ui {
            let ring = Color::new(1.0, 1.0, 1.0, 0.25);
            if mouse.is_some() {
                draw_circle_lines(mp.x, mp.y, st.mouse_radius, 1.0, ring);
            } else {
                match st.tool {
                    Tool::Paint => draw_circle_lines(mp.x, mp.y, st.brush_radius, 1.0, ring),
                    Tool::Wall => draw_circle(mp.x, mp.y, st.wall_thickness * 0.5, Color::new(0.6, 0.65, 0.8, 0.7)),
                    Tool::Erase => draw_circle_lines(mp.x, mp.y, st.brush_radius, 1.0, Color::new(1.0, 0.45, 0.4, 0.5)),
                }
            }
        }

        // ---- Controls ----
        let act = ui::draw_panel(&mut sim, &mut st, &perf);
        if act.clear { sim.clear(); }
        if act.spawn { sim.spawn_random(st.spawn_count as usize); }
        if act.nuclear_soup { load_nuclear_soup(&mut sim, 6000); }
        if act.dam_break { scenes::dam_break(&mut sim); }
        if act.oil_water { scenes::oil_and_water(&mut sim); }
        if act.clear_walls { sim.walls.clear(); }

        // F12: save the rendered particles (without the panel) to a PNG.
        if let Some(path) = shot_request.take() {
            get_screen_data().export_png(&path);
            if auto_shot.is_some() {
                println!("frame {:.2} ms (physics {:.2} ms) at {} particles", get_frame_time() * 1000.0, perf.step_ms, sim.len());
                break;
            }
        }

        next_frame().await;
    }
}
