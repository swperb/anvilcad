//! AnvilCAD's look: colours, the egui style, icons and the few custom widgets the main
//! window is built from.
//!
//! The layout follows the window arrangement CAD users know (menus, a tabbed command bar,
//! the feature tree on the left, a view toolbar in the viewport); see
//! `docs/re/user-interface.md`. The look is AnvilCAD's own: flat warm greys, the iron-oxide
//! accent from the logo, steel-blue selection, and line icons drawn here.
//!
//! Every custom widget reports the same accessibility label and role as the stock egui
//! widget it replaces, so the headless harness and its scripts find it by the same label.

use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Margin, Pos2, Rect, Response, Sense, Shadow, Stroke,
    StrokeKind, TextStyle, Ui, Vec2, WidgetInfo, WidgetType,
};

/// The colours of one theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// Behind the command bar and status bar.
    pub chrome: Color32,
    /// The left pane and windows.
    pub panel: Color32,
    /// Text fields and other inset surfaces.
    pub inset: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    /// Hover fill for flat buttons and rows.
    pub hover: Color32,
    /// Iron oxide: primary actions and AnvilCAD's mark.
    pub accent: Color32,
    pub on_accent: Color32,
    /// Selection fill and outline.
    pub select_fill: Color32,
    pub select: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub ok: Color32,
    /// Viewport background, top and bottom.
    pub view_top: Color32,
    pub view_bottom: Color32,
    /// The floating view toolbar.
    pub float_fill: Color32,
}

impl Palette {
    pub fn light() -> Palette {
        Palette {
            dark: false,
            chrome: Color32::from_rgb(0xf1, 0xef, 0xec),
            panel: Color32::from_rgb(0xfa, 0xf9, 0xf7),
            inset: Color32::from_rgb(0xff, 0xff, 0xff),
            border: Color32::from_rgb(0xdd, 0xd8, 0xd0),
            text: Color32::from_rgb(0x1f, 0x22, 0x26),
            text_weak: Color32::from_rgb(0x5f, 0x64, 0x6a),
            hover: Color32::from_rgb(0xe8, 0xe4, 0xde),
            accent: Color32::from_rgb(0xb4, 0x4a, 0x1d),
            on_accent: Color32::WHITE,
            select_fill: Color32::from_rgb(0xdb, 0xe7, 0xf3),
            select: Color32::from_rgb(0x2f, 0x6d, 0xb0),
            error: Color32::from_rgb(0xc0, 0x2b, 0x2b),
            warning: Color32::from_rgb(0xb0, 0x6a, 0x00),
            ok: Color32::from_rgb(0x2e, 0x8b, 0x57),
            view_top: Color32::from_rgb(0xdc, 0xe2, 0xe9),
            view_bottom: Color32::from_rgb(0xf8, 0xf8, 0xf7),
            float_fill: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0xe6),
        }
    }

    pub fn dark() -> Palette {
        Palette {
            dark: true,
            chrome: Color32::from_rgb(0x1c, 0x1e, 0x21),
            panel: Color32::from_rgb(0x22, 0x25, 0x28),
            inset: Color32::from_rgb(0x16, 0x18, 0x1a),
            border: Color32::from_rgb(0x36, 0x3a, 0x3f),
            text: Color32::from_rgb(0xe6, 0xe3, 0xde),
            text_weak: Color32::from_rgb(0x9a, 0x9f, 0xa5),
            hover: Color32::from_rgb(0x30, 0x34, 0x38),
            accent: Color32::from_rgb(0xe3, 0x82, 0x5a),
            on_accent: Color32::from_rgb(0x1b, 0x12, 0x0d),
            select_fill: Color32::from_rgb(0x24, 0x3b, 0x55),
            select: Color32::from_rgb(0x6c, 0xa6, 0xe0),
            error: Color32::from_rgb(0xf0, 0x6a, 0x6a),
            warning: Color32::from_rgb(0xe8, 0xa8, 0x3a),
            ok: Color32::from_rgb(0x5c, 0xc0, 0x86),
            view_top: Color32::from_rgb(0x34, 0x39, 0x3f),
            view_bottom: Color32::from_rgb(0x1a, 0x1c, 0x1f),
            float_fill: Color32::from_rgba_unmultiplied(0x26, 0x29, 0x2d, 0xe6),
        }
    }

    /// The palette for the theme `ui` is drawn in, with the user's colour overrides.
    pub fn of(ui: &Ui) -> Palette {
        Palette::for_ctx(ui.ctx(), ui.visuals().dark_mode)
    }

    /// The light or dark palette with the colour overrides set on `ctx` (`anvil.theme`).
    pub fn for_ctx(ctx: &egui::Context, dark: bool) -> Palette {
        let mut p = Palette::for_dark(dark);
        if let Some(o) = overrides(ctx) {
            for (name, c) in if dark { &o.dark } else { &o.light } {
                p.set(name, *c);
            }
        }
        p
    }

    /// Sets a colour by its field name; false if there is no such colour.
    pub fn set(&mut self, name: &str, c: Color32) -> bool {
        let slot = match name {
            "chrome" => &mut self.chrome,
            "panel" => &mut self.panel,
            "inset" => &mut self.inset,
            "border" => &mut self.border,
            "text" => &mut self.text,
            "text_weak" => &mut self.text_weak,
            "hover" => &mut self.hover,
            "accent" => &mut self.accent,
            "on_accent" => &mut self.on_accent,
            "select_fill" => &mut self.select_fill,
            "select" => &mut self.select,
            "error" => &mut self.error,
            "warning" => &mut self.warning,
            "ok" => &mut self.ok,
            "view_top" => &mut self.view_top,
            "view_bottom" => &mut self.view_bottom,
            "float_fill" => &mut self.float_fill,
            _ => return false,
        };
        *slot = c;
        true
    }

    pub fn for_dark(dark: bool) -> Palette {
        if dark {
            Palette::dark()
        } else {
            Palette::light()
        }
    }
}

fn overrides_id() -> egui::Id {
    egui::Id::new("anvilcad-theme-overrides")
}

/// Sets the user's colour overrides on `ctx` (kept per context, so each window, and each
/// headless session, has its own). Call [`apply`] afterwards to restyle.
pub fn set_overrides(ctx: &egui::Context, o: crate::script::ThemeOverrides) {
    ctx.data_mut(|d| d.insert_temp(overrides_id(), std::sync::Arc::new(o)));
}

