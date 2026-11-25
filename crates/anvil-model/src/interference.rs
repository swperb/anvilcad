//! Interference check: which components of an assembly overlap, by how much, which touch,
//! and which are closer than a clearance.
//!
//! This follows "Interference and clearance" in `docs/re/assemblies-and-mates.md`,
//! especially its "AnvilCAD decisions". Each pair of components is reported at most once,
//! as the most serious of:
//!
//! - **overlap:** material common to both, with its exact volume (the exact kernel's
//!   boolean intersection and exact mass integrals), or the faceted bodies' common volume
//!   where a part is only faceted or the exact intersection fails, marked approximate;
//! - **touching:** no common volume but boundaries within [`TOUCH_TOLERANCE`], reported
//!   only when asked for;
//! - **clearance:** a minimum distance above the touching tolerance and at most the
//!   clearance asked for.
//!
//! A component's own bodies are never checked against each other. The work is split over
//! threads by pair and can be cancelled through [`Progress`].

use crate::assembly::{Assembly, Parts};
use crate::exact;
use crate::measure::{body_gap, BodyGeo};
use anvil_brep as brep;
use anvil_brep::boolean::{self, BoolOp};
use anvil_kernel::{Aabb, Solid, Vec3};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;

/// Boundaries closer than this (mm) touch.
pub const TOUCH_TOLERANCE: f64 = 1e-6;
/// Common volumes at or below this (mm³) are not an overlap.
pub const VOLUME_TOLERANCE: f64 = 1e-6;

/// What to look for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    /// Also report pairs whose minimum distance is at most this (mm).
    pub clearance: Option<f64>,
    /// Also report pairs that touch without overlapping.
    pub touching: bool,
    /// Check hidden components too.
    pub include_hidden: bool,
    /// Only these components: with one, the pairs involving it; with several, the pairs
    /// among them. Every pair when empty.
    pub components: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FindingKind {
    /// Material common to both components, `volume` mm³.
    Overlap { volume: f64 },
    /// The components touch without overlapping.
    Touching,
    /// The components are `distance` mm apart, within the clearance asked for.
    Clearance { distance: f64 },
}

/// One pair of components that overlap, touch or are too close.
#[derive(Clone, Debug)]
pub struct Finding {
    pub a: u32,
    pub b: u32,
    pub kind: FindingKind,
    /// The closest points, on `a` and on `b`, for touching and clearance findings.
    pub points: Option<(Vec3, Vec3)>,
    /// The common volume, faceted, in assembly coordinates (empty unless an overlap).
    pub region: Solid,
    /// False when the result came from faceted bodies.
    pub exact: bool,
}

impl Finding {
    pub fn volume(&self) -> Option<f64> {
        match self.kind {
            FindingKind::Overlap { volume } => Some(volume),
            _ => None,
        }
    }

    pub fn distance(&self) -> Option<f64> {
        match self.kind {
            FindingKind::Overlap { .. } => None,
            FindingKind::Touching => Some(0.0),
            FindingKind::Clearance { distance } => Some(distance),
        }
    }

    /// `overlap`, `touching` or `clearance`.
    pub fn label(&self) -> &'static str {
        match self.kind {
            FindingKind::Overlap { .. } => "overlap",
            FindingKind::Touching => "touching",
            FindingKind::Clearance { .. } => "clearance",
        }
    }
}

/// The result of a check.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Overlaps first, largest first; then touching pairs; then clearances, nearest first.
    pub findings: Vec<Finding>,
    /// Components that could not be checked (their part did not load), with why.
    pub skipped: Vec<(u32, String)>,
    /// How many pairs of components were checked.
    pub pairs: usize,
}

/// Progress of a running check, shared with the thread that runs it.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    cancel: AtomicBool,
}

impl Progress {
    pub fn new() -> Progress {
        Progress::default()
    }

    /// Pairs checked and pairs in all.
    pub fn counts(&self) -> (usize, usize) {
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }

