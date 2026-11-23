//! The sandbox's control panel (drawn with the shared gui.rs): tools,
//! species creator, interaction matrix, physics and view settings.

use macroquad::prelude::*;
use particle_sim::gui::{Gui, Scale, col};
use particle_sim::render::{View, ViewMode};
use particle_sim::sim::{Config, Sim};
use particle_sim::types::{RuleMatrix, hsv};

pub const PANEL_W: f32 = 420.0;

/// What the left mouse button does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Paint,
    Erase,
    Wall,
    EraseWall,
    Force,
}

pub const TOOLS: [(Tool, &str); 5] = [
    (Tool::Paint, "Paint  B"),
    (Tool::Erase, "Erase  E"),
    (Tool::Wall, "Wall  W"),
    (Tool::EraseWall, "Erase wall  X"),
    (Tool::Force, "Push / pull  F"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Build,
    Species,
    Matrix,
    Physics,
    View,
}

const TABS: [(Tab, &str); 5] = [(Tab::Build, "Build"), (Tab::Species, "Species"), (Tab::Matrix, "Matrix"), (Tab::Physics, "Physics"), (Tab::View, "View")];

pub struct App {
    pub panel_open: bool,
    pub tab: Tab,
    pub paused: bool,
    pub step_once: bool,
    pub time_scale: f32,
    pub tool: Tool,
    /// Species the brush paints (type id).
    pub brush_kind: u8,
    /// Particles per frame while painting.
    pub brush_rate: f32,
    pub brush_radius: f32,
    pub force_strength: f32,
    pub force_radius: f32,
    /// Thickness of new walls (px).
    pub wall_thickness: f32,
    pub spawn_count: f32,
    /// Species open in the editor (type id).
    pub species_sel: u8,
    /// Selected matrix cell: (row = the species that reacts, column = the
    /// species it reacts to), as type ids.
    pub cell: (u8, u8),
    /// Matrix edits also set the mirrored cell (keeps it symmetric).
    pub mirror: bool,
    pub view: View,
    pub step_ms: f32,
    pub message: Option<(String, f64)>,
}

impl App {
    pub fn new(sim: &Sim) -> Self {
        let first = species_ids(sim).first().copied().unwrap_or(0);
        Self {
            panel_open: true,
            tab: Tab::Build,
            paused: false,
            step_once: false,
            time_scale: 1.0,
            tool: Tool::Paint,
            brush_kind: first,
            brush_rate: 10.0,
            brush_radius: 30.0,
            force_strength: 3000.0,
            force_radius: 150.0,
            wall_thickness: 6.0,
            spawn_count: 2000.0,
            species_sel: first,
            cell: (first, first),
            mirror: false,
            view: View::default(),
            step_ms: 0.0,
            message: None,
        }
    }

    pub fn say(&mut self, s: impl Into<String>) {
        self.message = Some((s.into(), get_time() + 4.0));
    }
}

/// The user's species (the built-in nuclear particles stay hidden).
pub fn species_ids(sim: &Sim) -> Vec<u8> {
    (0..sim.types.len() as u8).filter(|&k| sim.types[k as usize].custom).collect()
}

#[derive(Default)]
pub struct Actions {
    pub clear: bool,
    pub clear_walls: bool,
    pub spawn: bool,
    pub new_world: bool,
}

/// Is the mouse over the panel (or its collapsed button)?
pub fn area(app: &App) -> Rect {
    if app.panel_open { Rect::new(0.0, 0.0, PANEL_W, screen_height()) } else { Rect::new(8.0, 8.0, 170.0, 30.0) }
}

pub fn draw(ui: &mut Gui, sim: &mut Sim, app: &mut App) -> Actions {
    let mut act = Actions::default();
    if !app.panel_open {
        let r = area(app);
        ui.layout(r.x, r.y, r.w);
        ui.scope("mini");
        if ui.button("Show controls  [Tab]") {
            app.panel_open = true;
        }
        return act;
    }
    let (w, h) = (PANEL_W, screen_height());
    draw_rectangle(0.0, 0.0, w, h, col::PANEL);
    draw_line(w, 0.0, w, h, 1.0, col::EDGE);
    let pad = 12.0;
    ui.layout(pad, 10.0, w - 2.0 * pad);
    ui.scope("header");

    // ---- Header ----
    ui.text("PARTICLE SIM", pad, 10.0, 18.0, col::TEXT);
    let fps = format!("{} fps   physics {:.1} ms", get_fps(), app.step_ms);
    let fw = ui.text_width(&fps, 14.0);
    ui.text(&fps, w - pad - fw, 13.0, 14.0, col::DIM);
    ui.space(30.0);
    let pause = if app.paused { "▶ Resume" } else { "Pause" };
    match ui.buttons(&[pause, "Step", "Clear", "Hide"]) {
        Some(0) => app.paused = !app.paused,
        Some(1) => app.step_once = true,
        Some(2) => act.clear = true,
        Some(3) => app.panel_open = false,
        _ => {}
    }
    ui.slider("Simulation speed   [ / ]", &mut app.time_scale, 0.05, 2.0, Scale::Log, 1.0, |v| format!("×{v:.2}"));
    ui.note(&format!(
        "{} particles   {} species   {} wall segments{}",
        sim.len(),
        species_ids(sim).len(),
        sim.walls.segs.len(),
        if app.paused { "   PAUSED" } else { "" }
    ));
    if let Some((msg, _)) = &app.message {
        ui.label_col(msg, col::WARN);
    }
    ui.space(4.0);

    // ---- Tabs ----
    let labels: Vec<&str> = TABS.iter().map(|t| t.1).collect();
    let mut k = TABS.iter().position(|t| t.0 == app.tab).unwrap_or(0);
    ui.scope("tabs");
    ui.tabs(&labels, &mut k);
    app.tab = TABS[k].0;
    ui.space(4.0);
    let body = Rect::new(pad, ui.y, w - pad - 4.0, h - ui.y - 6.0);
    ui.scope(TABS[k].1);
    ui.scroll_begin(TABS[k].1, body);
    match app.tab {
        Tab::Build => build_tab(ui, sim, app, &mut act),
        Tab::Species => species_tab(ui, sim, app),
        Tab::Matrix => matrix_tab(ui, sim, app, &mut act),
        Tab::Physics => physics_tab(ui, sim),
        Tab::View => view_tab(ui, app),
    }
    ui.space(16.0);
    ui.scroll_end();
    act
}

// ---------------------------------------------------------------- helpers

/// A row per species: colour dot, name, particle count. Returns the clicked one.
fn species_list(ui: &mut Gui, sim: &Sim, sel: u8, key: &str) -> Option<u8> {
    let mut counts = vec![0usize; sim.types.len()];
    for &k in &sim.kind {
        counts[k as usize] += 1;
    }
    let mut hit = None;
    for k in species_ids(sim) {
        let t = &sim.types[k as usize];
        let r = ui.alloc(26.0);
        ui.space(2.0);
        let (clicked, hov, _) = ui.press(&format!("{key}{k}"), r);
        let on = k == sel;
        let bg = if on { col::ON } else if hov { col::HOVER } else { col::WIDGET };
        draw_rectangle(r.x, r.y, r.w, r.h, bg);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, if on { col::ACCENT } else { col::EDGE });
        draw_circle(r.x + 14.0, r.y + 13.0, (t.radius * 1.3).clamp(3.0, 9.0), t.color);
        ui.text(&t.name, r.x + 30.0, r.y + 4.0, ui.size - 1.0, col::TEXT);
        let tag = counts[k as usize].to_string();
        let tw = ui.text_width(&tag, ui.size - 3.0);
        ui.text(&tag, r.x + r.w - tw - 8.0, r.y + 6.0, ui.size - 3.0, col::DIM);
        if clicked {
            hit = Some(k);
        }
    }
    hit
}