fn overrides(ctx: &egui::Context) -> Option<std::sync::Arc<crate::script::ThemeOverrides>> {
    ctx.data(|d| d.get_temp(overrides_id()))
}

/// The model's colours in the viewport, with the user's overrides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneColors {
    pub body: [f32; 3],
    pub hover: [f32; 3],
    pub select: [f32; 3],
    pub edge: [f32; 3],
}

impl Default for SceneColors {
    fn default() -> SceneColors {
        use crate::render::{BODY_COLOR, EDGE_COLOR, HOVER_COLOR, SELECT_COLOR};
        SceneColors { body: BODY_COLOR, hover: HOVER_COLOR, select: SELECT_COLOR, edge: EDGE_COLOR }
    }
}

pub fn scene_colors(ctx: &egui::Context) -> SceneColors {
    let mut c = SceneColors::default();
    if let Some(o) = overrides(ctx) {
        for (name, rgb) in &o.model {
            match name.as_str() {
                "body" => c.body = *rgb,
                "hover_face" => c.hover = *rgb,
                "selected_face" => c.select = *rgb,
                "edge" => c.edge = *rgb,
                _ => {}
            }
        }
    }
    c
}

/// Sets egui's style to AnvilCAD's light or dark theme.
pub fn apply(ctx: &egui::Context, dark: bool) {
    let p = Palette::for_ctx(ctx, dark);
    let mut style = (*ctx.style()).clone();
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };

    let r6 = CornerRadius::same(6);
    v.override_text_color = Some(p.text);
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 36 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 100 } else { 30 }),
    };
    v.extreme_bg_color = p.inset;
    v.faint_bg_color = p.chrome;
    v.code_bg_color = p.chrome;
    v.hyperlink_color = p.select;
    v.error_fg_color = p.error;
    v.warn_fg_color = p.warning;
    v.selection.bg_fill = p.select_fill;
    v.selection.stroke = Stroke::new(1.0_f32, p.select);
    v.striped = false;
    v.slider_trailing_fill = true;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.noninteractive.corner_radius = r6;

    w.inactive.bg_fill = p.inset;
    w.inactive.weak_bg_fill = p.inset;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.inactive.corner_radius = r6;

    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.text_weak);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.hovered.corner_radius = r6;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.select_fill;
    w.active.weak_bg_fill = p.select_fill;
    w.active.bg_stroke = Stroke::new(1.0_f32, p.select);
    w.active.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.active.corner_radius = r6;
    w.active.expansion = 0.0;

    w.open.bg_fill = p.hover;
    w.open.weak_bg_fill = p.hover;
    w.open.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.open.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.open.corner_radius = r6;

    style.visuals = v;

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 6.0);
    s.button_padding = egui::vec2(10.0, 4.0);
    s.interact_size = egui::vec2(40.0, 24.0);
    s.menu_margin = Margin::same(6);
    s.window_margin = Margin::same(14);
    s.indent = 16.0;
    s.combo_width = 140.0;
    s.text_edit_width = 160.0;

    style.text_styles = [
        (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(17.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
    ]
    .into();
    ctx.set_style(style);
}

// ----- icons ------------------------------------------------------------------------------

/// The icons AnvilCAD draws. Each is a line drawing on a 16 x 16 grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Part,
    Material,
    Plane,
    Origin,
    Sketch,
    Extrude,
    ExtrudeCut,
    Revolve,
    RevolveCut,
    LinearPattern,
    CircularPattern,
    Fillet,
    Chamfer,
    Shell,
    Hole,
    MirrorFeature,
    Sweep,
    Draft,
    Dome,
    Loft,
    Rib,
    Bodies,
    Section,
    Convert,
    Axis,
    RefPoint,
    CoordSys,
    Measure,
    Imported,
    Select,
    Line,
    Centerline,
    Rectangle,
    Circle,
    Arc,
    Point,
    Dimension,
    ExitSketch,
    Mass,
    Check,
    Equations,
    Properties,
    /// A configuration: two stacked variants of a part.
    Configuration,
    /// An assembly: two blocks, one on the other.
    Assembly,
    /// Inserting a component: a block and a plus sign.
    InsertComponent,
    /// A mate: two faces brought together.
    Mate,
    ViewFront,
    ViewTop,
    ViewRight,
    ViewIso,
    Fit,
    Edges,
    Undo,
    Redo,
    Rebuild,
    // Sketch entity and editing tools.
    CenterRectangle,
    Rectangle3Corner,
    Rectangle3Center,
    Parallelogram,
    StraightSlot,
    CenterSlot,
    ArcSlot3,
    CenterArcSlot,
    Polygon,
    PerimeterCircle,
    TangentArc,
    Arc3,
    SketchFillet,
    SketchChamfer,
    Trim,
    Extend,
    Split,
    Offset,
    Mirror,
    SketchLinearPattern,
    SketchCircularPattern,
    Move,
    Copy,
    Rotate,
    Scale,
    Construction,
}

