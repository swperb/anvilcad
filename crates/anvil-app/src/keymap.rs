//! Key bindings from `init.lua`: parsing `"ctrl+shift+e"`, `"g g"` or `"<leader>f"`, and
//! matching the keys the user presses against them, including multi-key sequences.
//!
//! Modifier names: `cmd` (or `mod`) is Command on macOS and Ctrl elsewhere, the same as the
//! headless harness; `ctrl`, `shift` and `alt` (or `opt`, `option`) are those keys
//! everywhere. Key names are egui's (`a`, `F5`, `Enter`, `Space`, `PageDown`, `/`...), in any
//! case. See docs/SCRIPTING.md.

use egui::{Key, Modifiers};

/// One key with the modifiers held for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: Key,
    /// Command on macOS, Ctrl elsewhere.
    pub cmd: bool,
    /// The Ctrl key itself (on macOS, distinct from Command).
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    /// Whether a key press with these modifiers is this chord.
    pub fn matches(&self, key: Key, m: Modifiers) -> bool {
        if key != self.key || m.shift != self.shift || m.alt != self.alt {
            return false;
        }
        if cfg!(target_os = "macos") {
            // Command and Ctrl are separate keys.
            (m.command || m.mac_cmd) == self.cmd && m.ctrl == self.ctrl
        } else {
            // Ctrl is the command key, so `cmd+x` and `ctrl+x` are the same chord.
            (m.command || m.ctrl) == (self.cmd || self.ctrl)
        }
    }

    /// The modifiers egui reports when this chord is pressed.
    pub fn modifiers(&self) -> Modifiers {
        if cfg!(target_os = "macos") {
            Modifiers {
                alt: self.alt,
                ctrl: self.ctrl,
                shift: self.shift,
                mac_cmd: self.cmd,
                command: self.cmd,
            }
        } else {
            let c = self.cmd || self.ctrl;
            Modifiers { alt: self.alt, ctrl: c, shift: self.shift, mac_cmd: false, command: c }
        }
    }
}

/// Finds an egui key by name, ignoring case: `a`, `F5`, `enter`, `PageDown`, `/`.
pub fn key_by_name(name: &str) -> Option<Key> {
    if let Some(k) = Key::from_name(name) {
        return Some(k);
    }
    let lower = name.to_ascii_lowercase();
    if lower.chars().count() == 1 {
        return Key::from_name(&lower.to_ascii_uppercase());
    }
    let alias = match lower.as_str() {
        "esc" => "Escape",
        "return" => "Enter",
        "del" => "Delete",
        "pgup" => "PageUp",
        "pgdn" | "pgdown" => "PageDown",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        _ => "",
    };
    if !alias.is_empty() {
        return Key::from_name(alias);
    }
    Key::ALL.iter().copied().find(|k| k.name().to_ascii_lowercase() == lower)
}

/// Parses one chord such as `ctrl+shift+e`. A literal `+` key is written `plus`.
pub fn parse_chord(text: &str) -> Result<Chord, String> {
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key_name, mods) = parts.split_last().ok_or_else(|| "empty key".to_string())?;
    if key_name.is_empty() {
        return Err(format!("`{text}` has no key after its modifiers (write `plus` for the + key)"));
    }
    let key = key_by_name(key_name).ok_or_else(|| format!("unknown key `{key_name}` in `{text}`"))?;
    let mut c = Chord { key, cmd: false, ctrl: false, shift: false, alt: false };
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "cmd" | "mod" | "command" | "super" => c.cmd = true,
            "ctrl" | "control" => c.ctrl = true,
            "shift" => c.shift = true,
            "alt" | "opt" | "option" => c.alt = true,
            other => return Err(format!("unknown modifier `{other}` in `{text}`")),
        }
    }
    Ok(c)
}

/// Parses a binding: chords separated by spaces, where `<leader>` stands for the leader
/// sequence. `"g g"` is G then G; `"<leader>f"` with leader `"space"` is Space then F.
pub fn parse_sequence(text: &str, leader: &[Chord]) -> Result<Vec<Chord>, String> {
    let mut out = Vec::new();
    for word in text.split_whitespace() {
        let mut rest = word;
        while let Some(after) = rest.strip_prefix("<leader>") {
            if leader.is_empty() {
                return Err(format!(
                    "`{text}` uses <leader>, but no leader is set (anvil.leader = \"space\")"
                ));
            }
            out.extend_from_slice(leader);
            rest = after;
        }
        if !rest.is_empty() {
            out.push(parse_chord(rest)?);
        }
    }
    if out.is_empty() {
        return Err("empty key binding".into());
    }
    Ok(out)
}

/// What a key press did to the pending sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// No binding starts this way; the key is the app's as usual.
    NoMatch,
    /// The keys so far begin one or more bindings; wait for the next key.
    Pending,
    /// The binding at this index was completed.
    Fire(usize),
}

