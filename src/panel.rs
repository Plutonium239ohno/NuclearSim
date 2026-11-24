//! The Nuclear Game's control panel (drawn with gui.rs).

use crate::gui::{Gui, Scale, col};
use crate::{Game, Tool};
use macroquad::prelude::*;
use sim_core::game_scenes::{self as scenes, PRESETS, list_scenes, scene_dir};
use sim_core::nucdata::{self, Mode, format_time, info, nuclide_name, parse_nuclide};
use sim_core::nuclear::{EventKind, format_energy, sim_half_life, speed_for};
use sim_core::reactions;
use sim_core::reactor::{SAMPLE_DT, Sample};
use sim_core::reactor::DrainFilter;
use sim_core::render::ViewMode;
use sim_core::walls::WallKind;
use sim_core::sim::{Config, Sim};

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
    pub undo: bool,
    pub redo: bool,
    pub new_seed: bool,
    pub export_graphs: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Build,
    Reactor,
    Measure,
    Auto,
    Isotopes,
    View,
    Scenes,
    Physics,
    Log,
}

/// Two rows of tabs.
const TABS: [(Tab, &str); 9] = [
    (Tab::Build, "Build"),
    (Tab::Reactor, "Reactions"),
    (Tab::Measure, "Measure"),
    (Tab::Auto, "Auto"),
    (Tab::Isotopes, "Isotopes"),
    (Tab::View, "View"),
    (Tab::Scenes, "Scenes"),
    (Tab::Physics, "Physics"),
    (Tab::Log, "Log"),
];
const FIRST_ROW: usize = 5;

/// Log filters: (label, kinds shown).
const FILTERS: [(&str, &[EventKind]); 7] = [
    ("All", &[]),
    ("Fission", &[EventKind::Fission]),
    ("Fusion", &[EventKind::Fusion]),
    ("Breakup", &[EventKind::Breakup]),
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
    seed_text: String,
    /// Graph series shown (see SERIES).
    series: [bool; SERIES.len()],
    /// Spawner whose editor is open on the Reactions tab.
    pub open_spawner: Option<usize>,
    pub rule: crate::tabs::RuleBuilder,
    pub chart: crate::tabs::Chart,
}