/// Paints `icon` filling the square `rect`: `ink` for lines, `accent` for the highlight.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, ink: Color32, accent: Color32) {
    let s = rect.width().min(rect.height()) / 16.0;
    let o = rect.center() - Vec2::splat(8.0 * s);
    let p = |x: f32, y: f32| o + Vec2::new(x, y) * s;
    let w = (1.35 * s).max(1.0);
    let ln = Stroke::new(w, ink);
    let hi = Stroke::new(w, accent);
    let tint = accent.gamma_multiply(0.28);
    let line =
        |a: (f32, f32), b: (f32, f32), st: Stroke| painter.line_segment([p(a.0, a.1), p(b.0, b.1)], st);
    let poly = |pts: &[(f32, f32)], fill: Color32, st: Stroke| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| p(x, y)).collect();
        painter.add(egui::Shape::convex_polygon(v, fill, st));
    };
    let path = |pts: &[(f32, f32)], st: Stroke| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| p(x, y)).collect();
        painter.add(egui::Shape::line(v, st));
    };
    let arc = |c: (f32, f32), r: f32, a0: f32, a1: f32, st: Stroke| {
        let n = 20;
        let v: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                p(c.0 + r * a.cos(), c.1 - r * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(v, st));
    };
    let ellipse = |c: (f32, f32), rx: f32, ry: f32, st: Stroke| {
        let n = 28;
        let v: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / n as f32;
                p(c.0 + rx * a.cos(), c.1 + ry * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(v, st));
    };
    let dot = |x: f32, y: f32, c: Color32| painter.circle_filled(p(x, y), 1.3 * s, c);
    // A thin dashed line, as construction geometry is drawn.
    let dashed = |a: (f32, f32), b: (f32, f32)| {
        let thin = Stroke::new(w * 0.7, ink.gamma_multiply(0.6));
        painter.extend(egui::Shape::dashed_line(&[p(a.0, a.1), p(b.0, b.1)], thin, 2.0 * s, 1.5 * s));
    };
    // An isometric box; `face` picks the face filled with the accent tint (0 none, 1 front,
    // 2 top, 3 right, 4 all).
    let cube = |face: u8| {
        let front = [(2.5, 6.5), (10.0, 6.5), (10.0, 14.0), (2.5, 14.0)];
        let top = [(2.5, 6.5), (6.0, 3.0), (13.5, 3.0), (10.0, 6.5)];
        let right = [(10.0, 6.5), (13.5, 3.0), (13.5, 10.5), (10.0, 14.0)];
        let f = |n: u8| if face == n || face == 4 { tint } else { Color32::TRANSPARENT };
        poly(&front, f(1), ln);
        poly(&top, f(2), ln);
        poly(&right, f(3), ln);
    };

    match icon {
        Icon::Part => cube(2),
        Icon::Material => {
            poly(&[(8.0, 2.0), (14.0, 8.0), (8.0, 14.0), (2.0, 8.0)], tint, ln);
            line((5.0, 5.0), (11.0, 11.0), hi);
            line((6.5, 3.5), (12.5, 9.5), hi);
            line((3.5, 6.5), (9.5, 12.5), hi);
        }
        Icon::Plane => poly(&[(1.5, 12.0), (5.5, 4.0), (14.5, 4.0), (10.5, 12.0)], tint, ln),
        Icon::Origin => {
            line((4.0, 12.0), (14.0, 12.0), ln);
            line((4.0, 12.0), (4.0, 2.0), ln);
            line((4.0, 12.0), (1.0, 15.0), ln);
            painter.circle_filled(p(4.0, 12.0), 1.8 * s, accent);
        }
        Icon::Sketch => {
            poly(&[(2.0, 4.0), (11.0, 4.0), (11.0, 13.0), (2.0, 13.0)], Color32::TRANSPARENT, ln);
            poly(&[(8.0, 11.0), (14.0, 5.0), (15.5, 6.5), (9.5, 12.5), (7.5, 13.0)], accent, Stroke::NONE);
        }
        Icon::Extrude => {
            cube(2);
            line((8.0, 1.0), (8.0, 4.5), hi);
            path(&[(6.5, 2.5), (8.0, 1.0), (9.5, 2.5)], hi);
        }
        Icon::ExtrudeCut => {
            cube(0);
            ellipse((8.0, 4.75), 2.6, 1.0, hi);
            line((5.4, 4.75), (5.4, 9.0), hi);
            line((10.6, 4.75), (10.6, 9.0), hi);
        }
        Icon::Revolve | Icon::RevolveCut => {
            let cut = icon == Icon::RevolveCut;
            ellipse((8.0, 12.0), 6.0, 2.2, ln);
            ellipse((8.0, 4.0), 4.0, 1.5, if cut { hi } else { ln });
            line((2.0, 12.0), (4.0, 4.0), ln);
            line((14.0, 12.0), (12.0, 4.0), ln);
            for y in [1.0, 4.5, 8.0, 11.5] {
                line((8.0, y), (8.0, y + 2.0), hi);
            }
        }
        Icon::LinearPattern => {
            for (i, x) in [1.5_f32, 6.0, 10.5].into_iter().enumerate() {
                let f = if i == 0 { tint } else { Color32::TRANSPARENT };
                poly(
                    &[(x, 7.0), (x + 4.0, 7.0), (x + 4.0, 11.0), (x, 11.0)],
                    f,
                    if i == 0 { hi } else { ln },
                );
            }
            line((1.5, 13.8), (14.5, 13.8), ln);
            path(&[(12.5, 12.3), (14.5, 13.8), (12.5, 15.3)], ln);
        }
        Icon::CircularPattern => {
            painter.circle_stroke(p(8.0, 8.0), 5.5 * s, Stroke::new(w * 0.7, ink.gamma_multiply(0.5)));
            for i in 0..6 {
                let a = (i as f32 * 60.0 + 90.0).to_radians();
                let c = p(8.0 + 5.5 * a.cos(), 8.0 - 5.5 * a.sin());
                if i == 0 {
                    painter.circle(c, 1.9 * s, tint, hi);
                } else {
                    painter.circle(c, 1.6 * s, Color32::TRANSPARENT, ln);
                }
            }
        }
        Icon::Fillet => {
            path(&[(2.0, 14.0), (2.0, 2.0), (14.0, 2.0)], ln);
            path(&[(2.0, 14.0), (8.0, 14.0)], ln);
            path(&[(14.0, 2.0), (14.0, 8.0)], ln);
            arc((8.0, 8.0), 6.0, 270.0, 360.0, hi);
        }
        Icon::Chamfer => {
            path(&[(2.0, 14.0), (2.0, 2.0), (14.0, 2.0)], ln);
            path(&[(2.0, 14.0), (8.0, 14.0)], ln);
            path(&[(14.0, 2.0), (14.0, 8.0)], ln);
            line((8.0, 14.0), (14.0, 8.0), hi);
        }
        Icon::Shell => {
            path(&[(2.0, 3.0), (2.0, 14.0), (14.0, 14.0), (14.0, 3.0)], ln);
            path(&[(4.5, 3.0), (4.5, 11.5), (11.5, 11.5), (11.5, 3.0)], hi);
            line((2.0, 3.0), (4.5, 3.0), ln);
            line((11.5, 3.0), (14.0, 3.0), ln);
        }
        Icon::Hole => {
            // A counterbored hole in section.
            path(&[(1.5, 3.0), (4.0, 3.0), (4.0, 7.0), (6.0, 7.0), (6.0, 14.0)], ln);
            path(&[(14.5, 3.0), (12.0, 3.0), (12.0, 7.0), (10.0, 7.0), (10.0, 14.0)], ln);
            line((1.5, 14.0), (6.0, 14.0), ln);
            line((10.0, 14.0), (14.5, 14.0), ln);
            line((8.0, 1.5), (8.0, 15.0), hi);
        }
        Icon::MirrorFeature => {
            // A shape and its reflection either side of a dashed plane.
            poly(&[(2.0, 4.0), (6.5, 4.0), (6.5, 12.0), (4.0, 12.0)], tint, ln);
            poly(&[(9.5, 4.0), (14.0, 4.0), (12.0, 12.0), (9.5, 12.0)], Color32::TRANSPARENT, ln);
            for i in 0..4 {
                let y = 1.5 + i as f32 * 3.6;
                line((8.0, y), (8.0, y + 2.0), hi);
            }
        }
        Icon::Sweep => {
            // A round section carried along a bent path.
            path(&[(3.0, 13.0), (3.0, 8.0), (4.5, 5.0), (8.0, 3.5), (13.5, 3.5)], hi);
            arc((3.0, 13.0), 2.0, 0.0, 360.0, ln);
            path(&[(1.0, 13.0), (1.0, 8.0), (2.8, 4.0), (8.0, 1.5), (13.5, 1.5)], ln);
            path(&[(5.0, 13.0), (5.0, 8.0), (6.0, 6.2), (8.0, 5.5), (13.5, 5.5)], ln);
        }
        Icon::Draft => {
            // A block whose sides lean in towards the top, over its neutral plane.
            poly(&[(3.0, 13.0), (13.0, 13.0), (11.0, 4.0), (5.0, 4.0)], tint, ln);
            line((1.0, 13.0), (15.0, 13.0), hi);
        }
        Icon::Dome => {
            // A cylinder with a rounded cap.
            arc((8.0, 8.0), 5.0, 0.0, 180.0, hi);
            path(&[(3.0, 8.0), (3.0, 14.0), (13.0, 14.0), (13.0, 8.0)], ln);
        }
        Icon::Loft => {
            // A wide square joined by straight rulings to a small one above it.
            poly(&[(1.5, 14.0), (14.5, 14.0), (11.5, 11.0), (4.5, 11.0)], tint, ln);
            poly(&[(5.5, 3.0), (10.5, 3.0), (10.5, 6.0), (5.5, 6.0)], Color32::TRANSPARENT, ln);
            line((1.5, 14.0), (5.5, 6.0), hi);
            line((14.5, 14.0), (10.5, 6.0), hi);
        }
        Icon::Rib => {
            // A thin web standing between two walls.
            path(&[(1.5, 2.0), (1.5, 14.0), (14.5, 14.0), (14.5, 2.0)], ln);
            poly(&[(1.5, 6.5), (14.5, 6.5), (14.5, 8.5), (1.5, 8.5)], tint, hi);
        }
        Icon::Bodies => {
            // Two overlapping blocks.
            poly(&[(1.5, 6.0), (9.5, 6.0), (9.5, 14.0), (1.5, 14.0)], tint, ln);
            poly(&[(6.5, 2.0), (14.5, 2.0), (14.5, 10.0), (6.5, 10.0)], Color32::TRANSPARENT, hi);
        }
        Icon::Section => {
            // A block cut open, with its cap shaded.
            path(
                &[(2.0, 6.0), (8.0, 3.0), (14.0, 6.0), (14.0, 12.0), (8.0, 15.0), (2.0, 12.0), (2.0, 6.0)],
                ln,
            );
            poly(&[(8.0, 3.0), (14.0, 6.0), (14.0, 12.0), (8.0, 9.0)], tint, hi);
        }
        Icon::Convert => {
            // A face's edge carried down into the sketch.
            poly(&[(3.0, 2.0), (13.0, 2.0), (13.0, 7.0), (3.0, 7.0)], tint, ln);
            path(&[(3.0, 10.0), (13.0, 10.0), (13.0, 14.5), (3.0, 14.5), (3.0, 10.0)], hi);
            line((8.0, 7.0), (8.0, 9.0), ln);
        }
        Icon::Axis => {
            // A dashed line through a ring.
            for i in 0..5 {
                let y = 1.0 + i as f32 * 3.0;
                line((8.0, y), (8.0, y + 1.8), hi);
            }
            arc((8.0, 8.0), 5.0, 0.0, 360.0, ln);
        }
        Icon::RefPoint => {
            line((3.0, 8.0), (13.0, 8.0), ln);
            line((8.0, 3.0), (8.0, 13.0), ln);
            arc((8.0, 8.0), 2.2, 0.0, 360.0, hi);
        }
        Icon::CoordSys => {
            // Three axes from an origin.
            line((5.0, 11.0), (14.0, 11.0), hi);
            line((5.0, 11.0), (5.0, 2.0), ln);
            line((5.0, 11.0), (1.5, 14.5), ln);
        }
        Icon::Measure => {
            // A rule with ticks.
            poly(&[(1.5, 9.0), (14.5, 9.0), (14.5, 13.0), (1.5, 13.0)], tint, ln);
            for i in 0..6 {
                let x = 3.0 + i as f32 * 2.0;
                line((x, 9.0), (x, if i % 2 == 0 { 11.5 } else { 10.5 }), ln);
            }
            line((1.5, 5.0), (14.5, 5.0), hi);
            line((1.5, 3.5), (1.5, 6.5), hi);
            line((14.5, 3.5), (14.5, 6.5), hi);
        }
        Icon::Imported => {
            cube(1);
            line((6.25, 0.5), (6.25, 4.0), hi);
            path(&[(4.75, 2.5), (6.25, 4.0), (7.75, 2.5)], hi);
        }
        Icon::Select => {
            poly(&[(4.0, 2.0), (12.5, 10.0), (8.5, 10.5), (6.5, 14.5), (4.0, 2.0)], tint, ln);
        }
        Icon::Line => {
            line((3.0, 13.0), (13.0, 3.0), ln);
            dot(3.0, 13.0, accent);
            dot(13.0, 3.0, accent);
        }
        Icon::Centerline => {
            for i in 0..4 {
                let t = i as f32 * 2.8;
                line((2.5 + t, 13.5 - t), (4.3 + t, 11.7 - t), ln);
            }
            dot(2.5, 13.5, accent);
            dot(13.5, 2.5, accent);
        }
        Icon::Rectangle => {
            poly(&[(2.5, 4.0), (13.5, 4.0), (13.5, 12.0), (2.5, 12.0)], Color32::TRANSPARENT, ln);
            dot(2.5, 4.0, accent);
            dot(13.5, 12.0, accent);
        }
        Icon::Circle => {
            painter.circle_stroke(p(8.0, 8.0), 5.8 * s, ln);
            dot(8.0, 8.0, accent);
        }
        Icon::Point => {
            line((3.0, 8.0), (13.0, 8.0), hi);
            line((8.0, 3.0), (8.0, 13.0), hi);
            painter.circle_filled(p(8.0, 8.0), 2.2 * s, ink);
        }
        Icon::Arc => {
            arc((8.0, 11.0), 6.0, 0.0, 180.0, ln);
            dot(8.0, 11.0, accent);
            dot(2.0, 11.0, accent);
            dot(14.0, 11.0, accent);
        }
        Icon::Dimension => {
            line((2.0, 3.0), (2.0, 13.0), ln);
            line((14.0, 3.0), (14.0, 13.0), ln);
            line((2.5, 8.0), (13.5, 8.0), hi);
            path(&[(4.5, 6.3), (2.5, 8.0), (4.5, 9.7)], hi);
            path(&[(11.5, 6.3), (13.5, 8.0), (11.5, 9.7)], hi);
        }
        Icon::ExitSketch => {
            poly(&[(2.0, 4.0), (11.0, 4.0), (11.0, 13.0), (2.0, 13.0)], Color32::TRANSPARENT, ln);
            path(&[(6.0, 8.5), (9.0, 11.5), (15.0, 4.0)], Stroke::new(w * 1.4, accent));
        }
        Icon::Mass => {
            painter.circle_stroke(p(8.0, 4.0), 2.0 * s, ln);
            poly(&[(4.0, 6.5), (12.0, 6.5), (14.0, 14.0), (2.0, 14.0)], tint, ln);
        }
        Icon::Check => {
            painter.circle_stroke(p(7.0, 7.0), 4.8 * s, ln);
            line((10.5, 10.5), (14.5, 14.5), Stroke::new(w * 1.4, ink));
            path(&[(4.8, 7.2), (6.5, 8.9), (9.4, 5.4)], hi);
        }
        Icon::Equations => {
            path(&[(12.0, 3.0), (3.5, 3.0), (8.5, 8.0), (3.5, 13.0), (12.0, 13.0)], ln);
            line((12.0, 3.0), (12.0, 4.5), ln);
            line((12.0, 13.0), (12.0, 11.5), ln);
            dot(13.0, 8.0, accent);
        }
        Icon::Properties => {
            poly(&[(3.0, 2.0), (13.0, 2.0), (13.0, 14.0), (3.0, 14.0)], Color32::TRANSPARENT, ln);
            for y in [5.5, 8.5, 11.5] {
                dot(5.5, y, accent);
                line((7.5, y), (11.0, y), ln);
            }
        }
        Icon::Configuration => {
            poly(&[(5.0, 2.0), (14.0, 2.0), (14.0, 9.0), (5.0, 9.0)], Color32::TRANSPARENT, ln);
            poly(&[(2.0, 6.0), (11.0, 6.0), (11.0, 14.0), (2.0, 14.0)], tint, hi);
        }
        Icon::Assembly => {
            poly(&[(1.5, 9.0), (9.5, 9.0), (9.5, 14.5), (1.5, 14.5)], tint, ln);
            poly(&[(6.5, 1.5), (14.5, 1.5), (14.5, 7.0), (6.5, 7.0)], Color32::TRANSPARENT, hi);
            line((6.5, 9.0), (9.5, 7.0), ln);
        }
        Icon::InsertComponent => {
            poly(&[(1.5, 6.0), (10.0, 6.0), (10.0, 14.5), (1.5, 14.5)], tint, ln);
            line((12.5, 1.0), (12.5, 7.0), hi);
            line((9.5, 4.0), (15.5, 4.0), hi);
        }
        Icon::Mate => {
            poly(&[(1.5, 2.0), (6.5, 2.0), (6.5, 14.0), (1.5, 14.0)], tint, ln);
            poly(&[(9.5, 2.0), (14.5, 2.0), (14.5, 14.0), (9.5, 14.0)], Color32::TRANSPARENT, ln);
            line((7.0, 8.0), (9.0, 8.0), hi);
            path(&[(7.8, 6.5), (9.0, 8.0), (7.8, 9.5)], hi);
        }
        Icon::ViewFront => cube(1),
        Icon::ViewTop => cube(2),
        Icon::ViewRight => cube(3),
        Icon::ViewIso => cube(4),
        Icon::Fit => {
            for (cx, cy, dx, dy) in [
                (2.0, 2.0, 1.0, 1.0),
                (14.0, 2.0, -1.0, 1.0),
                (2.0, 14.0, 1.0, -1.0),
                (14.0, 14.0, -1.0, -1.0),
            ] {
                path(&[(cx + 4.0 * dx, cy), (cx, cy), (cx, cy + 4.0 * dy)], ln);
            }
            poly(&[(6.0, 6.0), (10.0, 6.0), (10.0, 10.0), (6.0, 10.0)], tint, hi);
        }
        Icon::Edges => {
            let thick = Stroke::new(w * 1.5, ink);
            poly(&[(2.5, 6.5), (10.0, 6.5), (10.0, 14.0), (2.5, 14.0)], tint, thick);
            poly(&[(2.5, 6.5), (6.0, 3.0), (13.5, 3.0), (10.0, 6.5)], tint, thick);
            poly(&[(10.0, 6.5), (13.5, 3.0), (13.5, 10.5), (10.0, 14.0)], tint, thick);
        }
        Icon::Undo | Icon::Redo => {
            let m = |x: f32| if icon == Icon::Undo { x } else { 16.0 - x };
            let n = 16;
            let v: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let a = (90.0 - 180.0 * i as f32 / n as f32).to_radians();
                    p(m(8.5 + 4.5 * a.cos()), 8.5 - 4.5 * a.sin())
                })
                .collect();
            painter.add(egui::Shape::line(v, ln));
            line((m(8.5), 4.0), (m(3.5), 4.0), ln);
            path(&[(m(5.5), 1.8), (m(3.3), 4.0), (m(5.5), 6.2)], ln);
            line((m(8.5), 13.0), (m(5.0), 13.0), ln);
        }
        Icon::Rebuild => {
            arc((8.0, 8.0), 5.5, 30.0, 300.0, ln);
            let tip = p(8.0 + 5.5 * 30f32.to_radians().cos(), 8.0 - 5.5 * 30f32.to_radians().sin());
            painter.add(egui::Shape::convex_polygon(
                vec![
                    tip + Vec2::new(-3.2, -1.2) * s,
                    tip + Vec2::new(1.6, -3.4) * s,
                    tip + Vec2::new(1.2, 1.8) * s,
                ],
                accent,
                Stroke::NONE,
            ));
        }
        Icon::CenterRectangle => {
            poly(&[(2.0, 4.0), (14.0, 4.0), (14.0, 12.0), (2.0, 12.0)], Color32::TRANSPARENT, ln);
            dashed((2.0, 4.0), (14.0, 12.0));
            dashed((14.0, 4.0), (2.0, 12.0));
            dot(8.0, 8.0, accent);
            dot(14.0, 12.0, accent);
        }
        Icon::Rectangle3Corner => {
            poly(&[(2.0, 10.0), (9.0, 3.0), (14.0, 8.0), (7.0, 15.0)], Color32::TRANSPARENT, ln);
            dot(2.0, 10.0, accent);
            dot(9.0, 3.0, accent);
            dot(14.0, 8.0, accent);
        }
        Icon::Rectangle3Center => {
            poly(&[(2.0, 10.0), (9.0, 3.0), (14.0, 8.0), (7.0, 15.0)], Color32::TRANSPARENT, ln);
            dashed((2.0, 10.0), (14.0, 8.0));
            dot(8.0, 9.0, accent);
            dot(11.5, 5.5, accent);
        }
        Icon::Parallelogram => {
            poly(&[(1.5, 12.0), (10.0, 12.0), (14.5, 4.0), (6.0, 4.0)], Color32::TRANSPARENT, ln);
            dot(1.5, 12.0, accent);
            dot(10.0, 12.0, accent);
            dot(14.5, 4.0, accent);
        }
        Icon::StraightSlot | Icon::CenterSlot => {
            line((5.0, 5.0), (11.0, 5.0), ln);
            line((5.0, 11.0), (11.0, 11.0), ln);
            arc((5.0, 8.0), 3.0, 90.0, 270.0, ln);
            arc((11.0, 8.0), 3.0, -90.0, 90.0, ln);
            if icon == Icon::StraightSlot {
                dot(5.0, 8.0, accent);
                dot(11.0, 8.0, accent);
            } else {
                dashed((5.0, 8.0), (11.0, 8.0));
                dot(8.0, 8.0, accent);
            }
        }
        Icon::ArcSlot3 | Icon::CenterArcSlot => {
            arc((8.0, 13.0), 9.5, 30.0, 150.0, ln);
            arc((8.0, 13.0), 5.5, 30.0, 150.0, ln);
            let end = |deg: f32| {
                let a = deg.to_radians();
                (8.0 + 7.5 * a.cos(), 13.0 - 7.5 * a.sin())
            };
            let (r, l) = (end(30.0), end(150.0));
            arc(r, 2.0, 210.0, 390.0, ln);
            arc(l, 2.0, 150.0, 330.0, ln);
            if icon == Icon::ArcSlot3 {
                dot(r.0, r.1, accent);
                dot(l.0, l.1, accent);
                dot(8.0, 5.5, accent);
            } else {
                dot(8.0, 13.0, accent);
            }
        }
        Icon::Polygon => {
            let v: Vec<(f32, f32)> = (0..6)
                .map(|k| {
                    let a = (k as f32 * 60.0).to_radians();
                    (8.0 + 6.2 * a.cos(), 8.0 - 6.2 * a.sin())
                })
                .collect();
            poly(&v, Color32::TRANSPARENT, ln);
            painter.circle_stroke(p(8.0, 8.0), 5.4 * s, Stroke::new(w * 0.6, ink.gamma_multiply(0.45)));
            dot(8.0, 8.0, accent);
        }
        Icon::PerimeterCircle => {
            painter.circle_stroke(p(8.0, 8.0), 5.8 * s, ln);
            for deg in [100.0_f32, 220.0, 340.0] {
                let a = deg.to_radians();
                dot(8.0 + 5.8 * a.cos(), 8.0 - 5.8 * a.sin(), accent);
            }
        }
        Icon::TangentArc => {
            line((1.5, 12.0), (8.0, 12.0), ln);
            arc((8.0, 7.0), 5.0, -90.0, 90.0, hi);
            dot(8.0, 12.0, accent);
        }
        Icon::Arc3 => {
            arc((8.0, 12.0), 6.0, 0.0, 180.0, ln);
            dot(2.0, 12.0, accent);
            dot(14.0, 12.0, accent);
            dot(8.0, 6.0, accent);
        }
        Icon::SketchFillet => {
            path(&[(3.0, 14.0), (3.0, 9.0)], ln);
            path(&[(8.0, 4.0), (14.0, 4.0)], ln);
            arc((8.0, 9.0), 5.0, 90.0, 180.0, hi);
            dashed((3.0, 9.0), (3.0, 4.0));
            dashed((3.0, 4.0), (8.0, 4.0));
        }
        Icon::SketchChamfer => {
            path(&[(3.0, 14.0), (3.0, 9.0)], ln);
            path(&[(8.0, 4.0), (14.0, 4.0)], ln);
            line((3.0, 9.0), (8.0, 4.0), hi);
            dashed((3.0, 9.0), (3.0, 4.0));
            dashed((3.0, 4.0), (8.0, 4.0));
        }
        Icon::Trim => {
            line((8.0, 2.0), (8.0, 14.0), ln);
            line((2.0, 8.0), (8.0, 8.0), ln);
            dashed((8.0, 8.0), (14.5, 8.0));
            line((10.5, 5.5), (13.5, 10.5), hi);
            line((10.5, 10.5), (13.5, 5.5), hi);
        }
        Icon::Extend => {
            line((13.5, 2.0), (13.5, 14.0), ln);
            line((2.0, 8.0), (7.0, 8.0), ln);
            line((7.0, 8.0), (12.5, 8.0), hi);
            path(&[(10.5, 6.3), (12.5, 8.0), (10.5, 9.7)], hi);
        }
        Icon::Split => {
            line((2.0, 11.0), (7.0, 7.0), ln);
            line((9.0, 5.5), (14.0, 1.5), ln);
            dot(8.0, 6.2, accent);
            line((6.0, 4.0), (10.0, 8.5), Stroke::new(w * 0.7, accent));
        }
        Icon::Offset => {
            path(&[(2.0, 14.0), (2.0, 6.0)], ln);
            arc((6.0, 6.0), 4.0, 90.0, 180.0, ln);
            path(&[(6.0, 2.0), (14.0, 2.0)], ln);
            path(&[(5.0, 14.0), (5.0, 7.0)], hi);
            arc((7.0, 7.0), 2.0, 90.0, 180.0, hi);
            path(&[(7.0, 5.0), (14.0, 5.0)], hi);
        }
        Icon::Mirror => {
            dashed((8.0, 1.5), (8.0, 14.5));
            poly(&[(2.0, 12.0), (6.0, 12.0), (6.0, 4.0)], Color32::TRANSPARENT, ln);
            poly(&[(14.0, 12.0), (10.0, 4.0), (10.0, 12.0)], tint, hi);
        }
        Icon::SketchLinearPattern => {
            for (i, x) in [3.0_f32, 8.0, 13.0].into_iter().enumerate() {
                painter.circle_stroke(p(x, 7.0), 1.9 * s, if i == 0 { hi } else { ln });
            }
            line((1.5, 12.5), (14.5, 12.5), ln);
            path(&[(12.5, 11.0), (14.5, 12.5), (12.5, 14.0)], ln);
        }
        Icon::SketchCircularPattern => {
            arc((8.0, 8.0), 5.5, 0.0, 300.0, Stroke::new(w * 0.6, ink.gamma_multiply(0.5)));
            for i in 0..5 {
                let a = (i as f32 * 72.0 + 90.0).to_radians();
                let c = p(8.0 + 5.5 * a.cos(), 8.0 - 5.5 * a.sin());
                painter.circle_stroke(c, 1.5 * s, if i == 0 { hi } else { ln });
            }
            dot(8.0, 8.0, accent);
        }
        Icon::Move => {
            line((8.0, 2.0), (8.0, 14.0), ln);
            line((2.0, 8.0), (14.0, 8.0), ln);
            path(&[(6.3, 3.8), (8.0, 2.0), (9.7, 3.8)], hi);
            path(&[(6.3, 12.2), (8.0, 14.0), (9.7, 12.2)], hi);
            path(&[(3.8, 6.3), (2.0, 8.0), (3.8, 9.7)], hi);
            path(&[(12.2, 6.3), (14.0, 8.0), (12.2, 9.7)], hi);
        }
        Icon::Copy => {
            poly(&[(2.0, 6.0), (10.0, 6.0), (10.0, 14.0), (2.0, 14.0)], Color32::TRANSPARENT, ln);
            poly(&[(6.0, 2.0), (14.0, 2.0), (14.0, 10.0), (6.0, 10.0)], tint, hi);
        }
        Icon::Rotate => {
            poly(&[(5.0, 6.0), (11.0, 6.0), (11.0, 12.0), (5.0, 12.0)], Color32::TRANSPARENT, ln);
            arc((8.0, 9.0), 6.5, 20.0, 160.0, hi);
            let a = 20f32.to_radians();
            let tip = (8.0 + 6.5 * a.cos(), 9.0 - 6.5 * a.sin());
            path(&[(tip.0 - 2.2, tip.1 - 0.6), tip, (tip.0 + 0.4, tip.1 - 2.3)], hi);
        }
        Icon::Scale => {
            poly(&[(2.0, 9.0), (7.0, 9.0), (7.0, 14.0), (2.0, 14.0)], tint, hi);
            poly(&[(2.0, 2.0), (14.0, 2.0), (14.0, 14.0), (2.0, 14.0)], Color32::TRANSPARENT, ln);
            line((7.0, 9.0), (12.0, 4.0), ln);
            path(&[(9.5, 4.0), (12.0, 4.0), (12.0, 6.5)], ln);
        }
        Icon::Construction => {
            dashed((2.5, 13.5), (13.5, 2.5));
            dot(2.5, 13.5, accent);
            dot(13.5, 2.5, accent);
        }
    }
}

