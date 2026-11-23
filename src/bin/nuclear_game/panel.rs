//! The Nuclear Game's control panel (drawn with gui.rs).

use crate::gui::{Gui, Scale, col};
use crate::{Game, Tool};
use macroquad::prelude::*;
use particle_sim::game_scenes::{self as scenes, PRESETS, list_scenes, scene_dir};
use particle_sim::nucdata::{self, Mode, format_time, info, nuclide_name, parse_nuclide};
use particle_sim::nuclear::{EventKind, format_energy, sim_half_life, speed_for};
use particle_sim::reactions;
use particle_sim::reactor::SAMPLE_DT;
use particle_sim::reactor::DrainFilter;
use particle_sim::render::ViewMode;
use particle_sim::walls::WallKind;
use particle_sim::sim::{Config, Sim};

pub const PANEL_W: f32 = 420.0;

#[derive(Default)]
pub struct Actions {
    pub preset: Option<usize>,
    pub clear: bool,
    pub save: bool,
    pub load: Option<String>,
    pub delete: Option<String>,
    pub reset_physics: bool,
    pub export: Option<bool>, // Some(true) = all events, Some(false) = shown
    pub refuel_all: bool,
    /// Switch to point nuclei (true) or nucleon clusters (false).
    pub point_mode: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Build,
    Reactor,
    View,
    Scenes,
    Physics,
    Log,
}

const TABS: [(Tab, &str); 6] = [
    (Tab::Build, "Build"),
    (Tab::Reactor, "Reactor"),
    (Tab::View, "View"),
    (Tab::Scenes, "Scenes"),
    (Tab::Physics, "Physics"),
    (Tab::Log, "Log"),
];

/// Log filters: (label, kinds shown).
const FILTERS: [(&str, &[EventKind]); 6] = [
    ("All", &[]),
    ("Fission", &[EventKind::Fission]),
    ("Fusion", &[EventKind::Fusion]),
    ("Capture", &[EventKind::Capture]),
    ("Decay", &[EventKind::Decay]),
    ("Other", &[EventKind::Scatter, EventKind::Annihilation, EventKind::Other]),
];

pub struct PanelState {
    pub tab: Tab,
    filter: usize,
    search: String,
    /// Global indices (dropped + position) of the events that pass the filter.
    shown: Vec<u64>,
    /// Filter the cache was built for, and how many events it has seen.
    cache_key: (usize, String),
    seen: u64,
}

impl Default for PanelState {
    fn default() -> Self {
        Self { tab: Tab::Build, filter: 0, search: String::new(), shown: Vec::new(), cache_key: (usize::MAX, String::new()), seen: 0 }
    }
}

impl PanelState {
    /// Events currently passing the log filter, oldest first (global index).
    pub fn shown(&self) -> &[u64] {
        &self.shown
    }

    /// Bring the filtered index up to date (incrementally when only new
    /// events arrived).
    fn refresh(&mut self, sim: &Sim) -> u32 {
        let ns = &sim.nuclear;
        let total = ns.dropped + ns.history.len() as u64;
        let key = (self.filter, self.search.trim().to_lowercase());
        if key != self.cache_key || total < self.seen {
            self.shown.clear();
            self.seen = ns.dropped;
            self.cache_key = key.clone();
        }
        let kinds = FILTERS[self.filter].1;
        let before = self.shown.len();
        let start = self.seen.max(ns.dropped);
        for g in start..total {
            let ev = &ns.history[(g - ns.dropped) as usize];
            if (kinds.is_empty() || kinds.contains(&ev.kind)) && (key.1.is_empty() || ev.text.to_lowercase().contains(&key.1)) {
                self.shown.push(g);
            }
        }
        self.seen = total;
        let cut = self.shown.partition_point(|&g| g < ns.dropped);
        if cut > 0 {
            self.shown.drain(..cut);
        }
        (self.shown.len().saturating_sub(before)) as u32
    }
}

/// Human-readable facts about nuclide (z, n): used by the loader and the
/// hover tooltip.
pub fn describe(z: u32, n: u32, cfg: &Config) -> Vec<String> {
    let mut out = Vec::new();
    let a = z + n;
    if a == 0 {
        return out;
    }
    let nuc = info(z, n);
    let sym = nucdata::SYMBOLS.get(z as usize).copied().unwrap_or("?");
    out.push(format!("{}   Z={z}  N={n}  A={a}{}", nuclide_name(z, n), if z > 0 { format!("  ({sym})") } else { String::new() }));
    match nuc.half_life {
        None => out.push("stable".into()),
        Some(t) if t < 1e-12 => out.push(format!("unbound: falls apart at once ({})", format_time(t))),
        Some(t) => out.push(format!("half-life {}   (here: {})", format_time(t), format_time(sim_half_life(t, cfg)))),
    }
    if !nuc.branches.is_empty() {
        let mut parts: Vec<String> = nuc
            .branches
            .iter()
            .map(|(m, f)| {
                if *f > 0.9999 {
                    m.name().to_string()
                } else if *f >= 0.01 {
                    format!("{} {:.1}%", m.short(), f * 100.0)
                } else {
                    format!("{} {:.1e}%", m.short(), f * 100.0)
                }
            })
            .collect();
        let d = nuc.delayed;
        if d.n > 0.0 {
            parts.push(format!("(β⁻n {:.1}%)", d.n * 100.0));
        }
        if d.p > 0.0 {
            parts.push(format!("(β⁺p {:.1}%)", d.p * 100.0));
        }
        out.push(format!("decays: {}", parts.join(", ")));
        if nuc.branches.iter().any(|b| b.0 == Mode::ElectronCapture) {
            out.push("  electron capture needs an electron at the nucleus".into());
        }
    }
    if a > 1 {
        out.push(format!(
            "binding {:.3} MeV/nucleon   mass excess {:.3} MeV{}",
            nucdata::binding(z, n) / a as f64,
            nucdata::mass_excess(z, n),
            if nuc.measured { "" } else { "  (liquid-drop estimate)" }
        ));
        let (abs, scat) = reactions::thermal_xs(z, n);
        let fis = if reactions::fissile(z, n) { "  FISSILE" } else { "" };
        out.push(format!("thermal n: absorb {}, scatter {}{fis}", barns(abs), barns(scat)));
    }
    out
}

/// Cross-section with sensible precision: "683 b", "2.68 b", "0.000519 b".
fn barns(b: f64) -> String {
    if b >= 100.0 {
        format!("{b:.0} b")
    } else if b >= 1.0 {
        format!("{b:.2} b")
    } else if b > 0.0 {
        format!("{} b", format!("{b:.3e}").parse::<f64>().map(|x| x.to_string()).unwrap_or_default())
    } else {
        "0 b".into()
    }
}

