//! 3D mouse input for AnvilCAD: 3Dconnexion SpaceMouse models and compatibles.
//!
//! [`Reader::start`] watches for a device on a thread of its own and reports what it does as
//! [`Event`]s: six axes of motion, button presses, and connections. It reads the device
//! directly (hidraw on Linux, hidapi on macOS and Windows), so no vendor driver or SDK is
//! needed. On Linux, when spacenavd is running, it reads from spacenavd instead, since the
//! daemon holds the device then. Nothing here ever blocks the caller.
//!
//! The device facts (IDs, report layouts, the spacenavd protocol) and their sources are in
//! docs/3D_MOUSE.md.

pub mod devices;
pub mod hid;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod spnav;

#[cfg(any(target_os = "macos", windows))]
mod desktop;

use std::sync::mpsc::{channel, Receiver, Sender};
#[cfg(not(any(target_os = "macos", windows)))]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

pub use devices::Layout;

/// The six axes, each from −1 to 1 (full deflection), in AnvilCAD's frame: `tx` cap moved
/// right, `ty` lifted up, `tz` pulled toward you; `rx`, `ry` and `rz` the cap turned
/// right-handed about those three directions (top tilted toward you, twisted anticlockwise seen from
/// above, rolled anticlockwise seen from the front).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Axes(pub [f64; 6]);

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// A device is being read. `name` is its model, or how it is reached.
    Connected { name: String, layout: Layout },
    /// The device went away (unplugged, or the daemon stopped).
    Disconnected,
    /// A device is there but cannot be read: what is wrong and what to do.
    Problem(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The current deflection. Devices send it continuously while the cap is moved and a
    /// zero deflection when it comes to rest.
    Motion(Axes),
    /// Buttons are numbered from 1 (see [`Layout::button_name`]).
    Button {
        number: u16,
        pressed: bool,
    },
    Status(Status),
}

/// Reads 3D mice in the background. Dropping it stops the reading within about half a
/// second, or at the device's next report.
///
/// On Linux each reader has a thread of its own. On macOS and Windows one service thread
/// reads for the whole process and serves the newest reader (see `desktop.rs` for why), so
/// starting a second reader takes the device from the first.
pub struct Reader {
    events: Receiver<Event>,
    #[cfg(not(any(target_os = "macos", windows)))]
    stop: Arc<AtomicBool>,
    #[cfg(any(target_os = "macos", windows))]
    generation: u64,
}

/// How long to wait before looking for a device again when there is none.
const RESCAN: Duration = Duration::from_secs(2);

impl Reader {
    /// Starts watching. `wake` is called after each event is queued, from the reader's
    /// thread, so a UI can repaint; it must not block.
    pub fn start(wake: impl Fn() + Send + 'static) -> Reader {
        let (tx, events) = channel();
        let send = move |e: Event| -> bool {
            let ok = Sender::send(&tx, e).is_ok();
            if ok {
                wake();
            }
            ok
        };
        Reader::serve(events, send)
    }

