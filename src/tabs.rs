//! The Measure and Isotopes tabs, the options for a new spawner (Build
//! tab), the nuclide-mix editor and the inventory on the Reactions tab.

use crate::Game;
use crate::gui::{Gui, Scale, col};
use crate::panel::{PanelState, describe, kind_color};
use macroquad::prelude::*;
use sim_core::control::watts;
use sim_core::detector::{self, DetShape, DetectFilter};
use sim_core::nucdata::{self, Mode, nuclide_name};
use sim_core::nuclear::{FRAG_FULL, FRAG_RELATIVISTIC, FRAG_START, energy_of, format_energy};
use sim_core::reactor::max_beam_energy;
use sim_core::sim::{Sim, unpack_zn};

// ------------------------------------------------------------------ beams

/// What collisions at `e_lab` MeV per nucleon do in this sim.
pub fn regime(e_lab: f64) -> (&'static str, Color) {
    if e_lab >= FRAG_RELATIVISTIC {
        ("relativistic, pions: beyond the model (treated as fragmentation)", col::BAD)
    } else if e_lab >= FRAG_FULL {
        ("fragmentation: nuclei shatter", col::WARN)
    } else if e_lab >= FRAG_START {
        ("mixed: fusion / transfer, increasingly fragmentation", col::WARN)
    } else if e_lab >= 0.01 {
        ("fusion and transfer reactions (compound nucleus)", col::GOOD)
    } else {
        ("slow: only tunnelling (or neutron capture)", col::DIM)
    }
}

/// Edit a spawner's list of nuclides: (Z, N, weight). Returns whether it
/// changed. An empty list means "just the one nuclide". `add` is offered
/// with an Add button.
pub fn mix_editor(ui: &mut Gui, list: &mut Vec<(u32, u32, u8)>, random: &mut bool, add: Option<(u32, u32)>, id: &str) -> bool {
    let mut changed = false;
    let mut remove = None;
    let mix = list.len() > 1;
    for (k, m) in list.iter_mut().enumerate() {
        let row = ui.alloc(30.0);
        let (x, y, w) = (row.x, row.y, row.w);
        ui.text(&nuclide_name(m.0, m.1), x, y + 6.0, ui.size - 1.0, col::TEXT);
        ui.layout(x + 90.0, y, w - 90.0 - 80.0);
        let mut wt = m.2 as u32;
        if mix && ui.stepper(&format!("weight##{id}w{k}"), &mut wt, 1, 20) {
            m.2 = wt as u8;
            changed = true;
        }
        ui.layout(x + w - 76.0, y, 76.0);
        if ui.button(&format!("Remove##{id}r{k}")) {
            remove = Some(k);
        }
        ui.layout(x, y + 34.0, w);
    }
    if let Some(k) = remove {
        list.remove(k);
        changed = true;
    }
    let full = list.len() >= sim_core::reactor::MIX_MAX;
    if let Some(nuc) = add.filter(|a| a.0 + a.1 > 0 && !full) {
        let name = nuclide_name(nuc.0, nuc.1);
        let label = if list.iter().any(|m| (m.0, m.1) == nuc) { format!("One more {name} in the mix##{id}add") } else { format!("Add {name}##{id}add") };
        if ui.button(&label) {
            match list.iter_mut().find(|m| (m.0, m.1) == nuc) {
                Some(m) => m.2 = m.2.saturating_add(1).min(20),
                None => list.push((nuc.0, nuc.1, 1)),
            }
            changed = true;
        }
    }
    if list.len() > 1 {
        let mut k = *random as usize;
        if ui.segmented(&[&format!("In turn##{id}t"), &format!("At random##{id}rn")], &mut k, 2) {
            *random = k == 1;
            changed = true;
        }
        let total: u32 = list.iter().map(|m| m.2 as u32).sum();
        let share: Vec<String> = list.iter().map(|m| format!("{} {:.0}%", nuclide_name(m.0, m.1), 100.0 * m.2 as f32 / total as f32)).collect();
        ui.note(&format!(
            "{}: {}. Beams give each the same energy per nucleon.",
            if *random { "Picked at random by weight" } else { "In turn, each weight times in a row" },
            share.join(", ")
        ));
    }
    changed
}