fn kind_color(k: EventKind) -> Color {
    match k {
        EventKind::Fission => Color::from_rgba(255, 140, 90, 255),
        EventKind::Fusion => Color::from_rgba(255, 220, 110, 255),
        EventKind::Capture => Color::from_rgba(120, 190, 255, 255),
        EventKind::Scatter => Color::from_rgba(150, 150, 170, 255),
        EventKind::Decay => Color::from_rgba(130, 225, 150, 255),
        EventKind::Annihilation => Color::from_rgba(230, 140, 255, 255),
        EventKind::Other => Color::from_rgba(170, 175, 190, 255),
    }
}

/// Is the mouse over the panel (or its collapsed button)?
pub fn area(g: &Game) -> Rect {
    if g.panel_open { Rect::new(0.0, 0.0, PANEL_W, screen_height()) } else { Rect::new(8.0, 8.0, 170.0, 30.0) }
}

pub fn draw(ui: &mut Gui, sim: &mut Sim, g: &mut Game, ps: &mut PanelState, saved: &mut Vec<String>) -> Actions {
    let mut act = Actions::default();
    // The log index is kept current even while another tab is open.
    let added = ps.refresh(sim);
    if !g.panel_open {
        let r = area(g);
        ui.layout(r.x, r.y, r.w);
        ui.scope("mini");
        if ui.button("Show controls  [Tab]") {
            g.panel_open = true;
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
    ui.text("NUCLEAR GAME", pad, 10.0, 18.0, col::TEXT);
    let fps = format!("{} fps   physics {:.1} ms", get_fps(), g.step_ms);
    let fw = ui.text_width(&fps, 14.0);
    ui.text(&fps, w - pad - fw, 13.0, 14.0, col::DIM);
    ui.space(30.0);
    let pause = if g.paused { "▶ Resume" } else { "Pause" };
    match ui.buttons(&[pause, "Step", "Clear", "Hide"]) {
        Some(0) => g.paused = !g.paused,
        Some(1) => g.step_once = true,
        Some(2) => act.clear = true,
        Some(3) => g.panel_open = false,
        _ => {}
    }
    ui.slider("Simulation speed   [ / ]", &mut g.time_scale, 0.05, 2.0, Scale::Log, 1.0, |v| format!("×{v:.2}"));
    let nuclei = sim.nuclear.census.iter().filter(|c| c.0.0 + c.0.1 > 1).map(|c| c.1).sum::<u32>();
    let t = sim.nuclear.time;
    ui.note(&format!("{} particles   {} nuclei   t = {:.1} s{}", sim.len(), nuclei, t, if g.paused { "   PAUSED" } else { "" }));
    let mut model = sim.point_nuclei as usize;
    if ui.segmented(&["Nucleon clusters", "Point nuclei  [P]"], &mut model, 2) {
        act.point_mode = Some(model == 1);
    }
    if let Some((msg, _)) = &g.message {
        ui.label_col(msg, col::WARN);
    }
    ui.space(4.0);

    // ---- Tabs ----
    let labels: Vec<String> = TABS
        .iter()
        .map(|(t, l)| match t {
            Tab::Log => format!("Log ({})", compact(sim.nuclear.dropped + sim.nuclear.history.len() as u64)),
            _ => l.to_string(),
        })
        .collect();
    let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let mut k = TABS.iter().position(|t| t.0 == ps.tab).unwrap_or(0);
    ui.scope("tabs");
    ui.tabs(&refs, &mut k);
    ps.tab = TABS[k].0;
    ui.space(4.0);
    let body = Rect::new(pad, ui.y, w - pad - 4.0, h - ui.y - 6.0);
    ui.scope(TABS[k].1);
    match ps.tab {
        Tab::Log => log_tab(ui, sim, ps, body, added, &mut act),
        tab => {
            ui.scroll_begin(TABS[k].1, body);
            match tab {
                Tab::Build => build_tab(ui, sim, g),
                Tab::Reactor => reactor_tab(ui, sim, &mut act),
                Tab::View => view_tab(ui, g),
                Tab::Scenes => scenes_tab(ui, g, saved, &mut act),
                Tab::Physics => physics_tab(ui, sim, &mut act),
                Tab::Log => {}
            }
            ui.space(16.0);
            ui.scroll_end();
        }
    }
    act
}

fn format_watts(w: f64) -> String {
    if w <= 0.0 {
        return "0 W".into();
    }
    let units = [(1e-15, "fW"), (1e-12, "pW"), (1e-9, "nW"), (1e-6, "µW"), (1e-3, "mW"), (1.0, "W")];
    let (k, u) = units.iter().rev().find(|(k, _)| w >= *k).copied().unwrap_or(units[0]);
    let x = w / k;
    let digits = if x >= 100.0 { 0 } else if x >= 10.0 { 1 } else { 2 };
    format!("{x:.digits$} {u} thermal")
}

fn compact(n: u64) -> String {
    if n >= 10_000 { format!("{}k", n / 1000) } else { n.to_string() }
}

// ---------------------------------------------------------------- tabs

fn build_tab(ui: &mut Gui, sim: &mut Sim, g: &mut Game) {
    ui.heading("Tools  [1-9]");
    let groups: [(&str, &[(Tool, &str)]); 3] = [
        ("Matter", &[(Tool::Nucleus, "Nucleus  1"), (Tool::Fill, "Fill area  4"), (Tool::Erase, "Erase  2"), (Tool::Electron, "Electron"), (Tool::Positron, "Positron"), (Tool::Spawner, "Spawner  8")]),
        ("Structure", &[(Tool::Wall, "Wall  3"), (Tool::EraseWall, "Erase wall"), (Tool::Pump, "Pump"), (Tool::Drain, "Drain  9"), (Tool::Force, "Push / pull  F")]),
        ("Reactor", &[(Tool::Rod, "Control rod  5"), (Tool::FuelChannel, "Fuel channel  6"), (Tool::Source, "n source  7")]),
    ];
    for (title, tools) in groups {
        ui.note(title);
        let names: Vec<&str> = tools.iter().map(|t| t.1).collect();
        let on = tools.iter().position(|t| t.0 == g.tool);
        if let Some(k) = ui.button_grid(&names, 3, on) {
            g.tool = tools[k].0;
        }
    }
    ui.note(match g.tool {
        Tool::Nucleus => "Click: place the nucleus below at rest.  Drag: launch it (energy shown).",
        Tool::Electron | Tool::Positron => "Click to place, drag to launch.",
        Tool::Erase => "Hold to erase nuclei and any reactor part (rods, channels, sources, spawners, drains, pumps).",
        Tool::Wall => "Drag to draw walls; Shift-drag for a straight wall.",
        Tool::EraseWall => "Hold to erase walls.",
        Tool::Rod => "Drag down to lay a control-rod channel. Rods move together from the Reactor tab.",
        Tool::FuelChannel => "Click to load the nuclide below and register a fuel channel the fuelling machine keeps loaded.",
        Tool::Source => "Click to add an Am-Be startup neutron source (as many as you like).",
        Tool::Fill => "Drag a rectangle to fill it with the nuclide below: moderator, fuel salt, propellant.",
        Tool::Spawner => "Click (or drag to aim) to add a spawner for the nuclide below: a fuel line, propellant feed or neutron gun.",
        Tool::Drain => "Drag a rectangle: matter entering it is removed and measured (exhaust nozzle, drain tank, off-gas).",
        Tool::Pump => "Drag a rectangle along the flow: it pushes matter (not neutrons) that way. Loops make a molten-salt circuit.",
        Tool::Force => "Hold the left button to pull matter towards the mouse, Shift to push it away. The right button does the same with any tool.",
    });
    match g.tool {
        Tool::Erase | Tool::EraseWall => {
            ui.slider("Eraser size", &mut g.brush_radius, 5.0, 150.0, Scale::Lin, 20.0, |v| format!("{v:.0} px"));
        }
        Tool::Wall => {
            let names: Vec<&str> = WallKind::ALL.iter().map(|k| k.name()).collect();
            let mut k = WallKind::ALL.iter().position(|&w| w == g.wall.kind).unwrap_or(0);
            if ui.segmented(&names, &mut k, 2) {
                g.wall.kind = WallKind::ALL[k];
                g.wall.reflect_neutrons = g.wall.kind.reflects_neutrons();
            }
            ui.note(match g.wall.kind {
                WallKind::Insulator => "Thick wall: no heat passes.",
                WallKind::Conductor => "Thin metal: takes heat from whatever touches one side and gives it to the other, and along connected conductors.",
                WallKind::FreezePlug => "Frozen salt: conducts like metal and melts away above the melt temperature (Physics tab).",
                WallKind::Filter => "Membrane that sorts what crosses it: what it stops bounces off. Insulating.",
            });
            if g.wall.kind == WallKind::Filter {
                filter_options(ui, g);
            } else {
                ui.note("Free neutrons");
                let mut k = g.wall.reflect_neutrons as usize;
                if ui.segmented(&["Pass through (window)", "Bounce back (reflector)"], &mut k, 2) {
                    g.wall.reflect_neutrons = k == 1;
                }
                if g.wall.reflect_neutrons != g.wall.kind.reflects_neutrons() {
                    ui.note(if g.wall.reflect_neutrons { "Drawn with cyan edges." } else { "Drawn with a dashed cyan centre line." });
                }
            }
            thickness_slider(ui, g);
            ui.note("Settings apply to the walls you draw next; walls already drawn keep theirs (hover one to see them).");
        }
        Tool::Spawner => {
            ui.slider("Spawn rate", &mut g.spawn_rate, 0.1, 30.0, Scale::Log, 2.0, |v| format!("{v:.1}/s"));
        }
        Tool::Drain => {
            let names: Vec<&str> = DrainFilter::ALL.iter().map(|f| f.name()).collect();
            let mut k = DrainFilter::ALL.iter().position(|&f| f == g.drain_filter).unwrap_or(0);
            if ui.segmented(&names, &mut k, 2) {
                g.drain_filter = DrainFilter::ALL[k];
            }
        }
        Tool::Pump => {
            ui.slider("Pump speed", &mut g.pump_speed, 5.0, 200.0, Scale::Log, 40.0, |v| format!("{v:.0} px/s"));
            ui.note("Sides (along the flow)");
            let opts = [None, Some(WallKind::Insulator), Some(WallKind::Conductor)];
            let mut k = opts.iter().position(|&o| o == g.pump_walls).unwrap_or(0);
            if ui.segmented(&["Open", "Insulator walls", "Conductor walls"], &mut k, 3) {
                g.pump_walls = opts[k];
            }
            if g.pump_walls.is_some() {
                thickness_slider(ui, g);
            }
        }
        Tool::Force => {
            ui.slider("Strength", &mut g.force_strength, 100.0, 50000.0, Scale::Log, 3000.0, |v| format!("{v:.0} px/s²"));
            ui.slider("Range", &mut g.force_radius, 10.0, 600.0, Scale::Log, 150.0, |v| format!("{v:.0} px"));
        }
        Tool::Fill => {
            ui.slider("Spacing between nuclei", &mut g.fill_gap, 0.0, 40.0, Scale::Lin, 4.0, |v| format!("{v:.0} px"));
        }
        _ => {}
    }

    ui.heading("Nucleus loader");
    let r = ui.text_field("Nuclide", &mut g.nuclide_text, "e.g. U-235, Co60, alpha");
    if r.changed {
        if let Some((z, n)) = parse_nuclide(&g.nuclide_text) {
            g.z = z;
            g.n = n;
            if !matches!(g.tool, Tool::FuelChannel) {
                g.tool = Tool::Nucleus;
            }
        }
    }
    if !g.nuclide_text.trim().is_empty() && parse_nuclide(&g.nuclide_text).is_none() {
        ui.label_col("Not recognised: try \"U-235\", \"Fe56\", \"n\", \"p\", \"alpha\"", col::BAD);
    }
    let (z0, n0) = (g.z, g.n);
    ui.stepper("Protons  Z", &mut g.z, 0, 118);
    ui.stepper("Neutrons  N", &mut g.n, 0, 180);
    let mut zf = g.z as f32;
    if ui.slider("Z", &mut zf, 0.0, 118.0, Scale::Lin, 92.0, |v| format!("{:.0}  {}", v.round(), nucdata::SYMBOLS.get(v.round() as usize).copied().unwrap_or(""))) {
        g.z = zf.round() as u32;
    }
    let mut nf = g.n as f32;
    if ui.slider("N", &mut nf, 0.0, 180.0, Scale::Lin, 143.0, |v| format!("{:.0}", v.round())) {
        g.n = nf.round() as u32;
    }
    if (g.z, g.n) != (z0, n0) {
        g.nuclide_text = nuclide_name(g.z, g.n);
        if !matches!(g.tool, Tool::FuelChannel) {
            g.tool = Tool::Nucleus;
        }
    }
    let lines = describe(g.z, g.n, &sim.cfg);
    if let Some((first, rest)) = lines.split_first() {
        ui.space(2.0);
        ui.label_col(first, col::ACCENT);
        for l in rest {
            ui.note(l);
        }
    }
    ui.space(6.0);
    let presets: [(&str, u32, u32); 24] = [
        ("n", 0, 1), ("p", 1, 0), ("D", 1, 1), ("T", 1, 2), ("He-3", 2, 1), ("He-4", 2, 2),
        ("Li-6", 3, 3), ("Li-7", 3, 4), ("Be-7", 4, 3), ("B-10", 5, 5), ("C-12", 6, 6), ("C-14", 6, 8),
        ("O-16", 8, 8), ("Fe-56", 26, 30), ("Co-60", 27, 33), ("Sr-90", 38, 52), ("Cd-113", 48, 65), ("Cs-137", 55, 82),
        ("Ra-226", 88, 138), ("Th-232", 90, 142), ("U-235", 92, 143), ("U-238", 92, 146), ("Pu-239", 94, 145), ("Cf-252", 98, 154),
    ];
    let names: Vec<&str> = presets.iter().map(|p| p.0).collect();
    let on = presets.iter().position(|p| (p.1, p.2) == (g.z, g.n));
    if let Some(k) = ui.button_grid(&names, 6, on) {
        let (_, z, n) = presets[k];
        g.z = z;
        g.n = n;
        g.nuclide_text = nuclide_name(z, n);
        if !matches!(g.tool, Tool::FuelChannel) {
            g.tool = Tool::Nucleus;
        }
    }
    ui.space(4.0);
    let mut matter = sim.cfg.matter;
    if ui.checkbox("In matter (off: vacuum, bare ions repel)", &mut matter) {
        sim.cfg = sim.cfg.clone().with_medium(matter);
    }
    ui.checkbox("Add electrons with each nucleus", &mut g.with_electrons);
    ui.space(6.0);
    ui.note("Right mouse: pull (Shift: push), strength and range set by the Push / pull tool.   Z: zoom window.   Hover anything for details.");
}

/// Thickness of the next wall (only new walls).
fn thickness_slider(ui: &mut Gui, g: &mut Game) {
    let mut t = g.wall.half * 2.0;
    if ui.slider("Thickness of new walls", &mut t, 1.0, 16.0, Scale::Lin, 4.0, |v| format!("{v:.0} px")) {
        g.wall.half = (t * 0.5).max(0.5);
    }
}

/// What a new filter wall sorts by, and which way.
fn filter_options(ui: &mut Gui, g: &mut Game) {
    use particle_sim::walls::{FilterRule, describe_filter};
    let w = &mut g.wall;
    let names: Vec<&str> = FilterRule::ALL.iter().map(|r| r.name()).collect();
    let mut k = FilterRule::ALL.iter().position(|&r| r == w.rule).unwrap_or(0);
    if ui.segmented(&names, &mut k, 4) {
        let new = FilterRule::ALL[k];
        if new != w.rule {
            // Sensible starting points: xenon; heavy nuclei (A >= 40).
            w.filter_z = match new {
                FilterRule::Mass => 40,
                _ if w.rule == FilterRule::Mass => 54,
                _ => w.filter_z,
            };
        }
        w.rule = new;
    }
    let senses: [&str; 2] = match w.rule {
        FilterRule::Element => ["Pass all but it", "Pass only it"],
        FilterRule::Charged => ["Pass only neutral", "Pass only charged"],
        FilterRule::Energy => ["Pass only below", "Pass only at or above"],
        FilterRule::Mass => ["Pass all but A ≥", "Pass only A ≥"],
    };
    let mut k = w.only as usize;
    if ui.segmented(&senses, &mut k, 2) {
        w.only = k == 1;
    }
    match w.rule {
        FilterRule::Element => {
            ui.stepper("Element  Z (0 = neutrons)", &mut w.filter_z, 0, 118);
            if ui.button(&format!("Use the loader's element ({})##filterz", nucdata::SYMBOLS.get(g.z as usize).copied().unwrap_or("n"))) {
                g.wall.filter_z = g.z;
            }
        }
        FilterRule::Charged => ui.note("Charged: protons, nuclei, electrons, positrons. Neutral: free neutrons (photons and neutrinos pass every wall)."),
        FilterRule::Energy => {
            ui.slider("Energy threshold", &mut w.threshold, 1e-9, 200.0, Scale::Log, 1.0, |v| particle_sim::nuclear::format_energy(v as f64));
            ui.note("Kinetic energy of the whole particle or nucleus. Thermal neutrons ~25 meV, fission neutrons ~2 MeV, alphas ~5 MeV, fission fragments ~70-100 MeV. Hover a particle to see its energy.");
        }
        FilterRule::Mass => {
            ui.stepper("Mass number  A", &mut w.filter_z, 1, 300);
            ui.note("Counts nuclei only (A = protons + neutrons). Free neutrons and protons are A = 1.");
        }
    }
    let w = &g.wall;
    ui.label_col(&format!("This filter {}", describe_filter(w.rule, w.filter_z, w.threshold, w.only)), col::ACCENT);
}

fn reactor_tab(ui: &mut Gui, sim: &mut Sim, act: &mut Actions) {
    if sim.reactor.is_empty() {
        ui.label("No reactor in this scene.");
        ui.note("Load one of the reactor scenes, or build your own in the Build tab: fuel channels or spawners, control rods, neutron sources, moderator (Fill with D or C-12), walls, pumps and drains.");
        if ui.button("Load the heavy-water reactor") {
            act.preset = Some(3);
        }
        if ui.button("Load the molten-salt reactor") {
            act.preset = Some(scenes::MSR);
        }
        if ui.button("Load the nuclear thermal rocket") {
            act.preset = Some(scenes::ROCKET);
        }
        return;
    }
    let re = &sim.reactor;
    let power = re.power();
    let period = re.period();
    let neutrons = re.samples.back().map(|s| s.neutrons).unwrap_or(0);
    let (state, sc) = match period {
        _ if (neutrons == 0 && power == 0.0) || (re.insertion >= 0.99 && !re.rods.is_empty()) => ("SHUT DOWN", col::DIM),
        Some(p) if p.is_finite() && p > 0.0 && p < 15.0 => ("SUPERCRITICAL", col::BAD),
        Some(p) if p.is_finite() && p < 0.0 && p > -15.0 => ("SUBCRITICAL", col::ACCENT),
        Some(_) => ("CRITICAL", col::GOOD),
        None => ("SOURCE RANGE", col::DIM),
    };
    // Big readout.
    let r = ui.alloc(56.0);
    draw_rectangle(r.x, r.y, r.w, r.h - 6.0, col::TRACK);
    draw_rectangle_lines(r.x, r.y, r.w, r.h - 6.0, 1.0, col::EDGE);
    ui.text(state, r.x + 10.0, r.y + 6.0, 20.0, sc);
    let ptxt = format!("{power:.1} fissions/s");
    let pw = ui.text_width(&ptxt, 20.0);
    ui.text(&ptxt, r.x + r.w - pw - 10.0, r.y + 6.0, 20.0, col::TEXT);
    // ~200 MeV per fission, 1 MeV = 1.602e-13 J.
    let watts = power as f64 * 200.0 * 1.602e-13;
    let sub = format!(
        "{neutrons} free neutrons   period {}   {}",
        match period {
            Some(p) if p.is_finite() => format!("{p:+.0} s"),
            Some(_) => "∞".into(),
            None => "—".into(),
        },
        format_watts(watts)
    );
    ui.text(&sub, r.x + 10.0, r.y + 30.0, 14.0, col::DIM);

    plot(ui, sim);

    if !sim.reactor.rods.is_empty() {
        ui.heading("Control rods  [↑ / ↓,  X = SCRAM]");
        let re = &mut sim.reactor;
        let mut pct = re.target * 100.0;
        if ui.slider("Rod position (target)", &mut pct, 0.0, 100.0, Scale::Lin, 100.0, |v| format!("{v:.0}% in")) {
            re.target = pct / 100.0;
            re.scram = false;
        }
        let bar = ui.alloc(22.0);
        let fill = Rect::new(bar.x, bar.y + 4.0, bar.w, 10.0);
        draw_rectangle(fill.x, fill.y, fill.w, fill.h, col::TRACK);
        draw_rectangle(fill.x, fill.y, fill.w * re.insertion, fill.h, if re.scram { col::BAD } else { Color::from_rgba(120, 126, 146, 255) });
        draw_rectangle_lines(fill.x, fill.y, fill.w, fill.h, 1.0, col::EDGE);
        let moving = (re.target - re.insertion).abs() > 1e-3;
        ui.note(&format!(
            "actual {:.0}% in{}",
            re.insertion * 100.0,
            if re.scram { "   SCRAM: rods dropping" } else if moving { "   driving…" } else { "" }
        ));
        match ui.buttons(&["Withdraw 10%", "Insert 10%", "SCRAM"]) {
            Some(0) => {
                re.target = (re.target - 0.1).max(0.0);
                re.scram = false;
            }
            Some(1) => re.target = (re.target + 0.1).min(1.0),
            Some(2) => re.scram(),
            _ => {}
        }
        let mut drive = re.drive * 100.0;
        if ui.slider("Rod drive speed", &mut drive, 1.0, 50.0, Scale::Log, 8.0, |v| format!("{v:.0}%/s")) {
            re.drive = drive / 100.0;
        }
        ui.note(&format!("{} rods   {} neutrons absorbed by them (¹⁰B + n → ⁷Li + α)", re.rods.len(), re.rod_captures));
    }
    if !sim.reactor.sites.is_empty() {
        ui.heading("Fuel");
        let re = &sim.reactor;
        let fresh = re.sites.iter().filter(|s| s.empty_for == 0.0).count();
        ui.kv("Fuel channels loaded", &format!("{fresh} / {}", re.sites.len()), if fresh == re.sites.len() { col::GOOD } else { col::WARN });
        ui.kv("Channels reloaded", &re.refuels.to_string(), col::TEXT);
        let census = &sim.nuclear.census;
        let count = |z: u32, n: u32| census.iter().find(|c| c.0 == (z, n)).map_or(0, |c| c.1);
        ui.kv("Xe-135 (poison)", &count(54, 81).to_string(), if count(54, 81) > 0 { col::WARN } else { col::DIM });
        ui.kv("Pu-239 (bred)", &count(94, 145).to_string(), col::TEXT);
        let re = &mut sim.reactor;
        ui.checkbox("On-power refuelling (CANDU fuelling machine)", &mut re.auto_refuel);
        ui.slider("Reload delay", &mut re.refuel_delay, 0.0, 30.0, Scale::Lin, 2.0, |v| format!("{v:.1} s"));
        if ui.button("Reload all spent channels now") {
            act.refuel_all = true;
        }
    }
    let re = &mut sim.reactor;
    ui.heading("Neutron sources");
    if re.sources.is_empty() {
        ui.note("None: place Am-Be sources with the n source tool (Build tab). A shut-down core has no neutrons of its own.");
    } else {
        ui.slider(&format!("Strength of each of {} sources", re.sources.len()), &mut re.source_rate, 0.0, 5.0, Scale::Lin, 0.5, |v| format!("{v:.2} n/s"));
    }
    flow_tab(ui, sim);
    ui.heading("How to run it");
    let re = &sim.reactor;
    if re.spawners.iter().any(|s| s.keep > 0) {
        ui.note("Molten salt: the fuel is in the salt, and its concentration (the fuel feed above) sets the power more than the rods do - the salt moderates poorly, so most fissions happen at keV energies where boron is nearly transparent. The salt's heat leaves through the conductor wall into the helium stream. The freeze plug in the floor is the safety: run it flat out (rods out, lots of fuel) for a couple of minutes and the salt gets hot enough to melt it; the salt drains into the tank below and the chain reaction stops. Its melt point is in the Physics tab.");
    } else if re.drains.iter().any(|d| d.filter == particle_sim::reactor::DrainFilter::All) && re.spawners.iter().any(|s| s.z + s.n <= 2) {
        ui.note("Rocket: withdraw the rods to heat the core. Fission heat warms the moderator; the moderator heats the conductor plate; the hydrogen pumped under the plate picks the heat up and leaves through the nozzle. The exhaust reading under Drains is your exhaust speed and thrust - more power, hotter plate, faster exhaust.");
    } else {
        ui.note("Withdraw the rods until the period turns positive and power climbs, then insert them a little to hold it level. Delayed neutrons from fission products (Br-87, I-137) are what make this possible: watch the period, not the instantaneous count. Xe-135 builds up after a run at power and soaks up neutrons.");
    }
}

/// Spawners, drains, pumps and heat.
fn flow_tab(ui: &mut Gui, sim: &mut Sim) {
    use particle_sim::thermal::{format_kelvin, kelvin_of, region_theta};
    use particle_sim::walls::WallKind;
    let cfg = sim.cfg.clone();
    let re = &mut sim.reactor;
    if !re.spawners.is_empty() {
        ui.heading("Spawners");
        for (k, sp) in re.spawners.iter_mut().enumerate() {
            if sp.keep > 0 {
                let mut keep = sp.keep as f32;
                let name = format!("{} concentration: keep in the salt  ({} added)", nuclide_name(sp.z, sp.n), sp.spawned);
                if ui.slider(&format!("{name}##keep{k}"), &mut keep, 1.0, 40.0, Scale::Lin, 24.0, |v| format!("{:.0}", v.round())) {
                    sp.keep = keep.round() as u32;
                }
            } else {
                let name = format!("{} → {:.0} px/s   ({} made)", nuclide_name(sp.z, sp.n), sp.vel.length(), sp.spawned);
                ui.slider(&format!("{name}##sp{k}"), &mut sp.rate, 0.0, 30.0, Scale::Lin, 2.0, |v| format!("{v:.1}/s"));
            }
        }
        match ui.buttons(&["Pause all", "Remove all"]) {
            Some(0) => re.spawners.iter_mut().for_each(|sp| sp.rate = 0.0),
            Some(1) => re.spawners.clear(),
            _ => {}
        }
    }
    if !re.drains.is_empty() {
        ui.heading("Drains & exhausts");
        for d in &re.drains {
            ui.label_col(&format!("{}   {} removed", d.filter.name(), d.removed), col::TEXT);
            ui.note(&format!(
                "{:.1} nuclei/s   {:.0} px/s out   {} per nucleon   thrust {:.0}",
                d.rate,
                d.speed,
                format_energy(d.energy),
                d.thrust
            ));
        }
    }
    if !re.pumps.is_empty() {
        ui.heading("Pumps");
        for (k, pm) in re.pumps.iter_mut().enumerate() {
            ui.slider(&format!("Pump {} speed##pm{k}", k + 1), &mut pm.speed, 0.0, 200.0, Scale::Lin, 40.0, |v| format!("{v:.0} px/s"));
        }
    }
    let walls = &sim.walls;
    let cond: Vec<f32> = walls.segs.iter().filter(|s| s.kind.conducts()).map(|s| s.theta).collect();
    if !cond.is_empty() {
        ui.heading("Heat");
        let hot = cond.iter().cloned().fold(0.0, f32::max);
        let mean = cond.iter().sum::<f32>() / cond.len() as f32;
        ui.kv("Hottest conductor wall", &format_kelvin(kelvin_of(&cfg, hot)), col::WARN);
        ui.kv("Mean conductor wall", &format_kelvin(kelvin_of(&cfg, mean)), col::TEXT);
        let plugs = walls.segs.iter().filter(|s| s.kind == WallKind::FreezePlug).count();
        if plugs > 0 {
            ui.kv("Freeze plug segments", &format!("{plugs} (melt at {})", format_kelvin(cfg.melt_temp as f64)), col::ACCENT);
        }
    }
    if let Some(t) = region_theta(sim, |_| true) {
        ui.kv("Mean matter temperature", &format_kelvin(kelvin_of(&cfg, t)), col::TEXT);
    }
}

/// Neutron count, fission rate and rod position over the recorded history.
fn plot(ui: &mut Gui, sim: &Sim) {
    let r = ui.alloc(128.0);
    let r = Rect::new(r.x, r.y + 4.0, r.w, r.h - 18.0);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.05, 0.055, 0.075, 1.0));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, col::EDGE);
    let s = &sim.reactor.samples;
    if s.len() < 2 {
        return;
    }
    // Show the whole history up to 2 minutes, filling the width.
    let n_show = ((120.0 / SAMPLE_DT) as usize).min(s.len());
    let pts: Vec<_> = s.iter().skip(s.len() - n_show).collect();
    let nmax = pts.iter().map(|p| p.neutrons as f32).fold(10.0, f32::max) * 1.15;
    let fmax = pts.iter().map(|p| p.fission_rate).fold(4.0, f32::max) * 1.15;
    let slots = (n_show.max(40) - 1) as f32;
    let xs = |k: usize| r.x + r.w * k as f32 / slots;
    // Rod insertion: a shaded band from the top, one rect per level.
    let mut k0 = 0;
    for k in 1..=pts.len() {
        if k == pts.len() || (pts[k].insertion - pts[k0].insertion).abs() > 0.004 {
            let (x0, x1) = (xs(k0).floor(), xs(k.min(pts.len() - 1)).floor());
            let bh = r.h * pts[k0].insertion;
            draw_rectangle(x0, r.y, (x1 - x0).max(1.0), bh, Color::new(0.42, 0.45, 0.56, 0.32));
            draw_line(x0, r.y + bh, x1.max(x0 + 1.0), r.y + bh, 1.0, Color::new(0.62, 0.65, 0.76, 0.9));
            k0 = k;
        }
    }
    let mut smooth = 0.0;
    let mut prev_f: Option<Vec2> = None;
    let mut prev_n: Option<Vec2> = None;
    for (k, p) in pts.iter().enumerate() {
        smooth = if k == 0 { p.fission_rate } else { smooth * 0.75 + p.fission_rate * 0.25 };
        let x = xs(k);
        let pf = vec2(x, r.y + r.h - r.h * smooth / fmax);
        let pn = vec2(x, r.y + r.h - r.h * p.neutrons as f32 / nmax);
        if let Some(a) = prev_f {
            draw_line(a.x, a.y, pf.x, pf.y, 1.5, Color::from_rgba(255, 140, 90, 255));
        }
        if let Some(a) = prev_n {
            draw_line(a.x, a.y, pn.x, pn.y, 1.5, Color::from_rgba(120, 190, 255, 255));
        }
        prev_f = Some(pf);
        prev_n = Some(pn);
    }
    ui.text(&format!("neutrons (max {:.0})", nmax / 1.15), r.x + 6.0, r.y + 3.0, 13.0, Color::from_rgba(120, 190, 255, 255));
    let t = format!("fissions/s (max {:.1})", fmax / 1.15);
    let tw = ui.text_width(&t, 13.0);
    ui.text(&t, r.x + r.w - tw - 6.0, r.y + 3.0, 13.0, Color::from_rgba(255, 140, 90, 255));
    let secs = n_show as f32 * SAMPLE_DT;
    ui.text(&format!("last {secs:.0} s   grey band from the top = rod insertion"), r.x, r.y + r.h + 2.0, 12.0, col::FAINT);
}

