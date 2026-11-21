//! Navigating the view with a 3D mouse (a 3Dconnexion SpaceMouse or compatible).
//!
//! `anvil-3dmouse` reads the device on its own thread and reports its six axes; this module
//! turns them into camera motion each frame. The cap moves the model ("object mode", the
//! default in other CAD software): push the cap right and the model moves right, twist it
//! and the model turns the same way, pull it toward you and the model comes closer (zooms in).
//!
//! Motion is a rate, not a step: a held deflection keeps the view moving at a steady speed,
//! whatever the frame rate, and the view stops when the cap is released.

use anvil_3dmouse::Axes;
use eframe::egui::Rect;

use crate::camera::Camera;

/// At speed 1 and full deflection: how fast the model turns, in radians per second.
pub const TURN_RATE: f64 = std::f64::consts::PI;
/// At speed 1 and full deflection: how fast the model pans, in view heights per second.
pub const PAN_RATE: f64 = 1.0;
/// At speed 1 and full deflection: zooming scales the view by e to this power each second.
pub const ZOOM_RATE: f64 = 1.5;
/// Deflections smaller than this fraction of full scale are ignored, so a cap at rest that
/// does not quite read zero leaves the view still.
pub const DEAD_ZONE: f64 = 0.04;
/// The longest frame step applied at once, in seconds. After a stall (a long rebuild, the
/// window coming back) the view moves one short step rather than jumping.
pub const MAX_STEP: f64 = 0.05;

/// How the axes move the view: the `anvil.opt.mouse_3d_*` options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Scales every rate; 1 is the default.
    pub speed: f64,
    /// Reverses an axis, in [`Axes`] order: tx, ty, tz, rx, ry, rz.
    pub invert: [bool; 6],
}

impl Default for Settings {
    fn default() -> Self {
        Settings { speed: 1.0, invert: [false; 6] }
    }
}

/// The response to one axis: zero inside the dead zone, then rising with the square of the
/// deflection, so small movements are fine and full deflection is fast. Input is clamped to
/// -1..1; anything not finite counts as zero.
pub fn response(x: f64) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    let x = x.clamp(-1.0, 1.0);
    let m = ((x.abs() - DEAD_ZONE) / (1.0 - DEAD_ZONE)).max(0.0);
    (m * m).copysign(x)
}

/// Whether the axes would move the view at all.
pub fn moves(axes: &Axes) -> bool {
    axes.0.iter().any(|&v| response(v) != 0.0)
}

