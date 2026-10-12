//! Small, dependency-free linear algebra for the kernel. Everything is f64.

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// Distances below this are treated as zero (model units are millimetres).
pub const LINEAR_TOL: f64 = 1e-6;
/// Angular tolerance in radians.
pub const ANGULAR_TOL: f64 = 1e-9;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub const fn v2(x: f64, y: f64) -> Vec2 {
    Vec2 { x, y }
}

pub const fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

impl Vec2 {
    pub const ZERO: Vec2 = v2(0.0, 0.0);

    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    /// z-component of the 3D cross product.
    pub fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Vec2 {
        let l = self.length();
        if l < 1e-300 {
            self
        } else {
            self / l
        }
    }
    /// Rotated 90 degrees counter-clockwise.
    pub fn perp(self) -> Vec2 {
        v2(-self.y, self.x)
    }
    pub fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }
    pub fn from_angle(a: f64) -> Vec2 {
        v2(a.cos(), a.sin())
    }
    pub fn lerp(self, o: Vec2, t: f64) -> Vec2 {
        self + (o - self) * t
    }
    pub fn distance(self, o: Vec2) -> f64 {
        (self - o).length()
    }
}

impl Vec3 {
    pub const ZERO: Vec3 = v3(0.0, 0.0, 0.0);
    pub const X: Vec3 = v3(1.0, 0.0, 0.0);
    pub const Y: Vec3 = v3(0.0, 1.0, 0.0);
    pub const Z: Vec3 = v3(0.0, 0.0, 1.0);

    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        v3(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn length_sq(self) -> f64 {
        self.dot(self)
    }
    pub fn normalized(self) -> Vec3 {
        let l = self.length();
        if l < 1e-300 {
            self
        } else {
            self / l
        }
    }
    pub fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        self + (o - self) * t
    }
    pub fn distance(self, o: Vec3) -> f64 {
        (self - o).length()
    }
    pub fn min(self, o: Vec3) -> Vec3 {
        v3(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    pub fn max(self, o: Vec3) -> Vec3 {
        v3(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
    pub fn to_f32(self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
    /// Some unit vector perpendicular to `self` (which must be non-zero).
    pub fn any_perpendicular(self) -> Vec3 {
        let a = if self.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        self.cross(a).normalized()
    }
}

macro_rules! impl_ops {
    ($t:ident, $($f:ident),+) => {
        impl Add for $t { type Output = $t; fn add(self, o: $t) -> $t { $t { $($f: self.$f + o.$f),+ } } }
        impl Sub for $t { type Output = $t; fn sub(self, o: $t) -> $t { $t { $($f: self.$f - o.$f),+ } } }
        impl Mul<f64> for $t { type Output = $t; fn mul(self, s: f64) -> $t { $t { $($f: self.$f * s),+ } } }
        impl Mul<$t> for f64 { type Output = $t; fn mul(self, v: $t) -> $t { v * self } }
        impl Div<f64> for $t { type Output = $t; fn div(self, s: f64) -> $t { $t { $($f: self.$f / s),+ } } }
        impl Neg for $t { type Output = $t; fn neg(self) -> $t { $t { $($f: -self.$f),+ } } }
        impl AddAssign for $t { fn add_assign(&mut self, o: $t) { $(self.$f += o.$f;)+ } }
        impl SubAssign for $t { fn sub_assign(&mut self, o: $t) { $(self.$f -= o.$f;)+ } }
    };
}
impl_ops!(Vec2, x, y);
impl_ops!(Vec3, x, y, z);

/// A right-handed orthonormal coordinate system. Sketches live in the XY plane of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub origin: Vec3,
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
}

impl Default for Frame {
    fn default() -> Self {
        Frame::WORLD
    }
}

impl Frame {
    pub const WORLD: Frame = Frame { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y, z: Vec3::Z };

    /// Builds a frame from an origin, a normal (local z) and a hint for local x.
    /// The hint is projected onto the plane; if it is parallel to the normal another axis is chosen.
    pub fn from_normal(origin: Vec3, normal: Vec3, x_hint: Vec3) -> Frame {
        let z = normal.normalized();
        let mut x = x_hint - z * x_hint.dot(z);
        if x.length() < 1e-9 {
            x = z.any_perpendicular();
        }
        let x = x.normalized();
        let y = z.cross(x);
        Frame { origin, x, y, z }
    }

    pub fn to_world(&self, p: Vec2) -> Vec3 {
        self.origin + self.x * p.x + self.y * p.y
    }
    pub fn to_world3(&self, p: Vec3) -> Vec3 {
        self.origin + self.x * p.x + self.y * p.y + self.z * p.z
    }
    pub fn dir_to_world(&self, d: Vec3) -> Vec3 {
        self.x * d.x + self.y * d.y + self.z * d.z
    }
    /// Projects a world point into the frame's XY plane.
    pub fn to_local(&self, p: Vec3) -> Vec2 {
        let d = p - self.origin;
        v2(d.dot(self.x), d.dot(self.y))
    }
    pub fn to_local3(&self, p: Vec3) -> Vec3 {
        let d = p - self.origin;
        v3(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }
    pub fn offset(&self, d: f64) -> Frame {
        Frame { origin: self.origin + self.z * d, ..*self }
    }
    /// Intersects a ray with the frame's XY plane, returning the local 2D hit point.
    pub fn intersect_ray(&self, origin: Vec3, dir: Vec3) -> Option<Vec2> {
        let denom = dir.dot(self.z);
        if denom.abs() < 1e-12 {
            return None;
        }
        let t = (self.origin - origin).dot(self.z) / denom;
        Some(self.to_local(origin + dir * t))
    }
}

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn empty() -> Aabb {
        Aabb {
            min: v3(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: v3(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
    pub fn add(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    pub fn diagonal(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            (self.max - self.min).length()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let f = Frame::from_normal(v3(1.0, 2.0, 3.0), v3(0.0, 1.0, 1.0), Vec3::X);
        let p = v2(3.5, -2.0);
        let w = f.to_world(p);
        let back = f.to_local(w);
        assert!((back - p).length() < 1e-12);
        assert!((f.x.cross(f.y) - f.z).length() < 1e-12);
    }
}