fn name_of(sim: &Sim, k: u8) -> &str {
    sim.types.get(k as usize).map_or("?", |t| t.name.as_str())
}

// ---------------------------------------------------------------- tabs

fn build_tab(ui: &mut Gui, sim: &mut Sim, app: &mut App, act: &mut Actions) {
    ui.heading("Tools");
    let labels: Vec<&str> = TOOLS.iter().map(|t| t.1).collect();
    let mut k = TOOLS.iter().position(|t| t.0 == app.tool).unwrap_or(0);
    if ui.segmented(&labels, &mut k, 3) {
        app.tool = TOOLS[k].0;
    }
    ui.note(match app.tool {
        Tool::Paint => "Hold the left button to paint the selected species (only into free space). Keys 1-9 pick a species.",
        Tool::Erase => "Hold the left button to remove particles under the circle.",
        Tool::Wall => "Drag to draw walls; Shift-drag for a straight wall.",
        Tool::EraseWall => "Hold the left button to remove walls under the circle.",
        Tool::Force => "Hold the left button to pull particles towards the mouse, Shift to push them away. The right button does the same with any tool.",
    });
    match app.tool {
        Tool::Paint => {
            if !species_ids(sim).contains(&app.brush_kind) {
                app.brush_kind = species_ids(sim).first().copied().unwrap_or(0);
            }
            if let Some(k) = species_list(ui, sim, app.brush_kind, "brush") {
                app.brush_kind = k;
            }
            ui.slider("Brush size", &mut app.brush_radius, 5.0, 150.0, Scale::Lin, 30.0, |v| format!("{v:.0} px"));
            ui.slider("Paint rate", &mut app.brush_rate, 1.0, 50.0, Scale::Log, 10.0, |v| format!("{v:.0} per frame"));
        }
        Tool::Erase | Tool::EraseWall => {
            ui.slider("Eraser size", &mut app.brush_radius, 5.0, 150.0, Scale::Lin, 30.0, |v| format!("{v:.0} px"));
        }
        Tool::Wall => {
            ui.slider("Thickness of new walls", &mut app.wall_thickness, 1.0, 20.0, Scale::Lin, 6.0, |v| format!("{v:.0} px"));
            ui.note("Walls already drawn keep their thickness.");
        }
        Tool::Force => {}
    }
    ui.heading("Mouse force");
    ui.slider("Strength", &mut app.force_strength, 100.0, 50000.0, Scale::Log, 3000.0, |v| format!("{v:.0} px/s²"));
    ui.slider("Range", &mut app.force_radius, 10.0, 600.0, Scale::Log, 150.0, |v| format!("{v:.0} px"));
    ui.note("Right mouse: pull (Shift: push), with any tool.");

    ui.heading("Fill & clear");
    ui.slider("Particles to spawn", &mut app.spawn_count, 100.0, 20000.0, Scale::Log, 2000.0, |v| format!("{v:.0}"));
    match ui.buttons(&["Spawn random", "Clear  C", "Clear walls"]) {
        Some(0) => act.spawn = true,
        Some(1) => act.clear = true,
        Some(2) => act.clear_walls = true,
        _ => {}
    }
    ui.note("Spawn random uses the species ticked \"In random spawn\" (Species tab).");
    ui.heading("Scenes");
    if ui.button("New random world  [R]") {
        act.new_world = true;
    }
    ui.note("Random interaction matrix, freshly spawned particles: particle life.");
}

