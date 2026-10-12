//! AnvilCAD desktop application.
//!
//! Usage:
//!   anvilcad [FILE.anvprt | --sample] [--screenshot OUT.png] [--edit-sketch FEATURE_ID]

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod camera;
mod render;
mod sample;
mod sketcher;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut open = None;
    let mut screenshot = None;
    let mut edit_sketch = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--edit-sketch" => edit_sketch = args.next().and_then(|s| s.parse().ok()),
            _ => open = Some(PathBuf::from(a)),
        }
    }

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        depth_buffer: 24,
        multisampling: 4,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("AnvilCAD")
            .with_inner_size([1400.0, 880.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AnvilCAD",
        options,
        Box::new(move |cc| Ok(Box::new(app::AnvilApp::new(&cc.egui_ctx, open, screenshot, edit_sketch)))),
    )
}
