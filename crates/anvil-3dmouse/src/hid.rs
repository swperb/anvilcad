//! Decoding 3D mouse HID input reports, and recognising a multi-axis controller from its
//! report descriptor. Layouts and sources: docs/3D_MOUSE.md.
//!
//! Raw HID axes: X is the cap moved right, Y pulled toward you, Z pressed down; rotations are
//! about the same axes. [`Axes`] is AnvilCAD's frame (right, up, toward you), so
//! tx = X, ty = −Z, tz = Y, rx = RX, ry = −RZ, rz = RY.

use crate::devices::Format;
use crate::{Axes, Event};

/// Raw counts that make full deflection. Devices report about ±350; values beyond are
/// clamped.
pub const FULL_SCALE: f64 = 350.0;

fn i16_at(r: &[u8], i: usize) -> i16 {
    match (r.get(i), r.get(i + 1)) {
        (Some(&lo), Some(&hi)) => i16::from_le_bytes([lo, hi]),
        _ => 0,
    }
}

/// Converts raw HID counts (X, Y, Z, RX, RY, RZ) to [`Axes`].
pub fn axes_from_raw(raw: [i16; 6]) -> Axes {
    let n = |v: i16| (f64::from(v) / FULL_SCALE).clamp(-1.0, 1.0);
    let [x, y, z, rx, ry, rz] = raw;
    Axes([n(x), -n(z), n(y), n(rx), -n(rz), n(ry)])
}

/// Turns one device's reports into events. It remembers the axes and the buttons held, so
/// each report updates part of the state and button events say what changed.
#[derive(Clone, Debug)]
pub struct Decoder {
    format: Format,
    raw: [i16; 6],
    /// Buttons held according to report 3 (a bitmask) and report 0x1C (a list), by
    /// number. Some devices send both; a button is held if either says so.
    from_mask: Vec<u16>,
    from_list: Vec<u16>,
}

impl Decoder {
    pub fn new(format: Format) -> Decoder {
        Decoder { format, raw: [0; 6], from_mask: Vec::new(), from_list: Vec::new() }
    }

