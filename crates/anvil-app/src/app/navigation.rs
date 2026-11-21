//! The 3D mouse in the app: starting the reader, its events, and moving the view each frame.
//! The device side is `anvil-3dmouse`; the axis mapping is `crate::mouse3d`.

use super::*;
use crate::mouse3d;
use crate::script::Action;
use anvil_3dmouse::{Axes, Event, Layout, Reader, Status};

/// When no report has come for this long, the cap is taken to be at rest. Devices repeat
/// their report about every 8 ms while the cap is moved, so this only matters if the final
/// zero report is lost. spacenavd sends only changes, so it gets no timeout.
const AT_REST_AFTER: f64 = 0.5;

#[derive(Default)]
pub(super) struct Mouse3dInput {
    pub(super) reader: Option<Reader>,
    /// Whether this app opens devices: the window does; the headless harness and tests do not.
    pub(super) allowed: bool,
    pub(super) axes: Axes,
    /// When the last motion report came (egui time, seconds).
    pub(super) last_motion: f64,
    /// The connected device: its name and button layout.
    pub(super) device: Option<(String, Layout)>,
}

impl AnvilApp {
    /// Lets the app read a 3D mouse (the windowed app calls this; `anvil.opt.mouse_3d` can
    /// still turn it off).
    pub fn enable_3d_mouse(&mut self, ctx: &egui::Context) {
        self.mouse3d.allowed = true;
        self.sync_3d_mouse(ctx);
    }

    /// Starts or stops the reader to match the options.
    pub(super) fn sync_3d_mouse(&mut self, ctx: &egui::Context) {
        let want = self.mouse3d.allowed && self.scripting.options.mouse_3d != Some(false);
        if want && self.mouse3d.reader.is_none() {
            let ctx = ctx.clone();
            self.mouse3d.reader = Some(Reader::start(move || ctx.request_repaint()));
        } else if !want && self.mouse3d.reader.is_some() {
            self.mouse3d.reader = None;
            self.mouse3d.axes = Axes::default();
            if self.mouse3d.device.take().is_some() {
                self.status = "3D mouse off (anvil.opt.mouse_3d = false)".into();
            }
        }
    }

    /// Handles an event as if the reader had sent it (the headless harness's `mouse3d`).
    pub fn inject_3d_mouse(&mut self, ctx: &egui::Context, e: Event) {
        self.on_3d_mouse(ctx, e);
    }

    /// The connected 3D mouse: its name and button layout.
    pub fn mouse_3d_device(&self) -> Option<(String, Layout)> {
        self.mouse3d.device.clone()
    }

    /// Handles what the reader sent since the last frame.
    pub(super) fn poll_3d_mouse(&mut self, ctx: &egui::Context) {
        let Some(reader) = &self.mouse3d.reader else { return };
        let events: Vec<Event> = reader.events().collect();
        for e in events {
            self.on_3d_mouse(ctx, e);
        }
    }

    pub(super) fn on_3d_mouse(&mut self, ctx: &egui::Context, e: Event) {
        match e {
            Event::Motion(a) => {
                self.mouse3d.axes = a;
                self.mouse3d.last_motion = ctx.input(|i| i.time);
                ctx.request_repaint();
            }
            Event::Button { number, pressed: true } => self.mouse_3d_button(ctx, number),
            Event::Button { pressed: false, .. } => {}
            Event::Status(Status::Connected { name, layout }) => {
                self.status = format!("3D mouse connected: {name}");
                self.mouse3d.device = Some((name, layout));
            }
            Event::Status(Status::Disconnected) => {
                self.mouse3d.axes = Axes::default();
                if let Some((name, _)) = self.mouse3d.device.take() {
                    self.status = format!("3D mouse disconnected: {name}");
                }
            }
            Event::Status(Status::Problem(text)) => self.status = text,
        }
    }

    /// Runs what a 3D mouse button is bound to: the last `anvil.mouse_3d_button` for it, or
    /// else the button's default.
    fn mouse_3d_button(&mut self, ctx: &egui::Context, number: u16) {
        let layout = self.mouse3d.device.as_ref().map_or(Layout::Unnamed, |(_, l)| *l);
        let bound = self.scripting.engine.as_ref().and_then(|e| {
            e.config.buttons.iter().rev().find(|b| layout.button_number(&b.button) == Some(number)).cloned()
        });
        let name = layout.button_name(number);
        let result = match bound {
            Some(b) => match &b.action {
                None => Ok(()),
                Some(Action::Builtin(cmd)) => self.run_command(ctx, cmd),
                Some(Action::Lua(index)) => {
                    let index = *index;
                    self.with_engine(ctx, |engine, host| engine.run_button(index, host))
                }
            },
            None => match anvil_3dmouse::devices::default_command(layout, number) {
                Some(cmd) => self.run_command(ctx, cmd),
                None => Ok(()),
            },
        };
        if let Err(e) = result {
            self.status = format!("3D mouse {name}: {e}");
        }
    }

    /// Moves the view by this frame's share of the 3D mouse's deflection.
    pub(super) fn step_3d_mouse(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let (now, dt, focused) = ctx.input(|i| (i.time, i.stable_dt as f64, i.focused));
        let spacenavd = matches!(self.mouse3d.device, Some((_, Layout::Spacenavd)));
        // Another window has the focus (the device would otherwise move this view while the
        // user works elsewhere), or the device has gone quiet: the cap is at rest.
        if !focused || (!spacenavd && now - self.mouse3d.last_motion > AT_REST_AFTER) {
            self.mouse3d.axes = Axes::default();
            return;
        }
        if !mouse3d::moves(&self.mouse3d.axes) {
            return;
        }
        let o = &self.scripting.options;
        let settings = mouse3d::Settings {
            speed: o.mouse_3d_speed.unwrap_or(1.0),
            invert: o.mouse_3d_invert.unwrap_or([false; 6]),
        };
        mouse3d::step(&mut self.camera, &self.mouse3d.axes, dt, &settings, rect);
        ctx.request_repaint();
    }
}
