//! A fine square raster of the uncut material in one 2.5D layer.
//!
//! Each cell is material or not, judged at its centre. The adaptive clearing generator
//! keeps one of these as its explicit model of what is still uncut, and the engagement
//! verifier replays a finished tool path into a fresh, finer one. A raster was chosen
//! over polygon booleans because removing a tool disk and asking "how much of this circle
//! touches material" are both constant-time per cell and cannot fail on degenerate
//! geometry, which matters when tens of thousands of small steps are taken; the error is
//! bounded by the cell size and the verifier measures it (see `docs/cam/ADAPTIVE.md`).

use crate::error::{CamError, Result};
use crate::geom::Region;
use anvil_kernel::{Vec2, v2};
use std::f64::consts::TAU;

/// Largest raster accepted (cells), about 64 MB.
pub const MAX_CELLS: usize = 64_000_000;

#[derive(Clone, Debug)]
pub struct Grid {
    /// Lower-left corner of cell (0, 0).
    pub origin: Vec2,
    /// Cell size, mm.
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
    data: Vec<bool>,
}

impl Grid {
    /// An empty grid covering `lo`..`hi` with `margin` around it.
    pub fn new(lo: Vec2, hi: Vec2, cell: f64, margin: f64) -> Result<Grid> {
        crate::error::positive("cell size", cell)?;
        if !(lo.x.is_finite() && lo.y.is_finite() && hi.x.is_finite() && hi.y.is_finite())
            || hi.x < lo.x
            || hi.y < lo.y
        {
            return Err(CamError::InvalidGeometry("raster bounds are not valid".into()));
        }
        let origin = lo - v2(margin, margin);
        let nx = ((hi.x - lo.x + 2.0 * margin) / cell).ceil() as usize + 1;
        let ny = ((hi.y - lo.y + 2.0 * margin) / cell).ceil() as usize + 1;
        let cells = nx.saturating_mul(ny);
        if cells > MAX_CELLS {
            return Err(CamError::LimitExceeded(format!(
                "the material raster would need {cells} cells; use a coarser resolution"
            )));
        }
        Ok(Grid { origin, cell, nx, ny, data: vec![false; cells] })
    }

    pub fn center(&self, i: usize, j: usize) -> Vec2 {
        self.origin + v2((i as f64 + 0.5) * self.cell, (j as f64 + 0.5) * self.cell)
    }

    /// The cell containing `p`, if it is on the grid.
    pub fn cell_of(&self, p: Vec2) -> Option<(usize, usize)> {
        let fx = (p.x - self.origin.x) / self.cell;
        let fy = (p.y - self.origin.y) / self.cell;
        if fx < 0.0 || fy < 0.0 || !fx.is_finite() || !fy.is_finite() {
            return None;
        }
        let (i, j) = (fx as usize, fy as usize);
        (i < self.nx && j < self.ny).then_some((i, j))
    }

    pub(crate) fn get(&self, i: usize, j: usize) -> bool {
        self.data[j * self.nx + i]
    }

    pub(crate) fn set(&mut self, i: usize, j: usize, v: bool) {
        self.data[j * self.nx + i] = v;
    }

    /// Material at point `p` (false off the grid).
    pub fn at(&self, p: Vec2) -> bool {
        self.cell_of(p).is_some_and(|(i, j)| self.get(i, j))
    }

    pub fn count(&self) -> usize {
        self.data.iter().filter(|b| **b).count()
    }

    pub fn cell_area(&self) -> f64 {
        self.cell * self.cell
    }

