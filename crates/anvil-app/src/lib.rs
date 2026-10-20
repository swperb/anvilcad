//! AnvilCAD desktop application, as a library so the `anvilcad` window and the
//! `anvilcad-agent` headless driver share one implementation.

pub mod app;
mod camera;
pub mod headless;
pub mod keymap;
pub mod render;
mod sample;
pub mod script;
mod sketcher;
pub mod theme;