fn view_tab(ui: &mut Gui, g: &mut Game) {
    ui.heading("Colour by  [V]");
    let modes: Vec<&str> = ViewMode::ALL.iter().map(|m| m.name()).collect();
    let mut k = ViewMode::ALL.iter().position(|&m| m == g.view.mode).unwrap_or(0);
    if ui.segmented(&modes, &mut k, modes.len()) {
        g.view.mode = ViewMode::ALL[k];
    }
    if g.view.mode == ViewMode::Speed {
        ui.slider("Hot speed", &mut g.view.speed_scale, 50.0, 1500.0, Scale::Log, 400.0, |v| format!("{v:.0} px/s"));
    }
    ui.checkbox("Nuclide labels (Element view)", &mut g.view.labels);
    ui.checkbox("Trails  [L]", &mut g.view.trails);
    if g.view.trails {
        ui.slider("Trail fade", &mut g.view.trail_fade, 0.01, 0.5, Scale::Log, 0.08, |v| format!("{v:.2}"));
    }
    ui.heading("Zoom  [Z]");
    ui.note("Z opens a magnifier that follows the mouse. Click to lock it; then the mouse works inside the window (place, launch, erase at the zoomed scale). Scroll to change magnification. Z or Esc closes it.");
}

fn scenes_tab(ui: &mut Gui, g: &mut Game, saved: &mut Vec<String>, act: &mut Actions) {
    ui.heading("Scenarios");
    for (k, (name, desc)) in PRESETS.iter().enumerate() {
        if ui.button(name) {
            act.preset = Some(k);
        }
        ui.note(desc);
        ui.space(4.0);
    }
    ui.heading("Save");
    let r = ui.text_field("Name", &mut g.save_name, "my scene");
    if ui.button("Save scene") || r.submitted {
        act.save = true;
    }
    ui.heading("Saved scenes");
    if saved.is_empty() {
        ui.note("(none yet)");
    }
    for name in saved.iter() {
        let row = ui.alloc(28.0);
        ui.space(4.0);
        let (x, y, w) = (row.x, row.y, row.w);
        ui.layout(x, y, w - 120.0);
        if ui.button(&format!("{name}##load{name}")) {
            act.load = Some(name.clone());
        }
        ui.layout(x + w - 114.0, y, 114.0);
        if ui.button(&format!("Delete##del{name}")) {
            act.delete = Some(name.clone());
        }
        ui.layout(x, y + 32.0, w);
    }
    if ui.button("Refresh list") {
        *saved = list_scenes();
    }
    ui.note(&format!("Folder: {}", scene_dir().display()));
}

