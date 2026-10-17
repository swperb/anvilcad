//! AnvilCAD CAM: 2.5D milling from features to checked G-code.
//!
//! - [`features`] recognises holes and pockets on a part body; [`job`] turns them into
//!   an operation list (facing, [`adaptive`] clearing, [`profile`] finishing, [`drill`]
//!   cycles) with tools and cutting data from [`tool`], and builds a setup into one
//!   machine-independent [`toolpath`].
//! - [`post`] writes it for a machine from a declarative post definition; [`verify`]
//!   checks the program text against the post's rules; [`sim`] simulates it on the
//!   stock; [`safety`] calls it safe only when both pass.
//! - [`registry`] supports shared posts (hashes, pins, edit review); the optional `ai`
//!   module proposes and reviews post edits, advisory only.
//!
//! Design notes, sources and the patents checked are in `docs/cam/`.

pub mod adaptive;
#[cfg(feature = "ai")]
pub mod ai;
pub mod drill;
pub mod error;
pub mod facing;
pub mod features;
pub mod geom;
pub mod job;
pub mod post;
pub mod profile;
pub mod raster;
pub mod registry;
pub mod safety;
pub mod sim;
pub mod tool;
pub mod toolpath;
pub mod verify;

pub use error::{CamError, Result};
