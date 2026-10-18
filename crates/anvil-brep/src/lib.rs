//! AnvilCAD exact geometry kernel (anvil-brep).
//!
//! A boundary-representation (B-rep) kernel written in pure Rust. Geometry is
//! exact (lines, circles, ellipses, B-splines, planes, cylinders, cones, tori, spheres); tessellation is only produced for
//! display and mass estimates.
//!
//! Stability rules for this crate:
//! - Public operations never panic on bad input. They return [`KernelError`].
//! - Every solid produced by an operation passes [`check::validate`] before it
//!   is returned, so callers never see a half-built body.
//! - All tolerances live in [`tol`] so they can be audited in one place.

pub mod blend;
pub mod boolean;
pub mod check;
pub mod dome;
pub mod draft;
pub mod error;
pub mod extrude;
pub mod freeform;
pub mod geom;
pub mod intersect;
pub mod loft;
pub mod mass;
pub mod mirror;
pub mod offset;
pub mod patch;
pub mod profile;
pub mod revolve;
pub mod rolling;
pub mod scale;
pub mod seam;
pub mod shell;
pub mod spline;
pub mod ssi;
pub mod sweep;
pub mod taper;
pub mod tess;
pub mod tol;
pub mod topo;

pub use error::KernelError;
pub use geom::{Curve, Frame, Surface};
pub use profile::{Loop2, Region, Segment};
pub use topo::{EdgeId, FaceId, FaceTag, Solid, VertexId};

pub type Result<T> = std::result::Result<T, KernelError>;
