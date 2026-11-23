//! Particle Sim: a sandbox of particle species, an interaction matrix
//! ("particle life"), gravity, charges and walls.
//!
//! Run: cargo run --release

mod ui;

use macroquad::prelude::*;
use particle_sim::gui::Gui;
use particle_sim::render::{Renderer, ViewMode};
use particle_sim::sim::{MouseForce, Sim};
use particle_sim::zoom::{Zoom, ZoomState};
use std::time::Instant;
use ui::{App, Tab, Tool};

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

/// The sandbox shows only the user's species: the built-in nuclear
/// particles (neutron, proton, electron...) stay in the type table, which
/// the library expects, but are never spawned or shown.
fn sandbox_sim() -> Sim {
    let mut sim = Sim::new(screen_width(), screen_height());
    for t in sim.types.iter_mut().filter(|t| !t.custom) {
        t.in_random_spawn = false;
    }
    sim.cfg.nuclear = false;
    sim.cfg.coulomb = false;
    sim.cfg.drag = 0.5;
    sim
}

/// Random interaction matrix and a fresh random fill.
fn new_world(sim: &mut Sim, count: usize) {
    sim.clear();
    let types = sim.types.clone();
    sim.rules.randomize_where(|k| types[k].custom);
    sim.cfg.rules = true;
    sim.cfg.uniform_gravity = false;
    sim.spawn_random(count);
}

fn erase_particles(sim: &mut Sim, at: Vec2, r: f32) {
    let pos = sim.pos.clone();
    sim.retain(|i| pos[i].distance(at) >= r);
}

