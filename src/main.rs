//! Nuclear Game: a sandbox for nuclear physics with real nuclide data.
//!
//! Run: cargo run --release

mod panel;
mod tabs;
use sim_core::gui;
use sim_core::history::History;
use sim_core::zoom::{Zoom, ZoomState};

use sim_core::game_scenes as scenes;

use macroquad::prelude::*;
use sim_core::nuclear::{energy_of, format_energy, place_nucleus};
use sim_core::reactor::{Drain, DrainFilter, Pump, Rod, Site, Spawner};
use sim_core::walls::{WallKind, WallSpec};
use sim_core::render::{Renderer, View, ViewMode};
use sim_core::sim::{MouseForce, Sim};
use sim_core::types::ids;
use std::time::Instant;

fn conf() -> Conf {
    Conf {
        window_title: "Nuclear Game".to_owned(),
        window_width: 1600,
        window_height: 900,
        window_resizable: true,
        // Starts fullscreen (NUCLEAR_GAME_WINDOWED=1 for a window; the
        // automated screenshots also use one, for a fixed size).
        fullscreen: std::env::var("NUCLEAR_GAME_WINDOWED").is_err() && std::env::var("PARTICLE_SIM_SHOT").is_err(),
        high_dpi: false,
        platform: miniquad::conf::Platform {
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

pub use sim_core::nuclear::game_config;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Nucleus,
    Electron,
    Positron,
    Erase,
    Wall,
    EraseWall,
    Rod,
    FuelChannel,
    Source,
    Fill,
    Spawner,
    Drain,
    Pump,
    Force,
    Inspect,
    Detector,
    Field,
}

const TOOL_KEYS: [(KeyCode, Tool); 9] = [
    (KeyCode::Key1, Tool::Nucleus),
    (KeyCode::Key2, Tool::Erase),
    (KeyCode::Key3, Tool::Wall),
    (KeyCode::Key4, Tool::Fill),
    (KeyCode::Key5, Tool::Rod),
    (KeyCode::Key6, Tool::FuelChannel),
    (KeyCode::Key7, Tool::Source),
    (KeyCode::Key8, Tool::Spawner),
    (KeyCode::Key9, Tool::Drain),
];

/// Tools laid out by dragging a rectangle.
fn rect_tool(t: Tool) -> bool {
    matches!(t, Tool::Fill | Tool::Drain | Tool::Pump | Tool::Rod | Tool::Detector | Tool::Field)
}

/// Fill a rectangle with nuclide (z, n) on a hexagonal lattice, `gap` px
/// between surfaces. Returns how many were placed.
fn fill(sim: &mut Sim, r: Rect, z: u32, n: u32, gap: f32) -> u32 {
    let a = (z + n).max(1);
    let rn = sim.types[ids::NEUTRON as usize].radius;
    let radius = if sim.point_nuclei && a >= 2 { sim_core::nuclear::POINT_RADIUS + 0.5 } else { rn * (a as f32 / 0.9069).sqrt() + rn };
    let d = 2.0 * radius + gap;
    let mut placed = 0;
    let mut y = r.y + radius;
    let mut row = 0;
    while y <= r.y + r.h - radius {
        let mut x = r.x + radius + if row % 2 == 1 { d * 0.5 } else { 0.0 };
        while x <= r.x + r.w - radius {
            if place_nucleus(sim, vec2(x, y), z, n, Vec2::ZERO) {
                placed += 1;
            }
            x += d;
        }
        y += d * 0.866;
        row += 1;
    }
    placed
}

pub struct Game {
    pub panel_open: bool,
    pub paused: bool,
    pub step_once: bool,
    pub time_scale: f32,
    pub tool: Tool,
    pub z: u32,
    pub n: u32,
    pub nuclide_text: String,
    pub last_text: String,
    pub with_electrons: bool,
    pub brush_radius: f32,
    /// Everything about the next wall drawn (kind, thickness, neutrons,
    /// filter rule). Walls already drawn keep their own.
    pub wall: WallSpec,
    pub spawn_rate: f32,
    pub drain_filter: DrainFilter,
    pub pump_speed: f32,
    /// Walls along a new pump's sides: None (open) or their kind.
    pub pump_walls: Option<WallKind>,
    pub fill_gap: f32,
    /// Mouse force: pull (px/s^2 at the centre) and reach (screen px).
    pub force_strength: f32,
    pub force_radius: f32,
    pub view: View,
    pub save_name: String,
    pub message: Option<(String, f64)>,
    pub step_ms: f32,
    /// Undo / redo snapshots of the scene.
    pub history: History,
    /// Step by exactly 1/60 s × speed each frame, whatever the frame rate:
    /// with the seed, runs repeat exactly.
    pub fixed_step: bool,
    /// Keep the current seed when a scenario is loaded (else a new one is drawn).
    pub keep_seed: bool,
    /// Next spawner: a beam (energy per nucleon, width, spread, divergence
    /// in degrees, pulses) rather than a feed.
    pub beam: bool,
    pub beam_mev: f32,
    pub beam_width: f32,
    pub beam_spread: f32,
    pub beam_div: f32,
    pub beam_pulse: f32,
    pub beam_pulse_len: f32,
    /// Next detector: a line (else a box), its filter (DetectFilter index)
    /// and whether it counts photons and neutrinos.
    pub det_line: bool,
    pub det_filter: usize,
    pub det_light: bool,
    /// Next field region: E (px/s² on a proton) and its direction (0 right,
    /// 1 down, 2 left, 3 up), B (a proton's turn rate, rad/s).
    pub field_e: f32,
    pub field_dir: usize,
    pub field_b: f32,
    /// Inspector: the nucleus (tag) being looked at, its last (Z, N).
    pub selected: Option<u32>,
    pub sel_last: Option<(u32, u32)>,
    pub follow: bool,
    pub follow_products: bool,
}

impl Game {
    fn say(&mut self, s: impl Into<String>) {
        self.message = Some((s.into(), get_time() + 5.0));
    }
}

fn new_sim() -> Sim {
    // Nuclear-only world (n, p, e-, e+, photon, neutrino) at game size.
    sim_core::nuclear::game_sim(screen_width(), screen_height())
}

/// Nucleus (Z, N, member indices) containing particle i.
fn nucleus_of(sim: &Sim, i: usize) -> (u32, u32, Vec<usize>) {
    let t = sim.tag[i];
    let members: Vec<usize> = (0..sim.len()).filter(|&j| sim.is_nuclear(j) && sim.tag[j] == t).collect();
    let (z, n) = members.iter().map(|&j| sim.zn_of(j)).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    (z, n, members)
}

/// Launch velocity from a drag vector (screen px): gentle near zero so slow
/// (thermal) speeds are reachable, steep further out.
fn launch_velocity(drag: Vec2, vmax: f32) -> Vec2 {
    let len = drag.length();
    if len < 3.0 {
        return Vec2::ZERO;
    }
    let speed = (3.0 * len * (1.0 + len / 100.0)).min(vmax);
    drag / len * speed
}

/// Hover text for walls and reactor parts at p.
fn part_info(sim: &Sim, p: Vec2, pick: f32) -> Option<Vec<String>> {
    use sim_core::thermal::{format_kelvin, kelvin_of};
    use sim_core::nuclear::format_energy;
    let re = &sim.reactor;
    if let Some(sp) = re.spawners.iter().find(|s| s.pos.distance(p) < pick + 5.0) {
        let mut l = vec![
            format!("{}: {}", if sp.beam { "Beam" } else { "Spawner" }, sim_core::nucdata::nuclide_name(sp.z, sp.n)),
            if sp.keep > 0 {
                format!("keeps {} of them in its area ({} added)", sp.keep, sp.spawned)
            } else if sp.beam {
                format!("{}/u, {:.1} per s, {:.0} px wide   ({} fired)", format_energy(sp.energy_per_nucleon(&sim.cfg)), sp.rate, sp.width, sp.spawned)
            } else {
                format!("{:.2} per s at {:.0} px/s   ({} so far)", sp.rate, sp.vel.length(), sp.spawned)
            },
        ];
        if sp.pulse_period > 0.0 {
            l.push(format!("pulsed: {:.2} s every {:.1} s", sp.pulse_len, sp.pulse_period));
        }
        l.push("Edit it on the Reactions tab.".into());
        return Some(l);
    }
    for (k, d) in re.detectors.iter().enumerate() {
        let near = match d.shape {
            sim_core::detector::DetShape::Box(r) => {
                let inner = Rect::new(r.x + pick, r.y + pick, (r.w - 2.0 * pick).max(0.0), (r.h - 2.0 * pick).max(0.0));
                r.contains(p) && !inner.contains(p)
            }
            sim_core::detector::DetShape::Line(a, b) => {
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                (a + ab * t).distance(p) < pick
            }
        };
        if near {
            let mut l = d.summary(sim.nuclear.time);
            l[0] = format!("D{} · {}", k + 1, l[0]);
            return Some(l);
        }
    }
    if let Some(f) = re.fields.iter().find(|f| f.rect.contains(p)) {
        return Some(vec!["Field region".into(), f.describe(), "Edit it on the Measure tab.".into()]);
    }
    if re.sources.iter().any(|s| s.distance(p) < pick + 4.0) {
        return Some(vec!["Am-Be neutron source".into(), format!("{:.2} neutrons/s, 1-8 MeV", re.source_rate)]);
    }
    if let Some(k) = sim.walls.nearest(p, pick * 0.5) {
        let w = &sim.walls.segs[k];
        let mut l = vec![format!("{} wall, {:.0} px thick", w.kind.name(), w.half * 2.0)];
        let neutrons = if w.reflect_neutrons { "reflects neutrons" } else { "neutrons pass through" };
        match w.kind {
            WallKind::Insulator => l.push(format!("no heat passes; {neutrons}")),
            WallKind::Filter => {
                l.push(w.filter_text());
                l.push("insulating; reflects what it stops".into());
            }
            _ => {
                l.push(format!("temperature {}", format_kelvin(kelvin_of(&sim.cfg, w.theta))));
                l.push(neutrons.into());
                if w.kind == WallKind::FreezePlug {
                    l.push(format!("melts above {}", format_kelvin(sim.cfg.melt_temp as f64)));
                }
            }
        }
        return Some(l);
    }
    if let Some(d) = re.drains.iter().find(|d| d.rect.contains(p)) {
        return Some(vec![
            format!("Drain: {}", d.filter.name()),
            format!("{} removed   {:.1}/s", d.removed, d.rate),
            format!("outflow {:.0} px/s, {} per nucleon", d.speed, format_energy(d.energy)),
        ]);
    }
    if let Some(pm) = re.pumps.iter().find(|q| q.rect.contains(p)) {
        return Some(vec!["Pump".into(), format!("pushes matter to {:.0} px/s (neutrons unaffected)", pm.speed)]);
    }
    None
}

/// Switch the nucleus model, converting the scene.
/// A fresh random seed (from the clock).
pub fn new_seed() -> u64 {
    sim_core::rng::mix(miniquad::date::now().to_bits()) % 1_000_000
}

/// Snapshot the scene for undo, before changing it.
pub fn snapshot(sim: &Sim, g: &mut Game) {
    g.history.record(scenes::to_text(sim));
}

/// Put back a snapshot, keeping the event log, the counters and the
/// reactor graphs (undo rewinds the scene, not the record of what happened).
fn restore(sim: &mut Sim, text: &str) {
    let old = std::mem::take(&mut sim.nuclear);
    let samples = std::mem::take(&mut sim.reactor.samples);
    let recording = std::mem::take(&mut sim.reactor.recording);
    if let Err(e) = scenes::from_text(sim, text) {
        eprintln!("undo: {e}");
    }
    let ns = &mut sim.nuclear;
    ns.counts = old.counts;
    ns.captures = old.captures;
    ns.fusions = old.fusions;
    ns.fissions = old.fissions;
    ns.annihilations = old.annihilations;
    ns.energy_released = old.energy_released;
    ns.breakups = old.breakups;
    ns.q_released = old.q_released;
    ns.q_absorbed = old.q_absorbed;
    ns.decay_heat = old.decay_heat;
    ns.log = old.log;
    ns.history = old.history;
    ns.dropped = old.dropped;
    ns.time = old.time;
    ns.log_breakups = old.log_breakups;
    sim.reactor.samples = samples;
    sim.reactor.recording = recording;
    sim.index_nuclei();
}

fn undo(sim: &mut Sim, g: &mut Game, redo: bool) {
    let now = scenes::to_text(sim);
    let state = if redo { g.history.redo(now) } else { g.history.undo(now) };
    match state {
        Some(t) => {
            restore(sim, &t);
            g.say(if redo { "Redo" } else { "Undo" });
        }
        None => g.say(if redo { "Nothing to redo" } else { "Nothing to undo" }),
    }
}

fn set_points(sim: &mut Sim, g: &mut Game, on: bool) {
    let lost = sim_core::nuclear::set_point_mode(sim, on);
    g.say(match (on, lost) {
        (true, _) => "Point nuclei: every nucleus is one particle; neutron interactions come from cross-sections.".to_string(),
        (false, 0) => "Nucleon clusters: nuclei are built from their protons and neutrons.".to_string(),
        (false, k) => format!("Nucleon clusters: {k} nuclei didn't fit at full size and were removed."),
    });
}

#[macroquad::main(conf)]
async fn main() {
    rand::srand(miniquad::date::now() as u64);
    let mut renderer = Renderer::new();
    let mut ui = gui::Gui::new();
    let mut ps = panel::PanelState::default();
    let mut sim = new_sim();
    let mut g = Game {
        panel_open: true,
        paused: false,
        step_once: false,
        time_scale: 1.0,
        tool: Tool::Nucleus,
        z: 92,
        n: 143,
        nuclide_text: "U-235".into(),
        last_text: "U-235".into(),
        with_electrons: false,
        brush_radius: 20.0,
        wall: WallSpec { filter_z: 54, ..WallSpec::new(WallKind::Insulator, 2.0) },
        spawn_rate: 2.0,
        drain_filter: DrainFilter::All,
        pump_speed: 40.0,
        pump_walls: None,
        fill_gap: 4.0,
        force_strength: 3000.0,
        force_radius: 150.0,
        view: View { mode: ViewMode::Element, ..View::default() },
        save_name: "my scene".into(),
        message: None,
        step_ms: 0.0,
        history: History::default(),
        fixed_step: false,
        keep_seed: false,
        beam: false,
        beam_mev: 1.0,
        beam_width: 12.0,
        beam_spread: 0.0,
        beam_div: 0.0,
        beam_pulse: 0.0,
        beam_pulse_len: 0.5,
        det_line: false,
        det_filter: 0,
        det_light: false,
        field_e: 0.0,
        field_dir: 0,
        field_b: 2.0,
        selected: None,
        sel_last: None,
        follow: false,
        follow_products: true,
    };
    let mut saved = scenes::list_scenes();
    let mut zoom = Zoom::default();
    let mut drag_start: Option<(Vec2, Vec2)> = None; // (world, screen)
    let mut wall_last: Option<Vec2> = None;
    let mut wall_start: Option<Vec2> = None;
    let mut rect_start: Option<Vec2> = None;

    // Dev automation (see the sandbox): PARTICLE_SIM_SCENE=<preset index>,
    // PARTICLE_SIM_ZOOM=1 opens a locked zoom, PARTICLE_SIM_SHOT=<png>.
    let auto_shot = std::env::var("PARTICLE_SIM_SHOT").ok();
    let shot_frame: u64 = std::env::var("PARTICLE_SIM_FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(240);
    sim.point_nuclei = std::env::var("PARTICLE_SIM_POINTS").is_ok();
    // PARTICLE_SIM_SEED=<n> fixes the seed (repeatable automation).
    sim.seed = std::env::var("PARTICLE_SIM_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or_else(new_seed);
    if let Some(k) = std::env::var("PARTICLE_SIM_SCENE").ok().and_then(|s| s.parse::<usize>().ok()) {
        scenes::load_preset(&mut sim, k);
    } else {
        scenes::load_preset(&mut sim, 4);
    }
    // PARTICLE_SIM_BEAMS=<MeV per nucleon>: two He-4 beams head-on in an
    // empty box (vacuum).
    if let Some(e) = std::env::var("PARTICLE_SIM_BEAMS").ok().and_then(|s| s.parse::<f64>().ok()) {
        sim.clear();
        sim.walls.clear();
        sim.nuclear.reset();
        sim.reactor.clear();
        sim.cfg = sim.cfg.clone().with_medium(false);
        let v = sim_core::nuclear::speed_for(&sim.cfg, e * 4.0, 4.0);
        let (w, h) = (sim.width, sim.height);
        sim.reactor.spawners.push(Spawner::new(vec2(w * 0.35, h * 0.5), 2, 2, vec2(v, 0.0), 12.0));
        sim.reactor.spawners.push(Spawner::new(vec2(w * 0.95, h * 0.5), 2, 2, vec2(-v, 0.0), 12.0));
    }
    // PARTICLE_SIM_OVERLAY=temp|flux|thermal
    g.view.overlay = match std::env::var("PARTICLE_SIM_OVERLAY").as_deref() {
        Ok("temp") => sim_core::overlay::OverlayKind::Temperature,
        Ok("flux") => sim_core::overlay::OverlayKind::Flux,
        Ok("thermal") => sim_core::overlay::OverlayKind::ThermalFlux,
        _ => sim_core::overlay::OverlayKind::None,
    };
    if let Ok(t) = std::env::var("PARTICLE_SIM_TAB") {
        ps.tab = match t.as_str() {
            "reactor" => panel::Tab::Reactor,
            "measure" => panel::Tab::Measure,
            "auto" => panel::Tab::Auto,
            "isotopes" => panel::Tab::Isotopes,
            "view" => panel::Tab::View,
            "scenes" => panel::Tab::Scenes,
            "physics" => panel::Tab::Physics,
            "log" => panel::Tab::Log,
            _ => panel::Tab::Build,
        };
    }
    // PARTICLE_SIM_TOOL=force|filter|pump selects a tool (filter also draws
    // two demo filter walls).
    match std::env::var("PARTICLE_SIM_TOOL").as_deref() {
        Ok("force") => g.tool = Tool::Force,
        Ok("beam") => {
            g.tool = Tool::Spawner;
            g.beam = true;
            g.z = 2;
            g.n = 2;
            g.beam_mev = 30.0;
        }
        Ok("detector") => g.tool = Tool::Detector,
        Ok("field") => g.tool = Tool::Field,
        Ok("pump") => {
            g.tool = Tool::Pump;
            g.pump_walls = Some(WallKind::Conductor);
        }
        Ok("filter") => {
            g.tool = Tool::Wall;
            g.wall.kind = WallKind::Filter;
            sim.walls.half = 3.0;
            sim.walls.add_filter(vec2(700.0, 120.0), vec2(1500.0, 120.0), 54, true);
            sim.walls.add_filter(vec2(700.0, 160.0), vec2(1500.0, 160.0), 0, false);
            use sim_core::walls::FilterRule;
            let f = |rule, z, threshold, only| WallSpec { rule, filter_z: z, threshold, only, ..WallSpec::new(WallKind::Filter, 3.0) };
            sim.walls.add_spec(vec2(700.0, 200.0), vec2(1500.0, 200.0), &f(FilterRule::Charged, 0, 0.0, true));
            sim.walls.add_spec(vec2(700.0, 240.0), vec2(1500.0, 240.0), &f(FilterRule::Energy, 0, 1.0, true));
            sim.walls.add_spec(vec2(700.0, 280.0), vec2(1500.0, 280.0), &f(FilterRule::Mass, 40, 0.0, true));
            let reflector = WallSpec { reflect_neutrons: true, ..WallSpec::new(WallKind::Conductor, 5.0) };
            sim.walls.add_spec(vec2(700.0, 330.0), vec2(1500.0, 330.0), &reflector);
            sim.walls.add_spec(vec2(700.0, 370.0), vec2(1500.0, 370.0), &WallSpec::new(WallKind::Conductor, 1.0));
            sim.walls.add_spec(vec2(700.0, 400.0), vec2(1500.0, 400.0), &WallSpec { reflect_neutrons: false, ..WallSpec::new(WallKind::Insulator, 4.0) });
            g.wall.rule = FilterRule::Energy;
            g.wall.only = true;
        }
        _ => {}
    }
    if let Some(r) = std::env::var("PARTICLE_SIM_ROD").ok().and_then(|s| s.parse::<f32>().ok()) {
        sim.reactor.insertion = r;
        sim.reactor.target = r;
    }
    if std::env::var("PARTICLE_SIM_ZOOM").is_ok() {
        zoom.state = ZoomState::Locked;
        zoom.center = vec2(screen_width() * 0.5, screen_height() * 0.5);
    }
    let mut frame: u64 = 0;

    loop {
        frame += 1;
        sim.width = screen_width();
        sim.height = screen_height();
        if g.message.as_ref().is_some_and(|m| get_time() > m.1) {
            g.message = None;
        }

        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        let screen_mouse: Vec2 = mouse_position().into();

        // ---- Zoom & mouse mapping ----
        ui.begin();
        let over_ui = panel::area(&g).contains(screen_mouse) || ui.dragging();
        let wheel = mouse_wheel().1;
        let mut click_used = false;
        if zoom.state == ZoomState::Aiming {
            zoom.center = screen_mouse;
            if !over_ui && is_mouse_button_pressed(MouseButton::Left) {
                zoom.state = ZoomState::Locked;
                click_used = true;
            }
        }
        let left = if g.panel_open { panel::PANEL_W } else { 0.0 };
        let screen_rect = Rect::new(0.0, 0.0, screen_width(), screen_height());
        let dst = zoom.dst(left, screen_rect);
        let in_window = zoom.state == ZoomState::Locked && dst.contains(screen_mouse);
        if zoom.state != ZoomState::Off && wheel != 0.0 && !over_ui && (zoom.state == ZoomState::Aiming || in_window) {
            zoom.factor = (zoom.factor * if wheel > 0.0 { 1.25 } else { 0.8 }).clamp(1.5, 24.0);
        }
        zoom.clamp_center(sim.width, sim.height);
        let src = zoom.src();
        let to_world = |p: Vec2| if in_window { src.point() + (p - dst.point()) / zoom.factor } else { p };
        let mouse = to_world(screen_mouse);
        // Over the panel, or over the zoom window while merely aiming: no tools.
        let blocked = over_ui || click_used || (zoom.state == ZoomState::Aiming);
        let world_scale = if in_window { zoom.factor } else { 1.0 };

        // ---- Tools ----
        let lmb_pressed = !blocked && is_mouse_button_pressed(MouseButton::Left);
        let lmb_down = !blocked && is_mouse_button_down(MouseButton::Left);
        // Anything the left button does to the scene can be undone.
        if lmb_pressed && !matches!(g.tool, Tool::Force | Tool::Inspect) {
            snapshot(&sim, &mut g);
        }
        match g.tool {
            Tool::Nucleus | Tool::Electron | Tool::Positron | Tool::Spawner => {
                if lmb_pressed {
                    drag_start = Some((mouse, screen_mouse));
                }
                if is_mouse_button_released(MouseButton::Left) {
                    if let Some((at, s0)) = drag_start.take() {
                        let v = launch_velocity(screen_mouse - s0, sim.cfg.max_speed);
                        let ok = match g.tool {
                            Tool::Nucleus => {
                                let ok = place_nucleus(&mut sim, at, g.z, g.n, v);
                                if ok && g.with_electrons && g.z > 0 {
                                    scenes::add_electrons(&mut sim, at, g.z + g.n, g.z);
                                }
                                ok
                            }
                            Tool::Spawner => {
                                if g.z + g.n > 0 {
                                    if g.beam {
                                        let drag = screen_mouse - s0;
                                        let dir = if drag.length() > 3.0 { drag.normalize() } else { Vec2::X };
                                        let mut b = Spawner::beam(&sim.cfg, at, g.z, g.n, dir, g.beam_mev as f64, g.spawn_rate);
                                        b.width = g.beam_width;
                                        b.energy_spread = g.beam_spread;
                                        b.divergence = g.beam_div.to_radians();
                                        b.pulse_period = g.beam_pulse;
                                        b.pulse_len = g.beam_pulse_len;
                                        sim.reactor.spawners.push(b);
                                    } else {
                                        sim.reactor.spawners.push(Spawner::new(at, g.z, g.n, v, g.spawn_rate));
                                    }
                                }
                                true
                            }
                            t => {
                                let kind = if t == Tool::Electron { ids::ELECTRON } else { ids::POSITRON };
                                sim.add(at, v, kind);
                                true
                            }
                        };
                        if !ok {
                            g.say("No room there (or the nucleus is empty).");
                        } else {
                            sim.index_nuclei();
                        }
                    }
                }
            }
            Tool::Erase => {
                if lmb_down {
                    let r = g.brush_radius / world_scale;
                    let hit: Vec<u32> = (0..sim.len())
                        .filter(|&i| sim.is_nuclear(i) && sim.pos[i].distance(mouse) < r)
                        .map(|i| sim.tag[i])
                        .collect();
                    let nuclear: Vec<bool> = (0..sim.len()).map(|i| sim.is_nuclear(i)).collect();
                    let (pos, tag) = (sim.pos.clone(), sim.tag.clone());
                    sim.retain(|i| {
                        if nuclear[i] { !hit.contains(&tag[i]) } else { pos[i].distance(mouse) >= r }
                    });
                    sim.index_nuclei();
                    let near_rect = |c: Rect| {
                        let q = vec2(mouse.x.clamp(c.x, c.x + c.w), mouse.y.clamp(c.y, c.y + c.h));
                        q.distance(mouse) < r
                    };
                    let re = &mut sim.reactor;
                    re.rods.retain(|rod| !near_rect(rod.inserted(1.0)));
                    re.sites.retain(|s| s.pos.distance(mouse) >= r);
                    re.sources.retain(|p| p.distance(mouse) >= r);
                    re.spawners.retain(|s| s.pos.distance(mouse) >= r);
                    re.drains.retain(|d| !near_rect(d.rect));
                    re.pumps.retain(|p| !near_rect(p.rect));
                    re.fields.retain(|f| !near_rect(f.rect));
                    re.detectors.retain(|d| match d.shape {
                        sim_core::detector::DetShape::Box(b) => !near_rect(b),
                        sim_core::detector::DetShape::Line(a, b) => {
                            let ab = b - a;
                            let t = ((mouse - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                            (a + ab * t).distance(mouse) >= r
                        }
                    });
                }
            }
            Tool::FuelChannel => {
                if lmb_pressed && g.z + g.n > 0 {
                    if place_nucleus(&mut sim, mouse, g.z, g.n, Vec2::ZERO) {
                        sim.reactor.sites.push(Site { pos: mouse, z: g.z, n: g.n, empty_for: 0.0 });
                        sim.index_nuclei();
                    } else {
                        g.say("No room for the fuel there.");
                    }
                }
            }
            Tool::Source => {
                if lmb_pressed {
                    sim.reactor.sources.push(mouse);
                    if sim.reactor.source_rate <= 0.0 {
                        sim.reactor.source_rate = 0.5;
                    }
                }
            }
            Tool::Fill | Tool::Drain | Tool::Pump | Tool::Rod | Tool::Detector | Tool::Field => {
                if lmb_pressed {
                    rect_start = Some(mouse);
                }
                if !is_mouse_button_down(MouseButton::Left) {
                    if let Some(a) = rect_start.take() {
                        let r = Rect::new(a.x.min(mouse.x), a.y.min(mouse.y), (a.x - mouse.x).abs(), (a.y - mouse.y).abs());
                        let drag = mouse - a;
                        let rn = sim.types[ids::NEUTRON as usize].radius;
                        match g.tool {
                            Tool::Fill if r.w > 2.0 && r.h > 2.0 => {
                                let k = fill(&mut sim, r, g.z, g.n, g.fill_gap);
                                sim.index_nuclei();
                                g.say(format!("Filled with {k} × {}", sim_core::nucdata::nuclide_name(g.z, g.n)));
                            }
                            Tool::Drain if r.w > 2.0 && r.h > 2.0 => sim.reactor.drains.push(Drain::new(r, g.drain_filter)),
                            Tool::Pump if r.w > 2.0 && r.h > 2.0 => {
                                // Flow along the drag, on its longer axis.
                                let dir = if drag.x.abs() >= drag.y.abs() { vec2(drag.x.signum(), 0.0) } else { vec2(0.0, drag.y.signum()) };
                                sim.reactor.pumps.push(Pump { rect: r, dir, speed: g.pump_speed });
                                if let Some(kind) = g.pump_walls {
                                    // Walls along the two sides parallel to the flow: a duct.
                                    let spec = WallSpec::new(kind, g.wall.half);
                                    let h = spec.half;
                                    if dir.x != 0.0 {
                                        sim.walls.add_spec(vec2(r.x, r.y - h), vec2(r.x + r.w, r.y - h), &spec);
                                        sim.walls.add_spec(vec2(r.x, r.y + r.h + h), vec2(r.x + r.w, r.y + r.h + h), &spec);
                                    } else {
                                        sim.walls.add_spec(vec2(r.x - h, r.y), vec2(r.x - h, r.y + r.h), &spec);
                                        sim.walls.add_spec(vec2(r.x + r.w + h, r.y), vec2(r.x + r.w + h, r.y + r.h), &spec);
                                    }
                                }
                            }
                            Tool::Detector if g.det_line && drag.length() > 4.0 => {
                                let f = sim_core::detector::DetectFilter::from_index(g.det_filter, g.z, g.n);
                                let mut d = sim_core::detector::Detector::new(sim_core::detector::DetShape::Line(a, mouse), f, g.det_light);
                                d.since = sim.nuclear.time;
                                sim.reactor.detectors.push(d);
                            }
                            Tool::Detector if !g.det_line && r.w > 4.0 && r.h > 4.0 => {
                                let f = sim_core::detector::DetectFilter::from_index(g.det_filter, g.z, g.n);
                                let mut d = sim_core::detector::Detector::new(sim_core::detector::DetShape::Box(r), f, g.det_light);
                                d.since = sim.nuclear.time;
                                sim.reactor.detectors.push(d);
                            }
                            Tool::Field if r.w > 4.0 && r.h > 4.0 => {
                                let dir = [vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(-1.0, 0.0), vec2(0.0, -1.0)][g.field_dir.min(3)];
                                sim.reactor.fields.push(sim_core::field::Field::new(r, dir * g.field_e, g.field_b));
                            }
                            Tool::Rod if r.h > 10.0 => {
                                let re = &mut sim.reactor;
                                if re.rods.is_empty() && re.sites.is_empty() {
                                    // A new reactor starts shut down.
                                    re.insertion = 1.0;
                                    re.target = 1.0;
                                }
                                re.rods.push(Rod { x: a.x, top: r.y, len: r.h, half_w: 1.25 * rn });
                            }
                            _ => {}
                        }
                    }
                }
            }
            Tool::Wall => {
                let spec = g.wall;
                let add = |sim: &mut Sim, a: Vec2, b: Vec2| sim.walls.add_spec(a, b, &spec);
                if lmb_pressed {
                    wall_last = Some(mouse);
                    wall_start = Some(mouse);
                }
                if lmb_down && !shift {
                    if let Some(last) = wall_last {
                        if last.distance(mouse) > 8.0 / world_scale {
                            add(&mut sim, last, mouse);
                            wall_last = Some(mouse);
                        }
                    }
                    wall_start = wall_last;
                }
                if !is_mouse_button_down(MouseButton::Left) {
                    match (wall_start, shift, wall_last) {
                        (Some(a), true, _) if a.distance(mouse) > 2.0 => add(&mut sim, a, mouse),
                        (_, false, Some(last)) if last.distance(mouse) > 1.0 => add(&mut sim, last, mouse),
                        _ => {}
                    }
                    wall_last = None;
                    wall_start = None;
                }
            }
            Tool::EraseWall => {
                if lmb_down {
                    sim.walls.erase_near(mouse, g.brush_radius / world_scale);
                }
            }
            Tool::Force => {} // below
            Tool::Inspect => {
                if lmb_pressed {
                    let pick = 10.0 / world_scale;
                    let best = (0..sim.len())
                        .filter(|&i| sim.is_nuclear(i) && sim.pos[i].distance(mouse) < pick + sim.types[sim.kind[i] as usize].radius)
                        .min_by(|&a, &b| sim.pos[a].distance(mouse).total_cmp(&sim.pos[b].distance(mouse)));
                    g.selected = best.map(|i| sim.tag[i]);
                    g.sel_last = best.map(|i| nucleus_of(&sim, i)).map(|(z, n, _)| (z, n));
                    if g.selected.is_some() {
                        ps.tab = panel::Tab::Measure;
                    }
                }
            }
        }
        if !matches!(g.tool, Tool::Nucleus | Tool::Electron | Tool::Positron | Tool::Spawner) {
            drag_start = None;
        }
        if !rect_tool(g.tool) {
            rect_start = None;
        }
        // Mouse force: right button with any tool, left with the Force tool.
        let pushing = !blocked && (is_mouse_button_down(MouseButton::Right) || (g.tool == Tool::Force && lmb_down));
        let force = if pushing {
            let sign = if shift { -1.0 } else { 1.0 };
            Some(MouseForce { pos: mouse, strength: sign * g.force_strength, radius: g.force_radius / world_scale })
        } else {
            None
        };

        // ---- Simulate ----
        if !g.paused || g.step_once {
            let frame_dt = if g.fixed_step { 1.0 / 60.0 } else { get_frame_time().clamp(1.0 / 240.0, 1.0 / 30.0) };
            let dt = frame_dt * if g.step_once { 1.0 } else { g.time_scale };
            let t0 = Instant::now();
            sim.step(dt, force);
            g.step_ms = g.step_ms * 0.9 + t0.elapsed().as_secs_f32() * 100.0;
            g.step_once = false;
        }

        // Rules can pause the game and leave messages.
        if std::mem::take(&mut sim.reactor.pause_request) {
            g.paused = true;
        }
        if let Some(m) = sim.reactor.notices.drain(..).last() {
            g.say(m);
        }
        // Inspector: keep track of the selected nucleus (and its products).
        let mut sel_pos = None;
        if let Some(tag) = g.selected {
            let bodies = sim.bodies();
            match bodies.get(&tag) {
                Some(b) => sel_pos = Some((b.pos, b.a())),
                None if g.follow_products => {
                    let next = sim.nuclear.children(tag).into_iter().filter_map(|(t, _)| bodies.get(&t).map(|b| (t, b.a()))).max_by_key(|x| x.1);
                    if let Some((t, _)) = next {
                        g.selected = Some(t);
                    }
                }
                None => {}
            }
            if let (true, Some((p, _))) = (g.follow, sel_pos) {
                zoom.state = ZoomState::Locked;
                zoom.center = p;
            }
        }

        // ---- Draw ----
        renderer.draw(&sim, &g.view);
        if let Some((p, a)) = sel_pos {
            let r = if sim.point_nuclei { 9.0 } else { sim.types[ids::NEUTRON as usize].radius * (a as f32 / 0.9069).sqrt() + 6.0 };
            let pulse = 0.6 + 0.3 * (get_time() as f32 * 5.0).sin();
            draw_circle_lines(p.x, p.y, r, 1.5, Color::new(0.55, 1.0, 0.6, pulse));
            draw_circle_lines(p.x, p.y, r + 4.0, 1.0, Color::new(0.55, 1.0, 0.6, pulse * 0.4));
        }
        // Drag-to-launch preview.
        if let Some((at, s0)) = drag_start {
            let v = launch_velocity(screen_mouse - s0, sim.cfg.max_speed);
            let a = match g.tool {
                Tool::Nucleus | Tool::Spawner => (g.z + g.n).max(1) as f32,
                _ => sim.types[ids::ELECTRON as usize].mass,
            };
            let e = energy_of(&sim.cfg, v.length(), a);
            let start = if in_window { dst.point() + (at - src.point()) * zoom.factor } else { at };
            draw_line(start.x, start.y, screen_mouse.x, screen_mouse.y, 2.0, Color::new(1.0, 0.85, 0.3, 0.8));
            draw_circle(start.x, start.y, 4.0, Color::new(1.0, 0.85, 0.3, 0.8));
            let label = if g.tool == Tool::Spawner && g.beam {
                format!("beam: {}/u   {} total", format_energy(g.beam_mev as f64), format_energy(g.beam_mev as f64 * a as f64))
            } else if v == Vec2::ZERO {
                "at rest".to_string()
            } else {
                format!("{:.0} px/s   {}", v.length(), format_energy(e))
            };
            draw_text(&label, screen_mouse.x + 12.0, screen_mouse.y - 10.0, 18.0, Color::new(1.0, 0.9, 0.5, 1.0));
        }
        if let (Some(a), true, Tool::Wall) = (wall_start, shift, g.tool) {
            draw_line(a.x, a.y, mouse.x, mouse.y, g.wall.half * 2.0, Color::new(0.6, 0.65, 0.8, 0.5));
        }
        if let Some(a) = rect_start {
            let rn = sim.types[ids::NEUTRON as usize].radius;
            let col = Color::new(0.8, 0.82, 0.9, 0.7);
            if g.tool == Tool::Detector && g.det_line {
                draw_line(a.x, a.y, mouse.x, mouse.y, 1.5, Color::new(0.55, 1.0, 0.6, 0.8));
            } else if g.tool == Tool::Rod {
                let (top, bottom) = (a.y.min(mouse.y), a.y.max(mouse.y));
                draw_rectangle_lines(a.x - 1.25 * rn, top, 2.5 * rn, bottom - top, 1.0, col);
            } else {
                let r = Rect::new(a.x.min(mouse.x), a.y.min(mouse.y), (a.x - mouse.x).abs(), (a.y - mouse.y).abs());
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, col);
                if g.tool == Tool::Pump {
                    let d = mouse - a;
                    let dir = if d.x.abs() >= d.y.abs() { vec2(d.x.signum(), 0.0) } else { vec2(0.0, d.y.signum()) };
                    let c = r.center();
                    draw_line(c.x, c.y, c.x + dir.x * 14.0, c.y + dir.y * 14.0, 2.0, col);
                }
            }
        }

        zoom.draw(&mut renderer, &sim, &g.view, left, screen_rect);

        // Cursor.
        if !over_ui && zoom.state != ZoomState::Aiming {
            let ring = Color::new(1.0, 1.0, 1.0, 0.3);
            match g.tool {
                Tool::Erase | Tool::EraseWall => {
                    draw_circle_lines(screen_mouse.x, screen_mouse.y, g.brush_radius, 1.0, Color::new(1.0, 0.45, 0.4, 0.6))
                }
                _ if pushing || g.tool == Tool::Force => {
                    let a = if pushing { 0.7 } else { 0.3 };
                    let c = if shift { Color::new(1.0, 0.55, 0.35, a) } else { Color::new(0.45, 0.8, 1.0, a) };
                    draw_circle_lines(screen_mouse.x, screen_mouse.y, g.force_radius, 1.0, c);
                    draw_circle(screen_mouse.x, screen_mouse.y, 2.5, c);
                }
                _ => draw_circle_lines(screen_mouse.x, screen_mouse.y, 4.0, 1.0, ring),
            }
        }

        // Hover tooltip: what is under the mouse?
        if !over_ui && drag_start.is_none() && zoom.state != ZoomState::Aiming {
            let pick = 8.0 / world_scale;
            let mut best: Option<(usize, f32)> = None;
            for i in 0..sim.len() {
                let d = sim.pos[i].distance(mouse);
                if d < pick + sim.types[sim.kind[i] as usize].radius && best.is_none_or(|b| d < b.1) {
                    best = Some((i, d));
                }
            }
            if let Some((i, _)) = best {
                let k = sim.kind[i];
                let lines = if sim.is_nuclear(i) {
                    let (z, n, members) = nucleus_of(&sim, i);
                    let v = members.iter().map(|&j| sim.vel[j]).sum::<Vec2>() / members.len() as f32;
                    let mut l = panel::describe(z, n, &sim.cfg);
                    let e = energy_of(&sim.cfg, v.length(), (z + n) as f32);
                    l.push(format!("moving {:.0} px/s = {}", v.length(), format_energy(e)));
                    l
                } else {
                    let t = &sim.types[k as usize];
                    let v = sim.vel[i].length();
                    vec![t.name.clone(), format!("speed {:.0} px/s ({:.2} c)", v, v / sim.cfg.light_speed)]
                };
                ui.draw_tooltip(&lines);
            } else if let Some(lines) = part_info(&sim, mouse, pick) {
                ui.draw_tooltip(&lines);
            }
        }

        panel::draw_legend(&ui, &sim, &renderer.overlay);

        // ---- Panel ----
        let act = panel::draw(&mut ui, &mut sim, &mut g, &mut ps, &mut saved);
        if let Some(t) = ui.tooltip.take() {
            ui.draw_tooltip(&t);
        }
        let typing = ui.typing();
        ui.end();

        // ---- Keyboard (after the panel: no shortcuts while typing a name
        // like "Co-60" into a text field) ----
        if !typing {
            if is_key_pressed(KeyCode::Tab) { g.panel_open = !g.panel_open; }
            if is_key_pressed(KeyCode::Space) { g.paused = !g.paused; }
            if is_key_pressed(KeyCode::Period) { g.step_once = true; }
            let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
            if ctrl && is_key_pressed(KeyCode::Z) {
                undo(&mut sim, &mut g, shift);
            }
            if ctrl && is_key_pressed(KeyCode::Y) {
                undo(&mut sim, &mut g, true);
            }
            if is_key_pressed(KeyCode::C) && !ctrl {
                snapshot(&sim, &mut g);
                sim.clear();
                sim.nuclear.reset();
                sim.reactor.samples.clear();
                sim.reactor.recording.clear();
            }
            if is_key_pressed(KeyCode::V) {
                let all = ViewMode::ALL;
                let k = all.iter().position(|&m| m == g.view.mode).unwrap_or(0);
                g.view.mode = all[(k + 1) % all.len()];
            }
            if is_key_pressed(KeyCode::L) { g.view.trails = !g.view.trails; }
            if is_key_pressed(KeyCode::O) {
                use sim_core::overlay::OverlayKind;
                let all = OverlayKind::ALL;
                let k = all.iter().position(|&o| o == g.view.overlay).unwrap_or(0);
                g.view.overlay = all[(k + 1) % all.len()];
            }
            if is_key_pressed(KeyCode::F) { g.tool = Tool::Force; }
            if is_key_pressed(KeyCode::I) { g.tool = Tool::Inspect; }
            if is_key_pressed(KeyCode::K) { g.tool = Tool::Detector; }
            if is_key_pressed(KeyCode::M) { g.tool = Tool::Field; }
            if is_key_pressed(KeyCode::P) {
                snapshot(&sim, &mut g);
                let on = !sim.point_nuclei;
                set_points(&mut sim, &mut g, on);
            }
            if is_key_pressed(KeyCode::LeftBracket) { g.time_scale = (g.time_scale / 1.5).max(0.05); }
            if is_key_pressed(KeyCode::RightBracket) { g.time_scale = (g.time_scale * 1.5).min(2.0); }
            for (k, t) in TOOL_KEYS {
                if is_key_pressed(k) { g.tool = t; }
            }
            if is_key_pressed(KeyCode::Z) && !ctrl {
                zoom.toggle();
            }
            if is_key_pressed(KeyCode::Escape) { zoom.state = ZoomState::Off; }
            // Rod bank: arrows drive the rods in 5% steps, X scrams.
            let re = &mut sim.reactor;
            if !re.rods.is_empty() {
                if is_key_pressed(KeyCode::Up) {
                    re.target = (re.target - 0.05).max(0.0);
                    re.scram = false;
                }
                if is_key_pressed(KeyCode::Down) { re.target = (re.target + 0.05).min(1.0); }
                if is_key_pressed(KeyCode::X) {
                    re.scram();
                    g.say("SCRAM: all rods in");
                }
            }
        }
        if let Some(all) = act.export {
            let msg = panel::export(&sim, &ps, all);
            println!("{msg}");
            g.say(msg);
        }
        if act.undo {
            undo(&mut sim, &mut g, false);
        }
        if act.redo {
            undo(&mut sim, &mut g, true);
        }
        if act.new_seed {
            sim.seed = new_seed();
            g.say(format!("New seed {}", sim.seed));
        }
        if act.export_graphs {
            let path = scenes::log_dir().join(format!("reactor-{}.csv", scenes::timestamp()));
            let msg = match sim_core::reactor::write_samples_csv(&path, &sim.reactor.recording) {
                Ok(n) => format!("Exported {n} samples ({:.0} s) to {}", n as f32 * sim_core::reactor::SAMPLE_DT, path.display()),
                Err(e) => format!("Export failed: {e}"),
            };
            println!("{msg}");
            g.say(msg);
        }
        if let Some(on) = act.point_mode {
            snapshot(&sim, &mut g);
            set_points(&mut sim, &mut g, on);
        }
        if act.refuel_all {
            sim.reactor.reload_now = true;
        }
        if act.clear {
            snapshot(&sim, &mut g);
            sim.clear();
            sim.nuclear.reset();
            sim.reactor.samples.clear();
            sim.reactor.recording.clear();
        }
        if let Some(k) = act.preset {
            snapshot(&sim, &mut g);
            if !g.keep_seed {
                sim.seed = new_seed();
            }
            scenes::load_preset(&mut sim, k);
            g.say(format!("Loaded: {} - {}", scenes::PRESETS[k].0, scenes::PRESETS[k].1));
        }
        if act.save {
            match scenes::save(&sim, &g.save_name) {
                Ok(p) => g.say(format!("Saved {}", p.display())),
                Err(e) => g.say(format!("Save failed: {e}")),
            }
            saved = scenes::list_scenes();
        }
        if let Some(name) = act.load {
            snapshot(&sim, &mut g);
            match scenes::load(&mut sim, &name) {
                Ok(()) => g.say(format!("Loaded {name}")),
                Err(e) => g.say(format!("Load failed: {e}")),
            }
        }
        if let Some(name) = act.delete {
            scenes::delete(&name);
            saved = scenes::list_scenes();
            g.say(format!("Deleted {name}"));
        }
        if act.reset_physics {
            let keep = (sim.cfg.substeps, sim.cfg.iterations);
            sim.cfg = game_config();
            (sim.cfg.substeps, sim.cfg.iterations) = keep;
            g.say("Physics reset to real-world defaults");
        }

        // PARTICLE_SIM_INSPECT=<frame>: inspect the heaviest nucleus then.
        if std::env::var("PARTICLE_SIM_INSPECT").ok().and_then(|s| s.parse::<u64>().ok()) == Some(frame) {
            if let Some((t, _)) = sim.bodies().into_iter().max_by_key(|(_, b)| b.a()) {
                g.selected = Some(t);
                g.follow = std::env::var("PARTICLE_SIM_FOLLOW").is_ok();
            }
        }
        if let Some(p) = &auto_shot {
            if frame == shot_frame {
                get_screen_data().export_png(p);
                println!("frame {:.2} ms (physics {:.2} ms) at {} particles", get_frame_time() * 1000.0, g.step_ms, sim.len());
                for l in sim.nuclear.log.iter().take(8) {
                    println!("  {l}");
                }
                break;
            }
        }
        next_frame().await;
    }
}
