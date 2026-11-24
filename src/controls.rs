//! The Controls tab: everything you can adjust while the scene runs, in one
//! place - beams and spawners, fields, control rods, fuel, neutron sources,
//! pumps - and the automation (controller and rules) that can adjust them
//! for you.

use crate::gui::{Gui, Scale, col};
use crate::panel::{Actions, PanelState};
use crate::tabs::{mix_editor, regime};
use macroquad::prelude::*;
use sim_core::control::{Action, Actuator, Measure, Rule, Trigger};
use sim_core::nucdata::{nuclide_name, parse_nuclide};
use sim_core::nuclear::{EventKind, energy_of, format_energy};
use sim_core::reactor::max_beam_energy;
use sim_core::sim::Sim;

/// A collapsible section whose open state lives in the panel state.
pub fn section(ui: &mut Gui, ps: &mut PanelState, key: &'static str, title: &str, badge: &str, default: bool) -> bool {
    let open = ps.open.entry(key).or_insert(default);
    ui.section(title, badge, open)
}

pub fn controls_tab(ui: &mut Gui, sim: &mut Sim, ps: &mut PanelState, act: &mut Actions) {
    let re = &sim.reactor;
    let nothing = re.spawners.is_empty() && re.fields.is_empty() && re.rods.is_empty() && re.sites.is_empty() && re.sources.is_empty() && re.pumps.is_empty();
    if nothing {
        ui.note("Nothing to adjust yet. Whatever you build that can be turned up or down while it runs shows up here: spawners and beams (tool 8), E/B fields (M), control rods (5), fuel channels (6), neutron sources (7) and pumps. Automation is below.");
    }

    let n = sim.reactor.spawners.len();
    if n > 0 {
        let firing = sim.reactor.throttle > 0.0;
        let badge = format!("{n}   {}", if firing { format!("{:.0}%", sim.reactor.throttle * 100.0) } else { "stopped".into() });
        if section(ui, ps, "spawners", "Beams & spawners", &badge, true) {
            spawners(ui, sim, ps);
        }
    }
    if !sim.reactor.fields.is_empty() {
        let badge = sim.reactor.fields.len().to_string();
        if section(ui, ps, "fields", "Fields", &badge, true) {
            fields(ui, sim);
        }
    }
    if !sim.reactor.rods.is_empty() {
        let badge = format!("{:.0}% in", sim.reactor.insertion * 100.0);
        if section(ui, ps, "rods", "Control rods  [↑ ↓, X = SCRAM]", &badge, true) {
            rods(ui, sim);
        }
    }
    if !sim.reactor.sites.is_empty() {
        let re = &sim.reactor;
        let fresh = re.sites.iter().filter(|s| s.empty_for == 0.0).count();
        let badge = format!("{fresh}/{} loaded", re.sites.len());
        if section(ui, ps, "fuel", "Fuel", &badge, true) {
            let re = &mut sim.reactor;
            ui.checkbox("On-power refuelling (CANDU fuelling machine)", &mut re.auto_refuel);
            ui.slider("Reload delay", &mut re.refuel_delay, 0.0, 30.0, Scale::Lin, 2.0, |v| format!("{v:.1} s"));
            if ui.button("Reload all spent channels now") {
                act.refuel_all = true;
            }
        }
    }
    let plant = !sim.reactor.rods.is_empty() || !sim.reactor.sites.is_empty();
    if plant || !sim.reactor.sources.is_empty() {
        let badge = sim.reactor.sources.len().to_string();
        if section(ui, ps, "sources", "Neutron sources", &badge, true) {
            let re = &mut sim.reactor;
            if re.sources.is_empty() {
                ui.note("None: place Am-Be sources with the n source tool (7). A shut-down core has no neutrons of its own.");
            } else {
                ui.slider(&format!("Strength of each of {} sources", re.sources.len()), &mut re.source_rate, 0.0, 5.0, Scale::Lin, 0.5, |v| format!("{v:.2} n/s"));
            }
        }
    }
    if !sim.reactor.pumps.is_empty() {
        let badge = sim.reactor.pumps.len().to_string();
        if section(ui, ps, "pumps", "Pumps", &badge, true) {
            for (k, pm) in sim.reactor.pumps.iter_mut().enumerate() {
                ui.slider(&format!("Pump {} speed##pm{k}", k + 1), &mut pm.speed, 0.0, 200.0, Scale::Lin, 40.0, |v| format!("{v:.0} px/s"));
            }
        }
    }

    // Automation: a switch for all of it, then its parts.
    let on = sim.reactor.auto_on;
    let active = sim.reactor.controller.on as usize + sim.reactor.rules.iter().filter(|r| r.enabled).count();
    let badge = if !on { "off".to_string() } else if active == 0 { "nothing set".into() } else { format!("on   {active} active") };
    if section(ui, ps, "auto", "Automation", &badge, false) {
        ui.checkbox("Automation on (controller and rules)", &mut sim.reactor.auto_on);
        if !sim.reactor.auto_on {
            ui.note("Off: the controller and the rules below keep their settings but do nothing.");
        }
        automation(ui, sim, ps);
    }
    ui.space(6.0);
    ui.note("Results (power, rates, graphs) are on the Reactions tab; detectors and the inspector on Measure. Hover anything in the scene for details.");
}

