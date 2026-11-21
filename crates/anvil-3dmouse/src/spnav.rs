//! The spacenavd client (Linux). spacenavd is the free driver many Linux users run for their
//! 3D mouse; when it is running it holds the device, so AnvilCAD reads from it instead.
//!
//! This is spacenavd's original protocol, which every version serves: the client connects to
//! a Unix socket and the daemon sends 32-byte packets of eight native-endian i32s:
//! `[0, x, y, z, rx, ry, rz, period]` for motion, `[1, button, ..]` for a press and
//! `[2, button, ..]` for a release. spacenavd's frame is right, up and into the screen, so
//! z and rz are negated for [`Axes`]. Sources: docs/3D_MOUSE.md.

use crate::hid::FULL_SCALE;
use crate::{Axes, Event};

pub const PACKET: usize = 32;

/// Decodes one packet; anything else (a later protocol's packets) gives `None`.
pub fn decode(p: &[u8; PACKET]) -> Option<Event> {
    let w = |i: usize| i32::from_ne_bytes([p[4 * i], p[4 * i + 1], p[4 * i + 2], p[4 * i + 3]]);
    let n = |v: i32| (f64::from(v) / FULL_SCALE).clamp(-1.0, 1.0);
    let button = |b: i32| u16::try_from(b).ok().and_then(|b| b.checked_add(1)).filter(|&b| b <= 255);
    match w(0) {
        0 => Some(Event::Motion(Axes([n(w(1)), n(w(2)), -n(w(3)), n(w(4)), n(w(5)), -n(w(6))]))),
        1 => button(w(1)).map(|number| Event::Button { number, pressed: true }),
        2 => button(w(1)).map(|number| Event::Button { number, pressed: false }),
        _ => None,
    }
}

/// Where spacenavd's socket may be, in the order libspnav looks: `$SPNAV_SOCKET`, a
/// `socket = PATH` line in /etc/spnavrc, `$XDG_RUNTIME_DIR/spnav.sock`, /var/run/spnav.sock.
pub fn socket_paths(
    spnav_socket: Option<String>,
    spnavrc: Option<&str>,
    xdg_runtime: Option<String>,
) -> Vec<String> {
    let mut v = Vec::new();
    v.extend(spnav_socket.filter(|s| !s.is_empty()));
    if let Some(rc) = spnavrc {
        for line in rc.lines() {
            let line = line.split('#').next().unwrap_or("");
            if let Some((k, val)) = line.split_once('=') {
                if k.trim() == "socket" && !val.trim().is_empty() {
                    v.push(val.trim().to_string());
                }
            }
        }
    }
    v.extend(xdg_runtime.filter(|s| !s.is_empty()).map(|d| format!("{d}/spnav.sock")));
    v.push("/var/run/spnav.sock".to_string());
    v.dedup();
    v
}

#[cfg(target_os = "linux")]
pub use client::connect;

#[cfg(target_os = "linux")]
mod client {
    use super::*;
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// Connects to spacenavd if it is running.
    pub fn connect() -> Option<UnixStream> {
        let rc = std::fs::read_to_string("/etc/spnavrc").ok();
        let paths = socket_paths(
            std::env::var("SPNAV_SOCKET").ok(),
            rc.as_deref(),
            std::env::var("XDG_RUNTIME_DIR").ok(),
        );
        paths.iter().find_map(|p| UnixStream::connect(p).ok())
    }

    /// Reads events until the daemon goes away, `send` fails (nobody is listening) or `stop`
    /// is set. Returns whether to keep running.
    pub fn run(mut s: UnixStream, stop: &AtomicBool, send: &mut dyn FnMut(Event) -> bool) -> bool {
        // A timeout lets the loop notice `stop` while the device is still.
        if s.set_read_timeout(Some(Duration::from_millis(500))).is_err() {
            return true;
        }
        let mut buf = [0u8; PACKET];
        let mut have = 0;
        while !stop.load(Ordering::Relaxed) {
            match s.read(&mut buf[have..]) {
                Ok(0) => return true,
                Ok(n) => {
                    have += n;
                    if have == PACKET {
                        have = 0;
                        if let Some(e) = decode(&buf) {
                            if !send(e) {
                                return false;
                            }
                        }
                    }
                }
                Err(e)
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return true,
            }
        }
        false
    }
}

#[cfg(target_os = "linux")]
pub use client::run;

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(w: [i32; 8]) -> [u8; PACKET] {
        let mut p = [0u8; PACKET];
        for (i, v) in w.iter().enumerate() {
            p[4 * i..4 * i + 4].copy_from_slice(&v.to_ne_bytes());
        }
        p
    }

    #[test]
    fn decodes_motion_into_anvilcad_axes() {
        // spacenavd: right 175, up 35, into the screen 350; rz into the screen 70.
        let e = decode(&packet([0, 175, 35, 350, 0, -350, 70, 8]));
        assert_eq!(e, Some(Event::Motion(Axes([0.5, 0.1, -1.0, 0.0, -1.0, -0.2]))));
        let e = decode(&packet([0, 9000, 0, 0, 0, 0, i32::MIN, 8]));
        assert_eq!(e, Some(Event::Motion(Axes([1.0, 0.0, 0.0, 0.0, 0.0, 1.0]))));
    }

    #[test]
    fn decodes_buttons_and_ignores_other_packets() {
        assert_eq!(
            decode(&packet([1, 0, 1, 0, 0, 0, 0, 0])),
            Some(Event::Button { number: 1, pressed: true })
        );
        assert_eq!(
            decode(&packet([2, 1, 0, 0, 0, 0, 0, 0])),
            Some(Event::Button { number: 2, pressed: false })
        );
        assert_eq!(decode(&packet([1, -1, 0, 0, 0, 0, 0, 0])), None);
        assert_eq!(decode(&packet([1, 255, 0, 0, 0, 0, 0, 0])), None);
        assert_eq!(decode(&packet([3, 0, 0, 0, 0, 0, 0, 0])), None);
        assert_eq!(decode(&packet([0x7faa_0001, 0, 0, 0, 0, 0, 0, 0])), None);
    }

    #[test]
    fn finds_the_socket_where_libspnav_looks() {
        let rc = "# spacenavd config\nsensitivity = 2\nsocket = /tmp/my.sock # mine\n";
        let p = socket_paths(Some("/a.sock".into()), Some(rc), Some("/run/user/1000".into()));
        assert_eq!(p, vec!["/a.sock", "/tmp/my.sock", "/run/user/1000/spnav.sock", "/var/run/spnav.sock"]);
        assert_eq!(socket_paths(Some(String::new()), None, None), vec!["/var/run/spnav.sock"]);
    }
}