fn physics_tab(ui: &mut Gui, sim: &mut Sim, act: &mut Actions) {
    let d = particle_sim::nuclear::game_config();
    let c = &mut sim.cfg;
    ui.note("Right-click any slider to reset it.");
    ui.heading("Time");
    ui.note("t here = x · t real ^ exponent   (x = 1, exponent = 1: real time)");
    ui.slider("x", &mut c.decay_scale, 0.01, 20.0, Scale::Log, d.decay_scale, |v| format!("{v:.2}"));
    ui.slider("Exponent", &mut c.time_exponent, 0.1, 1.0, Scale::Lin, d.time_exponent, |v| format!("{v:.2}"));
    ui.note(&format!(
        "free n {}   N-13 {}   H-3 {}   U-235 {}",
        format_time(sim_half_life(609.8, c)),
        format_time(sim_half_life(598.0, c)),
        format_time(sim_half_life(3.888e8, c)),
        format_time(sim_half_life(2.22e16, c)),
    ));
    ui.heading("Energy");
    ui.note("sim speed = v₁ · (E / MeV per nucleon) ^ (exponent / 2)");
    ui.slider("v₁ (speed of a 1 MeV neutron)", &mut c.mev_speed, 100.0, 1000.0, Scale::Lin, d.mev_speed, |v| format!("{v:.0} px/s"));
    ui.slider("Energy exponent", &mut c.energy_exponent, 0.2, 1.0, Scale::Lin, d.energy_exponent, |v| format!("{v:.2}"));
    ui.note(&format!(
        "thermal n {:.0} px/s   1 keV {:.0}   14 MeV {:.0}",
        speed_for(c, reactions::E_THERMAL, 1.0),
        speed_for(c, 1e-3, 1.0),
        speed_for(c, 14.1, 1.0),
    ));
    ui.heading("Interactions");
    ui.slider("Tunnelling boost", &mut c.tunneling, 1.0, 1000.0, Scale::Log, d.tunneling, |v| format!("×{v:.0}"));
    let mut matter = c.matter;
    if ui.checkbox("In matter (screened Coulomb, stopping)", &mut matter) {
        *c = c.clone().with_medium(matter);
    }
    let c = &mut sim.cfg;
    ui.checkbox("Coulomb repulsion", &mut c.coulomb);
    ui.slider("Coulomb k", &mut c.k, 100.0, 100000.0, Scale::Log, d.k, |v| format!("{v:.0}"));
    ui.slider("Stopping (matter)", &mut c.stopping, 0.0, 60.0, Scale::Lin, d.stopping, |v| format!("{v:.0} /s"));
    ui.slider("Temperature", &mut c.temperature, 0.0, 3000.0, Scale::Lin, d.temperature, |v| format!("{v:.0} K"));
    if sim.point_nuclei {
        let c = &mut sim.cfg;
        ui.heading("Point nuclei: neutron interactions");
        ui.note("A free neutron interacts with a nucleus when it passes within  r = scale · σ ^ exponent  (σ = total cross-section in barns at that energy). Then it is absorbed with probability σ_abs / σ, else scattered. The exponent compresses real cross-sections, as energy and time are compressed.");
        ui.slider("Scale", &mut c.xs_scale, 0.5, 15.0, Scale::Log, d.xs_scale, |v| format!("{v:.1} px"));
        ui.slider("Exponent", &mut c.xs_exponent, 0.1, 0.5, Scale::Lin, d.xs_exponent, |v| format!("{v:.2}"));
        let r = |z, n, e| particle_sim::nuclear::interaction_radius(c, z, n, e);
        let th = reactions::E_THERMAL;
        ui.note(&format!(
            "thermal n:  D {:.1} px   H {:.1}   C-12 {:.1}   U-238 {:.1}   U-235 {:.0}   B-10 {:.0}",
            r(1, 1, th), r(1, 0, th), r(6, 6, th), r(92, 146, th), r(92, 143, th), r(5, 5, th)
        ));
        ui.note(&format!("2 MeV n:  D {:.1} px   U-235 {:.1}", r(1, 1, 2.0), r(92, 143, 2.0)));
    }
    let c = &mut sim.cfg;
    ui.heading("Heat");
    ui.slider("Cooling to the surroundings", &mut c.cooling, 0.0, 60.0, Scale::Lin, d.cooling, |v| if v == 0.0 { "off (adiabatic)".into() } else { format!("{v:.0} /s") });
    ui.slider("Fission heat kept in matter", &mut c.heat_fraction, 0.0, 0.2, Scale::Lin, d.heat_fraction, |v| format!("{:.1}%", v * 100.0));
    ui.slider("Freeze plug melts at", &mut c.melt_temp, 300.0, 1e6, Scale::Log, d.melt_temp, |v| particle_sim::thermal::format_kelvin(v as f64));
    ui.note("With cooling on, matter is held near the temperature above. Turn it off (the rocket and molten-salt scenes do) and heat from fission stays in the matter, moves through conductor walls and leaves through drains.");
    ui.heading("Shape & contacts");
    ui.slider("Roundness", &mut c.roundness, 0.0, 300.0, Scale::Lin, d.roundness, |v| format!("{v:.0}"));
    ui.slider("Internal damping", &mut c.nuclear_damping, 0.0, 30.0, Scale::Lin, d.nuclear_damping, |v| format!("{v:.1}"));
    ui.slider("Restitution", &mut c.restitution, 0.0, 1.0, Scale::Lin, d.restitution, |v| format!("{v:.2}"));
    ui.slider("Drag", &mut c.drag, 0.0, 2.0, Scale::Lin, d.drag, |v| format!("{v:.2}"));
    ui.slider("Light speed", &mut c.light_speed, 400.0, 1500.0, Scale::Lin, d.light_speed, |v| format!("{v:.0} px/s"));
    ui.checkbox("Log instant breakups of unbound nuclei", &mut sim.nuclear.log_breakups);
    ui.space(6.0);
    if ui.button("Reset all to real-world defaults") {
        act.reset_physics = true;
    }
}