fn species_tab(ui: &mut Gui, sim: &mut Sim, app: &mut App) {
    let ids = species_ids(sim);
    if !ids.contains(&app.species_sel) {
        app.species_sel = ids.first().copied().unwrap_or(0);
    }
    ui.heading(&format!("Species ({} of {} max)", ids.len(), particle_sim::types::MAX_TYPES - particle_sim::types::ids::FIRST_CUSTOM as usize));
    if let Some(k) = species_list(ui, sim, app.species_sel, "sp") {
        app.species_sel = k;
    }
    match ui.buttons(&["Add", "Duplicate", "Remove"]) {
        Some(0) => match sim.add_species() {
            Some(k) => {
                app.species_sel = k;
                app.say(format!("Added {}", name_of(sim, k)));
            }
            None => app.say("Species limit reached"),
        },
        Some(1) if !ids.is_empty() => match sim.duplicate_species(app.species_sel) {
            Some(k) => app.species_sel = k,
            None => app.say("Species limit reached"),
        },
        Some(2) if ids.len() > 1 => {
            let k = app.species_sel;
            app.say(format!("Removed {} and its particles", name_of(sim, k)));
            sim.remove_species(k);
            // Ids above it moved down by one.
            for x in [&mut app.brush_kind, &mut app.cell.0, &mut app.cell.1] {
                if *x > k {
                    *x -= 1;
                }
            }
            app.species_sel = k.saturating_sub(1).max(species_ids(sim).first().copied().unwrap_or(0));
        }
        Some(2) => app.say("Keep at least one species"),
        _ => {}
    }
    if ids.is_empty() {
        return;
    }
    let k = app.species_sel;
    ui.heading("Edit");
    ui.scope(&format!("species{k}"));
    let t = &mut sim.types[k as usize];
    ui.text_field("Name", &mut t.name, "name");
    let changed = ui.slider("Colour", &mut t.hue, 0.0, 360.0, Scale::Lin, 180.0, |v| format!("{v:.0}°"));
    if changed {
        t.color = hsv(t.hue, 0.65, 0.95);
    }
    // Hue strip under the colour slider.
    let strip = ui.alloc(8.0);
    let n = 48;
    for i in 0..n {
        let x = strip.x + strip.w * i as f32 / n as f32;
        draw_rectangle(x, strip.y, strip.w / n as f32 + 0.5, 6.0, hsv(360.0 * i as f32 / n as f32, 0.65, 0.95));
    }
    ui.space(6.0);
    ui.slider("Radius", &mut t.radius, 1.0, 12.0, Scale::Lin, 4.0, |v| format!("{v:.1} px"));
    ui.slider("Mass", &mut t.mass, 0.05, 20.0, Scale::Log, 1.0, |v| format!("{v:.2}"));
    ui.slider("Charge", &mut t.charge, -3.0, 3.0, Scale::Lin, 0.0, |v| format!("{v:+.1}"));
    ui.note("Charge only matters with Electric force on (Physics tab).");
    ui.checkbox("Collides (bumps into other particles)", &mut t.collides);
    ui.checkbox("In random spawn", &mut t.in_random_spawn);
    let count = sim.kind.iter().filter(|&&x| x == k).count();
    if ui.button(&format!("Remove its {count} particles")) {
        let kind = sim.kind.clone();
        sim.retain(|i| kind[i] != k);
    }
}

