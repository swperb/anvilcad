//! Mass properties from a closed triangle mesh.

use crate::tess::TriMesh;
use glam::DVec3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassProperties {
    pub volume: f64,
    pub area: f64,
    pub centroid: DVec3,
}

pub fn properties(mesh: &TriMesh) -> MassProperties {
    let mut volume = 0.0;
    let mut area = 0.0;
    let mut moment = DVec3::ZERO;
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
        let v = a.dot(b.cross(c)) / 6.0;
        volume += v;
        moment += (a + b + c) * (v / 4.0);
        area += 0.5 * (b - a).cross(c - a).length();
    }
    let centroid = if volume.abs() > 0.0 { moment / volume } else { DVec3::ZERO };
    MassProperties { volume, area, centroid }
}
