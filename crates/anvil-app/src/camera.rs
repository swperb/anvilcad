//! Orthographic orbit camera.

use anvil_kernel::{v3, Aabb, Frame, Vec3};
use eframe::egui::{pos2, Pos2, Rect};

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub target: Vec3,
    /// Screen right, screen up, and toward the viewer, in world space.
    pub right: Vec3,
    pub up: Vec3,
    pub back: Vec3,
    /// Half of the visible height in model units.
    pub half_height: f64,
    /// Depth range for clipping.
    pub depth: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StdView {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Isometric,
}

impl StdView {
    pub const ALL: [StdView; 7] = [
        StdView::Front,
        StdView::Back,
        StdView::Left,
        StdView::Right,
        StdView::Top,
        StdView::Bottom,
        StdView::Isometric,
    ];
    pub fn label(&self) -> &'static str {
        match self {
            StdView::Front => "Front",
            StdView::Back => "Back",
            StdView::Left => "Left",
            StdView::Right => "Right",
            StdView::Top => "Top",
            StdView::Bottom => "Bottom",
            StdView::Isometric => "Isometric",
        }
    }
}

fn rotate(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    v * c + axis.cross(v) * s + axis * axis.dot(v) * (1.0 - c)
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Camera {
            target: Vec3::ZERO,
            right: Vec3::X,
            up: Vec3::Y,
            back: Vec3::Z,
            half_height: 60.0,
            depth: 2000.0,
        };
        c.set_view(StdView::Isometric);
        c
    }
}

impl Camera {
    pub fn set_basis(&mut self, back: Vec3, up_hint: Vec3) {
        let b = back.normalized();
        let r = up_hint.cross(b).normalized();
        self.back = b;
        self.right = r;
        self.up = b.cross(r);
    }

    pub fn set_view(&mut self, v: StdView) {
        let (b, u) = match v {
            StdView::Front => (Vec3::Z, Vec3::Y),
            StdView::Back => (-Vec3::Z, Vec3::Y),
            StdView::Left => (-Vec3::X, Vec3::Y),
            StdView::Right => (Vec3::X, Vec3::Y),
            StdView::Top => (Vec3::Y, -Vec3::Z),
            StdView::Bottom => (-Vec3::Y, Vec3::Z),
            StdView::Isometric => (v3(1.0, 1.0, 1.0), Vec3::Y),
        };
        self.set_basis(b, u);
    }

    /// Looks straight at a sketch plane, with the plane's axes on screen.
    pub fn normal_to(&mut self, f: &Frame) {
        self.back = f.z;
        self.right = f.x;
        self.up = f.y;
    }

    pub fn orbit(&mut self, dx: f64, dy: f64) {
        let k = 0.008;
        // Horizontal drag spins about the screen up axis, vertical about screen right.
        let (a1, ax1) = (-dx * k, self.up);
        self.right = rotate(self.right, ax1, a1);
        self.back = rotate(self.back, ax1, a1);
        let (a2, ax2) = (-dy * k, self.right);
        self.up = rotate(self.up, ax2, a2);
        self.back = rotate(self.back, ax2, a2);
        // Re-orthonormalize to stop drift.
        self.back = self.back.normalized();
        self.right = self.up.cross(self.back).normalized();
        self.up = self.back.cross(self.right);
    }

    /// Turns the model about axes through the view centre, in radians, right-handed: `rx`
    /// about screen right, `ry` about screen up and `rz` about the axis toward the viewer.
    /// The three make one rotation (about their sum as a vector), so the result does not
    /// depend on an order. `turn(0.0, a, 0.0)` is the same as `orbit(a / 0.008, 0.0)`.
    pub fn turn(&mut self, rx: f64, ry: f64, rz: f64) {
        let axis = self.right * rx + self.up * ry + self.back * rz;
        let angle = axis.length();
        if !angle.is_finite() || angle <= 1e-12 {
            return;
        }
        // Turning the model one way is turning the camera the other way about the same axis.
        let a = axis * (1.0 / angle);
        self.up = rotate(self.up, a, -angle);
        self.back = rotate(self.back, a, -angle).normalized();
        self.right = self.up.cross(self.back).normalized();
        self.up = self.back.cross(self.right);
    }

    pub fn pixels_per_unit(&self, rect: Rect) -> f64 {
        (rect.height() as f64 / 2.0) / self.half_height
    }

    pub fn pan(&mut self, dx: f64, dy: f64, rect: Rect) {
        let s = 1.0 / self.pixels_per_unit(rect);
        self.target = self.target - self.right * (dx * s) + self.up * (dy * s);
    }

    /// Zooms keeping the model point under the cursor fixed.
    pub fn zoom_at(&mut self, factor: f64, cursor: Pos2, rect: Rect) {
        let before = self.unproject_on_target_plane(cursor, rect);
        self.half_height = (self.half_height * factor).clamp(1e-3, 1e6);
        let after = self.unproject_on_target_plane(cursor, rect);
        self.target += before - after;
    }

