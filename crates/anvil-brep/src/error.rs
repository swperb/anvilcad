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
}