// ---------------------------------------------------------------- spawners

fn spawners(ui: &mut Gui, sim: &mut Sim, ps: &mut PanelState) {
    let re = &mut sim.reactor;
    ui.slider("Throttle (all spawners)", &mut re.throttle, 0.0, 1.0, Scale::Lin, 1.0, |v| format!("{:.0}%", v * 100.0));
    match ui.buttons(&["Stop all", "Run all", "Remove all"]) {
        Some(0) => re.throttle = 0.0,
        Some(1) => re.throttle = 1.0,
        Some(2) => {
            re.spawners.clear();
            return;
        }
        _ => {}
    }
    let cfg = sim.cfg.clone();
    let meet = vec2((crate::panel::PANEL_W + sim.width) * 0.5, sim.height * 0.5);
    let n = sim.reactor.spawners.len();
    let mut remove = None;
    let mut mirror = None;
    for k in 0..n {
        let sp = &mut sim.reactor.spawners[k];
        let a = sp.a();
        let e = sp.energy_per_nucleon(&cfg);
        let (title, badge) = if sp.keep > 0 {
            (format!("S{}  {} feed", k + 1, nuclide_name(sp.z, sp.n)), format!("keeps {}", sp.keep))
        } else if sp.beam {
            (format!("S{}  {} beam", k + 1, sp.label()), format!("{}/u  {:.1}/s", format_energy(e), sp.rate))
        } else {
            (format!("S{}  {} feed", k + 1, sp.label()), format!("{:.0} px/s  {:.1}/s", sp.vel.length(), sp.rate))
        };
        let open = ps.spawner_open.entry(k).or_insert(n <= 3);
        if !ui.subsection(&format!("{title}##spw{k}"), &badge, open) {
            continue;
        }
        if sp.keep > 0 {
            let mut keep = sp.keep as f32;
            if ui.slider(&format!("Keep in the salt##keep{k}"), &mut keep, 1.0, 40.0, Scale::Lin, 24.0, |v| format!("{:.0}", v.round())) {
                sp.keep = keep.round() as u32;
            }
            ui.note(&format!("{} added so far.", sp.spawned));
        } else {
            let mut mode = sp.beam as usize;
            if ui.segmented(&[&format!("Feed##fm{k}"), &format!("Beam##bm{k}")], &mut mode, 2) {
                sp.beam = mode == 1;
            }
            // What it fires: its nuclides, and a box to add another.
            let mut list: Vec<(u32, u32, u8)> = if sp.is_mix() { sp.nuclides() } else { vec![(sp.z, sp.n, 1)] };
            let mut random = sp.mix_random;
            let typed = ui.text_field(&format!("Add##add{k}"), ps.add_text.entry(k).or_default(), "a nuclide: D, T, He-3, U-235...");
            let cand = parse_nuclide(ps.add_text.get(&k).map(|s| s.as_str()).unwrap_or(""));
            let mut changed = mix_editor(ui, &mut list, &mut random, cand, &format!("sp{k}"));
            if typed.submitted {
                if let Some((z, nn)) = cand {
                    if let Some(m) = list.iter_mut().find(|m| (m.0, m.1) == (z, nn)) {
                        m.2 = m.2.saturating_add(1).min(20);
                    } else if list.len() < sim_core::reactor::MIX_MAX {
                        list.push((z, nn, 1));
                    }
                    changed = true;
                }
            }
            if changed {
                if list.is_empty() {
                    list.push((sp.z, sp.n, 1));
                }
                // A single nuclide replaced by another: fire the new one.
                if list.len() == 1 && (list[0].0, list[0].1) != (sp.z, sp.n) && sp.beam {
                    let dir = sp.vel.normalize_or(Vec2::X);
                    sp.set_mix(&list);
                    sp.set_energy(&cfg, dir, e);
                } else {
                    sp.set_mix(&list);
                }
                sp.mix_random = random;
                ps.add_text.insert(k, String::new());
            }
            ui.slider(&format!("Rate##r{k}"), &mut sp.rate, 0.0, 30.0, Scale::Log, 2.0, |v| format!("{v:.1}/s"));
            let dir = sp.vel.normalize_or(Vec2::X);
            let mut ang = (-dir.y).atan2(dir.x).to_degrees();
            if sp.beam {
                let top = max_beam_energy(&cfg) as f32;
                let mut ev = (e as f32).clamp(1e-3, top);
                if ui.slider(&format!("Energy per nucleon##e{k}"), &mut ev, 1e-3, top, Scale::Log, 1.0, |v| format_energy(v as f64) + "/u") {
                    sp.set_energy(&cfg, dir, ev as f64);
                }
                if ui.slider(&format!("Direction##a{k}"), &mut ang, -180.0, 180.0, Scale::Lin, 0.0, |v| format!("{v:.0}°")) {
                    sp.set_energy(&cfg, Vec2::from_angle(-ang.to_radians()), e);
                }
                ui.slider(&format!("Width##w{k}"), &mut sp.width, 0.0, 300.0, Scale::Lin, 12.0, |v| format!("{v:.0} px"));
                ui.slider(&format!("Energy spread##s{k}"), &mut sp.energy_spread, 0.0, 0.3, Scale::Lin, 0.0, |v| format!("±{:.0}%", v * 100.0));
                let mut deg = sp.divergence.to_degrees();
                if ui.slider(&format!("Divergence##d{k}"), &mut deg, 0.0, 15.0, Scale::Lin, 0.0, |v| format!("±{v:.1}°")) {
                    sp.divergence = deg.to_radians();
                }
                ui.slider(&format!("Pulse every##p{k}"), &mut sp.pulse_period, 0.0, 10.0, Scale::Lin, 0.0, |v| if v <= 0.0 { "continuous".into() } else { format!("{v:.1} s") });
                if sp.pulse_period > 0.0 {
                    let top = sp.pulse_period.max(0.1);
                    ui.slider(&format!("Pulse length##pl{k}"), &mut sp.pulse_len, 0.05, top, Scale::Lin, 0.5, |v| format!("{v:.2} s"));
                }
                if e as f32 >= top * 0.999 {
                    ui.label_sized("At the sim's speed limit: higher energies can't be shown.", ui.size - 2.5, col::WARN);
                }
                // What collisions at this energy do.
                let partner = sim.reactor.spawners.iter().enumerate().find(|(j, o)| *j != k && o.vel.normalize_or_zero().dot(dir) < -0.9);
                let mut line = format!("{} total; {} in the centre of mass on the same nucleus at rest", format_energy(e * a as f64), format_energy(e * a as f64 * 0.5));
                let mut e_lab = e;
                if let Some((j, o)) = partner {
                    let eo = o.energy_per_nucleon(&cfg);
                    // Relative speed (non-relativistic): u = sqrt(E/m) per nucleon each way.
                    e_lab = (e.sqrt() + eo.sqrt()).powi(2);
                    let mu = (a * o.a()) as f64 / (a + o.a()) as f64;
                    line = format!("{line}; head-on with S{}: {} ({}/u as beam on target)", j + 1, format_energy(mu * e_lab), format_energy(e_lab));
                }
                ui.note(&line);
                let (what, c) = regime(e_lab);
                ui.label_sized(&format!("Regime: {what}"), ui.size - 2.0, c);
            } else {
                let sp = &mut sim.reactor.spawners[k];
                let mut speed = sp.vel.length();
                let top = cfg.max_speed;
                if ui.slider(&format!("Speed##v{k}"), &mut speed, 0.0, top, Scale::Lin, 0.0, |v| format!("{v:.0} px/s   {}", format_energy(energy_of(&cfg, v, a as f32)))) {
                    sp.vel = dir * speed;
                }
                if ui.slider(&format!("Direction##fa{k}"), &mut ang, -180.0, 180.0, Scale::Lin, 0.0, |v| format!("{v:.0}°")) {
                    sp.vel = Vec2::from_angle(-ang.to_radians()) * speed.max(1e-3);
                }
                ui.note("A feed: nuclei start at this velocity from a small disc. Beam mode sets an energy per nucleon, a width and pulses instead.");
            }
        }
        let labels = [format!("Opposing beam##m{k}"), format!("Fire 10 now##b{k}"), format!("Remove##x{k}")];
        let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        match ui.buttons(&refs) {
            Some(0) => mirror = Some(k),
            Some(1) => sim.reactor.spawners[k].burst += 10,
            Some(2) => remove = Some(k),
            _ => {}
        }
        ui.space(4.0);
    }
    if let Some(k) = mirror {
        let m = sim.reactor.spawners[k].mirrored(meet);
        sim.reactor.spawners.push(m);
    }
    if let Some(k) = remove {
        sim.reactor.spawners.remove(k);
        ps.spawner_open.clear();
        ps.add_text.clear();
    }
}

