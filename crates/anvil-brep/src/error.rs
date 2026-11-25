use crate::topo::EdgeId;
use glam::DVec2;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KernelError {
    #[error("the profile has no closed contours")]
    EmptyProfile,
    #[error("input contains a non-finite number")]
    NonFinite,
    #[error("contour {contour} is not closed (gap of {gap:.6} mm after segment {segment})")]
    OpenLoop { contour: usize, segment: usize, gap: f64 },
    #[error("contour {contour} has a zero-length segment ({segment})")]
    DegenerateSegment { contour: usize, segment: usize },
    #[error("contour {0} encloses no area")]
    DegenerateLoop(usize),
    #[error("contours intersect at ({:.4}, {:.4})", point.x, point.y)]
    SelfIntersection { point: DVec2 },
    #[error("invalid coordinate frame")]
    InvalidFrame,
    #[error("extrude depth must be non-zero and finite")]
    InvalidDistance,
    #[error("topology check failed: {0}")]
    Topology(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("not supported yet: {0}")]
    Unsupported(String),
    #[error("boolean operation failed: {0}")]
    Boolean(String),
    #[error("tessellation failed: {0}")]
    Tessellation(String),
    /// A fillet or chamfer wider than the faces next to `edge` allow. `size` is the
    /// blend's size as asked for (a fillet's radius, a chamfer's distance, in mm) and
    /// `limit` the largest size of the same blend that fits there, when the kernel can
    /// tell: the closed-form tools measure it, the rolling-ball tool does not.
    #[error(
        "a {blend} of {size} does not fit on the faces next to this edge{}",
        limit.map_or(String::new(), |l| format!(" (at most {l:.6})"))
    )]
    BlendTooLarge { edge: EdgeId, blend: &'static str, size: f64, limit: Option<f64> },
}
