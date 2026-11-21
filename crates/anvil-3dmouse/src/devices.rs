//! The 3D mice AnvilCAD knows: USB vendor and product IDs, how each sends its axes, and the
//! names of its buttons. Sources for every entry are in docs/3D_MOUSE.md.

/// 3Dconnexion's vendor ID from when the company was part of Logitech.
pub const VID_LOGITECH: u16 = 0x046d;
/// 3Dconnexion's own vendor ID.
pub const VID_3DCONNEXION: u16 = 0x256f;

/// How a device sends its six axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Report 1 carries translation (3 × i16), report 2 rotation (3 × i16).
    Separate,
    /// Report 1 carries all six axes (6 × i16).
    Combined,
}

/// Which button layout a device has, for button names and default actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Two side buttons, Left and Right.
    TwoButton,
    /// SpaceMouse Pro and Pro Wireless (and the Universal Receiver, which they use).
    Pro,
    /// SpaceMouse Enterprise: the Pro's buttons and more.
    Enterprise,
    SpacePilotPro,
    SpaceExplorer,
    /// Buttons without known names: they are `Button 1`, `Button 2` and so on.
    Unnamed,
    /// A device read through spacenavd, which does not say which model it is. Buttons are
    /// unnamed; buttons 1 and 2 (Left and Right, or Menu and Fit, on most models) keep their
    /// usual defaults.
    Spacenavd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub vid: u16,
    pub pid: u16,
    pub name: &'static str,
    pub format: Format,
    pub layout: Layout,
}

const fn m(vid: u16, pid: u16, name: &'static str, format: Format, layout: Layout) -> Model {
    Model { vid, pid, name, format, layout }
}

use Format::{Combined, Separate};
use Layout::*;

/// Every six-axis device with a sourced product ID.
pub const MODELS: &[Model] = &[
    m(VID_LOGITECH, 0xc603, "SpaceMouse Plus XT", Separate, Unnamed),
    m(VID_LOGITECH, 0xc605, "CadMan", Separate, Unnamed),
    m(VID_LOGITECH, 0xc606, "SpaceMouse Classic", Separate, Unnamed),
    m(VID_LOGITECH, 0xc621, "Spaceball 5000", Separate, Unnamed),
    m(VID_LOGITECH, 0xc623, "SpaceTraveler", Separate, Unnamed),
    m(VID_LOGITECH, 0xc625, "SpacePilot", Separate, Unnamed),
    m(VID_LOGITECH, 0xc626, "SpaceNavigator", Separate, TwoButton),
    m(VID_LOGITECH, 0xc627, "SpaceExplorer", Separate, SpaceExplorer),
    m(VID_LOGITECH, 0xc628, "SpaceNavigator for Notebooks", Separate, TwoButton),
    m(VID_LOGITECH, 0xc629, "SpacePilot Pro", Separate, SpacePilotPro),
    m(VID_LOGITECH, 0xc62b, "SpaceMouse Pro", Separate, Pro),
    m(VID_3DCONNEXION, 0xc62e, "SpaceMouse Wireless", Combined, TwoButton),
    m(VID_3DCONNEXION, 0xc62f, "SpaceMouse Wireless", Combined, TwoButton),
    m(VID_3DCONNEXION, 0xc631, "SpaceMouse Pro Wireless", Combined, Pro),
    m(VID_3DCONNEXION, 0xc632, "SpaceMouse Pro Wireless", Combined, Pro),
    m(VID_3DCONNEXION, 0xc633, "SpaceMouse Enterprise", Combined, Enterprise),
    m(VID_3DCONNEXION, 0xc635, "SpaceMouse Compact", Separate, TwoButton),
    m(VID_3DCONNEXION, 0xc636, "SpaceMouse Module", Combined, Unnamed),
    m(VID_3DCONNEXION, 0xc638, "SpaceMouse Pro Wireless", Combined, Pro),
    m(VID_3DCONNEXION, 0xc63a, "SpaceMouse Wireless", Combined, TwoButton),
    m(VID_3DCONNEXION, 0xc641, "SpaceMouse", Separate, Unnamed),
    m(VID_3DCONNEXION, 0xc652, "3Dconnexion Universal Receiver", Combined, Pro),
];

/// 3Dconnexion devices that are not six-axis controllers: CadMouse models, keyboards and
/// numpads. They share the vendor ID, so they are excluded by product ID.
pub const NOT_3D_MICE: &[u16] =
    &[0xc62c, 0xc650, 0xc651, 0xc654, 0xc655, 0xc656, 0xc657, 0xc658, 0xc664, 0xc665, 0xc668];