// ------------------------------------------------------------------ fields

fn fields(ui: &mut Gui, sim: &mut Sim) {
    let mut remove = None;
    for (k, f) in sim.reactor.fields.iter_mut().enumerate() {
        let what = match (f.e.length() > 0.0, f.b != 0.0) {
            (true, true) => "E + B",
            (true, false) => "electric",
            (false, true) => "magnetic",
            _ => "off",
        };
        ui.label_col(&format!("F{}  {what}   {:.0} × {:.0} px at ({:.0}, {:.0})", k + 1, f.rect.w, f.rect.h, f.rect.x, f.rect.y), col::ACCENT);
        ui.slider(&format!("Magnetic: proton turn rate##b{k}"), &mut f.b, -10.0, 10.0, Scale::Lin, 0.0, |v| {
            if v == 0.0 { "off".into() } else { format!("{:.2} rad/s {}", v.abs(), if v > 0.0 { "↺" } else { "↻" }) }
        });
        let mut e = f.e.length();
        let mut ang = if e > 0.0 { (-f.e.y).atan2(f.e.x).to_degrees() } else { 0.0 };
        let mut changed = ui.slider(&format!("Electric: push on a proton##e{k}"), &mut e, 0.0, 3000.0, Scale::Lin, 0.0, |v| if v == 0.0 { "off".into() } else { format!("{v:.0} px/s²") });
        if e > 0.0 {
            changed |= ui.slider(&format!("Electric direction##ea{k}"), &mut ang, -180.0, 180.0, Scale::Lin, 0.0, |v| format!("{v:.0}°"));
        }
        if changed {
            f.e = Vec2::from_angle(-ang.to_radians()) * e;
        }
        ui.note(&f.describe());
        if let Some(r) = f.proton_radius(200.0).filter(|_| f.b.abs() >= 0.05) {
            ui.note(&format!("A proton at 200 px/s circles at {r:.0} px; a deuteron or alpha at the same speed at {:.0} px.", r * 2.0));
        }
        if ui.button(&format!("Remove F{}##fx{k}", k + 1)) {
            remove = Some(k);
        }
        ui.space(4.0);
    }
    if let Some(k) = remove {
        sim.reactor.fields.remove(k);
    }
    ui.note("Directions: 0° = right, counter-clockwise on screen. Right-click a slider to reset it.");
}

