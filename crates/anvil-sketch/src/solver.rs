//! Geometric constraint solver.
//!
//! Unknowns are every point coordinate plus each circle radius. Each constraint contributes
//! one or two residual equations. We minimise the residuals with a damped Gauss-Newton
//! iteration in its minimum-norm form, dx = -J^T (J J^T + mu I)^-1 r, which moves the
//! geometry as little as possible. That keeps under-defined sketches stable while the user
//! drags them, the same way commercial sketchers behave.

use crate::sketch::{Constraint, Geom, PointId, Sketch};
use anvil_kernel::{v2, Vec2};

const TOL: f64 = 1e-9;
const MAX_ITERS: usize = 200;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolveResult {
    pub converged: bool,
    pub iterations: usize,
    pub max_residual: f64,
    /// Remaining degrees of freedom. Zero means the sketch is fully defined.
    pub dof: usize,
    /// Indices of constraints that could not be satisfied.
    pub unsatisfied: Vec<usize>,
}

impl SolveResult {
    pub fn status_text(&self) -> String {
        if !self.converged {
            format!("Over defined / unsolvable ({} conflicting)", self.unsatisfied.len())
        } else if self.dof == 0 {
            "Fully defined".to_string()
        } else {
            format!("Under defined ({} DOF)", self.dof)
        }
    }
}

struct Layout {
    n_points: usize,
    radius_var: Vec<Option<usize>>,
    n: usize,
}

impl Sketch {
    fn layout(&self) -> Layout {
        let n_points = self.points.len();
        let mut n = 2 * n_points;
        let radius_var = self
            .entities
            .iter()
            .map(|e| match e.geom {
                Geom::Circle { .. } => {
                    n += 1;
                    Some(n - 1)
                }
                _ => None,
            })
            .collect();
        Layout { n_points, radius_var, n }
    }

    fn gather(&self, l: &Layout) -> Vec<f64> {
        let mut x = vec![0.0; l.n];
        for (i, p) in self.points.iter().enumerate() {
            x[2 * i] = p.x;
            x[2 * i + 1] = p.y;
        }
        for (e, rv) in l.radius_var.iter().enumerate() {
            if let (Some(v), Geom::Circle { r, .. }) = (rv, self.entities[e].geom) {
                x[*v] = r;
            }
        }
        x
    }

    fn scatter(&mut self, l: &Layout, x: &[f64]) {
        for i in 0..l.n_points {
            self.points[i].x = x[2 * i];
            self.points[i].y = x[2 * i + 1];
        }
        for (e, rv) in l.radius_var.iter().enumerate() {
            if let (Some(v), Geom::Circle { r, .. }) = (rv, &mut self.entities[e].geom) {
                *r = x[*v];
            }
        }
    }

