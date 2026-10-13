//! AnvilCAD desktop application.
//!
//! Usage:
//!   anvilcad [FILE.anvprt | --sample] [--screenshot OUT.png] [--edit-sketch FEATURE_ID]
//!            [--window properties|mass|dfm] [--material NAME]

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
    let mut window = None;
    let mut material = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--edit-sketch" => edit_sketch = args.next().and_then(|s| s.parse().ok()),
            "--window" => window = args.next(),
            "--material" => material = args.next(),
            _ => open = Some(PathBuf::from(a)),
        }
    }

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
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
        Box::new(move |cc| {
            if let Some(rs) = cc.wgpu_render_state.as_ref() {
                // Must match the depth buffer and multisampling requested above.
                render::install(rs, 4, eframe::egui_wgpu::depth_format_from_bits(24, 0));
            }
            let mut app = app::AnvilApp::new(&cc.egui_ctx, open, screenshot, edit_sketch);
            if let Some(m) = &material {
                app.set_material(m);
            }
            if let Some(w) = &window {
                app.show_window(w);
            }
            Ok(Box::new(app))
        }),
    )
}
