//! AnvilCAD geometry kernel.
//!
//! Layers, bottom to top:
//! - [`math`]: vectors, frames, bounding boxes
//! - [`profile`]: exact 2D loops of lines and arcs
//! - [`triangulate`]: polygon-with-holes triangulation
//! - [`solid`]: boundary representation, mass properties, picking, tessellation
//! - [`csg`]: boolean union / subtract / intersect
//! - [`build`]: extrude and revolve

pub mod blend;
pub mod build;
pub mod csg;
pub mod math;
pub mod profile;
pub mod shell;
pub mod solid;
pub mod triangulate;

pub use math::{v2, v3, Aabb, Frame, Vec2, Vec3};
pub use profile::{Loop2, Profile, Seg2};
pub use solid::{EdgeRef, FaceId, FaceTag, MassProperties, Mesh, ModelEdge, Polygon, Solid, SurfaceKind};
