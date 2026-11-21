//! macOS and Windows: reading the device through hidapi. A read returns one input report
//! with its report ID first, as on Linux.

use std::sync::atomic::{AtomicBool, Ordering};

use hidapi::HidApi;

use crate::devices::{lookup, VID_LOGITECH};
use crate::hid::Decoder;
use crate::{Event, Status};

/// One attempt: finds a 3D mouse and reads it until it goes away, `send` fails or `stop`
/// is set. Returns whether to keep running.
pub fn run_once(
    api: &mut HidApi,
    stop: &AtomicBool,
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
        if stop.load(Ordering::Relaxed) {
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