// ----- widgets ----------------------------------------------------------------------------

fn button_info(resp: &Response, label: &str, selected: Option<bool>) {
    let enabled = resp.enabled();
    match selected {
        Some(sel) => {
            resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, enabled, sel, label))
        }
        None => resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label)),
    }
}

/// A command-bar button: an icon with its name under it. `selected` is `Some` for tools
/// that stay on (sketch tools), and the widget then reports itself as a toggle.
pub fn command_button(ui: &mut Ui, icon: Icon, label: &str, selected: Option<bool>) -> Response {
    let p = Palette::of(ui);
    let font = FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, p.text);
    let width = (galley.size().x + 14.0).max(52.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 54.0), Sense::click());
    button_info(&resp, label, selected);
    if ui.is_rect_visible(rect) {
        let on = selected == Some(true);
        let fill = if on || resp.is_pointer_button_down_on() {
            p.select_fill
        } else if resp.hovered() {
            p.hover
        } else {
            Color32::TRANSPARENT
        };
        let painter = ui.painter();
        painter.rect(rect, CornerRadius::same(7), fill, Stroke::NONE, StrokeKind::Inside);
        if on {
            painter.rect_stroke(
                rect,
                CornerRadius::same(7),
                Stroke::new(1.0_f32, p.select),
                StrokeKind::Inside,
            );
        }
        let ink = if resp.enabled() { p.text } else { p.text_weak };
        let icon_rect =
            Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 18.0), Vec2::splat(22.0));
        paint_icon(painter, icon_rect, icon, ink, p.accent);
        painter.galley(Pos2::new(rect.center().x - galley.size().x / 2.0, rect.bottom() - 18.0), galley, ink);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A compact icon-only button with a tooltip, for the view toolbar and header.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, tip: &str, selected: Option<bool>) -> Response {
    let p = Palette::of(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    button_info(&resp, label, selected);
    if ui.is_rect_visible(rect) {
        let fill = if selected == Some(true) {
            p.select_fill
        } else if resp.hovered() {
            p.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
        let ink = if resp.enabled() { p.text } else { p.text_weak.gamma_multiply(0.6) };
        paint_icon(ui.painter(), rect.shrink(6.0), icon, ink, p.accent);
    }
    resp.on_hover_text(tip)
}

/// A group of command buttons with a small caption painted under them. The caption is
/// painted, not a widget, so it never clashes with a command's label in the harness.
pub fn command_group(ui: &mut Ui, caption: &str, add: impl FnOnce(&mut Ui)) {
    let p = Palette::of(ui);
    let r = ui
        .vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
            ui.horizontal(add);
            ui.add_space(14.0);
        })
        .response
        .rect;
    ui.painter().text(
        Pos2::new(r.center().x, r.bottom() - 6.0),
        Align2::CENTER_CENTER,
        caption.to_uppercase(),
        FontId::proportional(9.5),
        p.text_weak,
    );
}