    fn held(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self.from_mask.iter().chain(&self.from_list).copied().collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Decodes one input report (report ID first). Unknown or short reports give nothing.
    pub fn feed(&mut self, report: &[u8], out: &mut Vec<Event>) {
        let Some((&id, body)) = report.split_first() else { return };
        match id {
            1 if body.len() >= 6 => {
                // Windows pads every report to the longest one, so a separate-format
                // device's report 1 can be 13 bytes: the format, not the length, decides.
                let n = if self.format == Format::Combined && body.len() >= 12 { 6 } else { 3 };
                for i in 0..n {
                    self.raw[i] = i16_at(body, 2 * i);
                }
                out.push(Event::Motion(axes_from_raw(self.raw)));
            }
            2 if body.len() >= 6 => {
                for i in 0..3 {
                    self.raw[3 + i] = i16_at(body, 2 * i);
                }
                out.push(Event::Motion(axes_from_raw(self.raw)));
            }
            3 => {
                let before = self.held();
                self.from_mask.clear();
                for (byte, &bits) in body.iter().take(12).enumerate() {
                    for bit in 0..8 {
                        if bits & (1 << bit) != 0 {
                            self.from_mask.push((byte * 8 + bit + 1) as u16);
                        }
                    }
                }
                self.button_changes(&before, out);
            }
            0x1c => {
                let before = self.held();
                self.from_list = (0..6).map(|i| i16_at(body, 2 * i) as u16).filter(|&b| b != 0).collect();
                self.button_changes(&before, out);
            }
            _ => {}
        }
    }

    fn button_changes(&self, before: &[u16], out: &mut Vec<Event>) {
        let after = self.held();
        for &b in before.iter().filter(|b| !after.contains(b)) {
            out.push(Event::Button { number: b, pressed: false });
        }
        for &b in after.iter().filter(|b| !before.contains(b)) {
            out.push(Event::Button { number: b, pressed: true });
        }
    }

    /// Events that leave nothing moving or held, for when the device goes away.
    pub fn release_all(&mut self, out: &mut Vec<Event>) {
        let before = self.held();
        self.from_mask.clear();
        self.from_list.clear();
        self.button_changes(&before, out);
        if self.raw != [0; 6] {
            self.raw = [0; 6];
            out.push(Event::Motion(Axes::default()));
        }
    }
}

/// Whether a HID report descriptor declares a top-level multi-axis controller (usage page
/// Generic Desktop, usage 0x08). A receiver also exposes a mouse interface for a paired
/// CadMouse; this tells them apart.
pub fn is_multi_axis(descriptor: &[u8]) -> bool {
    let mut page: u32 = 0;
    let mut usage: Option<u32> = None;
    let mut depth = 0u32;
    let mut i = 0;
    while i < descriptor.len() {
        let prefix = descriptor[i];
        if prefix == 0xfe {
            // A long item: its data size is in the next byte.
            let size = descriptor.get(i + 1).copied().unwrap_or(0) as usize;
            i += 3 + size;
            continue;
        }
        let size = match prefix & 3 {
            3 => 4,
            n => n as usize,
        };
        let mut data: u32 = 0;
        for k in 0..size {
            data |= u32::from(descriptor.get(i + 1 + k).copied().unwrap_or(0)) << (8 * k);
        }
        match prefix & 0xfc {
            0x04 => page = data,
            0x08 => usage = Some(if size == 4 { data } else { (page << 16) | data }),
            0xa0 => {
                if depth == 0 && data == 1 && usage == Some((1 << 16) | 8) {
                    return true;
                }
                depth += 1;
                usage = None;
            }
            0xc0 => depth = depth.saturating_sub(1),
            // Main items (Input, Output, Feature) end the local usages.
            0x80 | 0x90 | 0xb0 => usage = None,
            _ => {}
        }
        i += 1 + size;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(v: &[i16]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn report(id: u8, body: &[u8]) -> Vec<u8> {
        let mut r = vec![id];
        r.extend_from_slice(body);
        r
    }

    fn motion(events: &[Event]) -> Option<[f64; 6]> {
        events.iter().rev().find_map(|e| match e {
            Event::Motion(a) => Some(a.0),
            _ => None,
        })
    }

    #[test]
    fn separate_reports_fill_translation_then_rotation() {
        let mut d = Decoder::new(Format::Separate);
        let mut out = Vec::new();
        // Cap 175 counts right, 70 toward you, 350 pressed down.
        d.feed(&report(1, &le(&[175, 70, 350])), &mut out);
        assert_eq!(motion(&out), Some([0.5, -1.0, 0.2, 0.0, 0.0, 0.0]));
        // Then rotation: RX 35, RY -350, RZ 700 (clamped).
        d.feed(&report(2, &le(&[35, -350, 700])), &mut out);
        assert_eq!(motion(&out), Some([0.5, -1.0, 0.2, 0.1, -1.0, -1.0]));
    }

    #[test]
    fn windows_padding_does_not_overwrite_rotation_on_separate_devices() {
        let mut d = Decoder::new(Format::Separate);
        let mut out = Vec::new();
        d.feed(&report(2, &le(&[0, 0, -175, 0, 0, 0])), &mut out);
        d.feed(&report(1, &le(&[0, 0, 0, 0, 0, 0])), &mut out);
        assert_eq!(motion(&out), Some([0.0, 0.0, 0.0, 0.0, 0.5, 0.0]));
    }

    #[test]
    fn combined_report_carries_all_six_axes() {
        let mut d = Decoder::new(Format::Combined);
        let mut out = Vec::new();
        d.feed(&report(1, &le(&[-350, 0, 0, 0, 0, 35])), &mut out);
        assert_eq!(motion(&out), Some([-1.0, 0.0, 0.0, 0.0, -0.1, 0.0]));
        // A short report 1 from a combined device updates translation only.
        d.feed(&report(1, &le(&[0, 0, 0])), &mut out);
        assert_eq!(motion(&out), Some([0.0, 0.0, 0.0, 0.0, -0.1, 0.0]));
    }

    #[test]
    fn buttons_report_presses_and_releases() {
        let mut d = Decoder::new(Format::Separate);
        let mut out = Vec::new();
        d.feed(&[3, 0b10, 0], &mut out);
        assert_eq!(out, vec![Event::Button { number: 2, pressed: true }]);
        out.clear();
        d.feed(&[3, 0b01, 0], &mut out);
        assert_eq!(
            out,
            vec![Event::Button { number: 2, pressed: false }, Event::Button { number: 1, pressed: true }]
        );
        out.clear();
        // Bit 26 of four bytes is button 27 (Lock on a SpaceMouse Pro).
        d.feed(&[3, 0, 0, 0, 0b100], &mut out);
        assert!(out.contains(&Event::Button { number: 27, pressed: true }));
        out.clear();
        d.feed(&[3, 0, 0, 0, 0], &mut out);
        assert_eq!(out, vec![Event::Button { number: 27, pressed: false }]);
    }

    #[test]
    fn button_lists_and_masks_combine() {
        let mut d = Decoder::new(Format::Combined);
        let mut out = Vec::new();
        // Enterprise: Tab (175) only in the list, Fit (2) in both.
        d.feed(&report(0x1c, &le(&[175, 2, 0, 0, 0, 0])), &mut out);
        d.feed(&[3, 0b10, 0, 0, 0], &mut out);
        assert_eq!(
            out,
            vec![Event::Button { number: 2, pressed: true }, Event::Button { number: 175, pressed: true }]
        );
        out.clear();
        // The mask releasing Fit does not release it while the list still holds it.
        d.feed(&[3, 0, 0, 0, 0], &mut out);
        assert!(out.is_empty());
        d.feed(&report(0x1c, &le(&[0; 6])), &mut out);
        assert_eq!(
            out,
            vec![Event::Button { number: 2, pressed: false }, Event::Button { number: 175, pressed: false }]
        );
    }

    #[test]
    fn junk_reports_are_ignored_and_release_all_stops_everything() {
        let mut d = Decoder::new(Format::Combined);
        let mut out = Vec::new();
        d.feed(&[], &mut out);
        d.feed(&[1, 5], &mut out);
        d.feed(&[2], &mut out);
        d.feed(&[0x17, 80], &mut out);
        d.feed(&[0x1c, 9, 0], &mut out);
        assert!(out.iter().all(|e| matches!(e, Event::Button { number: 9, pressed: true })));
        out.clear();
        d.feed(&report(1, &le(&[100, 0, 0, 0, 0, 0])), &mut out);
        out.clear();
        d.release_all(&mut out);
        assert_eq!(out, vec![Event::Button { number: 9, pressed: false }, Event::Motion(Axes::default())]);
    }

    #[test]
    fn recognises_multi_axis_descriptors() {
        // Usage Page (Generic Desktop), Usage (Multi-axis Controller), Collection
        // (Application), ... End Collection: the start of a SpaceNavigator's descriptor.
        let nav = [0x05, 0x01, 0x09, 0x08, 0xa1, 0x01, 0xa1, 0x00, 0x85, 0x01, 0xc0, 0xc0];
        assert!(is_multi_axis(&nav));
        // A mouse (usage 0x02), as a receiver's CadMouse interface declares.
        let mouse = [0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0xc0, 0xc0];
        assert!(!is_multi_axis(&mouse));
        // A usage 0x08 nested inside another collection is not top-level.
        let nested = [0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x08, 0xa1, 0x01, 0xc0, 0xc0];
        assert!(!is_multi_axis(&nested));
        // An extended (4-byte) usage naming the page itself.
        let extended = [0x0b, 0x08, 0x00, 0x01, 0x00, 0xa1, 0x01, 0xc0];
        assert!(is_multi_axis(&extended));
        // Truncated and empty descriptors do not panic.
        assert!(!is_multi_axis(&[0x05]));
        assert!(!is_multi_axis(&[0xfe, 200]));
        assert!(!is_multi_axis(&[]));
    }
}