    fn unproject_on_target_plane(&self, p: Pos2, rect: Rect) -> Vec3 {
        let s = 1.0 / self.pixels_per_unit(rect);
        let c = rect.center();
        self.target + self.right * ((p.x - c.x) as f64 * s) - self.up * ((p.y - c.y) as f64 * s)
    }

    pub fn project(&self, p: Vec3, rect: Rect) -> Pos2 {
        let d = p - self.target;
        let s = self.pixels_per_unit(rect);
        let c = rect.center();
        pos2(c.x + (d.dot(self.right) * s) as f32, c.y - (d.dot(self.up) * s) as f32)
    }

    /// Picking ray through a screen point: (origin in front of the scene, direction).
    pub fn ray(&self, p: Pos2, rect: Rect) -> (Vec3, Vec3) {
        let on_plane = self.unproject_on_target_plane(p, rect);
        (on_plane + self.back * self.depth, -self.back)
    }

    pub fn fit(&mut self, b: &Aabb, rect: Rect) {
        if b.is_empty() {
            self.target = Vec3::ZERO;
            self.half_height = 60.0;
            return;
        }
        self.target = b.center();
        let r = (b.diagonal() * 0.5).max(1.0);
        let aspect = (rect.width() / rect.height().max(1.0)).max(0.1) as f64;
        self.half_height = r * 1.15 / aspect.min(1.0);
        self.depth = (r * 20.0).max(2000.0);
    }

    /// Column-major model-view-projection matrix for OpenGL.
    pub fn mvp(&self, rect: Rect) -> [f32; 16] {
        let sy = self.half_height;
        let sx = sy * (rect.width() / rect.height().max(1.0)) as f64;
        let r = self.depth;
        let t = self.target;
        let row0 = [self.right.x / sx, self.right.y / sx, self.right.z / sx, -self.right.dot(t) / sx];
        let row1 = [self.up.x / sy, self.up.y / sy, self.up.z / sy, -self.up.dot(t) / sy];
        let row2 = [-self.back.x / r, -self.back.y / r, -self.back.z / r, self.back.dot(t) / r];
        let row3 = [0.0, 0.0, 0.0, 1.0];
        let rows = [row0, row1, row2, row3];
        let mut m = [0f32; 16];
        for c in 0..4 {
            for rr in 0..4 {
                m[c * 4 + rr] = rows[rr][c] as f32;
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn turn_about_up_matches_orbit() {
        let mut a = Camera::default();
        let mut b = Camera::default();
        a.turn(0.0, 0.3, 0.0);
        b.orbit(0.3 / 0.008, 0.0);
        assert!(close(a.right, b.right) && close(a.up, b.up) && close(a.back, b.back));
    }

    #[test]
    fn turn_rotates_the_model_right_handed_about_screen_axes() {
        let q = std::f64::consts::FRAC_PI_2;
        // From the front, a quarter turn about screen up brings the model's +Z face, which
        // was facing the viewer, round to the right: the camera now looks from -X.
        let mut c = Camera::default();
        c.set_view(StdView::Front);
        c.turn(0.0, q, 0.0);
        assert!(close(c.back, -Vec3::X) && close(c.up, Vec3::Y), "{c:?}");
        // A quarter turn about screen right tips the top face toward the viewer.
        let mut c = Camera::default();
        c.set_view(StdView::Front);
        c.turn(q, 0.0, 0.0);
        assert!(close(c.back, Vec3::Y) && close(c.right, Vec3::X), "{c:?}");
        // A quarter turn about the view axis turns the model anticlockwise on screen, so the
        // model's +X now points up the screen.
        let mut c = Camera::default();
        c.set_view(StdView::Front);
        c.turn(0.0, 0.0, q);
        assert!(close(c.up, Vec3::X) && close(c.back, Vec3::Z), "{c:?}");
    }

    #[test]
    fn turn_keeps_the_basis_orthonormal_and_ignores_bad_input() {
        let mut c = Camera::default();
        for i in 0..10_000 {
            let t = i as f64 * 1e-3;
            c.turn(0.01 * t.sin(), 0.013, 0.007 * t.cos());
        }
        assert!((c.right.length() - 1.0).abs() < 1e-12 && (c.up.length() - 1.0).abs() < 1e-12);
        assert!(c.right.dot(c.up).abs() < 1e-12 && c.up.dot(c.back).abs() < 1e-12);
        assert!(close(c.right.cross(c.up), c.back));
        let before = c;
        c.turn(f64::NAN, 0.0, 0.0);
        c.turn(0.0, f64::INFINITY, 0.0);
        c.turn(0.0, 0.0, 0.0);
        assert!(close(c.back, before.back) && close(c.up, before.up));
    }
}