fn matrix_tab(ui: &mut Gui, sim: &mut Sim, app: &mut App, act: &mut Actions) {
    let ids = species_ids(sim);
    ui.checkbox("Interaction rules on  [T]", &mut sim.cfg.rules);
    ui.note("Each cell says how the row's species reacts to the column's: green pulls it towards, red pushes it away. Rows and columns differ, so A can chase B while B flees A.");
    if ids.is_empty() {
        return;
    }
    if !ids.contains(&app.cell.0) || !ids.contains(&app.cell.1) {
        app.cell = (ids[0], ids[0]);
    }
    // ---- Grid ----
    let n = ids.len();
    let head = 26.0;
    let cell = ((ui.w - head) / n as f32).floor().min(44.0);
    let grid = ui.alloc(head + cell * n as f32);
    ui.space(6.0);
    let (x0, y0) = (grid.x + head, grid.y + head);
    let dot = (cell * 0.28).clamp(3.0, 8.0);
    for (j, &b) in ids.iter().enumerate() {
        let c = sim.types[b as usize].color;
        let hl = b == app.cell.1;
        let cx = x0 + (j as f32 + 0.5) * cell;
        if hl {
            draw_rectangle(x0 + j as f32 * cell, grid.y, cell, head - 2.0, col::HOVER);
        }
        draw_circle(cx, grid.y + head * 0.5, dot, c);
        let a_hl = b == app.cell.0;
        let cy = y0 + (j as f32 + 0.5) * cell;
        if a_hl {
            draw_rectangle(grid.x, y0 + j as f32 * cell, head - 2.0, cell, col::HOVER);
        }
        draw_circle(grid.x + head * 0.5, cy, dot, c);
    }
    let fine = ui.shift();
    let mut tip = None;
    for (i, &a) in ids.iter().enumerate() {
        for (j, &b) in ids.iter().enumerate() {
            let r = Rect::new(x0 + j as f32 * cell + 1.0, y0 + i as f32 * cell + 1.0, cell - 2.0, cell - 2.0);
            let (_, hov, held) = ui.press(&format!("cell{a}_{b}"), r);
            let v = sim.rules.get(a as usize, b as usize);
            if ui.pressed_in(r) {
                app.cell = (a, b);
            }
            let mut set = None;
            if held {
                let dx = ui.mouse_delta().x;
                if dx != 0.0 {
                    set = Some((v + dx / if fine { 600.0 } else { 120.0 }).clamp(-1.0, 1.0));
                }
            }
            if ui.right_clicked(r) {
                set = Some(0.0);
                app.cell = (a, b);
            }
            if let Some(nv) = set {
                sim.rules.set(a as usize, b as usize, nv);
                if app.mirror {
                    sim.rules.set(b as usize, a as usize, nv);
                }
            }
            let v = sim.rules.get(a as usize, b as usize);
            draw_rectangle(r.x, r.y, r.w, r.h, rule_color(v));
            let sel = (a, b) == app.cell;
            if sel || hov {
                draw_rectangle_lines(r.x, r.y, r.w, r.h, if sel { 2.0 } else { 1.0 }, if sel { col::TEXT } else { col::DIM });
            }
            if cell >= 30.0 {
                let s = if v.abs() < 0.05 { "0".to_string() } else { format!("{v:+.1}") };
                let size = 12.0;
                let tw = ui.text_width(&s, size);
                ui.text(&s, r.x + (r.w - tw) * 0.5, r.y + (r.h - size) * 0.5 - 1.0, size, Color::new(1.0, 1.0, 1.0, 0.85));
            }
            if hov {
                tip = Some(vec![
                    format!("{} → {}: {v:+.2}", name_of(sim, a), name_of(sim, b)),
                    describe(v, name_of(sim, a), name_of(sim, b)),
                    "drag left/right to change (Shift: fine), right-click: 0".into(),
                ]);
            }
        }
    }
    if let Some(t) = tip {
        ui.tip(t);
    }

    // ---- Selected cell ----
    let (a, b) = app.cell;
    let (na, nb) = (name_of(sim, a).to_string(), name_of(sim, b).to_string());
    let mut v = sim.rules.get(a as usize, b as usize);
    ui.label_col(&describe(v, &na, &nb), col::ACCENT);
    if ui.slider(&format!("{na} → {nb}##cell"), &mut v, -1.0, 1.0, Scale::Lin, 0.0, |v| format!("{v:+.2}")) {
        sim.rules.set(a as usize, b as usize, v);
        if app.mirror {
            sim.rules.set(b as usize, a as usize, v);
        }
    }
    if a != b {
        let back = sim.rules.get(b as usize, a as usize);
        ui.note(&format!("The other way ({nb} → {na}): {back:+.2}"));
    }
    ui.checkbox("Mirror edits (keep A→B = B→A)", &mut app.mirror);

    // ---- Whole matrix ----
    ui.heading("Whole matrix");
    let customs = |types: &[particle_sim::types::ParticleType], k: usize| types[k].custom;
    match ui.button_grid(&["Randomize  R", "Zero", "Make symmetric", "Flip signs", "Self-attract", "Random world"], 3, None) {
        Some(0) => {
            let types = sim.types.clone();
            sim.rules.randomize_where(|k| customs(&types, k));
            sim.cfg.rules = true;
        }
        Some(1) => sim.rules = RuleMatrix::zero(sim.types.len()),
        Some(2) => {
            for &p in &ids {
                for &q in &ids {
                    if p < q {
                        let m = 0.5 * (sim.rules.get(p as usize, q as usize) + sim.rules.get(q as usize, p as usize));
                        sim.rules.set(p as usize, q as usize, m);
                        sim.rules.set(q as usize, p as usize, m);
                    }
                }
            }
        }
        Some(3) => {
            for &p in &ids {
                for &q in &ids {
                    let x = sim.rules.get(p as usize, q as usize);
                    sim.rules.set(p as usize, q as usize, -x);
                }
            }
        }
        Some(4) => {
            for &p in &ids {
                sim.rules.set(p as usize, p as usize, 0.8);
            }
        }
        Some(5) => act.new_world = true,
        _ => {}
    }
    ui.note("Self-attract sets the diagonal (each species to itself) to +0.8: clumps. Random world also respawns the particles.");

    ui.heading("Strength");
    let d = Config::default();
    ui.slider("Range", &mut sim.cfg.rule_range, 10.0, 200.0, Scale::Lin, d.rule_range, |v| format!("{v:.0} px"));
    ui.slider("Strength", &mut sim.cfg.rule_strength, 0.0, 3000.0, Scale::Lin, d.rule_strength, |v| format!("{v:.0}"));
    ui.note("Range: how far a particle feels the others. Strength scales every cell. Some drag (Physics tab) keeps the motion from running away.");
}

