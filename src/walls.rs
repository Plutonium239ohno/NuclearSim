//! Static walls drawn by the user: thick line segments (capsules).
//!
//! Segments are bucketed into a coarse grid (rebuilt only when walls change),
//! so a particle only tests the handful of segments in its own cell.

use macroquad::math::{Vec2, vec2};

/// What a wall is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WallKind {
    /// Thick reflector: perfect heat insulator, bounces neutrons back.
    #[default]
    Insulator,
    /// Thin metal (heat-exchanger tube wall): passes heat between what
    /// touches it on either side, and along connected conductor walls.
    /// Neutrons go straight through, as they do through a few mm of metal.
    Conductor,
    /// Frozen salt: a conductor that melts away when it gets too hot (the
    /// molten-salt reactor's passive drain valve). Neutrons pass.
    FreezePlug,
    /// Membrane that sorts what crosses it by element (see `Seg::filter_z`
    /// and `Seg::only`). Insulating; reflects whatever it blocks.
    Filter,
}

/// What is hitting a wall: its element (Z; 0 for a free neutron,
/// `NOT_NUCLEAR` for leptons and the like), mass number, charge, whether it
/// is a free neutron, and its real kinetic energy (MeV; whole nucleus).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub z: u32,
    pub a: u32,
    pub charged: bool,
    pub neutron: bool,
    pub mev: f32,
}

pub const NOT_NUCLEAR: u32 = u32::MAX;

impl Hit {
    pub const OTHER: Hit = Hit { z: NOT_NUCLEAR, a: 0, charged: false, neutron: false, mev: 0.0 };
    pub const NEUTRON: Hit = Hit { z: 0, a: 1, charged: false, neutron: true, mev: 0.0 };
}

/// What a filter wall sorts by. The filter's `only` flag says whether what
/// matches passes (and everything else bounces), or the reverse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FilterRule {
    /// Matches one element (Z; 0 = free neutrons), every isotope.
    #[default]
    Element,
    /// Matches anything carrying charge (nuclei, protons, electrons...).
    Charged,
    /// Matches anything with at least `threshold` MeV of kinetic energy.
    Energy,
    /// Matches nuclei with mass number A >= `filter_z`.
    Mass,
}

impl FilterRule {
    pub const ALL: [FilterRule; 4] = [FilterRule::Element, FilterRule::Charged, FilterRule::Energy, FilterRule::Mass];

    pub fn name(self) -> &'static str {
        match self {
            FilterRule::Element => "Element",
            FilterRule::Charged => "Charge",
            FilterRule::Energy => "Energy",
            FilterRule::Mass => "Mass (A)",
        }
    }

    pub fn from_code(c: u8) -> Self {
        Self::ALL.get(c as usize).copied().unwrap_or_default()
    }
}

/// Everything that can be set on a wall before it is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallSpec {
    pub kind: WallKind,
    /// Half the thickness (px).
    pub half: f32,
    /// Non-filter walls: free neutrons bounce off (reflector) or pass
    /// (window).
    pub reflect_neutrons: bool,
    pub rule: FilterRule,
    /// Element filters: Z (0 = neutrons). Mass filters: the A threshold.
    pub filter_z: u32,
    /// Energy filters: the threshold (MeV).
    pub threshold: f32,
    /// Filters: what matches the rule passes (true), or only what doesn't.
    pub only: bool,
}

impl WallSpec {
    pub fn new(kind: WallKind, half: f32) -> Self {
        Self { kind, half, reflect_neutrons: kind.reflects_neutrons(), rule: FilterRule::Element, filter_z: 0, threshold: 1.0, only: false }
    }
}

impl WallKind {
    pub const ALL: [WallKind; 4] = [WallKind::Insulator, WallKind::Conductor, WallKind::FreezePlug, WallKind::Filter];