/// Settings for the next spawner (Build tab). What it fires is chosen in
/// the nuclide picker below these.
pub fn new_spawner_options(ui: &mut Gui, sim: &Sim, g: &mut Game) {
    let loader = (g.z, g.n);
    if g.mix.is_empty() {
        if ui.button(&format!("Fire a mix of nuclides instead (starting with {})", nuclide_name(g.z, g.n))) {
            g.mix.push((g.z, g.n, 1));
        }
    } else {
        ui.note("Fires this mix: choose another nuclide above, then add it.");
        mix_editor(ui, &mut g.mix, &mut g.mix_random, Some(loader), "new");
        if ui.button("No mix: fire just the nuclide above") {
            g.mix.clear();
        }
    }
    ui.slider("Rate", &mut g.spawn_rate, 0.1, 30.0, Scale::Log, 2.0, |v| format!("{v:.1}/s"));
    if g.beam {
        let top = max_beam_energy(&sim.cfg) as f32;
        g.beam_mev = g.beam_mev.min(top);
        ui.slider("Energy per nucleon", &mut g.beam_mev, 1e-3, top, Scale::Log, 1.0, |v| format_energy(v as f64) + "/u");
        ui.slider("Beam width", &mut g.beam_width, 0.0, 300.0, Scale::Lin, 12.0, |v| format!("{v:.0} px"));
        ui.slider("Energy spread", &mut g.beam_spread, 0.0, 0.3, Scale::Lin, 0.0, |v| format!("±{:.0}%", v * 100.0));
        ui.slider("Divergence", &mut g.beam_div, 0.0, 15.0, Scale::Lin, 0.0, |v| format!("±{v:.1}°"));
        ui.slider("Pulse every", &mut g.beam_pulse, 0.0, 10.0, Scale::Lin, 0.0, |v| if v <= 0.0 { "continuous".into() } else { format!("{v:.1} s") });
        if g.beam_pulse > 0.0 {
            ui.slider("Pulse length", &mut g.beam_pulse_len, 0.05, g.beam_pulse.max(0.1), Scale::Lin, 0.5, |v| format!("{v:.2} s"));
        }
        let a = (g.z + g.n).max(1) as f64;
        let e = g.beam_mev as f64;
        ui.note(&format!(
            "{} at {} total. On the same nucleus at rest: {} in the centre of mass; head-on with its mirror: {}.",
            nuclide_name(g.z, g.n),
            format_energy(e * a),
            format_energy(e * a * 0.5),
            format_energy(e * a * 2.0)
        ));
        let (what, c) = regime(e);
        ui.label_sized(&format!("Regime: {what}"), ui.size - 2.0, c);
        ui.note("Drag to aim (the drag sets the direction only; hold Ctrl to snap it).");
    }
}

// --------------------------------------------------------------- inventory

/// What the scene is made of, sorted for fission and fusion, with decay heat.
pub fn inventory_section(ui: &mut Gui, sim: &Sim) {
    let inv = sim_core::inventory::take(&sim.cfg, sim.nuclear.census.iter().map(|c| (c.0, c.1 as f64)));
    if inv.count == 0 {
        return;
    }
    ui.heading("What's in the scene");
    for (cat, row) in &inv.rows {
        let heat = if row.heat_w > 0.0 { format!("   {}", watts(row.heat_w)) } else { String::new() };
        ui.kv(cat.name(), &format!("{}{heat}", row.count), col::TEXT);
    }
    if inv.heat_w > 0.0 {
        ui.kv("Decay heat (from what's here)", &watts(inv.heat_w), col::WARN);
        let top: Vec<String> = inv.hottest.iter().take(5).map(|((z, n), c, w)| format!("{} ×{c} {}", nuclide_name(*z, *n), watts(*w))).collect();
        ui.note(&format!("Hottest: {}", top.join(",  ")));
    }
    let pool = &sim.reactor.pool;
    if !pool.items.is_empty() {
        let now = pool.now(&sim.cfg, sim.nuclear.time);
        let inv = sim_core::inventory::take(&sim.cfg, now.iter().copied());
        ui.heading("Spent fuel pool");
        ui.note(&format!("{} bundles and products taken out by the fuelling machine; each nuclide decays on (its daughters aren't followed).", pool.items.len()));
        for (cat, row) in &inv.rows {
            let heat = if row.heat_w > 0.0 { format!("   {}", watts(row.heat_w)) } else { String::new() };
            ui.kv(cat.name(), &format!("{}{heat}", row.count), col::TEXT);
        }
        ui.kv("Pool decay heat", &watts(inv.heat_w), col::WARN);
    }
    if inv.heat_w > 0.0 {
        ui.note("Decay heat on the sim's clock: what the radioactive nuclei give off per second of sim time (neutrinos' share excluded). It keeps coming after a SCRAM, from fission products in a reactor and from tritium and activation in a fusion machine.");
    }
}

