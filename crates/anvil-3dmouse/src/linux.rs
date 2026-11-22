//! Finding 3D mice among the Linux hidraw devices, with the standard library alone (sysfs
//! for what each device is, /dev/hidrawN to read it).

use crate::devices::{lookup, Model, VID_LOGITECH};
use crate::hid::is_multi_axis;

/// The vendor and product IDs in a hidraw device's `uevent`, from its
/// `HID_ID=BUS:VENDOR:PRODUCT` line (hexadecimal).
pub fn parse_uevent(uevent: &str) -> Option<(u16, u16)> {
    let id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut parts = id.trim().split(':');
    let _bus = parts.next()?;
    let vid = u32::from_str_radix(parts.next()?, 16).ok()?;
    let pid = u32::from_str_radix(parts.next()?, 16).ok()?;
    Some((u16::try_from(vid).ok()?, u16::try_from(pid).ok()?))
}

/// Whether an interface of a 3D mouse is the one with the axes. Interfaces that declare a
/// multi-axis controller are; so are the older Logitech-era models, which have a single
/// interface. A receiver's mouse interface (for a paired CadMouse) is not.
pub fn is_axes_interface(model: &Model, descriptor: Option<&[u8]>) -> bool {
    match descriptor {
        Some(d) if is_multi_axis(d) => true,
        _ => model.vid == VID_LOGITECH,
    }
}

/// A hidraw node that is a 3D mouse.
pub struct Found {
    pub node: std::path::PathBuf,
    pub model: Model,
}

/// The 3D mice among /sys/class/hidraw.
pub fn find() -> Vec<Found> {
    let mut found = Vec::new();
    let Ok(dir) = std::fs::read_dir("/sys/class/hidraw") else { return found };
    let mut names: Vec<String> =
        dir.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for name in names {
        let sys = std::path::Path::new("/sys/class/hidraw").join(&name).join("device");
        let Some((vid, pid)) =
            std::fs::read_to_string(sys.join("uevent")).ok().as_deref().and_then(parse_uevent)
        else {
            continue;
        };
        let Some(model) = lookup(vid, pid) else { continue };
        let descriptor = std::fs::read(sys.join("report_descriptor")).ok();
        if is_axes_interface(&model, descriptor.as_deref()) {
            found.push(Found { node: std::path::Path::new("/dev").join(&name), model });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::VID_3DCONNEXION;

    #[test]
    fn reads_ids_from_uevent() {
        let u =
            "DRIVER=hid-generic\nHID_ID=0003:0000256F:0000C635\nHID_NAME=3Dconnexion SpaceMouse Compact\n";
        assert_eq!(parse_uevent(u), Some((0x256f, 0xc635)));
        let bt = "HID_ID=0005:0000256F:0000C63A\n";
        assert_eq!(parse_uevent(bt), Some((0x256f, 0xc63a)));
        assert_eq!(parse_uevent("HID_NAME=x\n"), None);
        assert_eq!(parse_uevent("HID_ID=0003:zz:0000C635\n"), None);
        assert_eq!(parse_uevent("HID_ID=0003:0001256F:0000C635\n"), None);
    }

    #[test]
    fn picks_the_axes_interface() {
        let receiver = lookup(VID_3DCONNEXION, 0xc652).map(|m| {
            let axes = [0x05, 0x01, 0x09, 0x08, 0xa1, 0x01, 0xc0];
            let mouse = [0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0xc0];
            (
                is_axes_interface(&m, Some(&axes)),
                is_axes_interface(&m, Some(&mouse)),
                is_axes_interface(&m, None),
            )
        });
        assert_eq!(receiver, Some((true, false, false)));
        let nav = lookup(VID_LOGITECH, 0xc626).map(|m| is_axes_interface(&m, None));
        assert_eq!(nav, Some(true));
    }
}