    /// Evaluates all residuals. `owner[i]` is the constraint index of row i
    /// (`usize::MAX` for implicit arc equations).
    fn residuals(&self, l: &Layout, x: &[f64], out: &mut Vec<f64>, owner: &mut Vec<usize>) {
        out.clear();
        owner.clear();
        let p = |i: PointId| v2(x[2 * i], x[2 * i + 1]);
        let line = |e: usize| -> (Vec2, Vec2) {
            match self.entities[e].geom {
                Geom::Line { a, b } => (p(a), p(b)),
                _ => (Vec2::ZERO, Vec2::ZERO),
            }
        };
        let center = |e: usize| -> Vec2 {
            match self.entities[e].geom {
                Geom::Circle { c, .. } | Geom::Arc { c, .. } => p(c),
                Geom::Line { a, b } => (p(a) + p(b)) * 0.5,
            }
        };
        let radius = |e: usize| -> f64 {
            match self.entities[e].geom {
                Geom::Circle { .. } => x[l.radius_var[e].unwrap()],
                Geom::Arc { c, a, .. } => p(a).distance(p(c)),
                Geom::Line { a, b } => p(a).distance(p(b)),
            }
        };
        let size = |e: usize| -> f64 {
            match self.entities[e].geom {
                Geom::Line { a, b } => p(a).distance(p(b)),
                _ => radius(e),
            }
        };
        let dir = |e: usize| {
            let (a, b) = line(e);
            b - a
        };
        let norm_cross = |u: Vec2, v: Vec2| u.cross(v) / (u.length() * v.length()).max(1e-12);
        let norm_dot = |u: Vec2, v: Vec2| u.dot(v) / (u.length() * v.length()).max(1e-12);

        for (ci, c) in self.constraints.iter().enumerate() {
            let mut push = |v: f64| {
                out.push(v);
                owner.push(ci);
            };
            use Constraint::*;
            match *c {
                Fix { p: i, x: fx, y: fy } => {
                    push(p(i).x - fx);
                    push(p(i).y - fy);
                }
                Coincident { p: a, q: b } => {
                    push(p(a).x - p(b).x);
                    push(p(a).y - p(b).y);
                }
                Horizontal { line: e } => {
                    let (a, b) = line(e);
                    push(a.y - b.y);
                }
                Vertical { line: e } => {
                    let (a, b) = line(e);
                    push(a.x - b.x);
                }
                HorizontalPoints { p: a, q: b } => push(p(a).y - p(b).y),
                VerticalPoints { p: a, q: b } => push(p(a).x - p(b).x),
                Distance { p: a, q: b, value } => push(p(a).distance(p(b)) - value),
                HDistance { p: a, q: b, value } => push(p(b).x - p(a).x - value),
                VDistance { p: a, q: b, value } => push(p(b).y - p(a).y - value),
                Length { line: e, value } => push(size(e) - value),
                PointLineDistance { p: i, line: e, value } => {
                    let (a, b) = line(e);
                    let d = b - a;
                    push((d.cross(p(i) - a) / d.length().max(1e-12)).abs() - value);
                }
                Radius { entity, value } => push(radius(entity) - value),
                Diameter { entity, value } => push(2.0 * radius(entity) - value),
                Parallel { a, b } => push(norm_cross(dir(a), dir(b))),
                Perpendicular { a, b } => push(norm_dot(dir(a), dir(b))),
                Equal { a, b } => push(size(a) - size(b)),
                Angle { a, b, degrees } => {
                    let (u, v) = (dir(a), dir(b));
                    let ang = u.cross(v).atan2(u.dot(v));
                    let d = ang - degrees.to_radians();
                    push((d + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI);
                }
                PointOnLine { p: i, line: e } => {
                    let (a, b) = line(e);
                    let d = b - a;
                    push(d.cross(p(i) - a) / d.length().max(1e-12));
                }
                PointOnCircle { p: i, entity } => push(p(i).distance(center(entity)) - radius(entity)),
                Midpoint { p: i, line: e } => {
                    let (a, b) = line(e);
                    let m = (a + b) * 0.5;
                    push(p(i).x - m.x);
                    push(p(i).y - m.y);
                }
                Tangent { line: e, entity } => {
                    let (a, b) = line(e);
                    let d = b - a;
                    let dist = (d.cross(center(entity) - a) / d.length().max(1e-12)).abs();
                    push(dist - radius(entity));
                }
                Concentric { a, b } => {
                    let (ca, cb) = (center(a), center(b));
                    push(ca.x - cb.x);
                    push(ca.y - cb.y);
                }
            }
        }
        // Arcs keep both endpoints on the same radius.
        for e in &self.entities {
            if let Geom::Arc { c, a, b } = e.geom {
                out.push(p(a).distance(p(c)) - p(b).distance(p(c)));
                owner.push(usize::MAX);
            }
        }
    }

    fn jacobian(&self, l: &Layout, x: &[f64], m: usize) -> Vec<f64> {
        let n = l.n;
        let mut jac = vec![0.0; m * n];
        let mut xp = x.to_vec();
        let (mut rp, mut rm, mut ow) = (Vec::new(), Vec::new(), Vec::new());
        for j in 0..n {
            let h = 1e-7 * x[j].abs().max(1.0);
            xp[j] = x[j] + h;
            self.residuals(l, &xp, &mut rp, &mut ow);
            xp[j] = x[j] - h;
            self.residuals(l, &xp, &mut rm, &mut ow);
            xp[j] = x[j];
            for i in 0..m {
                jac[i * n + j] = (rp[i] - rm[i]) / (2.0 * h);
            }
        }
        jac
    }

    /// Solves the sketch in place.
    pub fn solve(&mut self) -> SolveResult {
        let l = self.layout();
        let mut x = self.gather(&l);
        let (mut r, mut owner) = (Vec::new(), Vec::new());
        self.residuals(&l, &x, &mut r, &mut owner);
        let m = r.len();
        let n = l.n;
        let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
        let max_abs = |r: &[f64]| r.iter().fold(0.0f64, |a, v| a.max(v.abs()));

        let mut c0 = cost(&r);
        let mut mu = 1e-9;
        let mut iters = 0;
        let mut r_new = Vec::new();
        let mut ow = Vec::new();
        while iters < MAX_ITERS && max_abs(&r) > TOL && m > 0 {
            iters += 1;
            let jac = self.jacobian(&l, &x, m);
            // A = J J^T + mu I
            let mut a = vec![0.0; m * m];
            for i in 0..m {
                for k in i..m {
                    let mut s = 0.0;
                    for j in 0..n {
                        s += jac[i * n + j] * jac[k * n + j];
                    }
                    a[i * m + k] = s;
                    a[k * m + i] = s;
                }
                a[i * m + i] += mu;
            }
            let Some(y) = solve_dense(&mut a, &r, m) else {
                mu *= 10.0;
                continue;
            };
            let mut xn = x.clone();
            for j in 0..n {
                let mut s = 0.0;
                for i in 0..m {
                    s += jac[i * n + j] * y[i];
                }
                xn[j] -= s;
            }
            self.residuals(&l, &xn, &mut r_new, &mut ow);
            let c1 = cost(&r_new);
            if c1 < c0 {
                x = xn;
                std::mem::swap(&mut r, &mut r_new);
                c0 = c1;
                mu = (mu * 0.1).max(1e-12);
            } else {
                mu *= 10.0;
                if mu > 1e10 {
                    break;
                }
            }
        }

        let converged = max_abs(&r) <= 1e-7;
        let jac = self.jacobian(&l, &x, m);
        let rank = matrix_rank(&jac, m, n);
        let mut unsatisfied: Vec<usize> = r
            .iter()
            .zip(&owner)
            .filter(|(v, &o)| v.abs() > 1e-7 && o != usize::MAX)
            .map(|(_, &o)| o)
            .collect();
        unsatisfied.dedup();
        self.scatter(&l, &x);
        SolveResult {
            converged,
            iterations: iters,
            max_residual: max_abs(&r),
            dof: n.saturating_sub(rank),
            unsatisfied,
        }
    }

    /// Moves a point toward `target` while keeping constraints satisfied.
    /// The sketch is left unchanged if no valid configuration is found.
    pub fn drag_point(&mut self, p: PointId, target: Vec2) -> SolveResult {
        let backup = self.clone();
        self.points[p].x = target.x;
        self.points[p].y = target.y;
        let ci = self.add_constraint(Constraint::Fix { p, x: target.x, y: target.y });
        let res = self.solve();
        self.constraints.remove(ci);
        if res.converged {
            return self.solve();
        }
        // The target is unreachable; settle for the nearest valid configuration.
        let mut res = self.solve();
        if !res.converged {
            *self = backup;
            res = self.solve();
        }
        res
    }
}

/// Solves `a x = b` with partial pivoting. `a` is overwritten.
fn solve_dense(a: &mut [f64], b: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut x = b.to_vec();
    for col in 0..n {
        let piv =
            (col..n).max_by(|&i, &j| a[i * n + col].abs().partial_cmp(&a[j * n + col].abs()).unwrap())?;
        if a[piv * n + col].abs() < 1e-300 {
            return None;
        }
        if piv != col {
            for k in 0..n {
                a.swap(piv * n + k, col * n + k);
            }
            x.swap(piv, col);
        }
        let d = a[col * n + col];
        for i in col + 1..n {
            let f = a[i * n + col] / d;
            if f != 0.0 {
                for k in col..n {
                    a[i * n + k] -= f * a[col * n + k];
                }
                x[i] -= f * x[col];
            }
        }
    }
    for i in (0..n).rev() {
        let mut s = x[i];
        for k in i + 1..n {
            s -= a[i * n + k] * x[k];
        }
        x[i] = s / a[i * n + i];
    }
    Some(x)
}

/// Numerical rank by Gaussian elimination with full pivoting.
fn matrix_rank(jac: &[f64], m: usize, n: usize) -> usize {
    let mut a = jac.to_vec();
    let scale = a.iter().fold(0.0f64, |s, v| s.max(v.abs())).max(1.0);
    let tol = 1e-8 * scale;
    let mut rows: Vec<usize> = (0..m).collect();
    let mut rank = 0;
    let mut used_cols = vec![false; n];
    for _ in 0..m.min(n) {
        // Find the largest remaining pivot.
        let mut best = (0.0, 0, 0);
        for (ri, &r) in rows.iter().enumerate() {
            for c in 0..n {
                if !used_cols[c] && a[r * n + c].abs() > best.0 {
                    best = (a[r * n + c].abs(), ri, c);
                }
            }
        }
        if best.0 <= tol {
            break;
        }
        let (_, ri, c) = best;
        let pr = rows.swap_remove(ri);
        used_cols[c] = true;
        rank += 1;
        let pv = a[pr * n + c];
        for &r in &rows {
            let f = a[r * n + c] / pv;
            if f != 0.0 {
                for k in 0..n {
                    a[r * n + k] -= f * a[pr * n + k];
                }
            }
        }
    }
    rank
}