/// A thin vertical rule between command groups.
pub fn group_rule(ui: &mut Ui) {
    let p = Palette::of(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, 62.0), Sense::hover());
    ui.painter().line_segment(
        [Pos2::new(rect.center().x, rect.top() + 6.0), Pos2::new(rect.center().x, rect.bottom() - 6.0)],
        Stroke::new(1.0_f32, p.border),
    );
}

/// A command-bar tab: text with an accent underline when it is the open tab.
pub fn tab(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let p = Palette::of(ui);
    let font = FontId::proportional(13.0);
    let color = if selected { p.text } else { p.text_weak };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 22.0, 30.0), Sense::click());
    button_info(&resp, label, Some(selected));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if resp.hovered() && !selected {
            painter.rect_filled(rect.shrink2(Vec2::new(0.0, 3.0)), CornerRadius::same(6), p.hover);
        }
        let color = if resp.hovered() { p.text } else { color };
        painter.galley(rect.center() - galley.size() / 2.0, galley, color);
        if selected {
            let bar = Rect::from_min_max(
                Pos2::new(rect.left() + 8.0, rect.bottom() - 2.5),
                Pos2::new(rect.right() - 8.0, rect.bottom()),
            );
            painter.rect_filled(bar, CornerRadius::same(2), p.accent);
        }
    }
    resp
}

