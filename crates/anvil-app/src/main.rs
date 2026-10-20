//! AnvilCAD desktop application.
//!
//! Usage:
//!   anvilcad [FILE.anvprt | --sample] [--screenshot OUT.png] [--edit-sketch FEATURE_ID]
//!            [--window properties|mass|dfm] [--material NAME]
//!            [--config INIT.lua | --no-config] [--script SCRIPT.lua]
//!
//! The configuration (options, theme, key bindings, user commands) is read from
//! `~/.config/anvilcad/init.lua` (`%APPDATA%\AnvilCAD\init.lua` on Windows) unless
//! `--config` or `--no-config` says otherwise; see docs/SCRIPTING.md.
//!
//! To drive the app from scripts or agents without opening a window, use `anvilcad-agent`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anvil_app::{app, render};
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut open = None;
    let mut screenshot = None;
    let mut edit_sketch = None;
    let mut window = None;
    let mut material = None;
    let mut config: Option<Option<PathBuf>> = Some(None);
    let mut script = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--edit-sketch" => edit_sketch = args.next().and_then(|s| s.parse().ok()),
            "--window" => window = args.next(),
            "--material" => material = args.next(),
            "--config" => config = Some(args.next().map(PathBuf::from)),
            "--no-config" => config = None,
            "--script" => script = args.next().map(PathBuf::from),
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
            let startup = open.is_none();
            let mut app = app::AnvilApp::new(&cc.egui_ctx, open, screenshot, edit_sketch);
            if let Some(path) = config {
                app.load_config(&cc.egui_ctx, path);
            }
            if startup {
                app.open_startup();
            }
            if let Some(m) = &material {
                app.set_material(m);
            }
            if let Some(w) = &window {
                app.show_window(w);
            }
            if let Some(p) = &script {
                match std::fs::read_to_string(p) {
                    Ok(src) => {
                        if let Err(e) = app.run_lua(&cc.egui_ctx, &src, &p.display().to_string()) {
                            eprintln!("anvilcad: {}: {e}", p.display());
                        }
                    }
                    Err(e) => eprintln!("anvilcad: could not read {}: {e}", p.display()),
                }
            }
            Ok(Box::new(app))
        }),
    )
}
