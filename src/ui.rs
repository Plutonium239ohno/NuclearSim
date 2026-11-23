//! Control panel: dark skin, collapsible sections, species + rule editors.

use macroquad::prelude::*;
use macroquad::ui::{Ui, hash, root_ui, widgets};
use particle_sim::nuclear::{nuclide_name, sim_half_life};
use particle_sim::sim::Sim;
use particle_sim::types::{hsv, ids};
use particle_sim::render::{View, ViewMode};

/// What the left mouse button does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Paint,
    Wall,
    Erase,
}

pub struct UiState {
    pub panel_open: bool,
    pub paused: bool,
    /// Brush particle type id.
    pub brush_kind: u8,
    pub brush_rate: f32,
    pub brush_radius: f32,
    pub mouse_strength: f32,
    pub mouse_radius: f32,
    pub substeps: f32,
    pub iterations: f32,
    pub spawn_count: f32,
    /// Selected custom species in the species editor (type id).
    pub species_sel: u8,
    /// Row of the rule matrix being edited (type id).
    pub rule_row: u8,
    pub tool: Tool,
    pub wall_thickness: f32,
    pub view: View,
}

impl UiState {
    pub fn new(sim: &Sim) -> Self {
        Self {
            panel_open: true,
            paused: false,
            brush_kind: ids::PROTON,
            brush_rate: 10.0,
            brush_radius: 30.0,
            mouse_strength: 3000.0,
            mouse_radius: 150.0,
            substeps: sim.cfg.substeps as f32,
            iterations: sim.cfg.iterations as f32,
            spawn_count: 1000.0,
            species_sel: ids::FIRST_CUSTOM,
            rule_row: ids::FIRST_CUSTOM,
            tool: Tool::Paint,
            wall_thickness: 6.0,
            view: View::default(),
        }
    }
}

/// Things the panel asks the main loop to do.
#[derive(Default)]
pub struct Actions {
    pub spawn: bool,
    pub clear: bool,
    pub nuclear_soup: bool,
    pub dam_break: bool,
    pub oil_water: bool,
    pub clear_walls: bool,
}

pub struct Perf {
    pub step_ms: f32,
    pub far_ms: f32,
    pub rules_ms: f32,
}

// ---------------------------------------------------------------- panel

/// Collapsible section; `open` = initially unfolded.
fn section(u: &mut Ui, id: u64, title: &str, open: bool, f: impl FnOnce(&mut Ui)) {
    let node = widgets::TreeNode::new(id, title);
    let node = if open { node.init_unfolded() } else { node };
    node.ui(u, f);
}

/// Combo box over a subset of type ids; keeps `sel` as a type id.
fn type_combo(u: &mut Ui, id: u64, label: &str, sim: &Sim, ids_list: &[u8], sel: &mut u8) {
    let names: Vec<&str> = ids_list.iter().map(|&k| sim.types[k as usize].name.as_str()).collect();
    let mut idx = ids_list.iter().position(|&k| k == *sel).unwrap_or(0);
    u.combo_box(id, label, &names, &mut idx);
    if let Some(&k) = ids_list.get(idx) {
        *sel = k;
    }
}

pub const PANEL_W: f32 = 430.0;

