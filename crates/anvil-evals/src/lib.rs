//! Evaluations for AnvilCAD's exact kernel (`anvil-brep`).
//!
//! Each case is a solid described as an analytic CSG tree ([`csg::Tree`]). Running a
//! case builds it with the kernel (extrusions, revolutions and booleans, as a user's
//! features would) and checks the result three ways:
//!
//! - **Computational.** Every body passes a deep audit (the kernel's validity check, edges
//!   and vertices on their surfaces, a closed and outward display mesh); its volume
//!   matches the exact or integrated reference; points sampled through the part are inside
//!   the kernel's result exactly when they are inside the reference.
//! - **Visual.** The display mesh, rendered as depth and normal images from four views,
//!   matches the reference ray-cast pixel for pixel ([`raster`]).
//! - **Performance.** Building and tessellating are timed, and `anvil-evals` compares the
//!   times with a stored baseline.
//!
//! A case either passes, fails gracefully (the kernel returns an error, which a user sees
//! as a message and loses no work), or fails: a wrong or invalid body, or a panic. A
//! failure is never acceptable. See `docs/kernel/EVALS.md`.

pub mod corpus;
pub mod csg;
pub mod fuzz;
pub mod oracle;
pub mod raster;
pub mod rng;

use anvil_brep::Solid;
use csg::Tree;
use serde::Serialize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::time::Instant;

/// What a case is expected to do with today's kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Expect {
    /// Must build and pass every check.
    Pass,
    /// Known to be beyond the kernel today: an error is accepted, a wrong result is not.
    /// A case that starts passing is reported so it can be promoted.
    Graceful,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    pub category: &'static str,
    pub tree: Tree,
    pub expect: Expect,
    /// The exact volume, when known in closed form.
    pub volume: Option<f64>,
    pub bodies: Option<usize>,
    /// The genus of the (single) body, when it is meaningful.
    pub genus: Option<u32>,
    /// Why the case exists, in a sentence.
    pub note: &'static str,
}