impl Default for PanelState {
    fn default() -> Self {
        Self { tab: Tab::Build, filter: 0, search: String::new(), shown: Vec::new(), cache_key: (usize::MAX, String::new()), seen: 0, seed_text: String::new(), series: [true, true, true, false, false, true, false, false], open_spawner: None, rule: Default::default(), chart: Default::default() }
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
    if !nuc.branches.is_empty() && nuc.half_life.is_some() {
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

pub(crate) fn kind_color(k: EventKind) -> Color {
    match k {
        EventKind::Fission => Color::from_rgba(255, 140, 90, 255),
        EventKind::Fusion => Color::from_rgba(255, 220, 110, 255),
        EventKind::Breakup => Color::from_rgba(215, 160, 255, 255),
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
    match ui.buttons(&[pause, "Step", "Undo", "Redo", "Clear", "Hide"]) {
        Some(0) => g.paused = !g.paused,
        Some(1) => g.step_once = true,
        Some(2) => act.undo = true,
        Some(3) => act.redo = true,
        Some(4) => act.clear = true,
        Some(5) => g.panel_open = false,
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
    let mut a = if k < FIRST_ROW { k } else { usize::MAX };
    if ui.tabs(&refs[..FIRST_ROW], &mut a) {
        k = a;
    }
    ui.space(-4.0);
    let mut b = if k >= FIRST_ROW { k - FIRST_ROW } else { usize::MAX };
    if ui.tabs(&refs[FIRST_ROW..], &mut b) {
        k = b + FIRST_ROW;
    }
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
                Tab::Reactor => reactor_tab(ui, sim, ps, &mut act, (g.z, g.n)),
                Tab::Measure => crate::tabs::measure_tab(ui, sim, g),
                Tab::Auto => crate::tabs::auto_tab(ui, sim, ps),
                Tab::Isotopes => crate::tabs::isotopes_tab(ui, sim, g, ps),
                Tab::View => view_tab(ui, g),
                Tab::Scenes => scenes_tab(ui, g, saved, &mut act),
                Tab::Physics => {
                    repeatable_section(ui, sim, g, ps, &mut act);
                    physics_tab(ui, sim, &mut act)
                }
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
    let groups: [(&str, &[(Tool, &str)]); 4] = [
        ("Matter", &[(Tool::Nucleus, "Nucleus  1"), (Tool::Fill, "Fill area  4"), (Tool::Erase, "Erase  2"), (Tool::Electron, "Electron"), (Tool::Positron, "Positron"), (Tool::Spawner, "Spawner  8")]),
        ("Structure", &[(Tool::Wall, "Wall  3"), (Tool::EraseWall, "Erase wall"), (Tool::Pump, "Pump"), (Tool::Drain, "Drain  9"), (Tool::Field, "E/B field  M"), (Tool::Force, "Push / pull  F")]),
        ("Fission reactor", &[(Tool::Rod, "Control rod  5"), (Tool::FuelChannel, "Fuel channel  6"), (Tool::Source, "n source  7")]),
        ("Measure", &[(Tool::Inspect, "Inspect  I"), (Tool::Detector, "Detector  K")]),
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
        Tool::Wall => "Draw walls in the shape below.",
        Tool::EraseWall => "Hold to erase walls.",
        Tool::Rod => "Drag down to lay a control-rod channel. Rods move together from the Reactor tab.",
        Tool::FuelChannel => "Click to load the nuclide below and register a fuel channel the fuelling machine keeps loaded.",
        Tool::Source => "Click to add an Am-Be startup neutron source (as many as you like).",
        Tool::Fill => "Drag a rectangle to fill it with the nuclide below: moderator, fuel salt, propellant.",
        Tool::Spawner => "Click (or drag to aim) to add a spawner for the nuclide below: a fuel line, propellant feed or neutron gun - or, in beam mode, an accelerator beam at a set energy (fusion targets, colliding beams, spallation).",
        Tool::Inspect => "Click a nucleus: its properties, the reaction or decay that made it and what it becomes (Measure tab).",
        Tool::Detector => "Drag a box (counts what enters it or is made in it) or a line (counts what crosses it). Readings and spectra on the Measure tab.",
        Tool::Field => "Drag a rectangle: a region of electric and/or magnetic field that steers charged particles (beams, plasma traps). Edit it on the Measure tab.",
        Tool::Drain => "Drag a rectangle: matter entering it is removed and measured (exhaust nozzle, drain tank, off-gas).",
        Tool::Pump => "Drag a rectangle along the flow: it pushes matter (not neutrons) that way. Loops make a molten-salt circuit.",
        Tool::Force => "Hold the left button to pull matter towards the mouse, Shift to push it away. The right button does the same with any tool.",
    });
    match g.tool {
        Tool::Erase | Tool::EraseWall => {
            ui.slider("Eraser size", &mut g.brush_radius, 5.0, 150.0, Scale::Lin, 20.0, |v| format!("{v:.0} px"));
        }
        Tool::Wall => {
            ui.note("Shape");
            let names: Vec<&str> = crate::draw::SHAPES.iter().map(|s| s.1).collect();
            let mut k = crate::draw::SHAPES.iter().position(|s| s.0 == g.wall_shape).unwrap_or(0);
            if ui.segmented(&names, &mut k, 3) {
                g.wall_shape = crate::draw::SHAPES[k].0;
            }
            ui.note(match g.wall_shape {
                crate::draw::Shape::Freehand => "Drag to draw; Shift-drag for a straight wall.",
                crate::draw::Shape::Line => "Drag from one end to the other.",
                crate::draw::Shape::Rect => "Drag from corner to corner (Ctrl: a square).",
                crate::draw::Shape::Circle => "Drag from the centre outwards: a regular polygon with the sides below.",
                crate::draw::Shape::Polygon => "Click each corner. Click the first corner to close the shape; double-click, Enter or Esc to stop with it open.",
            });
            if g.wall_shape == crate::draw::Shape::Circle {
                ui.stepper("Sides", &mut g.circle_sides, 3, 96);
            }
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
        Tool::Spawner => crate::tabs::new_spawner_options(ui, sim, g),
        Tool::Detector => {
            let mut k = g.det_line as usize;
            if ui.segmented(&["Box", "Line"], &mut k, 2) {
                g.det_line = k == 1;
            }
            ui.segmented(&sim_core::detector::DetectFilter::NAMES, &mut g.det_filter, 4);
            if g.det_filter >= 5 {
                ui.note(&format!("Uses the loader below: {}", if g.det_filter == 5 { nucdata::SYMBOLS.get(g.z as usize).copied().unwrap_or("n").to_string() } else { nuclide_name(g.z, g.n) }));
            }
            ui.checkbox("Count photons and neutrinos too", &mut g.det_light);
        }
        Tool::Field => {
            ui.slider("Magnetic: proton turn rate", &mut g.field_b, -10.0, 10.0, Scale::Lin, 2.0, |v| {
                if v == 0.0 { "off".into() } else { format!("{:.2} rad/s {}", v.abs(), if v > 0.0 { "↺" } else { "↻" }) }
            });
            ui.slider("Electric: push on a proton", &mut g.field_e, 0.0, 3000.0, Scale::Lin, 0.0, |v| if v == 0.0 { "off".into() } else { format!("{v:.0} px/s²") });
            if g.field_e > 0.0 {
                let mut k = g.field_dir;
                if ui.segmented(&["→", "↓", "←", "↑"], &mut k, 4) {
                    g.field_dir = k;
                }
            }
            ui.note("B bends charges into circles (positive: protons turn anticlockwise); E pushes positive charges along it, negative ones against it. Neutrons ignore both.");
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

    if matches!(
        g.tool,
        Tool::Wall | Tool::Nucleus | Tool::Electron | Tool::Positron | Tool::Spawner | Tool::Fill | Tool::Drain | Tool::Pump | Tool::Rod | Tool::Detector | Tool::Field
    ) {
        ui.heading("Snapping");
        ui.checkbox("Snap points to a grid  [G]", &mut g.snap.grid_on);
        if g.snap.grid_on {
            ui.slider("Grid spacing", &mut g.snap.grid, 5.0, 100.0, Scale::Lin, 20.0, |v| format!("{v:.0} px"));
        }
        ui.slider("Angle step (hold Ctrl)", &mut g.snap.angle_step, 1.0, 90.0, Scale::Lin, 15.0, |v| format!("{v:.0}°"));
        ui.note("Hold Ctrl while dragging: walls, launches, spawner aim and detector lines snap to the angle step; rectangles become squares.");
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
    use sim_core::walls::{FilterRule, describe_filter};
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
            ui.slider("Energy threshold", &mut w.threshold, 1e-9, 200.0, Scale::Log, 1.0, |v| sim_core::nuclear::format_energy(v as f64));
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

fn reactor_tab(ui: &mut Gui, sim: &mut Sim, ps: &mut PanelState, act: &mut Actions, loader: (u32, u32)) {
    let re = &sim.reactor;
    let last = |f: &dyn Fn(&Sample) -> f32| -> f32 {
        let k = re.samples.len().min(8);
        if k == 0 { 0.0 } else { re.samples.iter().rev().take(k).map(f).sum::<f32>() / k as f32 }
    };
    let lastw = |f: &dyn Fn(&Sample) -> f64| -> f64 {
        let k = re.samples.len().min(8);
        if k == 0 { 0.0 } else { re.samples.iter().rev().take(k).map(f).sum::<f64>() / k as f64 }
    };
    let fission = last(&|s| s.fission_rate);
    let fusion = last(&|s| s.fusion_rate);
    let breakup = last(&|s| s.breakup_rate);
    let capture = last(&|s| s.capture_rate);
    let decay = last(&|s| s.decay_rate);
    let (released, absorbed, beam) = (lastw(&|s| s.released_w), lastw(&|s| s.absorbed_w), lastw(&|s| s.beam_w));
    let decay_w = lastw(&|s| s.decay_w);
    let net = released - absorbed;
    let neutrons = re.samples.back().map(|s| s.neutrons).unwrap_or(0);
    let recorded = re.recording.len();
    let fission_plant = !re.rods.is_empty() || !re.sites.is_empty();
    let period = re.period();

    // ---- Headline: what kind of nuclear activity, how much power ----
    let (state, sc) = if fission_plant || fission > 0.05 {
        match period {
            _ if (neutrons == 0 && fission == 0.0) || (re.insertion >= 0.99 && !re.rods.is_empty()) => ("FISSION: SHUT DOWN", col::DIM),
            Some(p) if p.is_finite() && p > 0.0 && p < 15.0 => ("FISSION: SUPERCRITICAL", col::BAD),
            Some(p) if p.is_finite() && p < 0.0 && p > -15.0 => ("FISSION: SUBCRITICAL", col::ACCENT),
            Some(_) => ("FISSION: CRITICAL", col::GOOD),
            None => ("FISSION: SOURCE RANGE", col::DIM),
        }
    } else if fusion + breakup > 0.05 {
        if net > 0.0 { ("FUSION: NET ENERGY OUT", col::GOOD) } else { ("COLLISIONS: NET ENERGY IN", col::WARN) }
    } else if capture + decay > 0.05 {
        ("CAPTURES & DECAYS", col::DIM)
    } else {
        ("QUIET", col::DIM)
    };
    let r = ui.alloc(78.0);
    draw_rectangle(r.x, r.y, r.w, r.h - 6.0, col::TRACK);
    draw_rectangle_lines(r.x, r.y, r.w, r.h - 6.0, 1.0, col::EDGE);
    ui.text(state, r.x + 10.0, r.y + 6.0, 17.0, sc);
    let ptxt = signed_watts(net);
    let pw = ui.text_width(&ptxt, 17.0);
    ui.text(&ptxt, r.x + r.w - pw - 10.0, r.y + 6.0, 17.0, if net >= 0.0 { col::TEXT } else { col::WARN });
    let sub = format!("released {}   absorbed {}", short_watts(released), short_watts(absorbed));
    ui.text(&sub, r.x + 10.0, r.y + 30.0, 14.0, col::DIM);
    let third = if beam > 0.0 {
        format!("beams in {}   gain {}", short_watts(beam), if net > 0.0 { format!("{:.2}", net / beam) } else { "0 (net loss)".into() })
    } else if fission_plant || fission > 0.05 {
        format!(
            "{neutrons} free neutrons   period {}",
            match period {
                Some(p) if p.is_finite() => format!("{p:+.0} s"),
                Some(_) => "∞".into(),
                None => "—".into(),
            }
        )
    } else {
        format!("{neutrons} free neutrons")
    };
    let third = if decay_w > 0.0 { format!("{third}   decay heat {}", short_watts(decay_w)) } else { third };
    let (third, _) = ui.fit(&third, r.w - 20.0, 14.0);
    ui.text(&third, r.x + 10.0, r.y + 49.0, 14.0, col::DIM);
    ui.tip_if_hover(r, vec![
        "Reaction power: the Q values of the reactions in the last 2 s, per second.".into(),
        "Released = exothermic (Q > 0: fission, most fusion, captures).".into(),
        "Absorbed = endothermic (Q < 0): energy the collision itself paid.".into(),
        "Beams in = kinetic power put in by spawners. Gain = net ÷ beams in (1 = break-even).".into(),
        "Decay heat (radioactive decays, neutrinos excluded) is shown on its own: it keeps going after a SCRAM.".into(),
        "Watts are per second of sim time.".into(),
    ]);

    // ---- Rates ----
    let chips: [(&str, String, Color); 6] = [
        ("fission", rate_text(fission), kind_color(EventKind::Fission)),
        ("fusion", rate_text(fusion), kind_color(EventKind::Fusion)),
        ("breakup", rate_text(breakup), kind_color(EventKind::Breakup)),
        ("capture", rate_text(capture), kind_color(EventKind::Capture)),
        ("decay", rate_text(decay), kind_color(EventKind::Decay)),
        ("free n", neutrons.to_string(), NEUTRON_COL),
    ];
    let cw = (ui.w - 2.0 * 4.0) / 3.0;
    let row = ui.alloc(2.0 * 38.0 + 4.0);
    for (k, (name, val, c)) in chips.iter().enumerate() {
        let (cx, cy) = (row.x + (k % 3) as f32 * (cw + 4.0), row.y + (k / 3) as f32 * 40.0);
        draw_rectangle(cx, cy, cw, 36.0, col::WIDGET);
        draw_rectangle(cx, cy, 3.0, 36.0, *c);
        ui.text(name, cx + 9.0, cy + 3.0, 12.0, col::DIM);
        ui.text(val, cx + 9.0, cy + 16.0, 16.0, col::TEXT);
    }
    ui.space(4.0);
    ui.note("Per second of sim time, last 2 s. Fusion: nuclei merge into something heavier than either. Breakup: nothing heavier comes out (knock-out, charge exchange, spallation).");

    plot(ui, sim, ps);
    if ui.button(&format!("Export graphs (CSV, {:.0} s recorded)", recorded as f32 * SAMPLE_DT)) {
        act.export_graphs = true;
    }

    busiest(ui, sim);
    crate::tabs::inventory_section(ui, sim);

    if !fission_plant && sim.reactor.is_empty() && fission + fusion + breakup + capture + decay == 0.0 {
        ui.heading("Get something going");
        ui.note("Launch nuclei at each other (drag with the Nucleus tool, or point two Spawners at each other), or load a reactor.");
        ui.note("Fission:");
        match ui.buttons(&["Heavy-water reactor", "Molten-salt reactor", "Rocket"]) {
            Some(0) => act.preset = Some(3),
            Some(1) => act.preset = Some(scenes::MSR),
            Some(2) => act.preset = Some(scenes::ROCKET),
            _ => {}
        }
        ui.note("Fusion and beams:");
        match ui.buttons(&["D-T plasma", "Magnetic bottle", "Neutron generator", "Collider"]) {
            Some(0) => act.preset = Some(1),
            Some(1) => act.preset = Some(scenes::BOTTLE),
            Some(2) => act.preset = Some(scenes::GENERATOR),
            Some(3) => act.preset = Some(scenes::COLLIDER),
            _ => {}
        }
        return;
    }

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
    if fission_plant || !re.sources.is_empty() {
        ui.heading("Neutron sources");
        if re.sources.is_empty() {
            ui.note("None: place Am-Be sources with the n source tool (Build tab). A shut-down core has no neutrons of its own.");
        } else {
            ui.slider(&format!("Strength of each of {} sources", re.sources.len()), &mut re.source_rate, 0.0, 5.0, Scale::Lin, 0.5, |v| format!("{v:.2} n/s"));
        }
    }
    crate::tabs::spawners_section(ui, sim, ps, loader);
    flow_tab(ui, sim);
    ui.heading("How to run it");
    let re = &sim.reactor;
    if !fission_plant && re.spawners.iter().any(|s| s.beam) {
        ui.note("Beams: pick the energy per nucleon to pick the physics. Up to a few MeV/u, light nuclei tunnel and fuse (D + T, p + B-11) and the gain readout says whether you get out more than the beam put in. From ~15 MeV/u collisions start to shatter nuclei, and above ~80 MeV/u they all do: watch the Log fill with fragmentation. Detectors (Measure tab) count what comes out; the Auto tab can throttle the beams to hold a rate.");
    } else if !fission_plant && fusion > 0.0 {
        ui.note("Plasma: fusion comes from the fast tail of the thermal motion tunnelling through the Coulomb barrier. Hotter plasma fuses more; D-T lights at the lowest temperature, then D-D, D-He-3, and p-B-11 needs the most. A magnetic field (Field tool, M) keeps the ions in while the fusion neutrons fly out.");
    } else if re.spawners.iter().any(|s| s.keep > 0) {
        ui.note("Molten salt: the fuel is in the salt, and its concentration (the fuel feed above) sets the power more than the rods do - the salt moderates poorly, so most fissions happen at keV energies where boron is nearly transparent. The salt's heat leaves through the conductor wall into the helium stream. The freeze plug in the floor is the safety: run it flat out (rods out, lots of fuel) for a couple of minutes and the salt gets hot enough to melt it; the salt drains into the tank below and the chain reaction stops. Its melt point is in the Physics tab.");
    } else if re.drains.iter().any(|d| d.filter == sim_core::reactor::DrainFilter::All) && re.spawners.iter().any(|s| s.z + s.n <= 2) {
        ui.note("Rocket: withdraw the rods to heat the core. Fission heat warms the moderator; the moderator heats the conductor plate; the hydrogen pumped under the plate picks the heat up and leaves through the nozzle. The exhaust reading under Drains is your exhaust speed and thrust - more power, hotter plate, faster exhaust.");
    } else if fission_plant {
        ui.note("Withdraw the rods until the period turns positive and power climbs, then insert them a little to hold it level (or let the controller on the Auto tab do it). Delayed neutrons from fission products (Br-87, I-137) are what make this possible: watch the period, not the instantaneous count. Xe-135 builds up after a run at power and soaks up neutrons. After a SCRAM the decay heat keeps coming.");
    } else {
        ui.note("Launch nuclei at each other, fill a box with hot plasma, or set up a beam (Spawner tool in beam mode). The Measure tab has detectors and an inspector; the Auto tab can run things for you.");
    }
}

/// Spawners, drains, pumps and heat.
fn flow_tab(ui: &mut Gui, sim: &mut Sim) {
    use sim_core::thermal::{format_kelvin, kelvin_of, region_theta};
    use sim_core::walls::WallKind;
    let cfg = sim.cfg.clone();
    let re = &mut sim.reactor;
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
const NEUTRON_COL: Color = Color { r: 0.75, g: 0.78, b: 0.86, a: 1.0 };
const POWER_COL: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

const DECAY_COL: Color = Color { r: 1.0, g: 0.55, b: 0.75, a: 1.0 };

/// Graph series: name, colour, value of a sample.
const SERIES: [(&str, fn() -> Color, fn(&Sample) -> f32); 8] = [
    ("fission", || kind_color(EventKind::Fission), |s| s.fission_rate),
    ("fusion", || kind_color(EventKind::Fusion), |s| s.fusion_rate),
    ("breakup", || kind_color(EventKind::Breakup), |s| s.breakup_rate),
    ("capture", || kind_color(EventKind::Capture), |s| s.capture_rate),
    ("decay", || kind_color(EventKind::Decay), |s| s.decay_rate),
    ("neutrons", || NEUTRON_COL, |s| s.neutrons as f32),
    ("power", || POWER_COL, |s| s.net_w() as f32),
    ("decay heat", || DECAY_COL, |s| s.decay_w as f32),
];

/// The last two minutes: each shown series on its own scale (its maximum
/// is written on its toggle). Rods, if any, as a band from the top.
fn plot(ui: &mut Gui, sim: &Sim, ps: &mut PanelState) {
    // Toggles.
    let cols = 4;
    let gap = 3.0;
    let bw = (ui.w - gap * (cols as f32 - 1.0)) / cols as f32;
    let s = &sim.reactor.samples;
    let n_show = ((120.0 / SAMPLE_DT) as usize).min(s.len());
    let pts: Vec<&Sample> = s.iter().skip(s.len() - n_show).collect();
    let maxes: Vec<f32> = SERIES.iter().map(|(_, _, f)| pts.iter().map(|p| f(p).abs()).fold(0.0, f32::max)).collect();
    let rows = SERIES.len().div_ceil(cols);
    let area = ui.alloc(rows as f32 * 24.0);
    for (k, (name, c, _)) in SERIES.iter().enumerate() {
        let b = Rect::new(area.x + (k % cols) as f32 * (bw + gap), area.y + (k / cols) as f32 * 24.0, bw, 21.0);
        let (clicked, hov, _) = ui.press(&format!("series{k}"), b);
        if clicked {
            ps.series[k] = !ps.series[k];
        }
        let on = ps.series[k];
        draw_rectangle(b.x, b.y, b.w, b.h, if hov { col::HOVER } else { col::WIDGET });
        let c = c();
        draw_rectangle(b.x + 5.0, b.y + 7.0, 8.0, 8.0, if on { c } else { Color::new(c.r, c.g, c.b, 0.25) });
        let m = if *name == "power" || *name == "decay heat" { short_watts(maxes[k] as f64) } else if maxes[k] >= 10.0 { format!("{:.0}", maxes[k]) } else { format!("{:.1}", maxes[k]) };
        let label = format!("{name} {m}");
        let (t, _) = ui.fit(&label, b.w - 20.0, 12.0);
        ui.text(&t, b.x + 17.0, b.y + 3.0, 12.0, if on { col::TEXT } else { col::FAINT });
    }
    let r = ui.alloc(118.0);
    let r = Rect::new(r.x, r.y + 2.0, r.w, r.h - 18.0);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.05, 0.055, 0.075, 1.0));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, col::EDGE);
    if pts.len() < 2 {
        return;
    }
    let slots = (n_show.max(40) - 1) as f32;
    let xs = |k: usize| r.x + r.w * k as f32 / slots;
    if !sim.reactor.rods.is_empty() {
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
    }
    // Net power can go negative: its zero line sits mid-height when it does.
    for (k, (name, c, f)) in SERIES.iter().enumerate() {
        if !ps.series[k] || maxes[k] <= 0.0 {
            continue;
        }
        let signed = *name == "power" && pts.iter().any(|p| f(p) < 0.0);
        let top = maxes[k] * 1.15;
        let y_of = |v: f32| if signed { r.y + r.h * 0.5 - r.h * 0.5 * v / top } else { r.y + r.h - r.h * v / top };
        if signed {
            draw_line(r.x, r.y + r.h * 0.5, r.x + r.w, r.y + r.h * 0.5, 1.0, Color::new(1.0, 1.0, 1.0, 0.15));
        }
        let mut smooth = 0.0;
        let mut prev: Option<Vec2> = None;
        for (i, p) in pts.iter().enumerate() {
            let v = f(p);
            smooth = if i == 0 { v } else { smooth * 0.7 + v * 0.3 };
            let q = vec2(xs(i), y_of(smooth));
            if let Some(a) = prev {
                draw_line(a.x, a.y, q.x, q.y, 1.5, c());
            }
            prev = Some(q);
        }
    }
    let secs = n_show as f32 * SAMPLE_DT;
    let note = if sim.reactor.rods.is_empty() { format!("last {secs:.0} s   each line on its own scale") } else { format!("last {secs:.0} s   own scales   grey band = rod insertion") };
    ui.text(&note, r.x, r.y + r.h + 2.0, 12.0, col::FAINT);
}

/// The most frequent reactions of the last 10 s, and a warning when the
/// collisions are beyond what the reaction model is good for.
fn busiest(ui: &mut Gui, sim: &Sim) {
    let st = &sim.nuclear;
    let since = st.time - 10.0;
    let mut by: Vec<(String, EventKind, f64, u32)> = Vec::new();
    let mut per_nucleon: Vec<f64> = Vec::new();
    for ev in st.history.iter().rev().take_while(|e| e.t >= since) {
        if !matches!(ev.kind, EventKind::Fission | EventKind::Fusion | EventKind::Breakup | EventKind::Capture) {
            continue;
        }
        let eq = ev.equation().to_string();
        if let (Some(e), Some((lhs, _))) = (ev.energy(), eq.split_once(" → ")) {
            // Beam-on-target energy per nucleon: E_cm / reduced mass.
            let m: Vec<u32> = lhs.split(" + ").map(mass_number).collect();
            if m.len() == 2 && m[0] > 0 && m[1] > 0 {
                let mu = (m[0] * m[1]) as f64 / (m[0] + m[1]) as f64;
                per_nucleon.push(e / mu);
            }
        }
        match by.iter_mut().find(|x| x.0 == eq) {
            Some(x) => x.3 += 1,
            None => by.push((eq, ev.kind, ev.q().unwrap_or(0.0), 1)),
        }
    }
    if by.is_empty() {
        return;
    }
    by.sort_by(|a, b| b.3.cmp(&a.3));
    ui.heading("Busiest reactions  (last 10 s)");
    for (eq, kind, q, n) in by.iter().take(7) {
        let r = ui.alloc(20.0);
        let c = kind_color(*kind);
        ui.text(&format!("{n}×"), r.x, r.y + 2.0, 13.0, col::DIM);
        let qt = format!("Q {q:+.1} MeV");
        let qw = ui.text_width(&qt, 12.0);
        let (t, _) = ui.fit(eq, r.w - 40.0 - qw - 8.0, 13.0);
        ui.text(&t, r.x + 34.0, r.y + 2.0, 13.0, c);
        ui.text(&qt, r.x + r.w - qw, r.y + 3.0, 12.0, if *q >= 0.0 { col::GOOD } else { col::WARN });
    }
    if !per_nucleon.is_empty() {
        per_nucleon.sort_by(|a, b| a.total_cmp(b));
        let med = per_nucleon[per_nucleon.len() / 2];
        ui.note(&format!("Median collision energy: {}/u (as a beam on a target at rest).", format_energy(med)));
        let (what, c) = crate::tabs::regime(med);
        ui.label_sized(&format!("Regime: {what}"), ui.size - 2.0, c);
        if med >= sim_core::nuclear::FRAG_START {
            ui.note("Fast collisions shatter nuclei here (fragmentation): where they overlap, nucleons and light clusters fly out; the rest of each nucleus carries on, hot, and boils off neutrons. Above ~300 MeV/u real collisions make pions and need relativity - beyond this model.");
        }
    }
}

/// "He-3" -> 3, "n"/"p" -> 1, "2n" -> 2.
fn mass_number(name: &str) -> u32 {
    if let Some((_, a)) = name.trim().rsplit_once('-') {
        return a.parse().unwrap_or(0);
    }
    let name = name.trim();
    let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
    let k: u32 = digits.parse().unwrap_or(1);
    if matches!(&name[digits.len()..], "n" | "p") { k } else { 0 }
}

fn rate_text(r: f32) -> String {
    if r == 0.0 {
        "0".into()
    } else if r < 10.0 {
        format!("{r:.1}/s")
    } else {
        format!("{r:.0}/s")
    }
}

/// "3.2 pW" (absolute value).
fn short_watts(w: f64) -> String {
    format_watts(w.abs()).trim_end_matches(" thermal").to_string()
}

fn signed_watts(w: f64) -> String {
    if w == 0.0 { "0 W".into() } else { format!("{}{}", if w < 0.0 { "−" } else { "+" }, short_watts(w)) }
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
    ui.heading("Overlay  [O]");
    use sim_core::overlay::OverlayKind;
    let names: Vec<&str> = OverlayKind::ALL.iter().map(|k| k.name()).collect();
    let mut k = OverlayKind::ALL.iter().position(|&o| o == g.view.overlay).unwrap_or(0);
    if ui.segmented(&names, &mut k, 2) {
        g.view.overlay = OverlayKind::ALL[k];
    }
    ui.note(match g.view.overlay {
        OverlayKind::None => "Colour the scene by a field, averaged over the last second or two of sim time.",
        OverlayKind::Temperature => "Temperature of the matter: random motion of the nuclei relative to the local flow (pumped salt isn't counted as hot). Logarithmic scale from the ambient temperature.",
        OverlayKind::Flux => "Where neutrons travel: track length per area per time, all energies. Square-root scale, relative to the brightest spot.",
        OverlayKind::ThermalFlux => "The same for slow neutrons only (below 1 eV): where the moderator has done its job, and what the fuel and the rods see.",
    });
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
    let d = sim_core::nuclear::game_config();
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
        let r = |z, n, e| sim_core::nuclear::interaction_radius(c, z, n, e);
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
    ui.slider("Freeze plug melts at", &mut c.melt_temp, 300.0, 1e6, Scale::Log, d.melt_temp, |v| sim_core::thermal::format_kelvin(v as f64));
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

/// Seed, fixed time step: runs that repeat exactly.
fn repeatable_section(ui: &mut Gui, sim: &mut Sim, g: &mut Game, ps: &mut PanelState, act: &mut Actions) {
    ui.heading("Repeatable runs");
    if !ui.typing() {
        ps.seed_text = sim.seed.to_string();
    }
    let r = ui.text_field("Seed", &mut ps.seed_text, "number");
    if r.submitted || r.changed {
        if let Ok(v) = ps.seed_text.trim().parse::<u64>() {
            sim.seed = v;
        }
    }
    if ui.button("New seed") {
        act.new_seed = true;
    }
    ui.checkbox("Fixed time step (1/60 s per frame)", &mut g.fixed_step);
    ui.checkbox("Keep this seed when loading a scenario", &mut g.keep_seed);
    ui.note("Every random event (decays, reactions, scattering) is drawn from the seed and the step number. The same scene and seed, run with a fixed time step and without touching it, plays out exactly the same: saved scenes keep their seed, so loading one again replays it. Without the fixed step, the frame rate decides the step sizes.");
}

/// Colour scale of the overlay, bottom right of the screen.
pub fn draw_legend(ui: &Gui, sim: &Sim, overlay: &sim_core::overlay::Overlay) {
    let Some((title, lo, hi)) = overlay.legend(sim) else { return };
    let (w, h) = (230.0, 50.0);
    let (x, y) = (screen_width() - w - 12.0, screen_height() - h - 12.0);
    draw_rectangle(x, y, w, h, Color::new(0.06, 0.065, 0.09, 0.85));
    draw_rectangle_lines(x, y, w, h, 1.0, col::EDGE);
    ui.text(&title, x + 8.0, y + 5.0, 14.0, col::TEXT);
    let (bx, by, bw) = (x + 8.0, y + 24.0, w - 16.0);
    let n = 40;
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        draw_rectangle(bx + bw * i as f32 / n as f32, by, bw / n as f32 + 0.5, 8.0, sim_core::render::heat(t));
    }
    ui.text(&lo, bx, by + 10.0, 12.0, col::DIM);
    let tw = ui.text_width(&hi, 12.0);
    ui.text(&hi, bx + bw - tw, by + 10.0, 12.0, col::DIM);
}