    /// Sets every cell whose centre is inside `region` (even-odd over all its loops) to
    /// `value`.
    pub fn fill_region(&mut self, region: &Region, value: bool) {
        let segs = region.segments();
        let mut xs: Vec<f64> = Vec::new();
        for j in 0..self.ny {
            let y = self.origin.y + (j as f64 + 0.5) * self.cell;
            xs.clear();
            for (a, b) in &segs {
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks_exact(2) {
                let i0 = ((pair[0] - self.origin.x) / self.cell - 0.5).ceil().max(0.0) as usize;
                let i1f = ((pair[1] - self.origin.x) / self.cell - 0.5).floor();
                if i1f < 0.0 {
                    continue;
                }
                let i1 = (i1f as usize).min(self.nx - 1);
                for i in i0..=i1 {
                    self.set(i, j, value);
                }
            }
        }
    }

    /// Keeps only cells that are set in both grids (which must have the same layout).
    pub fn intersect(&mut self, other: &Grid) {
        for (a, b) in self.data.iter_mut().zip(&other.data) {
            *a = *a && *b;
        }
    }

    /// Range of cell columns in row `j` whose centres are within `r` of `c`.
    fn disk_row(&self, c: Vec2, r: f64, j: usize) -> Option<(usize, usize)> {
        let y = self.origin.y + (j as f64 + 0.5) * self.cell;
        let dy = y - c.y;
        let h2 = r * r - dy * dy;
        if h2 < 0.0 {
            return None;
        }
        let h = h2.sqrt();
        let i0 = ((c.x - h - self.origin.x) / self.cell - 0.5).ceil().max(0.0);
        let i1 = ((c.x + h - self.origin.x) / self.cell - 0.5).floor();
        if i1 < 0.0 || i0 > i1 || i0 >= self.nx as f64 {
            return None;
        }
        Some((i0 as usize, (i1 as usize).min(self.nx - 1)))
    }

    fn rows_of(&self, c: Vec2, r: f64) -> (usize, usize) {
        let j0 = ((c.y - r - self.origin.y) / self.cell - 0.5).ceil().max(0.0) as usize;
        let j1f = ((c.y + r - self.origin.y) / self.cell - 0.5).floor();
        let j1 = if j1f < 0.0 { 0 } else { (j1f as usize).min(self.ny.saturating_sub(1)) };
        (j0, j1)
    }

    /// Clears the disk of radius `r` at `c`; returns the number of cells cleared.
    pub fn clear_disk(&mut self, c: Vec2, r: f64) -> usize {
        let (j0, j1) = self.rows_of(c, r);
        let mut n = 0;
        for j in j0..=j1 {
            if let Some((i0, i1)) = self.disk_row(c, r, j) {
                let row = &mut self.data[j * self.nx + i0..=j * self.nx + i1];
                for v in row {
                    if *v {
                        *v = false;
                        n += 1;
                    }
                }
            }
        }
        n
    }

    /// Column ranges of row `j` inside the disk at `c` but outside the disk at `prev`
    /// (both of radius `r`).
    fn crescent_row(&self, prev: Vec2, c: Vec2, r: f64, j: usize) -> [Option<(usize, usize)>; 2] {
        let Some((i0, i1)) = self.disk_row(c, r, j) else { return [None, None] };
        match self.disk_row(prev, r, j) {
            Some((p0, p1)) if p0 <= i1 && p1 >= i0 => {
                let left = (i0 < p0).then(|| (i0, p0 - 1));
                let right = (i1 > p1).then(|| (p1 + 1, i1));
                [left, right]
            }
            _ => [Some((i0, i1)), None],
        }
    }

    /// Clears the disk of radius `r` at `c`, assuming the disk of the same radius at
    /// `prev` is already clear, so only the crescent between them is visited. Returns
    /// the cells cleared.
    pub fn clear_disk_after(&mut self, prev: Vec2, c: Vec2, r: f64) -> usize {
        let (j0, j1) = self.rows_of(c, r);
        let mut n = 0;
        for j in j0..=j1 {
            for (a, b) in self.crescent_row(prev, c, r, j).into_iter().flatten() {
                for v in &mut self.data[j * self.nx + a..=j * self.nx + b] {
                    if *v {
                        *v = false;
                        n += 1;
                    }
                }
            }
        }
        n
    }

    /// As [`Grid::clear_disk_after`], also appending the index of every cleared cell to
    /// `log` so the removal can be undone.
    pub fn clear_disk_after_logged(&mut self, prev: Vec2, c: Vec2, r: f64, log: &mut Vec<usize>) -> usize {
        let (j0, j1) = self.rows_of(c, r);
        let mut n = 0;
        for j in j0..=j1 {
            for (a, b) in self.crescent_row(prev, c, r, j).into_iter().flatten() {
                for k in j * self.nx + a..=j * self.nx + b {
                    if self.data[k] {
                        self.data[k] = false;
                        log.push(k);
                        n += 1;
                    }
                }
            }
        }
        n
    }

    /// Number of material cells inside the disk of radius `r` at `c`.
    pub fn count_disk(&self, c: Vec2, r: f64) -> usize {
        let (j0, j1) = self.rows_of(c, r);
        let mut n = 0;
        for j in j0..=j1 {
            if let Some((i0, i1)) = self.disk_row(c, r, j) {
                n += self.data[j * self.nx + i0..=j * self.nx + i1].iter().filter(|v| **v).count();
            }
        }
        n
    }

    /// Clears the band swept by a disk of radius `r` moving from `a` to `b` (the union
    /// of the disks along the segment); returns the cells cleared.
    pub fn clear_capsule(&mut self, a: Vec2, b: Vec2, r: f64) -> usize {
        let lo = v2(a.x.min(b.x) - r, a.y.min(b.y) - r);
        let hi = v2(a.x.max(b.x) + r, a.y.max(b.y) + r);
        let Some((i0, j0)) = self.clamp_cell(lo) else { return 0 };
        let Some((i1, j1)) = self.clamp_cell(hi) else { return 0 };
        let ab = b - a;
        let l2 = ab.dot(ab);
        let r2 = r * r;
        let mut n = 0;
        for j in j0..=j1 {
            for i in i0..=i1 {
                let k = j * self.nx + i;
                if !self.data[k] {
                    continue;
                }
                let p = self.center(i, j);
                let t = if l2 < 1e-24 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
                let q = a + ab * t - p;
                if q.dot(q) <= r2 {
                    self.data[k] = false;
                    n += 1;
                }
            }
        }
        n
    }

    fn clamp_cell(&self, p: Vec2) -> Option<(usize, usize)> {
        if self.nx == 0 || self.ny == 0 {
            return None;
        }
        let i = ((p.x - self.origin.x) / self.cell).floor().clamp(0.0, (self.nx - 1) as f64) as usize;
        let j = ((p.y - self.origin.y) / self.cell).floor().clamp(0.0, (self.ny - 1) as f64) as usize;
        Some((i, j))
    }

    /// Engagement angle (radians) of a tool of radius `r` at `c`: the total angle of its
    /// circumference that lies over material, sampled at `samples` points.
    pub fn engagement(&self, c: Vec2, r: f64, samples: usize) -> f64 {
        let n = samples.max(8);
        let mut hits = 0usize;
        for k in 0..n {
            let a = TAU * (k as f64 + 0.5) / n as f64;
            if self.at(c + v2(a.cos(), a.sin()) * r) {
                hits += 1;
            }
        }
        TAU * hits as f64 / n as f64
    }

    /// Like [`Grid::engagement`], but a point of the circumference counts only where the
    /// material there is at least two cells thick outward from the tool: a rim thinner
    /// than the raster can resolve, left along an earlier cut, is not a cut.
    pub fn engagement_solid(&self, c: Vec2, r: f64, samples: usize) -> f64 {
        let n = samples.max(8);
        let mut hits = 0usize;
        for k in 0..n {
            let a = TAU * (k as f64 + 0.5) / n as f64;
            let u = v2(a.cos(), a.sin());
            let p = c + u * r;
            if self.at(p) && self.at(p + u * self.cell) {
                hits += 1;
            }
        }
        TAU * hits as f64 / n as f64
    }

    /// The largest contiguous arc (radians) of the circumference over material, and the
    /// direction of its middle. Contiguity tells a single cut from two separate contacts.
    pub fn engagement_arc(&self, c: Vec2, r: f64, samples: usize) -> (f64, Vec2) {
        let n = samples.max(8);
        let hit: Vec<bool> = (0..n)
            .map(|k| {
                let a = TAU * (k as f64 + 0.5) / n as f64;
                self.at(c + v2(a.cos(), a.sin()) * r)
            })
            .collect();
        if hit.iter().all(|h| *h) {
            return (TAU, Vec2::ZERO);
        }
        let Some(start) = hit.iter().position(|h| !*h) else { return (0.0, Vec2::ZERO) };
        let (mut best, mut best_mid, mut run) = (0usize, 0.0f64, 0usize);
        for s in 1..=n {
            let k = (start + s) % n;
            if hit[k] {
                run += 1;
                if run > best {
                    best = run;
                    best_mid = (start + s) as f64 - (run as f64 - 1.0) * 0.5;
                }
            } else {
                run = 0;
            }
        }
        let a = TAU * (best_mid + 0.5) / n as f64;
        (TAU * best as f64 / n as f64, Vec2::from_angle(a))
    }

    /// Iterates over set cells.
    pub fn set_cells(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.data.iter().enumerate().filter(|(_, v)| **v).map(move |(k, _)| (k % self.nx, k / self.nx))
    }
}

/// Exact Euclidean distance transform (Felzenszwalb and Huttenlocher, "Distance
/// Transforms of Sampled Functions", Theory of Computing 8, 2012). Returns, for each
/// cell, the distance in cells from its centre to the nearest centre of a cell where
/// `feature` is true (infinity when there is none).
pub fn distance_transform(feature: &[bool], nx: usize, ny: usize) -> Vec<f64> {
    const INF: f64 = 1e20;
    let mut f: Vec<f64> = feature.iter().map(|b| if *b { 0.0 } else { INF }).collect();
    let mut buf_in = vec![0.0; nx.max(ny)];
    let mut buf_out = vec![0.0; nx.max(ny)];
    // Columns.
    for i in 0..nx {
        for j in 0..ny {
            buf_in[j] = f[j * nx + i];
        }
        dt_1d(&buf_in[..ny], &mut buf_out[..ny]);
        for j in 0..ny {
            f[j * nx + i] = buf_out[j];
        }
    }
    // Rows.
    for j in 0..ny {
        buf_in[..nx].copy_from_slice(&f[j * nx..(j + 1) * nx]);
        dt_1d(&buf_in[..nx], &mut buf_out[..nx]);
        f[j * nx..(j + 1) * nx].copy_from_slice(&buf_out[..nx]);
    }
    f.into_iter().map(|d| if d >= INF * 0.5 { f64::INFINITY } else { d.sqrt() }).collect()
}

/// One-dimensional squared distance transform by the lower envelope of parabolas.
fn dt_1d(f: &[f64], d: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut k = 0usize;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * q as f64 - 2.0 * p as f64);
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                // k == 0: replace the first parabola.
                v[0] = q;
                z[0] = f64::NEG_INFINITY;
                z[1] = f64::INFINITY;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f64::INFINITY;
            break;
        }
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f64 - p as f64;
        *out = dq * dq + f[p];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_and_clear() {
        let r = Region::rectangle(v2(0.0, 0.0), v2(10.0, 10.0)).unwrap();
        let mut g = Grid::new(v2(0.0, 0.0), v2(10.0, 10.0), 0.1, 1.0).unwrap();
        g.fill_region(&r, true);
        assert_eq!(g.count(), 100 * 100);
        let n = g.clear_disk(v2(5.0, 5.0), 2.0);
        let area = n as f64 * g.cell_area();
        assert!((area - std::f64::consts::PI * 4.0).abs() < 0.1, "{area}");
        assert_eq!(g.count_disk(v2(5.0, 5.0), 1.9), 0);
    }

    #[test]
    fn straight_cut_engagement() {
        // Material at x > 4; a tool of radius 5 at the origin overlaps 1 mm, so
        // engagement is 2 acos(4/5) on the full circumference.
        let r = Region::rectangle(v2(4.0, -20.0), v2(30.0, 20.0)).unwrap();
        let mut g = Grid::new(v2(-10.0, -20.0), v2(30.0, 20.0), 0.02, 0.0).unwrap();
        g.fill_region(&r, true);
        let e = g.engagement(v2(0.0, 0.0), 5.0, 3600);
        let want = 2.0 * (0.8f64).acos();
        assert!((e - want).abs() < 0.01, "{e} vs {want}");
        let (arc, dir) = g.engagement_arc(v2(0.0, 0.0), 5.0, 3600);
        assert!((arc - want).abs() < 0.01);
        assert!(dir.x > 0.99);
    }

    #[test]
    fn distance_transform_matches_brute_force() {
        let (nx, ny) = (23, 17);
        let mut f = vec![false; nx * ny];
        for &(i, j) in &[(3usize, 4usize), (20, 1), (11, 15), (0, 16)] {
            f[j * nx + i] = true;
        }
        let d = distance_transform(&f, nx, ny);
        for j in 0..ny {
            for i in 0..nx {
                let mut best = f64::INFINITY;
                for jj in 0..ny {
                    for ii in 0..nx {
                        if f[jj * nx + ii] {
                            let dd = ((i as f64 - ii as f64).powi(2) + (j as f64 - jj as f64).powi(2)).sqrt();
                            best = best.min(dd);
                        }
                    }
                }
                assert!((d[j * nx + i] - best).abs() < 1e-9, "({i},{j}) {} vs {best}", d[j * nx + i]);
            }
        }
    }
}
