//! The boundary between the modeller and an exact kernel.
//!
//! The rebuild builds each feature on an exact kernel while it can, regenerating the
//! faceted body from the exact bodies after every feature, and moves the part to the
//! faceted kernel ([`anvil_kernel`]) for good at the first feature the exact kernel
//! declines (see [`crate::rebuild`]). [`ExactKernel`] is what the rebuild asks of the
//! exact kernel. [`anvil_brep`] implements it ([`crate::exact::Brep`]).
//!
//! The arguments are the modeller's own types (sketch profiles, frames, face and edge
//! names), not any kernel's, so another exact kernel could implement the trait without
//! the features changing. The contract a kernel must keep:
//!
//! - **Face names.** Every face of a body it returns carries the modeller's name for it,
//!   a [`FaceId`]: the feature that made it and a stable index. An extrusion's start cap
//!   is index 0, its end cap 1, and the face swept by sketch entity `k` is `2 + k`; the
//!   faces a blend makes along its `k`-th edge are `2 + 8k`, `4 + 8k` and `6 + 8k`; a
//!   shell's cavity faces are 2 plus the rank of the face they follow among all of the
//!   part's names. Faces that are kept keep their names. [`ExactKernel::face_names`]
//!   lists them, and selections, edge references and patterns depend on them.
//! - **Refusals.** An operation the kernel cannot do exactly returns `Err` with a reason
//!   a user can read. It never returns a wrong or partial body: the rebuild then moves to
//!   the faceted kernel, so declining costs precision, while a wrong body costs the part.
//! - **Units.** Lengths in millimetres, angles in radians.
//!
//! The conformance tests at the end of this module check a kernel against exact answers;
//! any implementation must pass them.
//!
//! The trait covers the operations of the rebuild's own feature steps (extrude, revolve,
//! cut and boss, holes, fillets, chamfers, shells). Sweeps, lofts, patterns, ribs,
//! drafts, domes, body operations, sections and the measuring and reference-geometry code
//! still call [`anvil_brep`] directly, through [`crate::exact`] or on its bodies; they
//! move behind the trait in later changes.

use crate::document::BodyOp;
use anvil_kernel::{EdgeRef, FaceId, Frame, Profile, Solid, Vec2};

/// The cross-section of a fillet's round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Round {
    /// The ball's circular arc.
    Circular,
    /// A conic weighted by rho, from 0.5 (a parabola) to 0.95.
    ConicRho(f64),
    /// A conic with this radius of curvature at its shoulder point.
    ConicRadius(f64),
    /// A curve that meets each face with the face's own curvature.
    CurvatureContinuous,
}

/// How a fillet or chamfer feature treats its edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blend {
    /// A round of this radius.
    Fillet { radius: f64, round: Round },
    /// A bevel set back this far along both faces.
    Chamfer(f64),
    /// A bevel whose ends lie where both faces, offset this far into the part, meet.
    ChamferOffset(f64),
    /// A bevel set back `distance` along one of each edge's two named faces (the first in
    /// [`EdgeRef`] order, or the second when `flip`) and leaving it at `angle`.
    ChamferAngle { distance: f64, angle: f64, flip: bool },
}

/// What the rebuild asks of an exact kernel. Bodies are values: an operation takes the
/// part's bodies and returns the new ones, leaving its inputs alone. Tool bodies are owned
/// by `feature`, which names their faces.
pub trait ExactKernel {
    /// One closed body.
    type Body: Clone;

    /// `profiles` (in the sketch plane `frame`) extruded from `z0` to `z1` along the
    /// frame's normal: one tool body per region.
    fn extrude(
        &self,
        profiles: &[Profile],
        frame: &Frame,
        z0: f64,
        z1: f64,
        feature: u32,
    ) -> Result<Vec<Self::Body>, String>;

    /// As [`Self::extrude`], with the sides drafted: the section at height `z` is the
    /// profile offset outwards by `z * taper` (the tangent of the draft angle; negative
    /// draws it in).
    fn extrude_drafted(
        &self,
        profiles: &[Profile],
        frame: &Frame,
        z0: f64,
        z1: f64,
        taper: f64,
        feature: u32,
    ) -> Result<Vec<Self::Body>, String>;

    /// `profiles` turned by `angle` about the sketch line through `axis_point` along
    /// `axis_dir`: one tool body per region.
    fn revolve(
        &self,
        profiles: &[Profile],
        frame: &Frame,
        axis_point: Vec2,
        axis_dir: Vec2,
        angle: f64,
        feature: u32,
    ) -> Result<Vec<Self::Body>, String>;