pub fn draw_panel(sim: &mut Sim, st: &mut UiState, perf: &Perf) -> Actions {
    let mut act = Actions::default();

    if !st.panel_open {
        widgets::Window::new(hash!("mini"), vec2(10.0, 10.0), vec2(170.0, 34.0))
            .titlebar(false)
            .movable(false)
            .ui(&mut root_ui(), |u| {
                if u.button(None, "Show controls [Tab]") {
                    st.panel_open = true;
                }
            });
        return act;
    }

    let h = (screen_height() - 20.0).clamp(200.0, 1000.0);
    let paintable: Vec<u8> = (0..sim.types.len() as u8).filter(|&k| !sim.types[k as usize].ghost).collect();
    let customs: Vec<u8> = (0..sim.types.len() as u8).filter(|&k| sim.types[k as usize].custom).collect();

    widgets::Window::new(hash!("panel"), vec2(10.0, 10.0), vec2(PANEL_W, h))
        .label("Controls")
        .ui(&mut root_ui(), |u| {
            if u.button(None, "Hide panel [Tab]") {
                st.panel_open = false;
            }
            u.label(None, &format!("FPS {}   particles {}", get_fps(), sim.len()));
            u.label(None, &format!(
                "physics {:.2} ms  (long-range {:.2}, short-range {:.2})",
                perf.step_ms, perf.far_ms, perf.rules_ms
            ));

            section(u, hash!(), "View", true, |u| {
                let modes: Vec<&str> = ViewMode::ALL.iter().map(|m| m.name()).collect();
                let mut k = ViewMode::ALL.iter().position(|&m| m == st.view.mode).unwrap_or(0);
                u.combo_box(hash!(), "view [V]", &modes, &mut k);
                st.view.mode = ViewMode::ALL[k];
                match st.view.mode {
                    ViewMode::Speed => {
                        u.slider(hash!(), "hot speed", 50.0..1500.0, &mut st.view.speed_scale);
                    }
                    ViewMode::Element => {
                        u.checkbox(hash!(), "nuclide labels", &mut st.view.labels);
                        if !sim.cfg.nuclear {
                            u.label(None, "(nuclei only form in nuclear mode)");
                        }
                    }
                    ViewMode::Normal => {}
                }
                u.checkbox(hash!(), "Trails [L]", &mut st.view.trails);
                if st.view.trails {
                    u.slider(hash!(), "trail fade", 0.02..0.5, &mut st.view.trail_fade);
                }
                u.checkbox(hash!(), "Smooth liquid surfaces", &mut st.view.smooth_fluid);
            });

            section(u, hash!(), "Simulation", true, |u| {
                u.checkbox(hash!(), "Paused [Space]", &mut st.paused);
                u.slider(hash!(), "substeps", 1.0..16.0, &mut st.substeps);
                u.slider(hash!(), "solver iters", 1.0..10.0, &mut st.iterations);
                u.slider(hash!(), "drag", 0.0..5.0, &mut sim.cfg.drag);
                u.checkbox(hash!(), "Collisions [K]", &mut sim.cfg.collisions);
                u.slider(hash!(), "restitution", 0.0..1.0, &mut sim.cfg.restitution);
            });

            section(u, hash!(), "Forces", true, |u| {
                let c = &mut sim.cfg;
                u.checkbox(hash!(), "Uniform gravity [G]", &mut c.uniform_gravity);
                u.slider(hash!(), "g", 0.0..2000.0, &mut c.g);
                u.checkbox(hash!(), "Mutual gravity [M]", &mut c.mutual_gravity);
                u.slider(hash!(), "G", 0.0..2000.0, &mut c.big_g);
                u.checkbox(hash!(), "Coulomb [Q]", &mut c.coulomb);
                u.slider(hash!(), "k", 0.0..200000.0, &mut c.k);
                u.slider(hash!(), "softening", 1.0..40.0, &mut c.softening);
                u.checkbox(hash!(), "Barnes-Hut (off: exact)", &mut c.barnes_hut);
                u.slider(hash!(), "theta", 0.2..1.5, &mut c.theta);
            });

            section(u, hash!(), "Nuclear mode", true, |u| {
                let c = &mut sim.cfg;
                u.checkbox(hash!(), "Nuclear mode [N]", &mut c.nuclear);
                u.slider(hash!(), "strong force", 0.0..20000.0, &mut c.strong);
                u.slider(hash!(), "strong range", 0.1..1.0, &mut c.strong_range);
                u.slider(hash!(), "roundness", 0.0..200.0, &mut c.roundness);
                u.slider(hash!(), "tunneling x", 1.0..1000.0, &mut c.tunneling);
                u.slider(hash!(), "half-life x", 0.1..20.0, &mut c.decay_scale);
                u.slider(hash!(), "time exponent", 0.1..1.0, &mut c.time_exponent);
                u.slider(hash!(), "light speed", 200.0..1500.0, &mut c.light_speed);
                u.label(None, &format!(
                    "free n: t1/2 {:.1} s here (real 609 s)",
                    sim_half_life(609.8, c)
                ));
                u.label(None, "half-lives compressed: t = x * t_real^exponent");
                u.checkbox(hash!(), "log unbound breakups", &mut sim.nuclear.log_breakups);
                if u.button(None, "Load nuclear soup (p, n, e-)") {
                    act.nuclear_soup = true;
                }
                if c.nuclear {
                    let ns = &sim.nuclear;
                    let mut counts: Vec<String> = ns.counts.iter().map(|(m, n)| format!("{} {n}", m.name())).collect();
                    counts.sort();
                    if ns.captures + ns.fusions > 0 {
                        counts.push(format!("n-capture {}", ns.captures));
                        counts.push(format!("fusion {}", ns.fusions));
                    }
                    if ns.annihilations > 0 {
                        counts.push(format!("annihilation {}", ns.annihilations));
                    }
                    for chunk in counts.chunks(2) {
                        u.label(None, &chunk.join("   "));
                    }
                    let census: Vec<String> = ns
                        .census
                        .iter()
                        .filter(|((z, n), _)| z + n > 1)
                        .take(8)
                        .map(|((z, n), k)| format!("{} x{k}", nuclide_name(*z, *n)))
                        .collect();
                    if !census.is_empty() {
                        u.label(None, "nuclei:");
                        for chunk in census.chunks(4) {
                            u.label(None, &format!("  {}", chunk.join("  ")));
                        }
                    }
                    for line in ns.log.iter().take(6) {
                        u.label(None, line);
                    }
                }
            });

            section(u, hash!(), "Liquids & walls", true, |u| {
                if u.button(None, "Dam break") {
                    act.dam_break = true;
                }
                u.same_line(0.0);
                if u.button(None, "Oil & water") {
                    act.oil_water = true;
                }
                u.same_line(0.0);
                if u.button(None, "Clear walls") {
                    act.clear_walls = true;
                }
                u.slider(hash!(), "wall thickness", 2.0..20.0, &mut st.wall_thickness);
                u.label(None, &format!("{} wall segments", sim.walls.segs.len()));
                u.label(None, "Liquids: paint Water / Oil, or make any species");
                u.label(None, "a liquid in the Species editor. Needs gravity [G].");
            });

            section(u, hash!(), "Species", false, |u| {
                if customs.is_empty() {
                    u.label(None, "no custom species");
                } else {
                    if !customs.contains(&st.species_sel) {
                        st.species_sel = customs[0];
                    }
                    type_combo(u, hash!(), "edit", sim, &customs, &mut st.species_sel);
                    let t = &mut sim.types[st.species_sel as usize];
                    u.input_text(hash!("name", st.species_sel), "name", &mut t.name);
                    u.slider(hash!("rad", st.species_sel), "radius", 1.0..10.0, &mut t.radius);
                    u.slider(hash!("mass", st.species_sel), "mass", 0.05..10.0, &mut t.mass);
                    u.slider(hash!("chg", st.species_sel), "charge", -3.0..3.0, &mut t.charge);
                    u.slider(hash!("hue", st.species_sel), "hue", 0.0..360.0, &mut t.hue);
                    t.color = hsv(t.hue, 0.65, 0.95);
                    u.checkbox(hash!("rs", st.species_sel), "in random spawn", &mut t.in_random_spawn);
                    u.checkbox(hash!("col", st.species_sel), "collides", &mut t.collides);
                    u.checkbox(hash!("fl", st.species_sel), "liquid", &mut t.fluid);
                    if t.fluid {
                        u.slider(hash!("visc", st.species_sel), "viscosity", 0.0..1.0, &mut t.viscosity);
                        u.slider(hash!("coh", st.species_sel), "surface tension", 0.0..2.0, &mut t.cohesion);
                    }
                }
                if u.button(None, "Add species") {
                    if let Some(k) = sim.add_species() {
                        st.species_sel = k;
                    }
                }
                u.same_line(0.0);
                if u.button(None, "Remove last species") {
                    sim.remove_last_species();
                }
                u.label(None, "built-ins:");
                for k in [ids::NEUTRON, ids::PROTON, ids::ELECTRON, ids::POSITRON] {
                    let t = &sim.types[k as usize];
                    u.label(None, &format!("  {:<12} r {:.1}  m {:.2}  q {:+.0}", t.name, t.radius, t.mass, t.charge));
                }
            });

            section(u, hash!(), "Rule matrix", false, |u| {
                u.checkbox(hash!(), "Type rules [T]", &mut sim.cfg.rules);
                u.slider(hash!(), "range", 10.0..200.0, &mut sim.cfg.rule_range);
                u.slider(hash!(), "strength", 0.0..3000.0, &mut sim.cfg.rule_strength);
                if !paintable.contains(&st.rule_row) {
                    st.rule_row = paintable[0];
                }
                type_combo(u, hash!(), "species", sim, &paintable, &mut st.rule_row);
                let row = st.rule_row as usize;
                u.label(None, &format!("how {} reacts to... (+ pull, - push)", sim.types[row].name));
                for &j in &paintable {
                    let name = sim.types[j as usize].name.clone();
                    let v = sim.rules.get_mut(row, j as usize);
                    u.slider(hash!("rule", row * 64 + j as usize), &name, -1.0..1.0, v);
                }
                if u.button(None, "Randomize custom [R]") {
                    let types = &sim.types;
                    sim.rules.randomize_where(|k| types[k].custom && !types[k].fluid);
                }
                u.same_line(0.0);
                if u.button(None, "Randomize all") {
                    let types = &sim.types;
                    sim.rules.randomize_where(|k| !types[k].ghost && !types[k].fluid);
                }
                if u.button(None, "Zero all") {
                    sim.rules = particle_sim::types::RuleMatrix::zero(sim.types.len());
                }
                u.same_line(0.0);
                if u.button(None, "Make symmetric") {
                    let n = sim.rules.n;
                    for a in 0..n {
                        for b in a + 1..n {
                            let m = 0.5 * (sim.rules.get(a, b) + sim.rules.get(b, a));
                            sim.rules.set(a, b, m);
                            sim.rules.set(b, a, m);
                        }
                    }
                }
            });

            section(u, hash!(), "Brush & spawning", true, |u| {
                let tools = ["Paint particles [B]", "Draw walls [W]", "Erase walls [E]"];
                let all = [Tool::Paint, Tool::Wall, Tool::Erase];
                let mut k = all.iter().position(|&x| x == st.tool).unwrap_or(0);
                u.combo_box(hash!(), "tool", &tools, &mut k);
                st.tool = all[k];
                type_combo(u, hash!(), "brush [1-9]", sim, &paintable, &mut st.brush_kind);
                u.slider(hash!(), "brush rate", 1.0..50.0, &mut st.brush_rate);
                u.slider(hash!(), "brush size", 5.0..150.0, &mut st.brush_radius);
                u.slider(hash!(), "mouse force", 0.0..20000.0, &mut st.mouse_strength);
                u.slider(hash!(), "mouse radius", 20.0..500.0, &mut st.mouse_radius);
                u.slider(hash!(), "spawn count", 100.0..20000.0, &mut st.spawn_count);
                if u.button(None, "Spawn random") {
                    act.spawn = true;
                }
                u.same_line(0.0);
                if u.button(None, "Clear [C]") {
                    act.clear = true;
                }
                u.label(None, match st.tool {
                    Tool::Paint => "LMB paint   RMB attract   Shift+RMB repel",
                    Tool::Wall => "LMB draw   Shift+drag straight line",
                    Tool::Erase => "LMB erase walls (brush size)",
                });
            });
        });

    act
}