/// Matches key presses against bindings, one key at a time.
#[derive(Default)]
pub struct Matcher {
    pending: Vec<(Key, Modifiers)>,
    /// When the last key of the pending sequence was pressed (seconds, egui's clock).
    at: f64,
}

/// How long a sequence waits for its next key.
pub const SEQUENCE_TIMEOUT: f64 = 1.5;

impl Matcher {
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Feeds one key press, `now` in seconds. `bindings` are the parsed sequences.
    pub fn press(&mut self, bindings: &[Vec<Chord>], key: Key, mods: Modifiers, now: f64) -> Step {
        if !self.pending.is_empty() && now - self.at > SEQUENCE_TIMEOUT {
            self.pending.clear();
        }
        self.pending.push((key, mods));
        self.at = now;
        let prefix_of = |b: &Vec<Chord>| {
            b.len() >= self.pending.len() && b.iter().zip(&self.pending).all(|(c, (k, m))| c.matches(*k, *m))
        };
        if let Some(i) = bindings.iter().position(|b| b.len() == self.pending.len() && prefix_of(b)) {
            self.pending.clear();
            return Step::Fire(i);
        }
        if bindings.iter().any(prefix_of) {
            return Step::Pending;
        }
        let was_sequence = self.pending.len() > 1;
        self.pending.clear();
        if was_sequence {
            // The sequence broke; this key may still start (or be) a binding of its own.
            return self.press(bindings, key, mods, now);
        }
        Step::NoMatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;

    #[test]
    fn parses_chords() {
        let c = parse_chord("ctrl+shift+e").unwrap();
        assert_eq!((c.key, c.ctrl, c.shift, c.alt, c.cmd), (Key::E, true, true, false, false));
        let c = parse_chord("CMD+Alt+F5").unwrap();
        assert_eq!((c.key, c.cmd, c.alt), (Key::F5, true, true));
        assert_eq!(parse_chord("pagedown").unwrap().key, Key::PageDown);
        assert_eq!(parse_chord("esc").unwrap().key, Key::Escape);
        assert_eq!(parse_chord("/").unwrap().key, Key::Slash);
        assert_eq!(parse_chord("shift+plus").unwrap().key, Key::Plus);
        assert_eq!(parse_chord("space").unwrap().key, Key::Space);
        assert!(parse_chord("hyper+a").unwrap_err().contains("unknown modifier `hyper`"));
        assert!(parse_chord("ctrl+nope").unwrap_err().contains("unknown key `nope`"));
        assert!(parse_chord("ctrl+").unwrap_err().contains("no key"));
    }

    #[test]
    fn parses_sequences_with_a_leader() {
        let leader = parse_sequence("space", &[]).unwrap();
        let s = parse_sequence("<leader>f", &leader).unwrap();
        assert_eq!(s.iter().map(|c| c.key).collect::<Vec<_>>(), [Key::Space, Key::F]);
        let s = parse_sequence("g g", &[]).unwrap();
        assert_eq!(s.len(), 2);
        assert!(parse_sequence("<leader>x", &[]).unwrap_err().contains("no leader"));
        assert!(parse_sequence("   ", &[]).is_err());
    }

    #[test]
    fn matches_single_keys_and_sequences() {
        let bindings = vec![
            parse_sequence("g g", &[]).unwrap(),
            parse_sequence("g p", &[]).unwrap(),
            parse_sequence("x", &[]).unwrap(),
        ];
        let mut m = Matcher::default();
        assert_eq!(m.press(&bindings, Key::G, NONE, 0.0), Step::Pending);
        assert_eq!(m.press(&bindings, Key::P, NONE, 0.5), Step::Fire(1));
        assert!(!m.is_pending());
        // A key that starts nothing is left to the app.
        assert_eq!(m.press(&bindings, Key::Q, NONE, 1.0), Step::NoMatch);
        // A sequence that breaks hands its last key on: g then x fires x.
        assert_eq!(m.press(&bindings, Key::G, NONE, 2.0), Step::Pending);
        assert_eq!(m.press(&bindings, Key::X, NONE, 2.2), Step::Fire(2));
        // A sequence times out.
        assert_eq!(m.press(&bindings, Key::G, NONE, 3.0), Step::Pending);
        assert_eq!(m.press(&bindings, Key::G, NONE, 3.0 + SEQUENCE_TIMEOUT + 0.1), Step::Pending);
        assert_eq!(m.press(&bindings, Key::G, NONE, 4.7), Step::Fire(0));
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let c = parse_chord("ctrl+shift+e").unwrap();
        assert!(c.matches(Key::E, c.modifiers()));
        assert!(!c.matches(Key::E, Modifiers::SHIFT));
        let plain = parse_chord("e").unwrap();
        assert!(plain.matches(Key::E, NONE));
        assert!(!plain.matches(Key::E, Modifiers::SHIFT));
        // `cmd` is the platform's command key, as the app's own shortcuts use.
        let save = parse_chord("cmd+s").unwrap();
        assert!(save.matches(Key::S, Modifiers::COMMAND));
    }
}