/// What AnvilCAD treats a device as, or `None` if it is not a 3D mouse. A 3Dconnexion device
/// that is not in [`MODELS`] or [`NOT_3D_MICE`] (a model newer than this table) is read as a
/// combined-format device with unnamed buttons, as current models are.
pub fn lookup(vid: u16, pid: u16) -> Option<Model> {
    if let Some(m) = MODELS.iter().find(|m| m.vid == vid && m.pid == pid) {
        return Some(*m);
    }
    if vid == VID_3DCONNEXION && !NOT_3D_MICE.contains(&pid) {
        return Some(m(vid, pid, "3Dconnexion 3D mouse", Combined, Unnamed));
    }
    None
}

/// Button labels, as (button number, label).
type Labels = &'static [(u16, &'static str)];

/// Named buttons, by button number (bit index in report 3, plus one; the same number report
/// 0x1C sends).
const TWO_BUTTON: Labels = &[(1, "Left"), (2, "Right")];
const PRO: Labels = &[
    (1, "Menu"),
    (2, "Fit"),
    (3, "T"),
    (5, "R"),
    (6, "F"),
    (9, "Roll"),
    (13, "1"),
    (14, "2"),
    (15, "3"),
    (16, "4"),
    (23, "Esc"),
    (24, "Alt"),
    (25, "Shift"),
    (26, "Ctrl"),
    (27, "Lock"),
];
const ENTERPRISE_EXTRA: Labels = &[
    (11, "ISO"),
    (17, "5"),
    (18, "6"),
    (19, "7"),
    (20, "8"),
    (21, "9"),
    (22, "10"),
    (36, "Enter"),
    (37, "Delete"),
    (77, "11"),
    (78, "12"),
    (103, "V1"),
    (104, "V2"),
    (105, "V3"),
    (175, "Tab"),
    (176, "Space"),
];
// The number keys 1 to 5 are left unnamed: the sources disagree on their bits.
const SPACEPILOT_PRO: Labels = &[
    (1, "Menu"),
    (2, "Fit"),
    (3, "T"),
    (5, "R"),
    (6, "F"),
    (9, "Roll"),
    (11, "ISO"),
    (23, "Esc"),
    (24, "Alt"),
    (25, "Shift"),
    (26, "Ctrl"),
    (27, "Lock"),
    (28, "Pan/Zoom"),
    (29, "Dominant"),
    (30, "Plus"),
    (31, "Minus"),
];
const SPACE_EXPLORER: Labels = &[
    (1, "1"),
    (2, "2"),
    (3, "T"),
    (4, "L"),
    (5, "R"),
    (6, "F"),
    (7, "Esc"),
    (8, "Alt"),
    (9, "Shift"),
    (10, "Ctrl"),
    (11, "Fit"),
    (12, "Menu"),
    (13, "Plus"),
    (14, "Minus"),
    (15, "2D"),
];

impl Layout {
    fn named(self) -> (Labels, Labels) {
        match self {
            TwoButton => (TWO_BUTTON, &[]),
            Pro => (PRO, &[]),
            Enterprise => (PRO, ENTERPRISE_EXTRA),
            SpacePilotPro => (SPACEPILOT_PRO, &[]),
            SpaceExplorer => (SPACE_EXPLORER, &[]),
            Unnamed | Spacenavd => (&[], &[]),
        }
    }

    /// The name of a button: its label, or `Button N`.
    pub fn button_name(self, number: u16) -> String {
        let (a, b) = self.named();
        match a.iter().chain(b).find(|(n, _)| *n == number) {
            Some((_, label)) => (*label).to_string(),
            None => format!("Button {number}"),
        }
    }

    /// The button a name refers to on this layout, ignoring case: a label, or `Button N`.
    pub fn button_number(self, name: &str) -> Option<u16> {
        let (a, b) = self.named();
        if let Some((n, _)) = a.iter().chain(b).find(|(_, l)| l.eq_ignore_ascii_case(name)) {
            return Some(*n);
        }
        parse_button_n(name)
    }
}

/// `Button N` (any case, any spacing) as N, for N from 1 to 255.
pub fn parse_button_n(name: &str) -> Option<u16> {
    let rest = name.trim();
    let rest = rest.get(..6).filter(|p| p.eq_ignore_ascii_case("button")).map(|_| &rest[6..])?;
    rest.trim().parse::<u16>().ok().filter(|n| (1..=255).contains(n))
}