    /// The fraction done, or `None` before the pairs are counted.
    pub fn fraction(&self) -> Option<f32> {
        let (d, t) = self.counts();
        (t > 0).then(|| d as f32 / t as f32)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// The check was cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

/// One body of a component, placed in the assembly.
pub struct Body {
    /// The exact body, when the part was built on the exact kernel.
    exact: Option<brep::Solid>,
    /// The faceted body: given for a faceted part, tessellated from the exact body on
    /// first use otherwise (on the thread that checks it, not the caller's).
    faceted: OnceLock<Solid>,
    bounds: Aabb,
    geo: OnceLock<Option<BodyGeo>>,
}

impl Body {
    /// A body of a part built on the exact kernel.
    pub fn exact(solid: brep::Solid) -> Body {
        let bounds = match solid.bounds() {
            Some((lo, hi)) => Aabb { min: v3(lo), max: v3(hi) },
            None => Aabb::empty(),
        };
        Body { exact: Some(solid), faceted: OnceLock::new(), bounds, geo: OnceLock::new() }
    }

    /// A body of a part built on the faceted kernel.
    pub fn faceted(solid: Solid) -> Body {
        let bounds = solid.bounds();
        Body { exact: None, faceted: OnceLock::from(solid), bounds, geo: OnceLock::new() }
    }

    fn mesh(&self) -> &Solid {
        self.faceted.get_or_init(|| match &self.exact {
            Some(e) => exact::faceted(std::slice::from_ref(e)).unwrap_or_default(),
            None => Solid::default(),
        })
    }

    fn geo(&self) -> Option<&BodyGeo> {
        self.geo
            .get_or_init(|| {
                let exact = self.exact.as_ref().and_then(BodyGeo::exact);
                exact.or_else(|| (!self.mesh().is_empty()).then(|| BodyGeo::faceted(self.mesh())))
            })
            .as_ref()
    }
}

/// The bodies of one component, placed in the assembly.
pub struct ComponentBodies {
    pub id: u32,
    pub bodies: Vec<Body>,
}

impl ComponentBodies {
    fn bounds(&self) -> Option<Aabb> {
        self.bodies.iter().map(|b| b.bounds).filter(|b| !b.is_empty()).reduce(|mut a, b| {
            a.add(b.min);
            a.add(b.max);
            a
        })
    }
}

fn v3(d: glam::DVec3) -> Vec3 {
    Vec3 { x: d.x, y: d.y, z: d.z }
}

/// True when the boxes are within `pad` of each other.
fn near(a: &Aabb, b: &Aabb, pad: f64) -> bool {
    a.min.x <= b.max.x + pad
        && b.min.x <= a.max.x + pad
        && a.min.y <= b.max.y + pad
        && b.min.y <= a.max.y + pad
        && a.min.z <= b.max.z + pad
        && b.min.z <= a.max.z + pad
}

impl Assembly {
    /// The bodies of the components a check with `opts` looks at, placed in the assembly,
    /// and the components whose part could not be loaded.
    pub fn check_bodies(
        &self,
        parts: &mut Parts,
        opts: &Options,
    ) -> (Vec<ComponentBodies>, Vec<(u32, String)>) {
        let mut out = Vec::new();
        let mut skipped = Vec::new();
        for c in &self.components {
            if c.hidden && !opts.include_hidden {
                continue;
            }
            let part = match parts.get(&c.path, c.configuration.as_deref()) {
                Ok(p) => p,
                Err(e) => {
                    skipped.push((c.id, e));
                    continue;
                }
            };
            let st = &part.state;
            let bodies = match st.exact.as_deref().filter(|b| !b.is_empty()) {
                Some(exact) => {
                    let f = exact::frame(&c.placement);
                    exact.iter().map(|b| Body::exact(b.transformed(&f))).collect()
                }
                None if st.body.is_empty() => Vec::new(),
                None => vec![Body::faceted(st.body.transformed(&c.placement))],
            };
            out.push(ComponentBodies { id: c.id, bodies });
        }
        (out, skipped)
    }

    /// Checks the assembly for interference (see the module comment). Loads parts on the
    /// calling thread, then checks the pairs.
    pub fn interference(
        &self,
        parts: &mut Parts,
        opts: &Options,
        progress: &Progress,
    ) -> Result<Report, Cancelled> {
        let (bodies, skipped) = self.check_bodies(parts, opts);
        let mut report = check(&bodies, opts, progress)?;
        report.skipped = skipped;
        Ok(report)
    }
}

/// The pairs of components to check, as indices into `comps`.
fn pairs(comps: &[ComponentBodies], opts: &Options) -> Vec<(usize, usize)> {
    let chosen = |i: usize| comps.get(i).is_some_and(|c| opts.components.contains(&c.id));
    let mut out = Vec::new();
    for i in 0..comps.len() {
        for j in i + 1..comps.len() {
            let keep = match opts.components.len() {
                0 => true,
                1 => chosen(i) || chosen(j),
                _ => chosen(i) && chosen(j),
            };
            if keep {
                out.push((i, j));
            }
        }
    }
    out
}

/// Checks every pair of `comps` (see the module comment).
pub fn check(comps: &[ComponentBodies], opts: &Options, progress: &Progress) -> Result<Report, Cancelled> {
    let todo = pairs(comps, opts);
    progress.total.store(todo.len().max(1), Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    let clearance = opts.clearance.filter(|c| c.is_finite() && *c > 0.0);
    let pad = clearance.unwrap_or(0.0).max(TOUCH_TOLERANCE) + TOUCH_TOLERANCE;
    let bounds: Vec<Option<Aabb>> = comps.iter().map(ComponentBodies::bounds).collect();
    let next = AtomicUsize::new(0);
    let threads =
        std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8).min(todo.len().max(1));
    let found: Vec<Vec<Finding>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        if progress.cancelled() {
                            break;
                        }
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&(i, j)) = todo.get(k) else { break };
                        let close = matches!((&bounds[i], &bounds[j]), (Some(a), Some(b)) if near(a, b, pad));
                        if close {
                            if let Some(f) =
                                check_pair(&comps[i], &comps[j], opts.touching, clearance, progress)
                            {
                                mine.push(f);
                            }
                        }
                        progress.done.fetch_add(1, Ordering::Relaxed);
                    }
                    mine
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap_or_default()).collect()
    });
    if progress.cancelled() {
        return Err(Cancelled);
    }
    let mut findings: Vec<Finding> = found.into_iter().flatten().collect();
    let rank = |f: &Finding| match f.kind {
        FindingKind::Overlap { volume } => (0, -volume),
        FindingKind::Touching => (1, 0.0),
        FindingKind::Clearance { distance } => (2, distance),
    };
    findings.sort_by(|x, y| {
        let (kx, vx) = rank(x);
        let (ky, vy) = rank(y);
        kx.cmp(&ky).then(vx.total_cmp(&vy)).then((x.a, x.b).cmp(&(y.a, y.b)))
    });
    Ok(Report { findings, skipped: Vec::new(), pairs: todo.len() })
}