// ------------------------------------------------------------------ measure

pub fn measure_tab(ui: &mut Gui, sim: &mut Sim, g: &mut Game) {
    inspector(ui, sim, g);
    detectors(ui, sim, g);
}

fn inspector(ui: &mut Gui, sim: &Sim, g: &mut Game) {
    ui.heading("Inspector  [I]");
    let Some(tag) = g.selected else {
        ui.note("Pick the Inspect tool (I) and click any nucleus or nucleon: its properties, where it came from and what it turns into.");
        return;
    };
    let bodies = sim.bodies();
    let st = &sim.nuclear;
    match bodies.get(&tag) {
        Some(b) => {
            let lines = describe(b.z, b.n, &sim.cfg);
            if let Some((first, rest)) = lines.split_first() {
                ui.label_col(first, col::ACCENT);
                for l in rest {
                    ui.note(l);
                }
            }
            let a = b.a().max(1) as f32;
            let e = energy_of(&sim.cfg, b.vel.length(), a);
            ui.kv("Kinetic energy", &format!("{}  ({}/u)", format_energy(e), format_energy(e / a as f64)), col::TEXT);
            ui.kv("Speed, position", &format!("{:.0} px/s   at ({:.0}, {:.0})", b.vel.length(), b.pos.x, b.pos.y), col::DIM);
            g.sel_last = Some((b.z, b.n));
        }
        None => {
            let was = g.sel_last.map(|(z, n)| nuclide_name(z, n)).unwrap_or_else(|| "It".into());
            ui.label_col(&format!("{was} is gone"), col::WARN);
            let kids = st.children(tag);
            if kids.is_empty() {
                ui.note("No products recorded: absorbed by a control rod, taken by a drain, erased, or left the scene.");
            } else {
                let names: Vec<String> = kids
                    .iter()
                    .filter_map(|(t, _)| bodies.get(t).map(|b| nuclide_name(b.z, b.n)))
                    .collect();
                if let Some(ev) = kids[0].1.event.and_then(|g| st.event(g)) {
                    ui.note(&format!("Became, at t = {:.2} s:", ev.t));
                    ui.label_sized(ev.equation(), ui.size - 1.0, kind_color(ev.kind));
                }
                if !names.is_empty() {
                    ui.note(&format!("Still here: {}", names.join(", ")));
                }
            }
        }
    }
    // Where it came from: up to 8 generations back, along the heavier parent.
    ui.space(4.0);
    ui.note("History (newest first):");
    let mut t = tag;
    let mut any = false;
    for _ in 0..8 {
        let Some(birth) = st.born.get(&t) else { break };
        any = true;
        match birth.event.and_then(|g| st.event(g)) {
            Some(ev) => {
                let r = ui.alloc(19.0);
                ui.text(&format!("{:.1} s", ev.t), r.x, r.y + 2.0, 12.0, col::FAINT);
                let (txt, _) = ui.fit(ev.equation(), r.w - 52.0, 13.0);
                ui.text(&txt, r.x + 50.0, r.y + 2.0, 13.0, kind_color(ev.kind));
                ui.tip_if_hover(r, vec![ev.equation().to_string(), ev.details().to_string()]);
            }
            None => ui.note(&format!("{:.1} s  an instant breakup of an unbound nucleus", birth.t)),
        }
        let heavier = birth.parents.iter().filter(|p| p.0 != u32::MAX).max_by_key(|p| {
            let (z, n) = unpack_zn(p.1);
            z + n
        });
        match heavier {
            Some(p) => t = p.0,
            None => break,
        }
    }
    if !any {
        ui.note("  placed in the scene (by you, a spawner, a fuel channel or the scenario).");
    } else if !st.born.contains_key(&t) {
        ui.note("  …which was placed in the scene.");
    }
    ui.space(4.0);
    ui.checkbox("Follow it with the zoom window", &mut g.follow);
    ui.checkbox("When it reacts or decays, follow its heaviest product", &mut g.follow_products);
    if ui.button("Stop inspecting") {
        g.selected = None;
        g.follow = false;
    }
}