/// How a feature-tree row is drawn.
#[derive(Clone, Copy, Default)]
pub struct RowLook {
    pub selected: bool,
    /// Suppressed or rolled back.
    pub dimmed: bool,
    pub error: bool,
    /// Extra indent, in levels.
    pub indent: u8,
}

/// A full-width row in the feature tree: an icon and a name. It reports itself as a
/// selectable label named `label`.
pub fn tree_row(ui: &mut Ui, icon: Icon, label: &str, look: RowLook) -> Response {
    let p = Palette::of(ui);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::click());
    button_info(&resp, label, Some(look.selected));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if look.selected {
            painter.rect_filled(rect, CornerRadius::same(5), p.select_fill);
            let bar = Rect::from_min_size(
                rect.left_top() + Vec2::new(0.0, 4.0),
                Vec2::new(3.0, rect.height() - 8.0),
            );
            painter.rect_filled(bar, CornerRadius::same(2), p.select);
        } else if resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(5), p.hover);
        }
        let x = rect.left() + 8.0 + look.indent as f32 * 14.0;
        let (ink, accent) = if look.error {
            (p.error, p.error)
        } else if look.dimmed {
            (p.text_weak.gamma_multiply(0.7), p.text_weak.gamma_multiply(0.7))
        } else {
            (p.text_weak, p.accent)
        };
        paint_icon(
            painter,
            Rect::from_min_size(Pos2::new(x, rect.center().y - 8.0), Vec2::splat(16.0)),
            icon,
            ink,
            accent,
        );
        let color = if look.error {
            p.error
        } else if look.dimmed {
            p.text_weak
        } else {
            p.text
        };
        let mut job =
            egui::text::LayoutJob::simple_singleline(label.to_string(), FontId::proportional(13.5), color);
        if look.dimmed {
            job.sections[0].format.italics = true;
        }
        let galley = ui.fonts(|f| f.layout_job(job));
        painter.galley(Pos2::new(x + 24.0, rect.center().y - galley.size().y / 2.0), galley, color);
        if look.error {
            painter.circle_filled(Pos2::new(rect.right() - 12.0, rect.center().y), 4.0, p.error);
        }
    }
    resp
}