/// Moves the camera by `dt` seconds of the given deflection, in the viewport `rect`.
pub fn step(camera: &mut Camera, axes: &Axes, dt: f64, settings: &Settings, rect: Rect) {
    let dt = if dt.is_finite() { dt.clamp(0.0, MAX_STEP) } else { 0.0 };
    let speed = if settings.speed.is_finite() { settings.speed.max(0.0) } else { 1.0 };
    if dt == 0.0 || speed == 0.0 || rect.height() <= 0.0 {
        return;
    }
    let mut v = [0.0; 6];
    for (i, out) in v.iter_mut().enumerate() {
        let r = response(axes.0[i]);
        *out = if settings.invert[i] { -r } else { r } * speed * dt;
    }
    let [tx, ty, tz, rx, ry, rz] = v;
    if rx != 0.0 || ry != 0.0 || rz != 0.0 {
        camera.turn(rx * TURN_RATE, ry * TURN_RATE, rz * TURN_RATE);
    }
    if tx != 0.0 || ty != 0.0 {
        // `pan` takes a mouse drag in pixels: a drag right moves the model right, a drag
        // down moves it down. One view height is rect.height() pixels.
        let h = rect.height() as f64;
        camera.pan(tx * PAN_RATE * h, -ty * PAN_RATE * h, rect);
    }
    if tz != 0.0 {
        // Pulling the cap toward you brings the model closer: a smaller view.
        camera.zoom_at((-tz * ZOOM_RATE).exp(), rect.center(), rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::StdView;
    use anvil_kernel::{v3, Vec3};
    use eframe::egui::{pos2, Rect};

    fn rect() -> Rect {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0))
    }

    fn front() -> Camera {
        let mut c = Camera::default();
        c.set_view(StdView::Front);
        c.target = Vec3::ZERO;
        c.half_height = 50.0;
        c
    }

    fn axes(v: [f64; 6]) -> Axes {
        Axes(v)
    }

    #[test]
    fn response_has_a_dead_zone_and_a_square_law() {
        assert_eq!(response(0.0), 0.0);
        assert_eq!(response(DEAD_ZONE), 0.0);
        assert_eq!(response(-DEAD_ZONE * 0.5), 0.0);
        assert_eq!(response(1.0), 1.0);
        assert_eq!(response(-1.0), -1.0);
        assert_eq!(response(7.0), 1.0);
        let half = DEAD_ZONE + (1.0 - DEAD_ZONE) * 0.5;
        assert!((response(half) - 0.25).abs() < 1e-12);
        assert!((response(-half) + 0.25).abs() < 1e-12);
        assert_eq!(response(f64::NAN), 0.0);
        assert_eq!(response(f64::INFINITY), 0.0);
    }

    #[test]
    fn full_deflection_pans_one_view_height_per_second() {
        // Cap right for 0.05 s: the model moves right, so the target moves left by
        // 0.05 view heights (100 mm view, so 5 mm).
        let mut c = front();
        step(&mut c, &axes([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]), 0.05, &Settings::default(), rect());
        assert!((c.target - v3(-5.0, 0.0, 0.0)).length() < 1e-9, "{:?}", c.target);
        // Cap up: the model moves up, the target down.
        let mut c = front();
        step(&mut c, &axes([0.0, 1.0, 0.0, 0.0, 0.0, 0.0]), 0.05, &Settings::default(), rect());
        assert!((c.target - v3(0.0, -5.0, 0.0)).length() < 1e-9, "{:?}", c.target);
    }

    #[test]
    fn pulling_the_cap_zooms_in_about_the_view_centre() {
        let mut c = front();
        step(&mut c, &axes([0.0, 0.0, 1.0, 0.0, 0.0, 0.0]), 0.04, &Settings::default(), rect());
        assert!((c.half_height - 50.0 * (-0.04 * ZOOM_RATE).exp()).abs() < 1e-9);
        assert!(c.target.length() < 1e-9);
        let mut c = front();
        step(&mut c, &axes([0.0, 0.0, -1.0, 0.0, 0.0, 0.0]), 0.04, &Settings::default(), rect());
        assert!(c.half_height > 50.0);
    }

    #[test]
    fn twisting_turns_the_model_at_the_turn_rate() {
        // 0.05 s at full twist about screen up turns the model by TURN_RATE * 0.05.
        let mut c = front();
        step(&mut c, &axes([0.0, 0.0, 0.0, 0.0, 1.0, 0.0]), 0.05, &Settings::default(), rect());
        let a = TURN_RATE * 0.05;
        // The model turned +a about up, so the camera looks from a turned -a about up.
        let want = v3(-a.sin(), 0.0, a.cos());
        assert!((c.back - want).length() < 1e-9, "{:?}", c.back);
    }

    #[test]
    fn speed_and_invert_scale_and_reverse() {
        let s = Settings { speed: 2.0, invert: [true, false, false, false, false, false] };
        let mut c = front();
        step(&mut c, &axes([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]), 0.05, &s, rect());
        assert!((c.target - v3(10.0, 0.0, 0.0)).length() < 1e-9, "{:?}", c.target);
    }

    #[test]
    fn long_or_bad_steps_are_bounded() {
        // A two-second stall moves the view by one MAX_STEP, not two seconds' worth.
        let mut a = front();
        let mut b = front();
        step(&mut a, &axes([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]), 2.0, &Settings::default(), rect());
        step(&mut b, &axes([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]), MAX_STEP, &Settings::default(), rect());
        assert!((a.target - b.target).length() < 1e-12);
        let before = front();
        let mut c = front();
        let bad = Settings { speed: f64::NAN, invert: [false; 6] };
        step(&mut c, &axes([0.0; 6]), 0.05, &Settings::default(), rect());
        step(&mut c, &axes([1.0; 6]), f64::NAN, &Settings::default(), rect());
        step(&mut c, &axes([1.0; 6]), -1.0, &Settings::default(), rect());
        step(&mut c, &axes([1.0; 6]), 0.05, &Settings::default(), Rect::NOTHING);
        assert!((c.target - before.target).length() < 1e-12 && c.half_height == before.half_height);
        // A speed that is not a number falls back to 1.
        let mut d = front();
        step(&mut d, &axes([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]), 0.05, &bad, rect());
        assert!((d.target - v3(-5.0, 0.0, 0.0)).length() < 1e-9);
        assert!(!moves(&axes([0.01, -0.02, 0.0, 0.03, 0.0, 0.0])));
        assert!(moves(&axes([0.0, 0.0, 0.0, 0.0, 0.0, 0.5])));
    }
}