/// The common volume of two bodies, its faceted shape, and whether it is exact.
fn common(a: &Body, b: &Body) -> (f64, Solid, bool) {
    if let (Some(ea), Some(eb)) = (&a.exact, &b.exact) {
        if let Ok(pieces) = boolean::boolean(ea, eb, BoolOp::Intersect) {
            let volumes: Option<f64> =
                pieces.iter().map(|p| brep::mass::exact_properties(p).ok().map(|m| m.volume.abs())).sum();
            if let Some(volume) = volumes {
                let region = if volume > VOLUME_TOLERANCE {
                    exact::faceted(&pieces).unwrap_or_default()
                } else {
                    Solid::default()
                };
                return (volume, region, true);
            }
        }
    }
    let region = a.mesh().intersect(b.mesh());
    let volume = region.mass_properties().volume.abs();
    (volume, region, false)
}

/// The finding for one pair of components, if any.
fn check_pair(
    a: &ComponentBodies,
    b: &ComponentBodies,
    touching: bool,
    clearance: Option<f64>,
    progress: &Progress,
) -> Option<Finding> {
    let mut volume = 0.0;
    let mut polygons = Vec::new();
    let mut exact = true;
    let body_pairs: Vec<(&Body, &Body)> =
        a.bodies.iter().flat_map(|x| b.bodies.iter().map(move |y| (x, y))).collect();
    for (x, y) in &body_pairs {
        if progress.cancelled() {
            return None;
        }
        if !near(&x.bounds, &y.bounds, TOUCH_TOLERANCE) {
            continue;
        }
        let (v, region, e) = common(x, y);
        if v > VOLUME_TOLERANCE {
            volume += v;
            polygons.extend(region.polygons);
            exact &= e;
        }
    }
    if volume > VOLUME_TOLERANCE {
        return Some(Finding {
            a: a.id,
            b: b.id,
            kind: FindingKind::Overlap { volume },
            points: None,
            region: Solid::new(polygons),
            exact,
        });
    }
    if !touching && clearance.is_none() {
        return None;
    }
    let reach = clearance.unwrap_or(0.0).max(TOUCH_TOLERANCE) + TOUCH_TOLERANCE;
    let mut best: Option<(f64, Vec3, Vec3, bool)> = None;
    for (x, y) in &body_pairs {
        if progress.cancelled() {
            return None;
        }
        if !near(&x.bounds, &y.bounds, reach) {
            continue;
        }
        let (Some(gx), Some(gy)) = (x.geo(), y.geo()) else { continue };
        if let Some((d, p, q)) = body_gap(gx, gy) {
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, p, q, x.exact.is_some() && y.exact.is_some()));
            }
        }
    }
    let (d, p, q, e) = best?;
    let kind = if d <= TOUCH_TOLERANCE {
        if !touching {
            return None;
        }
        FindingKind::Touching
    } else {
        match clearance {
            Some(c) if d <= c + TOUCH_TOLERANCE => FindingKind::Clearance { distance: d },
            _ => return None,
        }
    };
    Some(Finding { a: a.id, b: b.id, kind, points: Some((p, q)), region: Solid::default(), exact: e })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        BodyOp, Dim, Document, EndCondition, ExtrudeFeature, FeatureKind, PlaneRef, SketchFeature,
    };
    use anvil_kernel::{v2, v3 as kv3, Frame};
    use std::f64::consts::PI;

    fn extrude(sketch: impl FnOnce(&mut anvil_sketch::Sketch), h: f64) -> Document {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sketch(&mut sf.sketch);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(h),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        doc
    }

    /// A `w` x `d` x `h` block with its corner at the origin.
    fn block(w: f64, d: f64, h: f64) -> Document {
        extrude(
            |s| {
                s.add_rectangle(v2(0.0, 0.0), v2(w, d));
            },
            h,
        )
    }

    fn at(x: f64, y: f64, z: f64) -> Frame {
        Frame { origin: kv3(x, y, z), ..Frame::WORLD }
    }

    fn assembly(parts: &[(&str, Document)], placed: &[(&str, Frame)]) -> (Assembly, Parts, Vec<u32>) {
        let mut p = Parts::new(".");
        for (name, doc) in parts {
            p.insert(name, None, doc.clone());
        }
        let mut asm = Assembly::new();
        let ids = placed.iter().map(|(name, f)| asm.add_component(name, None, *f)).collect();
        (asm, p, ids)
    }

    /// The exact mass integrals are good to about 1e-9 of the body's size, so volumes
    /// are compared to 1e-7 of their value.
    fn same_volume(got: Option<f64>, want: f64) -> bool {
        got.is_some_and(|v| (v - want).abs() <= 1e-7 * want)
    }

    fn run(asm: &Assembly, parts: &mut Parts, opts: &Options) -> Report {
        asm.interference(parts, opts, &Progress::new()).expect("not cancelled")
    }

    #[test]
    fn two_overlapping_boxes_overlap_by_their_exact_common_volume() {
        // 20x20x20 and 10x10x10 at (15, 15, 15): the common part is 5x5x5 = 125 mm³.
        let (asm, mut parts, ids) = assembly(
            &[("big.anvprt", block(20.0, 20.0, 20.0)), ("small.anvprt", block(10.0, 10.0, 10.0))],
            &[("big.anvprt", Frame::WORLD), ("small.anvprt", at(15.0, 15.0, 15.0))],
        );
        let r = run(&asm, &mut parts, &Options::default());
        assert_eq!(r.pairs, 1);
        let [f] = r.findings.as_slice() else { panic!("{:?}", r.findings) };
        assert_eq!((f.a, f.b), (ids[0], ids[1]));
        assert!(f.exact);
        assert!(same_volume(f.volume(), 125.0), "{:?}", f.volume());
        // The region shown in the view is the 5 mm cube at (15..20)^3.
        let b = f.region.bounds();
        assert!(
            (b.min - kv3(15.0, 15.0, 15.0)).length() < 1e-6
                && (b.max - kv3(20.0, 20.0, 20.0)).length() < 1e-6
        );
        assert!(same_volume(Some(f.region.mass_properties().volume), 125.0));
    }

    #[test]
    fn a_pin_in_an_undersized_hole_overlaps_by_the_annulus() {
        // A 40x40x10 plate with a 10 mm hole, and a 10.2 mm pin 30 long through it: the
        // overlap is the ring between r = 5 and r = 5.1 over the plate's 10 mm.
        let plate = extrude(
            |s| {
                s.add_rectangle(v2(-20.0, -20.0), v2(20.0, 20.0));
                let c = s.add_point(v2(0.0, 0.0));
                s.add_circle(c, 5.0);
            },
            10.0,
        );
        let pin = extrude(
            |s| {
                let c = s.add_point(v2(0.0, 0.0));
                s.add_circle(c, 5.1);
            },
            30.0,
        );
        let (asm, mut parts, _) = assembly(
            &[("plate.anvprt", plate), ("pin.anvprt", pin)],
            &[("plate.anvprt", Frame::WORLD), ("pin.anvprt", at(0.0, 0.0, -10.0))],
        );
        let r = run(&asm, &mut parts, &Options::default());
        let [f] = r.findings.as_slice() else { panic!("{:?}", r.findings) };
        let want = PI * (5.1 * 5.1 - 5.0 * 5.0) * 10.0;
        assert!(f.exact);
        assert!(same_volume(f.volume(), want), "{:?} vs {want}", f.volume());
    }

    #[test]
    fn boxes_face_to_face_touch_and_do_not_overlap() {
        // A 10 mm cube on top of a 40x30x10 block, inside its top face.
        let (asm, mut parts, ids) = assembly(
            &[("base.anvprt", block(40.0, 30.0, 10.0)), ("cube.anvprt", block(10.0, 10.0, 10.0))],
            &[("base.anvprt", Frame::WORLD), ("cube.anvprt", at(5.0, 5.0, 10.0))],
        );
        // Not asked for touching: nothing to report.
        assert!(run(&asm, &mut parts, &Options::default()).findings.is_empty());
        let r = run(&asm, &mut parts, &Options { touching: true, ..Default::default() });
        let [f] = r.findings.as_slice() else { panic!("{:?}", r.findings) };
        assert_eq!(f.kind, FindingKind::Touching);
        assert_eq!((f.a, f.b), (ids[0], ids[1]));
        let (p, q) = f.points.expect("closest points");
        assert!((p.z - 10.0).abs() < 1e-9 && (q.z - 10.0).abs() < 1e-9, "{p:?} {q:?}");
        // With a clearance, touching still wins over clearance.
        let r =
            run(&asm, &mut parts, &Options { touching: true, clearance: Some(1.0), ..Default::default() });
        assert_eq!(r.findings.iter().map(|f| f.kind).collect::<Vec<_>>(), [FindingKind::Touching]);
    }

    #[test]
    fn a_pin_in_a_clearance_hole_is_reported_at_the_exact_gap() {
        // A 9.5 mm pin in the 10 mm hole, off centre by 0.1 mm in x: the nearest gap is
        // 5 - 4.75 - 0.1 = 0.15 mm.
        let plate = extrude(
            |s| {
                s.add_rectangle(v2(-20.0, -20.0), v2(20.0, 20.0));
                let c = s.add_point(v2(0.0, 0.0));
                s.add_circle(c, 5.0);
            },
            10.0,
        );
        let pin = extrude(
            |s| {
                let c = s.add_point(v2(0.0, 0.0));
                s.add_circle(c, 4.75);
            },
            30.0,
        );
        let (asm, mut parts, _) = assembly(
            &[("plate.anvprt", plate), ("pin.anvprt", pin)],
            &[("plate.anvprt", Frame::WORLD), ("pin.anvprt", at(0.1, 0.0, -10.0))],
        );
        let none = run(&asm, &mut parts, &Options { clearance: Some(0.1), ..Default::default() });
        assert!(none.findings.is_empty(), "{:?}", none.findings);
        let r =
            run(&asm, &mut parts, &Options { clearance: Some(0.5), touching: true, ..Default::default() });
        let [f] = r.findings.as_slice() else { panic!("{:?}", r.findings) };
        let d = f.distance().unwrap_or(f64::NAN);
        assert!(matches!(f.kind, FindingKind::Clearance { .. }));
        assert!((d - 0.15).abs() < 1e-9, "{d}");
        let (p, q) = f.points.expect("points");
        assert!((p - q).length() - d < 1e-12);
        assert!(p.x > 4.0 && q.x > 4.0, "the gap is on the +x side: {p:?} {q:?}");
    }

    #[test]
    fn two_blocks_apart_report_the_exact_clearance_and_a_clean_assembly_reports_nothing() {
        let (asm, mut parts, _) = assembly(
            &[("a.anvprt", block(10.0, 10.0, 10.0))],
            &[("a.anvprt", Frame::WORLD), ("a.anvprt", at(12.5, 3.0, 0.0)), ("a.anvprt", at(0.0, 40.0, 0.0))],
        );
        let clean = run(&asm, &mut parts, &Options { touching: true, ..Default::default() });
        assert!(clean.findings.is_empty());
        assert_eq!(clean.pairs, 3);
        let r = run(&asm, &mut parts, &Options { clearance: Some(2.5), ..Default::default() });
        let [f] = r.findings.as_slice() else { panic!("{:?}", r.findings) };
        assert_eq!(f.kind, FindingKind::Clearance { distance: 2.5 });
        // A clearance just under the gap reports nothing.
        assert!(run(&asm, &mut parts, &Options { clearance: Some(2.49), ..Default::default() })
            .findings
            .is_empty());
    }

    #[test]
    fn hidden_components_and_the_component_filter() {
        let (mut asm, mut parts, ids) = assembly(
            &[("a.anvprt", block(10.0, 10.0, 10.0))],
            &[("a.anvprt", Frame::WORLD), ("a.anvprt", at(5.0, 0.0, 0.0)), ("a.anvprt", at(0.0, 5.0, 0.0))],
        );
        let all = run(&asm, &mut parts, &Options::default());
        // Each pair overlaps: 5x10x10, 10x5x10 and 5x5x10.
        let v: Vec<f64> = all.findings.iter().filter_map(Finding::volume).collect();
        assert!(
            v.len() == 3 && v.iter().zip([500.0, 500.0, 250.0]).all(|(g, w)| same_volume(Some(*g), w)),
            "{v:?}"
        );
        let one = run(&asm, &mut parts, &Options { components: vec![ids[1]], ..Default::default() });
        assert_eq!(one.pairs, 2);
        assert!(one.findings.iter().all(|f| f.a == ids[1] || f.b == ids[1]));
        let two = run(&asm, &mut parts, &Options { components: vec![ids[1], ids[2]], ..Default::default() });
        assert_eq!((two.pairs, two.findings.len()), (1, 1));
        if let Some(c) = asm.component_mut(ids[2]) {
            c.hidden = true;
        }
        let shown = run(&asm, &mut parts, &Options::default());
        assert_eq!((shown.pairs, shown.findings.len()), (1, 1));
        let with_hidden = run(&asm, &mut parts, &Options { include_hidden: true, ..Default::default() });
        assert_eq!(with_hidden.findings.len(), 3);
    }

    #[test]
    fn faceted_parts_overlap_approximately_and_a_cancelled_check_says_so() {
        let (asm, mut parts, _) = assembly(
            &[("big.anvprt", block(20.0, 20.0, 20.0))],
            &[("big.anvprt", Frame::WORLD), ("big.anvprt", at(15.0, 15.0, 15.0))],
        );
        let (mut comps, _) = asm.check_bodies(&mut parts, &Options::default());
        for c in &mut comps {
            for b in &mut c.bodies {
                b.mesh();
                b.exact = None;
            }
        }
        let r = check(&comps, &Options::default(), &Progress::new()).expect("not cancelled");
        let [f] = r.findings.as_slice() else { panic!("{:?}", r.findings) };
        assert!(!f.exact);
        assert!(same_volume(f.volume(), 125.0), "{:?}", f.volume());
        let p = Progress::new();
        p.cancel();
        assert_eq!(check(&comps, &Options::default(), &p).err(), Some(Cancelled));
        assert_eq!(p.counts().0, 0);
    }
}