fn rule_color(v: f32) -> Color {
    let base = col::WIDGET;
    let t = v.abs().min(1.0).powf(0.7);
    let target = if v >= 0.0 { Color::new(0.25, 0.78, 0.42, 1.0) } else { Color::new(0.88, 0.3, 0.3, 1.0) };
    Color::new(base.r + (target.r - base.r) * t, base.g + (target.g - base.g) * t, base.b + (target.b - base.b) * t, 1.0)
}

fn describe(v: f32, a: &str, b: &str) -> String {
    let how = match v.abs() {
        x if x < 0.05 => return format!("{a} ignores {b}"),
        x if x < 0.35 => "weakly",
        x if x < 0.7 => "",
        _ => "strongly",
    };
    let sep = if how.is_empty() { "" } else { " " };
    if v > 0.0 { format!("{a} is {how}{sep}pulled towards {b}") } else { format!("{a} is {how}{sep}pushed away from {b}") }
}

fn physics_tab(ui: &mut Gui, sim: &mut Sim) {
    let d = Config::default();
    let c = &mut sim.cfg;
    ui.note("Right-click any slider to reset it.");
    ui.heading("Motion");
    ui.checkbox("Collisions  [K]", &mut c.collisions);
    ui.slider("Bounciness", &mut c.restitution, 0.0, 1.0, Scale::Lin, d.restitution, |v| format!("{v:.2}"));
    ui.slider("Drag", &mut c.drag, 0.0, 5.0, Scale::Lin, d.drag, |v| format!("{v:.2} /s"));
    ui.slider("Speed limit", &mut c.max_speed, 100.0, 5000.0, Scale::Log, d.max_speed, |v| format!("{v:.0} px/s"));

    ui.heading("Forces");
    ui.checkbox("Gravity (downwards)  [G]", &mut c.uniform_gravity);
    if c.uniform_gravity {
        ui.slider("g", &mut c.g, 0.0, 2000.0, Scale::Lin, d.g, |v| format!("{v:.0} px/s²"));
    }
    ui.checkbox("Mutual gravity (everything attracts)  [M]", &mut c.mutual_gravity);
    if c.mutual_gravity {
        ui.slider("G", &mut c.big_g, 0.0, 2000.0, Scale::Lin, d.big_g, |v| format!("{v:.0}"));
    }
    ui.checkbox("Electric force (charges)  [Q]", &mut c.coulomb);
    if c.coulomb {
        ui.slider("k", &mut c.k, 0.0, 200000.0, Scale::Lin, d.k, |v| format!("{v:.0}"));
    }
    if c.mutual_gravity || c.coulomb {
        ui.slider("Softening", &mut c.softening, 1.0, 40.0, Scale::Lin, d.softening, |v| format!("{v:.1} px"));
        ui.checkbox("Barnes-Hut approximation (off: exact, slow)", &mut c.barnes_hut);
        if c.barnes_hut {
            ui.slider("Accuracy θ", &mut c.theta, 0.2, 1.5, Scale::Lin, d.theta, |v| format!("{v:.2}"));
        }
    }

    ui.heading("Solver");
    let mut sub = c.substeps as f32;
    if ui.slider("Substeps", &mut sub, 1.0, 16.0, Scale::Lin, d.substeps as f32, |v| format!("{:.0}", v.round())) {
        c.substeps = sub.round().max(1.0) as u32;
    }
    let mut it = c.iterations as f32;
    if ui.slider("Solver iterations", &mut it, 1.0, 10.0, Scale::Lin, d.iterations as f32, |v| format!("{:.0}", v.round())) {
        c.iterations = it.round().max(1.0) as u32;
    }
    ui.note("More substeps and iterations: stiffer, more accurate contacts, slower.");
}