/// A small caption over a section of the left pane.
pub fn section_label(ui: &mut Ui, text: &str) {
    let p = Palette::of(ui);
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text.to_uppercase()).size(10.0).color(p.text_weak).strong());
}

/// The filled accent button for a panel's main action (OK).
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let p = Palette::of(ui);
    let text = egui::RichText::new(label).color(p.on_accent).strong();
    ui.add(egui::Button::new(text).fill(p.accent).stroke(Stroke::NONE).min_size(Vec2::new(84.0, 28.0)))
}

/// A plain button sized to sit next to [`primary_button`].
pub fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(label).min_size(Vec2::new(84.0, 28.0)))
}

/// AnvilCAD's mark (an anvil with hot stock on its face), drawn into `rect`.
pub fn paint_mark(painter: &egui::Painter, rect: Rect, ink: Color32, accent: Color32) {
    // The logo's mark is drawn on a 48 x 48 grid (docs/assets/logo.svg).
    let s = rect.height() / 40.0;
    let o = rect.min - Vec2::new(2.0, 6.0) * s;
    let p = |x: f32, y: f32| o + Vec2::new(x, y) * s;
    let st = Stroke::new((3.0 * s).max(1.4), ink);
    let pts = [
        (3.0, 13.0),
        (45.0, 13.0),
        (45.0, 20.0),
        (35.0, 20.0),
        (32.0, 23.0),
        (31.0, 30.0),
        (38.0, 36.0),
        (38.0, 41.0),
        (12.0, 41.0),
        (12.0, 36.0),
        (19.0, 30.0),
        (18.0, 24.0),
        (15.0, 21.5),
        (8.0, 18.5),
        (3.0, 13.0),
    ];
    painter.add(egui::Shape::line(pts.iter().map(|&(x, y)| p(x, y)).collect(), st));
    painter.rect_filled(Rect::from_min_max(p(22.0, 7.5), p(35.0, 12.0)), CornerRadius::same(1), accent);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text on the panel must stay readable in both themes (WCAG AA, 4.5:1).
    #[test]
    fn text_contrast_meets_aa() {
        fn lum(c: Color32) -> f64 {
            let ch = |v: u8| {
                let v = v as f64 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        }
        fn ratio(a: Color32, b: Color32) -> f64 {
            let (x, y) = (lum(a), lum(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        }
        for p in [Palette::light(), Palette::dark()] {
            for bg in [p.panel, p.chrome] {
                assert!(ratio(p.text, bg) >= 7.0, "body text {:?} on {:?}", p.text, bg);
                assert!(ratio(p.text_weak, bg) >= 4.5, "weak text {:?} on {:?}", p.text_weak, bg);
                assert!(ratio(p.error, bg) >= 4.5, "error text on {:?}", bg);
            }
            assert!(ratio(p.text, p.select_fill) >= 7.0, "selected row text");
            assert!(ratio(p.on_accent, p.accent) >= 4.5, "OK button text");
        }
    }
}