fn detectors(ui: &mut Gui, sim: &mut Sim, g: &Game) {
    ui.heading("Detectors  [K]");
    if sim.reactor.detectors.is_empty() {
        ui.note("None yet. With the Detector tool (K), drag a box (counts what enters it or is made in it) or a line (counts what crosses it). Each can count everything, neutral or charged, positive or negative particles, one element or one isotope.");
        return;
    }
    let now = sim.nuclear.time;
    let mut remove = None;
    let n = sim.reactor.detectors.len();
    for k in 0..n {
        let d = &mut sim.reactor.detectors[k];
        let shape = match d.shape {
            DetShape::Box(_) => "box",
            DetShape::Line(..) => "line",
        };
        ui.label_col(&format!("D{}  ({shape}): {}", k + 1, d.filter.describe()), col::GOOD);
        ui.kv("Counted", &format!("{}   in {:.0} s   now {:.1}/s", d.count, (now - d.since).max(0.0), d.rate), col::TEXT);
        if let Some(e) = d.mean_energy() {
            ui.kv("Mean energy", &format_energy(e), col::TEXT);
        }
        let top = d.top_species();
        if !top.is_empty() {
            let t: Vec<String> = top.iter().take(8).map(|(s, c)| format!("{} {c}", s.name())).collect();
            ui.note(&t.join("   "));
        }
        histogram(ui, &d.hist);
        let mut fk = d.filter.index();
        if ui.segmented(&DetectFilter::NAMES, &mut fk, 4) {
            d.filter = DetectFilter::from_index(fk, g.z, g.n);
            d.reset(now);
        }
        if matches!(d.filter, DetectFilter::Element(_) | DetectFilter::Isotope(..)) {
            if ui.button(&format!("Use the loader's nuclide ({})##dl{k}", nuclide_name(g.z, g.n))) {
                d.filter = DetectFilter::from_index(fk, g.z, g.n);
                d.reset(now);
            }
        }
        ui.checkbox(&format!("Count photons and neutrinos too##lt{k}"), &mut d.light);
        let labels = [format!("Reset##dr{k}"), format!("Remove##dx{k}")];
        let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        match ui.buttons(&refs) {
            Some(0) => d.reset(now),
            Some(1) => remove = Some(k),
            _ => {}
        }
        ui.space(6.0);
    }
    if let Some(k) = remove {
        sim.reactor.detectors.remove(k);
    }
}