impl Case {
    pub fn new(name: impl Into<String>, category: &'static str, tree: Tree) -> Case {
        Case {
            name: name.into(),
            category,
            tree,
            expect: Expect::Pass,
            volume: None,
            bodies: None,
            genus: None,
            note: "",
        }
    }
    pub fn volume(mut self, v: f64) -> Case {
        self.volume = Some(v);
        self
    }
    pub fn bodies(mut self, n: usize) -> Case {
        self.bodies = Some(n);
        self
    }
    pub fn genus(mut self, g: u32) -> Case {
        self.genus = Some(g);
        self
    }
    pub fn graceful(mut self) -> Case {
        self.expect = Expect::Graceful;
        self
    }
    pub fn note(mut self, s: &'static str) -> Case {
        self.note = s;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Status {
    Pass,
    /// The kernel returned an error.
    Graceful,
    /// A wrong or invalid result.
    Fail,
    Panic,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseResult {
    pub name: String,
    pub category: String,
    pub expect: Expect,
    pub status: Status,
    /// The kernel's error, the panic message, or the checks that failed.
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub bodies: usize,
    pub faces: usize,
    pub reference_volume: f64,
    /// An estimate of the integrated reference's error (zero when it is exact).
    pub reference_error: f64,
    pub kernel_volume: f64,
    pub build_ms: f64,
    pub tess_ms: f64,
    pub membership: Option<oracle::Membership>,
    pub views: Vec<raster::Diff>,
}

impl CaseResult {
    /// A result that breaks its expectation: a failure or panic always does, and a
    /// graceful error does when the case should pass.
    pub fn regressed(&self) -> bool {
        match self.status {
            Status::Pass => false,
            Status::Graceful => self.expect == Expect::Pass,
            Status::Fail | Status::Panic => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    /// Resolution of the reference volume integration grid.
    pub volume_grid: usize,
    /// Points sampled for inside/outside agreement.
    pub membership_samples: usize,
    /// Resolution of the visual comparison images (0 to skip).
    pub image_size: usize,
    /// Where to write comparison images, if anywhere.
    pub image_dir: Option<PathBuf>,
    /// Timing repetitions (the fastest is reported).
    pub repeats: usize,
    /// Write images for failing cases only.
    pub images_on_failure_only: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            volume_grid: 400,
            membership_samples: 300,
            image_size: 96,
            image_dir: None,
            repeats: 1,
            images_on_failure_only: false,
        }
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".into()
    }
}

/// Builds a case with the kernel, catching panics.
pub fn build(tree: &Tree) -> Result<Result<Vec<Solid>, String>, String> {
    match catch_unwind(AssertUnwindSafe(|| tree.build())) {
        Ok(r) => Ok(r.map_err(|e| e.to_string())),
        Err(p) => Err(panic_text(&*p)),
    }
}

pub fn run(case: &Case, opt: &Options) -> CaseResult {
    let mut r = CaseResult {
        name: case.name.clone(),
        category: case.category.into(),
        expect: case.expect,
        status: Status::Pass,
        reasons: Vec::new(),
        warnings: Vec::new(),
        bodies: 0,
        faces: 0,
        reference_volume: 0.0,
        reference_error: 0.0,
        kernel_volume: 0.0,
        build_ms: 0.0,
        tess_ms: 0.0,
        membership: None,
        views: Vec::new(),
    };
    match case.volume {
        Some(v) => r.reference_volume = v,
        None => {
            // The grid integral converges like the cell size at silhouettes; twice the
            // change from halving the grid bounds its error.
            let fine = case.tree.volume(opt.volume_grid);
            let coarse = case.tree.volume(opt.volume_grid / 2);
            r.reference_volume = fine;
            r.reference_error = 2.0 * (fine - coarse).abs();
        }
    }
    let mut bodies = None;
    let mut best = f64::INFINITY;
    for _ in 0..opt.repeats.max(1) {
        let t = Instant::now();
        let b = build(&case.tree);
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
        bodies = Some(b);
    }
    r.build_ms = best;
    let bodies = match bodies {
        Some(Ok(Ok(b))) => b,
        Some(Ok(Err(e))) => {
            r.status = Status::Graceful;
            r.reasons.push(e);
            return r;
        }
        Some(Err(p)) => {
            r.status = Status::Panic;
            r.reasons.push(format!("panic while building: {p}"));
            return r;
        }
        None => return r,
    };
    check_bodies(case, &bodies, opt, &mut r);
    r
}

fn check_bodies(case: &Case, bodies: &[Solid], opt: &Options, r: &mut CaseResult) {
    r.bodies = bodies.len();
    r.faces = bodies.iter().map(|b| b.faces.len()).sum();
    let fail = |r: &mut CaseResult, s: String| {
        r.status = Status::Fail;
        r.reasons.push(s);
    };
    if let Some(n) = case.bodies
        && n != bodies.len()
    {
        fail(r, format!("{} bodies, expected {n}", bodies.len()));
    }
    let tess = |p: &anvil_brep::tess::TessParams| {
        catch_unwind(AssertUnwindSafe(|| oracle::meshes(bodies, p)))
            .unwrap_or_else(|p| Err(format!("panic while tessellating: {}", panic_text(&*p))))
    };
    let mut best = f64::INFINITY;
    let mut display = Err(String::new());
    for _ in 0..opt.repeats.max(1) {
        let t = Instant::now();
        display = tess(&oracle::display_params());
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
    }
    r.tess_ms = best;
    let display = match display {
        Ok(m) => m,
        Err(e) => {
            if e.starts_with("panic") {
                r.status = Status::Panic;
                r.reasons.push(e);
            } else {
                fail(r, format!("display tessellation failed: {e}"));
            }
            return;
        }
    };
    for (i, (b, m)) in bodies.iter().zip(&display).enumerate() {
        let a = oracle::audit(b, m);
        for e in a.errors {
            fail(r, format!("body {i}: {e}"));
        }
        for w in a.warnings {
            r.warnings.push(format!("body {i}: {w}"));
        }
        if let (Some(g), 1) = (case.genus, bodies.len())
            && a.genus != Some(g)
        {
            fail(r, format!("genus {:?}, expected {g}", a.genus));
        }
    }

    // Volume, from a fine tessellation.
    let fine = match tess(&oracle::fine_params()) {
        Ok(m) => m,
        Err(e) => {
            fail(r, format!("fine tessellation failed: {e}"));
            return;
        }
    };
    r.kernel_volume = fine.iter().map(|m| anvil_brep::mass::properties(m).volume).sum();
    let (lo, hi) = case.tree.bounds();
    let size = (hi - lo).max_element().max(1e-12);
    let (want, rel) = match case.volume {
        Some(v) => (v, 5e-4),
        None => (r.reference_volume, 3e-3),
    };
    let err = (r.kernel_volume - want).abs();
    if err > rel * want.abs() + r.reference_error + 1e-9 * size.powi(3) {
        fail(
            r,
            format!(
                "volume {:.9} differs from the reference {want:.9} by {:.3e} (relative)",
                r.kernel_volume,
                err / want.abs().max(1e-300)
            ),
        );
    }

    // Inside and outside.
    if opt.membership_samples > 0 {
        let margin = 2e-3 * size + 0.02;
        let m = oracle::membership(&case.tree, &display, opt.membership_samples, margin, 7);
        if m.wrong > 0 {
            fail(
                r,
                format!(
                    "{} of {} sample points classified wrongly, e.g. {:?}",
                    m.wrong,
                    m.samples - m.skipped,
                    m.examples.first()
                ),
            );
        }
        r.membership = Some(m);
    }

    // Pixels.
    if opt.image_size > 0 {
        let pad = glam::DVec3::splat(size * 0.02);
        let (lo, hi) = (lo - pad, hi + pad);
        let depth_tol = 2e-3 * size + 0.03;
        let mut rows = Vec::new();
        for view in &raster::VIEWS {
            let a = raster::render_reference(&case.tree, view, lo, hi, opt.image_size);
            let b = raster::render_mesh(&display, view, lo, hi, opt.image_size);
            let (d, mask) = raster::compare(view, &a, &b, depth_tol);
            if d.bad_fraction() > 0.005 {
                fail(
                    r,
                    format!(
                        "view {}: {} of {} pixels differ ({} coverage, {} depth, {} normal)",
                        d.view,
                        d.coverage + d.depth + d.normal,
                        d.judged,
                        d.coverage,
                        d.depth,
                        d.normal
                    ),
                );
            }
            r.views.push(d);
            rows.push((*view, a, b, mask));
        }
        if let Some(dir) = &opt.image_dir
            && (!opt.images_on_failure_only || r.status != Status::Pass)
        {
            let file = dir.join(format!("{}.png", sanitize(&case.name)));
            if let Err(e) = raster::write_png(&file, &rows) {
                r.warnings.push(format!("could not write {}: {e}", file.display()));
            }
        }
    }
}

pub fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}