/// Every button name any layout uses, for checking `init.lua` (a name need only exist on
/// some device, since the configuration does not know which one is plugged in).
pub fn all_button_labels() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = [TWO_BUTTON, PRO, ENTERPRISE_EXTRA, SPACEPILOT_PRO, SPACE_EXPLORER]
        .iter()
        .flat_map(|t| t.iter().map(|(_, l)| *l))
        .collect();
    v.sort_unstable_by_key(|l| l.to_ascii_lowercase());
    v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    v
}

/// What a button does when `init.lua` does not say: the name of a built-in command. The
/// left button (or Menu) opens the command palette and the right button (or Fit) zooms to
/// fit, as the two buttons do in 3Dconnexion's own driver; the view keys go to those views.
pub fn default_command(layout: Layout, number: u16) -> Option<&'static str> {
    if layout == Spacenavd {
        return match number {
            1 => Some("Command Palette"),
            2 => Some("Zoom to Fit"),
            _ => None,
        };
    }
    let name = layout.button_name(number);
    Some(match name.as_str() {
        "Left" | "Menu" => "Command Palette",
        "Right" | "Fit" => "Zoom to Fit",
        "T" => "Top",
        "F" => "Front",
        "R" => "Right",
        "L" => "Left",
        "ISO" => "Isometric",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_finds_models_and_skips_other_3dconnexion_devices() {
        let compact = lookup(0x256f, 0xc635).map(|m| (m.name, m.format, m.layout));
        assert_eq!(compact, Some(("SpaceMouse Compact", Format::Separate, Layout::TwoButton)));
        let nav = lookup(0x046d, 0xc626).map(|m| m.name);
        assert_eq!(nav, Some("SpaceNavigator"));
        // A CadMouse is not a 3D mouse.
        assert_eq!(lookup(0x256f, 0xc650), None);
        // A Logitech mouse is not either.
        assert_eq!(lookup(0x046d, 0xc077), None);
        // An unknown newer 3Dconnexion device is read as a combined-format 3D mouse.
        let new = lookup(0x256f, 0xc6ff).map(|m| (m.format, m.layout));
        assert_eq!(new, Some((Format::Combined, Layout::Unnamed)));
        // Every product ID is listed once.
        for (i, a) in MODELS.iter().enumerate() {
            assert!(MODELS[i + 1..].iter().all(|b| (a.vid, a.pid) != (b.vid, b.pid)), "{a:?}");
            assert!(!(a.vid == VID_3DCONNEXION && NOT_3D_MICE.contains(&a.pid)), "{a:?}");
        }
    }

    #[test]
    fn button_names_round_trip() {
        assert_eq!(Layout::Pro.button_name(2), "Fit");
        assert_eq!(Layout::Pro.button_number("fit"), Some(2));
        assert_eq!(Layout::Enterprise.button_number("Tab"), Some(175));
        assert_eq!(Layout::Pro.button_number("Tab"), None);
        assert_eq!(Layout::Unnamed.button_name(7), "Button 7");
        assert_eq!(Layout::Unnamed.button_number("button 7"), Some(7));
        assert_eq!(Layout::Pro.button_number(" Button  13 "), Some(13));
        assert_eq!(parse_button_n("Button 0"), None);
        assert_eq!(parse_button_n("Button 256"), None);
        assert_eq!(parse_button_n("Butt"), None);
        assert_eq!(parse_button_n("Buttön 1"), None);
        for layout in [TwoButton, Pro, Enterprise, SpacePilotPro, SpaceExplorer] {
            let (a, b) = layout.named();
            for (n, label) in a.iter().chain(b) {
                assert_eq!(layout.button_number(label), Some(*n), "{layout:?} {label}");
            }
        }
        let labels = all_button_labels();
        assert!(labels.contains(&"Fit") && labels.contains(&"Left") && labels.contains(&"V3"));
    }

    #[test]
    fn default_commands_follow_the_labels() {
        assert_eq!(default_command(TwoButton, 1), Some("Command Palette"));
        assert_eq!(default_command(TwoButton, 2), Some("Zoom to Fit"));
        assert_eq!(default_command(Pro, 2), Some("Zoom to Fit"));
        assert_eq!(default_command(Pro, 3), Some("Top"));
        assert_eq!(default_command(SpaceExplorer, 11), Some("Zoom to Fit"));
        assert_eq!(default_command(SpaceExplorer, 4), Some("Left"));
        assert_eq!(default_command(Pro, 23), None);
        assert_eq!(default_command(Unnamed, 1), None);
        assert_eq!(default_command(Spacenavd, 2), Some("Zoom to Fit"));
        assert_eq!(default_command(Spacenavd, 3), None);
    }
}