// -------------------------------------------------------------------- rods

fn rods(ui: &mut Gui, sim: &mut Sim) {
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
    ui.note(&format!("actual {:.0}% in{}", re.insertion * 100.0, if re.scram { "   SCRAM: rods dropping" } else if moving { "   driving…" } else { "" }));
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

// -------------------------------------------------------------- automation

const TRIGGERS: [&str; 5] = ["At time", "Every", "Above", "Below", "First of"];
const ACTIONS: [&str; 6] = ["SCRAM", "Rods to", "Throttle to", "Burst", "Sources to", "Pause"];
const FIRST_KINDS: [EventKind; 5] = [EventKind::Fission, EventKind::Fusion, EventKind::Breakup, EventKind::Capture, EventKind::Decay];

fn measures(sim: &Sim) -> Vec<Measure> {
    let mut m: Vec<Measure> = Measure::ALL.iter().copied().filter(|m| !matches!(m, Measure::Detector(_))).collect();
    for k in 0..sim.reactor.detectors.len() {
        m.push(Measure::Detector(k));
    }
    m
}

fn measure_names(ms: &[Measure]) -> Vec<String> {
    ms.iter().map(|m| m.short().to_string() + &if let Measure::Detector(k) = m { format!(" {}", k + 1) } else { String::new() }).collect()
}

/// Slider for a level of reading `m` (log scale in its range).
fn level_slider(ui: &mut Gui, label: &str, m: Measure, v: &mut f64) -> bool {
    let (lo, hi) = m.range();
    let mut x = (*v).clamp(lo, hi) as f32;
    let changed = ui.slider(label, &mut x, lo as f32, hi as f32, Scale::Log, ((lo * hi).sqrt()) as f32, |y| m.format(y as f64));
    *v = x as f64;
    changed
}

fn automation(ui: &mut Gui, sim: &mut Sim, ps: &mut PanelState) {
    // ---- Controller ----
    ui.space(4.0);
    ui.label_col("Controller", col::TEXT);
    ui.note("Holds a reading at a set point, with the control rods (fission) or by throttling every spawner (beams and feeds: fusion, or a molten-salt feed).");
    let ms = measures(sim);
    let mut c = sim.reactor.controller;
    ui.checkbox("Controller on", &mut c.on);
    if c.on {
        let names = measure_names(&ms);
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let mut k = ms.iter().position(|m| *m == c.measure).unwrap_or(0);
        if ui.segmented(&refs, &mut k, 4) {
            c.measure = ms[k];
            let (lo, hi) = c.measure.range();
            c.setpoint = (lo * hi).sqrt();
        }
        level_slider(ui, &format!("Set point: {}", c.measure.name()), c.measure, &mut c.setpoint);
        let mut a = (c.actuator == Actuator::Feeds) as usize;
        if ui.segmented(&["Move the rods", "Throttle spawners"], &mut a, 2) {
            c.actuator = if a == 1 { Actuator::Feeds } else { Actuator::Rods };
        }
        ui.slider("Gain", &mut c.gain, 0.1, 5.0, Scale::Log, 1.0, |v| format!("×{v:.2}"));
        let now = c.measure.value(sim);
        ui.kv("Reading now", &now.map(|v| c.measure.format(v)).unwrap_or_else(|| "—".into()), col::TEXT);
        match c.actuator {
            Actuator::Rods if sim.reactor.rods.is_empty() => ui.label_sized("There are no control rods: lay some with the Control rod tool (5), or throttle the spawners.", ui.size - 2.0, col::WARN),
            Actuator::Feeds if sim.reactor.spawners.is_empty() => ui.label_sized("There are no spawners to throttle.", ui.size - 2.0, col::WARN),
            Actuator::Rods => ui.kv("Rods", &format!("{:.0}% in (target {:.0}%)", sim.reactor.insertion * 100.0, sim.reactor.target * 100.0), col::DIM),
            Actuator::Feeds => ui.kv("Throttle", &format!("{:.0}%", sim.reactor.throttle * 100.0), col::DIM),
        }
        if c.measure == Measure::Period {
            ui.note("Period: keeps the power from rising faster than one e-fold per set-point seconds (a startup rate limit).");
        }
    }
    sim.reactor.controller = c;

    // ---- Rules ----
    ui.space(8.0);
    ui.label_col("Rules", col::TEXT);
    ui.note("Do something when something happens: a trip that SCRAMs, timed rod moves, beam pulses, a pause at the first fusion.");
    let mut remove = None;
    for (k, r) in sim.reactor.rules.iter_mut().enumerate() {
        let row = ui.alloc(24.0);
        let (x, y, w) = (row.x, row.y, row.w);
        ui.layout(x, y, 26.0);
        ui.checkbox(&format!("##en{k}"), &mut r.enabled);
        ui.layout(x + 26.0, y, w - 26.0);
        let fired = if r.fired > 0 { format!("   ({}×)", r.fired) } else { String::new() };
        let (t, _) = ui.fit(&format!("{}{fired}", r.describe()), w - 26.0 - 80.0, 13.0);
        ui.text(&t, x + 26.0, y + 4.0, 13.0, if r.enabled { col::TEXT } else { col::FAINT });
        ui.layout(x + w - 76.0, y, 76.0);
        if ui.button(&format!("Remove##rx{k}")) {
            remove = Some(k);
        }
        ui.layout(x, y + 28.0, w);
    }
    if let Some(k) = remove {
        sim.reactor.rules.remove(k);
    }
    if !sim.reactor.rules.is_empty() && ui.button("Re-arm all (fire again)") {
        sim.reactor.rules.iter_mut().for_each(|r| r.rearm());
    }
    let quick: [(&str, Rule); 6] = [
        ("SCRAM if fission > 20/s", Rule::new(Trigger::Above(Measure::FissionRate, 20.0), Action::Scram)),
        ("SCRAM if period < 10 s", Rule::new(Trigger::Below(Measure::Period, 10.0), Action::Scram)),
        ("Stop spawners if fusion > 20/s", Rule::new(Trigger::Above(Measure::FusionRate, 20.0), Action::Throttle(0.0))),
        ("Pause at the first fusion", Rule::new(Trigger::First(EventKind::Fusion), Action::Pause)),
        ("Rods to 50% at t + 10 s", Rule::new(Trigger::At(sim.nuclear.time + 10.0), Action::Rods(0.5))),
        ("Burst of 5 every 2 s", Rule::new(Trigger::Every(2.0), Action::Burst(5))),
    ];
    ui.note("Add a ready-made rule:");
    let names: Vec<&str> = quick.iter().map(|q| q.0).collect();
    if let Some(k) = ui.button_grid(&names, 2, None) {
        sim.reactor.rules.push(quick[k].1);
    }

    // ---- Builder ----
    if !section(ui, ps, "rule_builder", "Make your own rule", "", false) {
        return;
    }
    let b = &mut ps.rule;
    ui.note("When…");
    ui.segmented(&TRIGGERS, &mut b.trigger, 5);
    let ms = measures(sim);
    match b.trigger {
        0 => {
            ui.slider("At sim time", &mut b.time, 0.0, 600.0, Scale::Lin, 10.0, |v| format!("{v:.1} s"));
            ui.note(&format!("now t = {:.1} s", sim.nuclear.time));
        }
        1 => {
            ui.slider("Every", &mut b.time, 0.1, 60.0, Scale::Log, 2.0, |v| format!("{v:.1} s"));
        }
        2 | 3 => {
            let names = measure_names(&ms);
            let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            b.measure = b.measure.min(ms.len() - 1);
            ui.segmented(&refs, &mut b.measure, 4);
            let m = ms[b.measure];
            level_slider(ui, &format!("Level: {}", m.name()), m, &mut b.level);
        }
        _ => {
            let names: Vec<&str> = FIRST_KINDS.iter().map(|k| k.name()).collect();
            ui.segmented(&names, &mut b.kind, 5);
        }
    }
    ui.note("…do:");
    ui.segmented(&ACTIONS, &mut b.action, 3);
    match b.action {
        1 => {
            ui.slider("Rods to", &mut b.value, 0.0, 1.0, Scale::Lin, 0.5, |v| format!("{:.0}% in", v * 100.0));
        }
        2 => {
            ui.slider("Throttle to", &mut b.value, 0.0, 1.0, Scale::Lin, 0.0, |v| format!("{:.0}%", v * 100.0));
        }
        3 => {
            ui.slider("Nuclei per spawner", &mut b.value, 1.0, 100.0, Scale::Log, 5.0, |v| format!("{:.0}", v.round()));
        }
        4 => {
            ui.slider("Source strength", &mut b.value, 0.0, 5.0, Scale::Lin, 0.5, |v| format!("{v:.2} n/s"));
        }
        _ => {}
    }
    let trigger = match b.trigger {
        0 => Trigger::At(b.time as f64),
        1 => Trigger::Every(b.time.max(0.05) as f64),
        2 => Trigger::Above(ms[b.measure.min(ms.len() - 1)], b.level),
        3 => Trigger::Below(ms[b.measure.min(ms.len() - 1)], b.level),
        _ => Trigger::First(FIRST_KINDS[b.kind.min(FIRST_KINDS.len() - 1)]),
    };
    let action = match b.action {
        0 => Action::Scram,
        1 => Action::Rods(b.value.clamp(0.0, 1.0)),
        2 => Action::Throttle(b.value.clamp(0.0, 1.0)),
        3 => Action::Burst(b.value.round().max(1.0) as u32),
        4 => Action::Sources(b.value.max(0.0)),
        _ => Action::Pause,
    };
    let rule = Rule::new(trigger, action);
    ui.label_col(&rule.describe(), col::ACCENT);
    if ui.button("Add this rule") {
        sim.reactor.rules.push(rule);
    }
}

/// The rule being put together in the Automation section.
pub struct RuleBuilder {
    pub trigger: usize,
    pub measure: usize,
    pub level: f64,
    pub time: f32,
    pub kind: usize,
    pub action: usize,
    pub value: f32,
}

impl Default for RuleBuilder {
    fn default() -> Self {
        Self { trigger: 2, measure: 0, level: 10.0, time: 10.0, kind: 1, action: 0, value: 0.5 }
    }
}