fn view_tab(ui: &mut Gui, app: &mut App) {
    ui.heading("Colour by  [V]");
    let modes = [ViewMode::Normal, ViewMode::Speed];
    let names = ["Species", "Speed"];
    let mut k = modes.iter().position(|&m| m == app.view.mode).unwrap_or(0);
    if ui.segmented(&names, &mut k, 2) {
        app.view.mode = modes[k];
    }
    if app.view.mode == ViewMode::Speed {
        ui.slider("Hot speed", &mut app.view.speed_scale, 50.0, 1500.0, Scale::Log, 400.0, |v| format!("{v:.0} px/s"));
    }
    ui.checkbox("Trails  [L]", &mut app.view.trails);
    if app.view.trails {
        ui.slider("Trail fade", &mut app.view.trail_fade, 0.01, 0.5, Scale::Log, 0.08, |v| format!("{v:.2}"));
    }
    ui.heading("Zoom  [Z]");
    ui.note("Z opens a magnifier that follows the mouse. Click to lock it; then the mouse works inside the window (paint, erase, draw, push at the zoomed scale). Scroll to change magnification. Z or Esc closes it.");
    ui.heading("Keys");
    for (k, v) in [
        ("Tab", "show / hide this panel"),
        ("Space  /  .", "pause  /  single step"),
        ("[  ]", "slower / faster"),
        ("B E W X F", "paint, erase, wall, erase wall, push/pull"),
        ("1-9", "species to paint"),
        ("R", "randomize the matrix"),
        ("T  K", "rules, collisions on/off"),
        ("G  M  Q", "gravity, mutual gravity, electric force"),
        ("C", "clear particles"),
        ("F12", "screenshot"),
    ] {
        ui.kv(k, v, col::TEXT);
    }
}