    /// The bodies with `tool` applied: a boss merges with every body it touches (and
    /// stands alone when it touches none), a cut is subtracted from every body.
    fn combine(
        &self,
        bodies: &[Self::Body],
        tool: &Self::Body,
        op: BodyOp,
    ) -> Result<Vec<Self::Body>, String>;

    /// The material `a` and `b` share: no bodies when they share none.
    fn intersect(&self, a: &Self::Body, b: &Self::Body) -> Result<Vec<Self::Body>, String>;

    /// The bodies with the named edges blended. With `propagate`, creases running smoothly
    /// on from them are blended too.
    fn blend(
        &self,
        bodies: &[Self::Body],
        edges: &[EdgeRef],
        blend: Blend,
        propagate: bool,
        feature: u32,
    ) -> Result<Vec<Self::Body>, String>;

    /// The bodies hollowed to a wall of `thickness`, open through the named faces.
    fn shell(
        &self,
        bodies: &[Self::Body],
        remove: &[FaceId],
        thickness: f64,
        feature: u32,
    ) -> Result<Vec<Self::Body>, String>;

    /// The faceted solid for the bodies, its polygons tagged with the faces' names and
    /// surfaces: what the part shows, picks and exports from.
    fn faceted(&self, bodies: &[Self::Body]) -> Result<Solid, String>;

    /// The names of the body's faces.
    fn face_names(&self, body: &Self::Body) -> Vec<FaceId>;
}