    #[cfg(any(target_os = "macos", windows))]
    fn serve(events: Receiver<Event>, send: impl FnMut(Event) -> bool + Send + 'static) -> Reader {
        Reader { events, generation: desktop::subscribe(Box::new(send)) }
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    fn serve(events: Receiver<Event>, mut send: impl FnMut(Event) -> bool + Send + 'static) -> Reader {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let spawned =
            std::thread::Builder::new().name("anvil-3dmouse".into()).spawn(move || watch(&flag, &mut send));
        // If the thread cannot be made the app simply has no 3D mouse; the sender is gone,
        // so the receiver yields nothing.
        drop(spawned);
        Reader { events, stop }
    }

    /// The events that have arrived since the last call.
    pub fn events(&self) -> impl Iterator<Item = Event> + '_ {
        self.events.try_iter()
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        #[cfg(not(any(target_os = "macos", windows)))]
        self.stop.store(true, Ordering::Relaxed);
        #[cfg(any(target_os = "macos", windows))]
        desktop::unsubscribe(self.generation);
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn sleep_unless_stopped(stop: &AtomicBool) {
    let step = Duration::from_millis(100);
    let mut waited = Duration::ZERO;
    while waited < RESCAN && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(step);
        waited += step;
    }
}

#[cfg(target_os = "linux")]
fn watch(stop: &AtomicBool, send: &mut dyn FnMut(Event) -> bool) {
    use std::io::Read;
    let mut last_problem: Option<String> = None;
    while !stop.load(Ordering::Relaxed) {
        if let Some(socket) = spnav::connect() {
            last_problem = None;
            let connected =
                Status::Connected { name: "3D mouse (spacenavd)".into(), layout: Layout::Spacenavd };
            if !send(Event::Status(connected)) || !spnav::run(socket, stop, send) {
                return;
            }
            if !send(Event::Motion(Axes::default())) || !send(Event::Status(Status::Disconnected)) {
                return;
            }
            continue;
        }
        let mut problem = None;
        for found in linux::find() {
            let mut file = match std::fs::File::open(&found.node) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    // Short enough for the status bar of the narrowest window.
                    problem = Some(format!(
                        "{}: no permission to read {} (docs/3D_MOUSE.md has the fix)",
                        found.model.name,
                        found.node.display()
                    ));
                    continue;
                }
                Err(_) => continue,
            };
            problem = None;
            last_problem = None;
            let connected =
                Status::Connected { name: found.model.name.to_string(), layout: found.model.layout };
            if !send(Event::Status(connected)) {
                return;
            }
            let mut decoder = hid::Decoder::new(found.model.format);
            let mut buf = [0u8; 64];
            let mut events = Vec::new();
            // Each read is one report. The read blocks while the device is still; `stop` is
            // checked after every report, and a dropped Reader also makes `send` fail.
            let keep = loop {
                if stop.load(Ordering::Relaxed) {
                    break false;
                }
                match file.read(&mut buf) {
                    Ok(0) => break true,
                    Ok(n) => {
                        decoder.feed(&buf[..n], &mut events);
                        if !events.drain(..).all(&mut *send) {
                            break false;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break true,
                }
            };
            decoder.release_all(&mut events);
            if !keep || !events.drain(..).all(&mut *send) || !send(Event::Status(Status::Disconnected)) {
                return;
            }
            break;
        }
        if let Some(p) = problem {
            if last_problem.as_ref() != Some(&p) {
                if !send(Event::Status(Status::Problem(p.clone()))) {
                    return;
                }
                last_problem = Some(p);
            }
        }
        sleep_unless_stopped(stop);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn watch(_stop: &AtomicBool, _send: &mut dyn FnMut(Event) -> bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_starts_and_stops_without_a_device() {
        // On a machine with no 3D mouse the reader finds nothing and reports nothing but,
        // at most, a problem; dropping it stops the thread.
        let r = Reader::start(|| {});
        std::thread::sleep(Duration::from_millis(50));
        assert!(r.events().all(|e| matches!(e, Event::Status(_))));
        drop(r);
    }

    #[test]
    fn readers_started_and_dropped_in_quick_succession_do_not_crash() {
        // Turning the 3D mouse off and on again drops a reader and starts another. On macOS
        // a second reader used hidapi from a new thread after the first reader's thread,
        // where hidapi had been initialised, had exited, and that crashed the process. This
        // must run to the end on every platform.
        let mut readers = Vec::new();
        for _ in 0..8 {
            readers.push(Reader::start(|| {}));
            std::thread::sleep(Duration::from_millis(5));
            if readers.len() > 2 {
                drop(readers.remove(0));
            }
        }
        std::thread::sleep(Duration::from_millis(300));
        drop(readers);
        std::thread::sleep(Duration::from_millis(300));
    }
}