fn log_tab(ui: &mut Gui, sim: &mut Sim, ps: &mut PanelState, body: Rect, added: u32, act: &mut Actions) {
    ui.layout(body.x, body.y, body.w - 6.0);
    let ns = &sim.nuclear;
    ui.note(&format!(
        "fission {}   fusion {}   capture {}   annihilation {}   energy {}",
        ns.fissions,
        ns.fusions,
        ns.captures,
        ns.annihilations,
        format_energy(ns.energy_released)
    ));
    let mut counts: Vec<(Mode, u64)> = ns.counts.iter().map(|(m, c)| (*m, *c)).collect();
    counts.sort();
    if !counts.is_empty() {
        let line: Vec<String> = counts.iter().map(|(m, c)| format!("{} {c}", m.short())).collect();
        ui.note(&format!("decays: {}", line.join("   ")));
    }
    let names: Vec<&str> = FILTERS.iter().map(|f| f.0).collect();
    ui.tabs(&names, &mut ps.filter);
    ui.text_field("Search", &mut ps.search, "nuclide or word, e.g. U-235, β⁻, Xe");
    let total = ns.dropped + ns.history.len() as u64;
    match ui.buttons(&["Export all (CSV)", "Export shown", "Clear log"]) {
        Some(0) => act.export = Some(true),
        Some(1) => act.export = Some(false),
        Some(2) => {
            let ns = &mut sim.nuclear;
            ns.history.clear();
            ns.log.clear();
            ns.dropped = 0;
            ps.cache_key = (usize::MAX, String::new());
        }
        _ => {}
    }
    ui.note(&format!(
        "{} events{}, newest first. Hover a row for the full text.",
        total,
        if ps.shown.len() as u64 != total { format!(" ({} shown)", ps.shown.len()) } else { String::new() }
    ));
    let ns = &sim.nuclear;
    let list = Rect::new(body.x, ui.y + 2.0, body.w, body.y + body.h - ui.y - 2.0);
    let row_h = 38.0;
    // Keep the rows under the mouse still while new events arrive.
    {
        let off = ui.scroll_offset("loglist");
        if *off > 0.0 && added > 0 {
            *off += added as f32 * row_h;
        }
    }
    ui.scroll_begin("loglist", list);
    let n = ps.shown.len();
    let top = ui.y;
    let first = (((list.y - top) / row_h).floor().max(0.0)) as usize;
    let last = ((((list.y + list.h) - top) / row_h).ceil() as usize + 1).min(n);
    for k in first..last {
        let g = ps.shown[n - 1 - k];
        let Some(ev) = ns.history.get((g - ns.dropped) as usize) else { continue };
        let r = Rect::new(ui.x(), top + k as f32 * row_h, ui.w, row_h - 2.0);
        let hov = ui.hover(r);
        draw_rectangle(r.x, r.y, r.w, r.h, if hov { col::HOVER } else if k % 2 == 0 { Color::new(1.0, 1.0, 1.0, 0.025) } else { Color::new(0.0, 0.0, 0.0, 0.0) });
        draw_rectangle(r.x, r.y, 3.0, r.h, kind_color(ev.kind));
        let tstr = format!("{:.1} s", ev.t);
        let tw = ui.text_width(&tstr, 12.0);
        ui.text(&tstr, r.x + r.w - tw - 4.0, r.y + 4.0, 12.0, col::FAINT);
        let (eq, cut1) = ui.fit(ev.equation(), r.w - tw - 18.0, 15.0);
        ui.text(&eq, r.x + 9.0, r.y + 2.0, 15.0, col::TEXT);
        let det = format!("{} · {}", ev.kind.name(), ev.details());
        let (det, cut2) = ui.fit(&det, r.w - 14.0, 12.5);
        ui.text(&det, r.x + 9.0, r.y + 20.0, 12.5, kind_color(ev.kind));
        if hov {
            let mut tip = vec![ev.equation().to_string(), ev.details().to_string(), format!("#{}   at t = {:.2} s", g + 1, ev.t)];
            if !(cut1 || cut2) {
                tip.truncate(3);
            }
            ui.tip(tip);
        }
    }
    ui.space(n as f32 * row_h);
    if n == 0 {
        ui.note("No events yet.");
    }
    ui.scroll_end();
}

/// Write the log to a CSV file; returns a message for the status line.
pub fn export(sim: &Sim, ps: &PanelState, all: bool) -> String {
    let ns = &sim.nuclear;
    let path = scenes::log_dir().join(format!("events-{}.csv", scenes::timestamp()));
    let res = if all {
        scenes::write_events_csv(&path, ns.history.iter().enumerate().map(|(k, e)| (ns.dropped + k as u64 + 1, e)))
    } else {
        scenes::write_events_csv(
            &path,
            ps.shown().iter().filter_map(|&g| ns.history.get((g - ns.dropped) as usize).map(|e| (g + 1, e))),
        )
    };
    match res {
        Ok(n) => format!("Exported {n} events to {}", path.display()),
        Err(e) => format!("Export failed: {e}"),
    }
}