    pub fn name(self) -> &'static str {
        match self {
            WallKind::Insulator => "Insulator",
            WallKind::Conductor => "Conductor",
            WallKind::FreezePlug => "Freeze plug",
            WallKind::Filter => "Filter",
        }
    }

    /// Default neutron behaviour: insulators reflect, metal and salt pass.
    pub fn reflects_neutrons(self) -> bool {
        self == WallKind::Insulator
    }

    pub fn conducts(self) -> bool {
        matches!(self, WallKind::Conductor | WallKind::FreezePlug)
    }

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Self {
        match c {
            1 => WallKind::Conductor,
            2 => WallKind::FreezePlug,
            3 => WallKind::Filter,
            _ => WallKind::Insulator,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub a: Vec2,
    pub b: Vec2,
    pub kind: WallKind,
    /// Heat content per unit of capacity: the wall's temperature in sim
    /// units (mean kinetic energy of what it is in equilibrium with). Only
    /// meaningful for conducting walls.
    pub theta: f32,
    /// Half the thickness (px).
    pub half: f32,
    /// Non-filter walls: whether free neutrons bounce off.
    pub reflect_neutrons: bool,
    /// Filter walls: see `WallSpec`.
    pub rule: FilterRule,
    pub filter_z: u32,
    pub threshold: f32,
    pub only: bool,
}

impl Seg {
    pub fn new(a: Vec2, b: Vec2, spec: &WallSpec, theta: f32) -> Self {
        Seg {
            a,
            b,
            kind: spec.kind,
            theta,
            half: spec.half.max(0.25),
            reflect_neutrons: spec.reflect_neutrons,
            rule: spec.rule,
            filter_z: spec.filter_z,
            threshold: spec.threshold,
            only: spec.only,
        }
    }

    pub fn spec(&self) -> WallSpec {
        WallSpec {
            kind: self.kind,
            half: self.half,
            reflect_neutrons: self.reflect_neutrons,
            rule: self.rule,
            filter_z: self.filter_z,
            threshold: self.threshold,
            only: self.only,
        }
    }

    /// Does `hit` match this filter's rule?
    #[inline(always)]
    pub fn matches(&self, hit: Hit) -> bool {
        match self.rule {
            FilterRule::Element => hit.z == self.filter_z,
            FilterRule::Charged => hit.charged,
            FilterRule::Energy => hit.mev >= self.threshold,
            FilterRule::Mass => hit.z != NOT_NUCLEAR && hit.a >= self.filter_z,
        }
    }

    /// Does this wall stop `hit`?
    #[inline(always)]
    pub fn blocks(&self, hit: Hit) -> bool {
        match self.kind {
            WallKind::Filter => self.matches(hit) != self.only,
            _ => !hit.neutron || self.reflect_neutrons,
        }
    }

    /// One line saying what a filter passes ("passes only charged particles").
    pub fn filter_text(&self) -> String {
        describe_filter(self.rule, self.filter_z, self.threshold, self.only)
    }
}

/// Plain-words description of a filter setting.
pub fn describe_filter(rule: FilterRule, z: u32, threshold: f32, only: bool) -> String {
    match rule {
        FilterRule::Element => {
            let el = if z == 0 { "free neutrons".to_string() } else { crate::nucdata::SYMBOLS.get(z as usize).copied().unwrap_or("?").to_string() };
            if only { format!("passes only {el}") } else { format!("passes everything but {el}") }
        }
        FilterRule::Charged => if only { "passes only charged particles".into() } else { "passes only neutral particles".into() },
        FilterRule::Energy => {
            let e = crate::nuclear::format_energy(threshold as f64);
            if only { format!("passes only particles with ≥ {e}") } else { format!("passes only particles below {e}") }
        }
        FilterRule::Mass => if only { format!("passes only nuclei with A ≥ {z}") } else { format!("passes everything but nuclei with A ≥ {z}") },
    }
}

pub struct Walls {
    pub segs: Vec<Seg>,
    /// Half the thickness (px) of walls added with `add`/`add_kind`. Each
    /// wall keeps its own (`Seg::half`).
    pub half: f32,
    /// Is there an energy filter (so hits need their energy)?
    pub needs_energy: bool,
    cell: f32,
    inv: f32,
    cols: usize,
    rows: usize,
    start: Vec<u32>,
    items: Vec<u32>,
    /// (width, height, margin) the buckets were built for; None = stale.
    built: Option<(f32, f32, f32)>,
    /// Bumped whenever the segments change (for caches built on them).
    pub version: u64,
    /// Temperature (sim units) new conducting walls start at.
    pub ambient_theta: f32,
}

impl Default for Walls {
    fn default() -> Self {
        Self {
            segs: Vec::new(),
            half: 3.0,
            needs_energy: false,
            cell: 32.0,
            inv: 1.0 / 32.0,
            cols: 0,
            rows: 0,
            start: Vec::new(),
            items: Vec::new(),
            built: None,
            version: 0,
            ambient_theta: 0.0,
        }
    }
}

#[inline(always)]
fn closest(s: &Seg, p: Vec2) -> Vec2 {
    let ab = s.b - s.a;
    let l2 = ab.length_squared();
    let t = if l2 > 1e-12 { ((p - s.a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    s.a + ab * t
}

#[inline(always)]
fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// Do segments p0-p1 and q0-q1 properly intersect?
#[inline(always)]
fn crosses(p0: Vec2, p1: Vec2, q0: Vec2, q1: Vec2) -> bool {
    let r = p1 - p0;
    let s = q1 - q0;
    let den = cross(r, s);
    if den.abs() < 1e-9 {
        return false;
    }
    let t = cross(q0 - p0, s) / den;
    let u = cross(q0 - p0, r) / den;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
}

impl Walls {
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    pub fn add(&mut self, a: Vec2, b: Vec2) {
        self.add_kind(a, b, WallKind::Insulator);
    }

    pub fn add_kind(&mut self, a: Vec2, b: Vec2, kind: WallKind) {
        self.add_spec(a, b, &WallSpec::new(kind, self.half));
    }

    pub fn add_spec(&mut self, a: Vec2, b: Vec2, spec: &WallSpec) {
        self.segs.push(Seg::new(a, b, spec, self.ambient_theta));
        self.touch();
    }

    /// A filter wall passing only element `z` (`only`) or all but it.
    pub fn add_filter(&mut self, a: Vec2, b: Vec2, z: u32, only: bool) {
        let spec = WallSpec { filter_z: z, only, ..WallSpec::new(WallKind::Filter, self.half) };
        self.add_spec(a, b, &spec);
    }

    /// Mark the segments as changed.
    pub fn touch(&mut self) {
        self.built = None;
        self.version += 1;
        self.needs_energy = self.segs.iter().any(|s| s.kind == WallKind::Filter && s.rule == FilterRule::Energy);
    }

    /// Thickest wall's half thickness.
    pub fn max_half(&self) -> f32 {
        self.segs.iter().fold(0.0, |m, s| m.max(s.half))
    }

    /// Does a wall lie across the straight line from p to q? (Heat shared
    /// between atoms doesn't go through walls.)
    pub fn separates(&self, p: Vec2, q: Vec2) -> bool {
        self.segs.iter().any(|s| crosses(p, q, s.a, s.b))
    }

    /// Distance from p to segment k and the unit normal pointing at p.
    pub fn distance_to(&self, k: usize, p: Vec2) -> (f32, Vec2) {
        let s = &self.segs[k];
        let d = p - closest(s, p);
        let l = d.length();
        let n = if l > 1e-6 {
            d / l
        } else {
            let ab = (s.b - s.a).normalize_or_zero();
            vec2(-ab.y, ab.x)
        };
        (l, n)
    }

    /// Indices of segments within `r` (plus the wall half-thickness) of p.
    pub fn near(&self, p: Vec2, r: f32) -> Vec<usize> {
        self.segs
            .iter()
            .enumerate()
            .filter(|(_, s)| closest(s, p).distance_squared(p) < (r + s.half) * (r + s.half))
            .map(|(k, _)| k)
            .collect()
    }

    /// Closest segment to p within r, if any.
    pub fn nearest(&self, p: Vec2, r: f32) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (k, s) in self.segs.iter().enumerate() {
            let d = closest(s, p).distance(p);
            if d < r + s.half && best.is_none_or(|b| d < b.1) {
                best = Some((k, d));
            }
        }
        best.map(|b| b.0)
    }

    pub fn clear(&mut self) {
        self.segs.clear();
        self.touch();
    }

    /// Remove every segment that comes within `r` of `p`.
    pub fn erase_near(&mut self, p: Vec2, r: f32) {
        let n = self.segs.len();
        self.segs.retain(|s| closest(s, p).distance_squared(p) > r * r);
        if self.segs.len() != n {
            self.touch();
        }
    }

    /// (Re)bucket the segments. `margin` = the largest particle radius plus the
    /// furthest a particle can move in one substep, so a particle always finds
    /// every wall it could touch or have crossed in its own cell.
    pub fn prepare(&mut self, width: f32, height: f32, margin: f32) {
        let key = (width, height, margin);
        if self.built == Some(key) {
            return;
        }
        self.built = Some(key);
        self.cols = ((width * self.inv).ceil() as usize).max(1);
        self.rows = ((height * self.inv).ceil() as usize).max(1);
        let ncell = self.cols * self.rows;
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); ncell];
        for (k, s) in self.segs.iter().enumerate() {
            let reach = margin + s.half;
            let lo = s.a.min(s.b) - Vec2::splat(reach);
            let hi = s.a.max(s.b) + Vec2::splat(reach);
            let (x0, y0) = self.coords(lo);
            let (x1, y1) = self.coords(hi);
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    // Only cells whose square actually comes near the segment.
                    let c = vec2((cx as f32 + 0.5) * self.cell, (cy as f32 + 0.5) * self.cell);
                    let lim = reach + self.cell * std::f32::consts::FRAC_1_SQRT_2;
                    if closest(s, c).distance_squared(c) <= lim * lim {
                        lists[cy * self.cols + cx].push(k as u32);
                    }
                }
            }
        }
        self.start.clear();
        self.items.clear();
        self.start.push(0);
        for l in lists {
            self.items.extend(l);
            self.start.push(self.items.len() as u32);
        }
    }

    #[inline(always)]
    fn coords(&self, p: Vec2) -> (usize, usize) {
        let cx = ((p.x * self.inv) as i32).clamp(0, self.cols as i32 - 1) as usize;
        let cy = ((p.y * self.inv) as i32).clamp(0, self.rows as i32 - 1) as usize;
        (cx, cy)
    }

    #[inline(always)]
    fn bucket(&self, p: Vec2) -> &[u32] {
        if self.built.is_none() || self.segs.is_empty() {
            return &[];
        }
        let (cx, cy) = self.coords(p);
        let c = cy * self.cols + cx;
        &self.items[self.start[c] as usize..self.start[c + 1] as usize]
    }

    /// Push a particle of radius `r` out of every wall. `prev` is where it was
    /// at the start of the substep: if it moved *through* a wall's centre line,
    /// it is put back on the side it came from (no tunnelling).
    /// `hit` says what the particle is: free neutrons pass all but insulator
    /// (reflector) walls, filters pass what they are set to.
    #[inline]
    pub fn resolve(&self, mut p: Vec2, prev: Vec2, r: f32, hit: Hit) -> Vec2 {
        for &k in self.bucket(p) {
            let s = &self.segs[k as usize];
            if !s.blocks(hit) {
                continue;
            }
            let rr = r + s.half;
            let ab = s.b - s.a;
            let side = cross(ab, prev - s.a);
            let n_side = if ab.length_squared() > 1e-12 {
                let n = vec2(-ab.y, ab.x).normalize();
                if side >= 0.0 { n } else { -n }
            } else {
                Vec2::X
            };
            if crosses(prev, p, s.a, s.b) {
                p = closest(s, p) + n_side * rr;
                continue;
            }
            let c = closest(s, p);
            let d = p - c;
            let d2 = d.length_squared();
            if d2 < rr * rr {
                let n = if d2 > 1e-10 { d / d2.sqrt() } else { n_side };
                p = c + n * rr;
            }
        }
        p
    }

    /// Outward normal of a wall the particle is touching, if any.
    #[inline]
    pub fn contact(&self, p: Vec2, r: f32, hit: Hit) -> Option<Vec2> {
        let mut out: Option<Vec2> = None;
        for &k in self.bucket(p) {
            let s = &self.segs[k as usize];
            if !s.blocks(hit) {
                continue;
            }
            let rr = r + s.half + 1e-2;
            let d = p - closest(s, p);
            let d2 = d.length_squared();
            if d2 < rr * rr && d2 > 1e-10 {
                let n = d / d2.sqrt();
                out = Some(out.map_or(n, |o| (o + n).normalize_or_zero()));
            }
        }
        out
    }

    /// Is a particle of radius `r` at `p` overlapping a wall?
    pub fn blocked(&self, p: Vec2, r: f32) -> bool {
        self.segs.iter().any(|s| closest(s, p).distance_squared(p) < (r + s.half) * (r + s.half))
    }
}