/// Energy spectrum, log energy axis, bar heights on a square-root scale.
fn histogram(ui: &mut Gui, h: &[u64; detector::BINS]) {
    let max = h.iter().copied().max().unwrap_or(0);
    let r = ui.alloc(74.0);
    let r = Rect::new(r.x, r.y, r.w, 56.0);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.05, 0.055, 0.075, 1.0));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, col::EDGE);
    let bw = r.w / detector::BINS as f32;
    if max > 0 {
        for (k, &c) in h.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let f = (c as f32 / max as f32).sqrt();
            let bh = (r.h - 4.0) * f;
            let x = r.x + k as f32 * bw;
            draw_rectangle(x + 0.5, r.y + r.h - bh, bw - 1.0, bh, Color::new(0.55, 1.0, 0.6, 0.8));
            let bar = Rect::new(x, r.y, bw, r.h);
            ui.tip_if_hover(bar, vec![format!("{} – {}", format_energy(detector::bin_edge(k)), format_energy(detector::bin_edge(k + 1))), format!("{c} counted")]);
        }
    }
    // Decade labels.
    for (label, mev) in [("meV", 1e-9), ("eV", 1e-6), ("keV", 1e-3), ("MeV", 1.0), ("GeV", 1e3)] {
        let x = r.x + detector::bin_of(mev * 1.0001) as f32 * bw;
        draw_line(x, r.y + r.h, x, r.y + r.h + 3.0, 1.0, col::FAINT);
        ui.text(label, x + 1.0, r.y + r.h + 2.0, 11.0, col::FAINT);
    }
}

// ----------------------------------------------------------------- isotopes

/// View of the chart of nuclides.
pub struct Chart {
    /// Cell size (px) and the (N, Z) at the bottom-left corner.
    pub cell: f32,
    pub n0: f32,
    pub z0: f32,
    pub fitted: bool,
}

impl Default for Chart {
    fn default() -> Self {
        Self { cell: 6.0, n0: 0.0, z0: 0.0, fitted: false }
    }
}

fn mode_color(z: u32, n: u32) -> Option<Color> {
    let nuc = nucdata::lookup(z, n)?;
    match nuc.half_life {
        None => Some(Color::from_rgba(215, 218, 230, 255)),
        Some(t) if t < 1e-12 => None,
        Some(_) => {
            let m = nuc.branches.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|b| b.0);
            Some(match m {
                Some(Mode::BetaMinus) | Some(Mode::DoubleBetaMinus) => Color::from_rgba(90, 150, 255, 255),
                Some(Mode::BetaPlus) | Some(Mode::ElectronCapture) => Color::from_rgba(255, 120, 95, 255),
                Some(Mode::Alpha) => Color::from_rgba(255, 215, 80, 255),
                Some(Mode::Fission) => Color::from_rgba(110, 214, 140, 255),
                Some(_) => Color::from_rgba(200, 130, 255, 255),
                None => Color::from_rgba(150, 150, 160, 255),
            })
        }
    }
}