#[macroquad::main(conf)]
async fn main() {
    rand::srand(miniquad::date::now() as u64);
    let mut renderer = Renderer::new();
    let mut ui = Gui::new();
    let mut sim = sandbox_sim();
    let mut app = App::new(&sim);
    new_world(&mut sim, 3000);
    let mut zoom = Zoom::default();
    // Wall tool: last committed point of the stroke, and where a Shift-drag
    // straight wall started.
    let mut wall_last: Option<Vec2> = None;
    let mut wall_start: Option<Vec2> = None;

    // Dev automation: PARTICLE_SIM_SHOT=<png> saves frame PARTICLE_SIM_FRAMES
    // (240) and exits; PARTICLE_SIM_VIEW=speed|
    // trails, PARTICLE_SIM_TAB=species|matrix|physics|view, PARTICLE_SIM_ZOOM=1.
    let auto_shot = std::env::var("PARTICLE_SIM_SHOT").ok();
    let shot_frame: u64 = std::env::var("PARTICLE_SIM_FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(240);
    match std::env::var("PARTICLE_SIM_VIEW").as_deref() {
        Ok("speed") => app.view.mode = ViewMode::Speed,
        Ok("trails") => app.view.trails = true,
        _ => {}
    }
    app.tab = match std::env::var("PARTICLE_SIM_TAB").as_deref() {
        Ok("species") => Tab::Species,
        Ok("matrix") => Tab::Matrix,
        Ok("physics") => Tab::Physics,
        Ok("view") => Tab::View,
        _ => Tab::Build,
    };
    if std::env::var("PARTICLE_SIM_ZOOM").is_ok() {
        zoom.state = ZoomState::Locked;
        zoom.center = vec2(screen_width() * 0.6, screen_height() * 0.5);
    }
    let mut frame: u64 = 0;
    let mut shot_request: Option<String> = None;

    loop {
        frame += 1;
        sim.width = screen_width();
        sim.height = screen_height();
        if app.message.as_ref().is_some_and(|m| get_time() > m.1) {
            app.message = None;
        }
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        let screen_mouse: Vec2 = mouse_position().into();

        // ---- Zoom & mouse mapping ----
        ui.begin();
        let over_ui = ui::area(&app).contains(screen_mouse) || ui.dragging();
        let wheel = mouse_wheel().1;
        let mut click_used = false;
        if zoom.state == ZoomState::Aiming {
            zoom.center = screen_mouse;
            if !over_ui && is_mouse_button_pressed(MouseButton::Left) {
                zoom.state = ZoomState::Locked;
                click_used = true;
            }
        }
        let left = if app.panel_open { ui::PANEL_W } else { 0.0 };
        let dst = zoom.dst(left);
        let in_window = zoom.state == ZoomState::Locked && dst.contains(screen_mouse);
        if zoom.state != ZoomState::Off && wheel != 0.0 && !over_ui && (zoom.state == ZoomState::Aiming || in_window) {
            zoom.factor = (zoom.factor * if wheel > 0.0 { 1.25 } else { 0.8 }).clamp(1.5, 24.0);
        }
        zoom.clamp_center(sim.width, sim.height);
        let src = zoom.src();
        let mouse = if in_window { src.point() + (screen_mouse - dst.point()) / zoom.factor } else { screen_mouse };
        let blocked = over_ui || click_used || zoom.state == ZoomState::Aiming;
        let world_scale = if in_window { zoom.factor } else { 1.0 };

        // ---- Tools ----
        let lmb_pressed = !blocked && is_mouse_button_pressed(MouseButton::Left);
        let lmb_down = !blocked && is_mouse_button_down(MouseButton::Left);
        match app.tool {
            Tool::Paint => {
                if lmb_down && (app.brush_kind as usize) < sim.types.len() {
                    sim.spawn_in_circle(mouse, app.brush_radius / world_scale, app.brush_rate as usize, app.brush_kind);
                }
            }
            Tool::Erase => {
                if lmb_down {
                    erase_particles(&mut sim, mouse, app.brush_radius / world_scale);
                }
            }
            Tool::EraseWall => {
                if lmb_down {
                    sim.walls.erase_near(mouse, app.brush_radius / world_scale);
                }
            }
            Tool::Wall => {
                // Only walls drawn from now on take this thickness.
                sim.walls.half = app.wall_thickness * 0.5;
                if lmb_pressed {
                    wall_last = Some(mouse);
                    wall_start = Some(mouse);
                }
                if lmb_down && !shift {
                    if let Some(last) = wall_last {
                        if last.distance(mouse) > 8.0 / world_scale {
                            sim.walls.add(last, mouse);
                            wall_last = Some(mouse);
                        }
                    }
                    wall_start = wall_last;
                }
                if !is_mouse_button_down(MouseButton::Left) {
                    match (wall_start, shift, wall_last) {
                        (Some(a), true, _) if a.distance(mouse) > 2.0 => sim.walls.add(a, mouse),
                        (_, false, Some(last)) if last.distance(mouse) > 1.0 => sim.walls.add(last, mouse),
                        _ => {}
                    }
                    wall_last = None;
                    wall_start = None;
                }
            }
            Tool::Force => {}
        }
        // Mouse force: right button with any tool, left with Push / pull.
        let pushing = !blocked && (is_mouse_button_down(MouseButton::Right) || (app.tool == Tool::Force && is_mouse_button_down(MouseButton::Left)));
        let force = pushing.then(|| MouseForce {
            pos: mouse,
            strength: if shift { -app.force_strength } else { app.force_strength },
            radius: app.force_radius / world_scale,
        });

        // ---- Simulate ----
        if !app.paused || app.step_once {
            let dt = get_frame_time().clamp(1.0 / 240.0, 1.0 / 30.0) * if app.step_once { 1.0 } else { app.time_scale };
            let t0 = Instant::now();
            sim.step(dt, force);
            app.step_ms = app.step_ms * 0.9 + t0.elapsed().as_secs_f32() * 100.0;
            app.step_once = false;
        }

        // ---- Draw ----
        renderer.draw(&sim, &app.view);
        if let (Some(a), true, Tool::Wall) = (wall_start, shift, app.tool) {
            draw_line(a.x, a.y, mouse.x, mouse.y, app.wall_thickness, Color::new(0.6, 0.65, 0.8, 0.5));
        }
        zoom.draw(&mut renderer, &sim, &app.view, left);

        // Cursor.
        if !over_ui && zoom.state != ZoomState::Aiming {
            let ring = Color::new(1.0, 1.0, 1.0, 0.3);
            let (x, y) = (screen_mouse.x, screen_mouse.y);
            match app.tool {
                _ if pushing || app.tool == Tool::Force => {
                    let a = if pushing { 0.7 } else { 0.3 };
                    let c = if shift { Color::new(1.0, 0.55, 0.35, a) } else { Color::new(0.45, 0.8, 1.0, a) };
                    draw_circle_lines(x, y, app.force_radius, 1.0, c);
                    draw_circle(x, y, 2.5, c);
                }
                Tool::Paint => {
                    let c = sim.types.get(app.brush_kind as usize).map_or(ring, |t| Color::new(t.color.r, t.color.g, t.color.b, 0.5));
                    draw_circle_lines(x, y, app.brush_radius, 1.0, c);
                }
                Tool::Erase | Tool::EraseWall => draw_circle_lines(x, y, app.brush_radius, 1.0, Color::new(1.0, 0.45, 0.4, 0.6)),
                Tool::Wall => draw_circle(x, y, app.wall_thickness * 0.5 * world_scale, Color::new(0.6, 0.65, 0.8, 0.7)),
                Tool::Force => {}
            }
        }

        // Hover: what is under the mouse?
        if !over_ui && !pushing && !lmb_down && zoom.state != ZoomState::Aiming {
            let pick = 6.0 / world_scale;
            let mut best: Option<(usize, f32)> = None;
            for i in 0..sim.len() {
                let d = sim.pos[i].distance(mouse);
                if d < pick + sim.types[sim.kind[i] as usize].radius && best.is_none_or(|b| d < b.1) {
                    best = Some((i, d));
                }
            }
            if let Some((i, _)) = best {
                let t = &sim.types[sim.kind[i] as usize];
                let mut l = vec![t.name.clone(), format!("speed {:.0} px/s", sim.vel[i].length())];
                l.push(format!("radius {:.1}   mass {:.2}{}", t.radius, t.mass, if t.charge != 0.0 { format!("   charge {:+.1}", t.charge) } else { String::new() }));
                ui.draw_tooltip(&l);
            }
        }

        // ---- Panel ----
        let act = ui::draw(&mut ui, &mut sim, &mut app);
        if let Some(t) = ui.tooltip.take() {
            ui.draw_tooltip(&t);
        }
        let typing = ui.typing();
        ui.end();

        // ---- Keyboard (not while typing a species name) ----
        if !typing {
            let c = &mut sim.cfg;
            if is_key_pressed(KeyCode::Tab) { app.panel_open = !app.panel_open; }
            if is_key_pressed(KeyCode::Space) { app.paused = !app.paused; }
            if is_key_pressed(KeyCode::Period) { app.step_once = true; }
            if is_key_pressed(KeyCode::LeftBracket) { app.time_scale = (app.time_scale / 1.5).max(0.05); }
            if is_key_pressed(KeyCode::RightBracket) { app.time_scale = (app.time_scale * 1.5).min(2.0); }
            if is_key_pressed(KeyCode::G) { c.uniform_gravity = !c.uniform_gravity; }
            if is_key_pressed(KeyCode::M) { c.mutual_gravity = !c.mutual_gravity; }
            if is_key_pressed(KeyCode::Q) { c.coulomb = !c.coulomb; }
            if is_key_pressed(KeyCode::T) { c.rules = !c.rules; }
            if is_key_pressed(KeyCode::K) { c.collisions = !c.collisions; }
            if is_key_pressed(KeyCode::R) {
                let types = sim.types.clone();
                sim.rules.randomize_where(|k| types[k].custom);
                sim.cfg.rules = true;
                app.say("Randomized the interaction matrix");
            }
            if is_key_pressed(KeyCode::C) { sim.clear(); }
            if is_key_pressed(KeyCode::B) { app.tool = Tool::Paint; }
            if is_key_pressed(KeyCode::E) { app.tool = Tool::Erase; }
            if is_key_pressed(KeyCode::W) { app.tool = Tool::Wall; }
            if is_key_pressed(KeyCode::X) { app.tool = Tool::EraseWall; }
            if is_key_pressed(KeyCode::F) { app.tool = Tool::Force; }
            if is_key_pressed(KeyCode::V) {
                app.view.mode = if app.view.mode == ViewMode::Speed { ViewMode::Normal } else { ViewMode::Speed };
            }
            if is_key_pressed(KeyCode::L) { app.view.trails = !app.view.trails; }
            if is_key_pressed(KeyCode::Z) { zoom.toggle(); }
            if is_key_pressed(KeyCode::Escape) { zoom.state = ZoomState::Off; }
            if is_key_pressed(KeyCode::F12) {
                shot_request = Some(format!("screenshot-{}.png", miniquad::date::now() as u64));
            }
            let digits = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4, KeyCode::Key5, KeyCode::Key6, KeyCode::Key7, KeyCode::Key8, KeyCode::Key9];
            let species = ui::species_ids(&sim);
            for (i, k) in digits.iter().enumerate() {
                if is_key_pressed(*k) {
                    if let Some(&s) = species.get(i) {
                        app.brush_kind = s;
                        app.tool = Tool::Paint;
                    }
                }
            }
        }
        if act.clear { sim.clear(); }
        if act.clear_walls { sim.walls.clear(); }
        if act.spawn {
            let n = sim.spawn_random(app.spawn_count as usize);
            if n < app.spawn_count as usize {
                app.say(format!("Only {n} fitted"));
            }
        }
        if act.new_world { new_world(&mut sim, app.spawn_count as usize); }

        if let Some(p) = &auto_shot {
            if frame == shot_frame {
                shot_request = Some(p.clone());
            }
        }
        if let Some(path) = shot_request.take() {
            get_screen_data().export_png(&path);
            if auto_shot.is_some() {
                println!("frame {:.2} ms (physics {:.2} ms) at {} particles", get_frame_time() * 1000.0, app.step_ms, sim.len());
                break;
            }
            app.say(format!("Saved {path}"));
        }
        next_frame().await;
    }
}
