//! macOS and Windows: reading the device through hidapi. A read returns one input report
//! with its report ID first, as on Linux.
//!
//! All hidapi use happens on one thread that lives as long as the process. hidapi's macOS
//! backend schedules its process-wide HID manager on the run loop of the thread that first
//! initialises it, and the `hidapi` crate never shuts it down; once that thread has exited,
//! the next thread to enumerate devices crashes the process (SIGTRAP). So a [`crate::Reader`]
//! does not start a thread of its own here: it subscribes to the service thread, which
//! serves one reader at a time, the newest.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use hidapi::HidApi;

use crate::devices::{lookup, VID_LOGITECH};
use crate::hid::Decoder;
use crate::{Event, Status, RESCAN};

/// Delivers an event to a reader; false once nobody is listening.
pub type Sink = Box<dyn FnMut(Event) -> bool + Send>;

/// The reader being served, by the generation it subscribed with.
static LISTENER: Mutex<Option<(u64, Sink)>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Whether the service thread could be started (it is started once, by the first reader).
static SERVICE: OnceLock<bool> = OnceLock::new();

fn listener() -> MutexGuard<'static, Option<(u64, Sink)>> {
    LISTENER.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Makes `send` the reader the service serves, in place of any earlier one. Returns its
/// generation, for [`unsubscribe`].
pub fn subscribe(send: Sink) -> u64 {
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    *listener() = Some((generation, send));
    // If the thread cannot be made, the app simply has no 3D mouse.
    let _started =
        SERVICE.get_or_init(|| std::thread::Builder::new().name("anvil-3dmouse".into()).spawn(serve).is_ok());
    generation
}

/// Stops serving the reader of this generation, if it is still the one being served.
pub fn unsubscribe(generation: u64) {
    let mut l = listener();
    if l.as_ref().is_some_and(|(g, _)| *g == generation) {
        *l = None;
    }
}

fn current() -> Option<u64> {
    listener().as_ref().map(|(g, _)| *g)
}

/// Sends to the reader of `generation`; false if it is no longer the one being served.
fn deliver(generation: u64, e: Event) -> bool {
    match listener().as_mut() {
        Some((g, send)) if *g == generation => send(e),
        _ => false,
    }
}

/// The service thread: never returns, so hidapi stays on this thread.
fn serve() {
    let mut api = HidApi::new().map_err(|e| e.to_string());
    let mut served = 0;
    let mut last_problem = None;
    loop {
        let Some(generation) = current() else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        if generation != served {
            served = generation;
            last_problem = None;
        }
        let gone = || current() != Some(generation);
        let mut send = |e: Event| deliver(generation, e);
        match &mut api {
            Ok(api) => {
                run_once(api, &gone, &mut send, &mut last_problem);
            }
            Err(e) => {
                if last_problem.is_none() {
                    let text = format!("3D mouse support could not start: {e}");
                    send(Event::Status(Status::Problem(text.clone())));
                    last_problem = Some(text);
                }
            }
        }
        // Wait before looking again, unless a new reader subscribes or this one leaves.
        let step = Duration::from_millis(100);
        let mut waited = Duration::ZERO;
        while waited < RESCAN && !gone() {
            std::thread::sleep(step);
            waited += step;
        }
    }
}

/// One attempt: finds a 3D mouse and reads it until it goes away, `send` fails or `stop`
/// says to. Returns whether to keep running.
pub fn run_once(
    api: &mut HidApi,
    stop: &dyn Fn() -> bool,
    send: &mut dyn FnMut(Event) -> bool,
    last_problem: &mut Option<String>,
) -> bool {
    if api.refresh_devices().is_err() {
        return true;
    }
    // The axes are the multi-axis controller collection (usage page 1, usage 8). The older
    // Logitech-era models have one collection, whatever it declares.
    let found = api.device_list().find_map(|d| {
        let model = lookup(d.vendor_id(), d.product_id())?;
        let axes = (d.usage_page() == 1 && d.usage() == 8) || model.vid == VID_LOGITECH;
        axes.then(|| (d.path().to_owned(), model))
    });
    let Some((path, model)) = found else { return true };

    #[cfg(target_os = "macos")]
    api.set_open_exclusive(false);
    let dev = match api.open_path(&path) {
        Ok(d) => d,
        Err(_) => {
            // Short enough for the status bar of the narrowest window.
            let text = format!(
                "{} could not be opened; 3Dconnexion's driver may be holding it (docs/3D_MOUSE.md)",
                model.name
            );
            if last_problem.as_deref() != Some(text.as_str()) {
                *last_problem = Some(text.clone());
                return send(Event::Status(Status::Problem(text)));
            }
            return true;
        }
    };
    *last_problem = None;
    if !send(Event::Status(Status::Connected { name: model.name.to_string(), layout: model.layout })) {
        return false;
    }
    let mut decoder = Decoder::new(model.format);
    let mut buf = [0u8; 64];
    let mut events = Vec::new();
    let keep = loop {
        if stop() {
            break false;
        }
        match dev.read_timeout(&mut buf, 250) {
            Ok(0) => continue,
            Ok(n) => {
                decoder.feed(&buf[..n.min(buf.len())], &mut events);
                if !events.drain(..).all(&mut *send) {
                    break false;
                }
            }
            Err(_) => break true,
        }
    };
    decoder.release_all(&mut events);
    let sent = events.drain(..).all(&mut *send);
    keep && sent && send(Event::Status(Status::Disconnected))
}
