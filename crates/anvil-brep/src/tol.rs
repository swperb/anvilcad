//! Tolerances. Model units are millimetres.

/// Two points closer than this are the same point.
pub const LINEAR: f64 = 1e-6;

/// Lengths and areas below this are treated as degenerate.
pub const DEGENERATE: f64 = 1e-9;

/// Default chordal deviation used for display tessellation (mm).
pub const CHORD: f64 = 0.01;

/// Default maximum angle between tessellation samples on a curve (radians).
pub const MAX_ANGLE: f64 = std::f64::consts::PI / 36.0;

pub fn is_finite2(v: glam::DVec2) -> bool {
    v.x.is_finite() && v.y.is_finite()
}

pub fn is_finite3(v: glam::DVec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}