/// Checks an [`ExactKernel`] against exact answers. Any kernel implementing the trait
/// must pass these; [`crate::exact::Brep`] runs them in its tests.
#[cfg(test)]
pub(crate) mod conformance {
    use super::*;
    use anvil_kernel::{v2, Loop2};
    use std::f64::consts::PI;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
        Profile::new(Loop2::polygon(&[v2(x0, y0), v2(x1, y0), v2(x1, y1), v2(x0, y1)], 0), vec![])
    }

    fn volume<K: ExactKernel>(k: &K, bodies: &[K::Body]) -> f64 {
        k.faceted(bodies).expect("faceted").mass_properties().volume
    }

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1.0)
    }

    fn one<K: ExactKernel>(mut v: Vec<K::Body>) -> K::Body {
        assert_eq!(v.len(), 1, "one body expected");
        v.remove(0)
    }

    /// A 10 mm cube from the world origin, owned by feature 1.
    fn cube<K: ExactKernel>(k: &K) -> K::Body {
        one::<K>(k.extrude(&[rect(0.0, 0.0, 10.0, 10.0)], &Frame::WORLD, 0.0, 10.0, 1).expect("cube"))
    }

    /// The edge between the cube's top (its end cap) and the side swept by the rectangle's
    /// first segment (y = 0).
    fn top_front() -> EdgeRef {
        EdgeRef::new(FaceId { feature: 1, index: FaceId::END_CAP }, FaceId { feature: 1, index: 2 })
    }

    pub(crate) fn run<K: ExactKernel>(k: &K) {
        extrusions_and_face_names(k);
        booleans(k);
        revolves(k);
        blends(k);
        shells(k);
        refusals(k);
    }

    fn extrusions_and_face_names<K: ExactKernel>(k: &K) {
        let c = cube(k);
        assert!(close(volume(k, std::slice::from_ref(&c)), 1000.0, 1e-9));
        let mut names = k.face_names(&c);
        names.sort();
        let want: Vec<FaceId> = (0..6).map(|index| FaceId { feature: 1, index }).collect();
        assert_eq!(names, want, "caps 0 and 1, sides 2 + segment");
        // Drafted in by 10 degrees over 5 mm: a frustum of a square pyramid.
        let t = 10f64.to_radians().tan();
        let d =
            k.extrude_drafted(&[rect(0.0, 0.0, 10.0, 10.0)], &Frame::WORLD, 0.0, 5.0, -t, 2).expect("draft");
        let top = 10.0 - 2.0 * 5.0 * t;
        let want = 5.0 / 3.0 * (100.0 + top * top + 10.0 * top);
        assert!(close(volume(k, &d), want, 1e-9), "{} vs {want}", volume(k, &d));
    }

    fn booleans<K: ExactKernel>(k: &K) {
        let c = cube(k);
        let bar = one::<K>(k.extrude(&[rect(5.0, 2.0, 15.0, 8.0)], &Frame::WORLD, 2.0, 8.0, 2).expect("bar"));
        let boss = k.combine(std::slice::from_ref(&c), &bar, BodyOp::Boss).expect("boss");
        assert_eq!(boss.len(), 1);
        assert!(close(volume(k, &boss), 1000.0 + 5.0 * 6.0 * 6.0, 1e-9));
        let cut = k.combine(std::slice::from_ref(&c), &bar, BodyOp::Cut).expect("cut");
        assert!(close(volume(k, &cut), 1000.0 - 5.0 * 6.0 * 6.0, 1e-9));
        // A boss that touches nothing stands alone.
        let far =
            one::<K>(k.extrude(&[rect(20.0, 0.0, 21.0, 1.0)], &Frame::WORLD, 0.0, 1.0, 3).expect("far"));
        assert_eq!(k.combine(std::slice::from_ref(&c), &far, BodyOp::Boss).expect("apart").len(), 2);
        // There must be a body to cut.
        assert!(k.combine(&[], &bar, BodyOp::Cut).is_err());
        let common = k.intersect(&c, &bar).expect("intersect");
        assert!(close(volume(k, &common), 5.0 * 6.0 * 6.0, 1e-9));
        assert!(k.intersect(&c, &far).expect("apart").is_empty());
    }

    fn revolves<K: ExactKernel>(k: &K) {
        // A 2 x 3 rectangle 1 from the sketch's y axis, turned a full turn: a tube.
        let t = k
            .revolve(&[rect(1.0, 0.0, 3.0, 3.0)], &Frame::WORLD, v2(0.0, 0.0), v2(0.0, 1.0), 2.0 * PI, 1)
            .expect("revolve");
        let want = PI * (9.0 - 1.0) * 3.0;
        assert!(close(volume(k, &t), want, 2e-3), "{} vs {want}", volume(k, &t));
    }

    fn blends<K: ExactKernel>(k: &K) {
        let c = [cube(k)];
        let e = [top_front()];
        let ch = k.blend(&c, &e, Blend::Chamfer(2.0), false, 5).expect("chamfer");
        assert!(close(volume(k, &ch), 1000.0 - 2.0 * 10.0, 1e-9));
        // The bevel is named after the blend and its edge: 2 + 8k + 2 for edge k = 0.
        assert!(ch.iter().flat_map(|b| k.face_names(b)).any(|f| f == FaceId { feature: 5, index: 4 }));
        let ang = Blend::ChamferAngle { distance: 2.0, angle: 30f64.to_radians(), flip: false };
        let ch = k.blend(&c, &e, ang, false, 5).expect("angle chamfer");
        // Set back 2 along one face and 2 tan 30 along the other.
        assert!(close(volume(k, &ch), 1000.0 - 0.5 * 2.0 * 2.0 * 30f64.to_radians().tan() * 10.0, 1e-9));
        let f =
            k.blend(&c, &e, Blend::Fillet { radius: 2.0, round: Round::Circular }, false, 5).expect("fillet");
        let want = 1000.0 - (4.0 - PI) * 10.0;
        assert!(close(volume(k, &f), want, 1e-3), "{} vs {want}", volume(k, &f));
        // An edge that is not there.
        let gone = EdgeRef::new(FaceId { feature: 9, index: 0 }, FaceId { feature: 9, index: 1 });
        assert!(k.blend(&c, &[gone], Blend::Chamfer(1.0), false, 5).is_err());
    }

    fn shells<K: ExactKernel>(k: &K) {
        let c = [cube(k)];
        let top = FaceId { feature: 1, index: FaceId::END_CAP };
        let s = k.shell(&c, &[top], 1.0, 6).expect("shell");
        assert!(close(volume(k, &s), 1000.0 - 8.0 * 8.0 * 9.0, 1e-9));
        let gone = FaceId { feature: 9, index: 0 };
        assert!(k.shell(&c, &[gone], 1.0, 6).is_err());
    }

    fn refusals<K: ExactKernel>(k: &K) {
        let c = [cube(k)];
        // Too big to fit on the faces: refused, not built wrong.
        assert!(k.blend(&c, &[top_front()], Blend::Chamfer(20.0), false, 5).is_err());
        assert!(k.shell(&c, &[FaceId { feature: 1, index: FaceId::END_CAP }], 6.0, 6).is_err());
        assert!(k.extrude(&[], &Frame::WORLD, 0.0, 1.0, 1).is_err());
    }
}