pub fn isotopes_tab(ui: &mut Gui, sim: &Sim, g: &mut Game, ps: &mut PanelState) {
    let census: Vec<((u32, u32), u32)> = sim.nuclear.census.iter().filter(|c| c.0.0 + c.0.1 > 1).cloned().collect();
    let ch = &mut ps.chart;
    let r = ui.alloc(330.0);
    let r = Rect::new(r.x, r.y, r.w, 320.0);
    let fit = |ch: &mut Chart| {
        if census.is_empty() {
            *ch = Chart { cell: 2.2, n0: 0.0, z0: 0.0, fitted: true };
            return;
        }
        let (mut nlo, mut nhi, mut zlo, mut zhi) = (u32::MAX, 0, u32::MAX, 0);
        for ((z, n), _) in &census {
            nlo = nlo.min(*n);
            nhi = nhi.max(*n);
            zlo = zlo.min(*z);
            zhi = zhi.max(*z);
        }
        let (wn, wz) = ((nhi - nlo + 6) as f32, (zhi - zlo + 6) as f32);
        ch.cell = (r.w / wn).min(r.h / wz).clamp(2.0, 24.0);
        ch.n0 = (nlo as f32 + nhi as f32) * 0.5 - r.w / ch.cell * 0.5;
        ch.z0 = (zlo as f32 + zhi as f32) * 0.5 - r.h / ch.cell * 0.5;
        ch.fitted = true;
    };
    if !ch.fitted {
        fit(ch);
    }
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.05, 0.055, 0.075, 1.0));
    let cell = ch.cell;
    let to_screen = |n: f32, z: f32| vec2(r.x + (n - ch.n0) * cell, r.y + r.h - (z - ch.z0 + 1.0) * cell);
    // Magic numbers.
    for m in [2u32, 8, 20, 28, 50, 82, 126] {
        let p = to_screen(m as f32, 0.0);
        if p.x > r.x && p.x < r.right() {
            draw_line(p.x, r.y, p.x, r.bottom(), 1.0, Color::new(1.0, 1.0, 1.0, 0.07));
        }
        let q = to_screen(0.0, m as f32);
        if q.y > r.y && q.y < r.bottom() {
            draw_line(r.x, q.y + cell, r.right(), q.y + cell, 1.0, Color::new(1.0, 1.0, 1.0, 0.07));
        }
    }
    let visible = |p: Vec2| p.x + cell >= r.x && p.y + cell >= r.y && p.x <= r.right() && p.y <= r.bottom();
    let clip = |p: Vec2| -> Rect {
        let x0 = p.x.max(r.x);
        let y0 = p.y.max(r.y);
        Rect::new(x0, y0, (p.x + cell).min(r.right()) - x0, (p.y + cell).min(r.bottom()) - y0)
    };
    for nuc in nucdata::all() {
        let p = to_screen(nuc.n as f32, nuc.z as f32);
        if !visible(p) {
            continue;
        }
        if let Some(c) = mode_color(nuc.z, nuc.n) {
            let q = clip(p);
            let gap = if cell >= 4.0 { 0.5 } else { 0.0 };
            draw_rectangle(q.x, q.y, (q.w - gap).max(0.0), (q.h - gap).max(0.0), Color::new(c.r, c.g, c.b, 0.22));
        }
    }
    let most = census.iter().map(|c| c.1).max().unwrap_or(1) as f32;
    for ((z, n), count) in &census {
        let p = to_screen(*n as f32, *z as f32);
        if !visible(p) {
            continue;
        }
        let c = mode_color(*z, *n).unwrap_or(Color::from_rgba(150, 150, 160, 255));
        let f = 0.45 + 0.55 * ((*count as f32).ln_1p() / most.ln_1p()).min(1.0);
        // Small cells: a bigger marker so what's present stands out.
        let grow = ((5.0 - cell) * 0.5).max(0.0);
        let q = clip(p - Vec2::splat(grow));
        let q = Rect::new(q.x, q.y, (q.w + 2.0 * grow).min(r.right() - q.x), (q.h + 2.0 * grow).min(r.bottom() - q.y));
        draw_rectangle(q.x, q.y, q.w, q.h, Color::new(c.r * f + (1.0 - f) * 0.2, c.g * f + (1.0 - f) * 0.2, c.b * f + (1.0 - f) * 0.2, 1.0));
        draw_rectangle_lines(q.x - 0.5, q.y - 0.5, q.w + 1.0, q.h + 1.0, 1.0, Color::new(1.0, 1.0, 1.0, 0.9));
        if cell >= 18.0 {
            let label = nuclide_name(*z, *n);
            let (t, _) = ui.fit(&label, cell - 2.0, 9.0);
            ui.text(&t, p.x + 1.0, p.y + 1.0, 9.0, Color::new(0.05, 0.05, 0.08, 1.0));
        }
    }
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, col::EDGE);
    ui.text("N →", r.right() - 26.0, r.bottom() - 14.0, 12.0, col::FAINT);
    ui.text("Z ↑", r.x + 3.0, r.y + 2.0, 12.0, col::FAINT);
    // Hover, click (load it), drag (pan).
    let (clicked, hov, held) = ui.press("chart", r);
    if held {
        let d = ui.mouse_delta();
        ch.n0 -= d.x / cell;
        ch.z0 += d.y / cell;
    }
    if hov {
        let m: Vec2 = mouse_position().into();
        let mut n = (ch.n0 + (m.x - r.x) / cell).floor();
        let mut z = (ch.z0 + (r.bottom() - m.y) / cell).floor();
        // Zoomed out, what's present is drawn bigger than its cell: hovering
        // such a marker means that nuclide, not the neighbour under it.
        let grow = ((5.0 - cell) * 0.5).max(0.0);
        if grow > 0.0 {
            let hit = census.iter().filter(|((cz, cn), _)| {
                let p = vec2(r.x + (*cn as f32 - ch.n0) * cell, r.y + r.h - (*cz as f32 - ch.z0 + 1.0) * cell);
                Rect::new(p.x - grow, p.y - grow, cell + 2.0 * grow, cell + 2.0 * grow).contains(m)
            });
            if let Some(((cz, cn), _)) = hit.min_by(|a, b| {
                let d = |c: &((u32, u32), u32)| vec2(r.x + (c.0.1 as f32 - ch.n0 + 0.5) * cell, r.y + r.h - (c.0.0 as f32 - ch.z0 + 0.5) * cell).distance(m);
                d(a).total_cmp(&d(b))
            }) {
                n = *cn as f32;
                z = *cz as f32;
            }
        }
        if n >= 0.0 && z >= 0.0 {
            // Outline what the tooltip describes.
            let p = vec2(r.x + (n - ch.n0) * cell, r.y + r.h - (z - ch.z0 + 1.0) * cell);
            let pad = grow.max(1.0);
            draw_rectangle_lines(p.x - pad, p.y - pad, cell + 2.0 * pad, cell + 2.0 * pad, 1.5, col::ACCENT);
            let (z, n) = (z as u32, n as u32);
            let mut lines = describe(z, n, &sim.cfg);
            if lines.is_empty() {
                lines.push("(nothing)".into());
            }
            let count = census.iter().find(|c| c.0 == (z, n)).map_or(0, |c| c.1);
            lines.push(format!("{count} in the scene now. Click to choose it (for the Nucleus, Fill or Spawner tool)."));
            ui.tip(lines);
            if clicked && ui.mouse_delta().length() < 2.0 && z + n > 0 {
                g.choose_nuclide(z, n);
            }
        }
    }
    match ui.buttons(&["Zoom +", "Zoom −", "Fit", "All"]) {
        Some(k @ (0 | 1)) => {
            let f = if k == 0 { 1.4 } else { 1.0 / 1.4 };
            let (cn, cz) = (ch.n0 + r.w / ch.cell * 0.5, ch.z0 + r.h / ch.cell * 0.5);
            ch.cell = (ch.cell * f).clamp(1.5, 40.0);
            ch.n0 = cn - r.w / ch.cell * 0.5;
            ch.z0 = cz - r.h / ch.cell * 0.5;
        }
        Some(2) => fit(ch),
        Some(3) => *ch = Chart { cell: (r.w / 180.0).min(r.h / 120.0), n0: 0.0, z0: 0.0, fitted: true },
        _ => {}
    }
    let legend: [(&str, Color); 6] = [
        ("stable", Color::from_rgba(215, 218, 230, 255)),
        ("β⁻", Color::from_rgba(90, 150, 255, 255)),
        ("β⁺/EC", Color::from_rgba(255, 120, 95, 255)),
        ("α", Color::from_rgba(255, 215, 80, 255)),
        ("fission", Color::from_rgba(110, 214, 140, 255)),
        ("n/p emission", Color::from_rgba(200, 130, 255, 255)),
    ];
    let row = ui.alloc(18.0);
    let mut x = row.x;
    for (name, c) in legend {
        draw_rectangle(x, row.y + 4.0, 9.0, 9.0, c);
        ui.text(name, x + 12.0, row.y + 1.0, 12.0, col::DIM);
        x += 14.0 + ui.text_width(name, 12.0) + 8.0;
    }
    ui.note("Lit squares are in the scene now (white outline; brighter = more). Fission products sit right of the stable valley and walk up-left along it by β⁻ decay; fusion climbs it from the bottom-left corner. Drag to pan; click a square to load it. Fit: zoom to what's here; All: the whole chart.");
    if !census.is_empty() {
        ui.heading("Present now");
        let names: Vec<String> = census.iter().take(18).map(|((z, n), c)| format!("{} {c}", nuclide_name(*z, *n))).collect();
        ui.note(&names.join("   "));
    }
}
