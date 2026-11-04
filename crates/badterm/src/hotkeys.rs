//! BadTerm's hotkeys: Alt (Cmd on macOS) with a letter, or Enter. Each
//! action is known by the key it has out of the box; the settings can move
//! it to another (see [`set`]), and what the screens say follows.

use std::collections::BTreeMap;
use std::sync::RwLock;

use winit::keyboard::KeyCode;

/// The actions, by their usual key, and what they do.
pub const ALL: [(&str, &str); 22] = [
    ("D", "Dialing directory"),
    ("R", "Call again, or redial now"),
    ("L", "Send login name, password"),
    ("H", "Hang up"),
    ("X", "Exit BadTerm"),
    ("U", "Upload"),
    ("G", "Download"),
    ("C", "Capture to a file"),
    ("S", "Save scrollback or screen"),
    ("F", "View ANSI art"),
    ("B", "Scrollback"),
    ("V", "Paste"),
    ("O", "Settings"),
    ("P", "Phosphor mode"),
    ("A", "Aspect ratio"),
    ("W", "8- or 9-dot characters"),
    ("Enter", "Full screen"),
    ("I", "iCE colors or blinking"),
    ("E", "CP437 or UTF-8"),
    ("K", "Reveal (Viewdata)"),
    ("M", "Sound and music"),
    ("Z", "Help"),
];

/// The keys an action can be given.
pub const KEYS: [&str; 27] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W",
    "X", "Y", "Z", "Enter",
];

/// The moved ones: usual key -> key now.
static MOVED: RwLock<BTreeMap<String, String>> = RwLock::new(BTreeMap::new());

/// Take the settings' changes (usual key -> key), ignoring any that name
/// no action or no key.
pub fn set(moved: &BTreeMap<String, String>) {
    let valid = moved
        .iter()
        .filter(|(from, to)| ALL.iter().any(|(k, _)| k == from) && KEYS.contains(&to.as_str()))
        .map(|(a, b)| (a.clone(), b.clone()))
        .collect();
    if let Ok(mut m) = MOVED.write() {
        *m = valid;
    }
}

/// The key the action usually on `usual` is on now.
pub fn key(usual: &str) -> String {
    MOVED.read().ok().and_then(|m| m.get(usual).cloned()).unwrap_or_else(|| usual.to_string())
}

/// How to press it: "Alt-D", or "Cmd-D" on macOS.
pub fn label(usual: &str) -> String {
    format!("{}-{}", crate::ui::HOTKEY, key(usual))
}

/// The action a pressed key is for, by its usual key.
pub fn action(pressed: &str) -> Option<&'static str> {
    ALL.iter().map(|(k, _)| *k).find(|k| key(k) == pressed)
}

/// Give `usual`'s action the key `to`, in `moved` (the settings); an
/// action that had `to` takes the key this one leaves.
pub fn assign(moved: &mut BTreeMap<String, String>, usual: &str, to: &str) {
    set(moved);
    let old = key(usual);
    if let Some(other) = action(to).filter(|o| *o != usual) {
        put(moved, other, &old);
    }
    put(moved, usual, to);
    set(moved);
}

fn put(moved: &mut BTreeMap<String, String>, usual: &str, to: &str) {
    if usual == to {
        moved.remove(usual);
    } else {
        moved.insert(usual.to_string(), to.to_string());
    }
}

pub fn name(code: KeyCode) -> Option<&'static str> {
    Some(match code {
        KeyCode::Enter | KeyCode::NumpadEnter => "Enter",
        KeyCode::KeyA => "A",
        KeyCode::KeyB => "B",
        KeyCode::KeyC => "C",
        KeyCode::KeyD => "D",
        KeyCode::KeyE => "E",
        KeyCode::KeyF => "F",
        KeyCode::KeyG => "G",
        KeyCode::KeyH => "H",
        KeyCode::KeyI => "I",
        KeyCode::KeyJ => "J",
        KeyCode::KeyK => "K",
        KeyCode::KeyL => "L",
        KeyCode::KeyM => "M",
        KeyCode::KeyN => "N",
        KeyCode::KeyO => "O",
        KeyCode::KeyP => "P",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyR => "R",
        KeyCode::KeyS => "S",
        KeyCode::KeyT => "T",
        KeyCode::KeyU => "U",
        KeyCode::KeyV => "V",
        KeyCode::KeyW => "W",
        KeyCode::KeyX => "X",
        KeyCode::KeyY => "Y",
        KeyCode::KeyZ => "Z",
        _ => return None,
    })
}

pub fn code(name: &str) -> Option<KeyCode> {
    Some(match name {
        "Enter" => KeyCode::Enter,
        "A" => KeyCode::KeyA,
        "B" => KeyCode::KeyB,
        "C" => KeyCode::KeyC,
        "D" => KeyCode::KeyD,
        "E" => KeyCode::KeyE,
        "F" => KeyCode::KeyF,
        "G" => KeyCode::KeyG,
        "H" => KeyCode::KeyH,
        "I" => KeyCode::KeyI,
        "J" => KeyCode::KeyJ,
        "K" => KeyCode::KeyK,
        "L" => KeyCode::KeyL,
        "M" => KeyCode::KeyM,
        "N" => KeyCode::KeyN,
        "O" => KeyCode::KeyO,
        "P" => KeyCode::KeyP,
        "Q" => KeyCode::KeyQ,
        "R" => KeyCode::KeyR,
        "S" => KeyCode::KeyS,
        "T" => KeyCode::KeyT,
        "U" => KeyCode::KeyU,
        "V" => KeyCode::KeyV,
        "W" => KeyCode::KeyW,
        "X" => KeyCode::KeyX,
        "Y" => KeyCode::KeyY,
        "Z" => KeyCode::KeyZ,
        _ => return None,
    })
}

/// A pressed key as the action's usual one, for the code that handles
/// them: unchanged if it is not a letter or Enter (the macros' digits), and
/// None for a letter no action has now.
pub fn translate(pressed: KeyCode) -> Option<KeyCode> {
    match name(pressed) {
        Some(n) => action(n).and_then(code),
        None => Some(pressed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_fit_the_editor() {
        // Two columns in the settings' hotkeys window.
        let half = ALL.len().div_ceil(2);
        for (i, (_, what)) in ALL.iter().enumerate() {
            assert!(what.len() <= if i < half { 25 } else { 23 }, "{what}");
        }
    }

    #[test]
    fn moving_keys_swaps_and_translates() {
        let mut moved = BTreeMap::new();
        // Directory to Y: a free key.
        assign(&mut moved, "D", "Y");
        assert_eq!(key("D"), "Y");
        assert_eq!(translate(KeyCode::KeyY), Some(KeyCode::KeyD));
        assert_eq!(translate(KeyCode::KeyD), None);
        assert_eq!(translate(KeyCode::Digit1), Some(KeyCode::Digit1));
        // Help to Y: the directory takes Help's Z.
        assign(&mut moved, "Z", "Y");
        assert_eq!((key("Z"), key("D")), ("Y".to_string(), "Z".to_string()));
        // Back where they were: nothing is stored.
        assign(&mut moved, "Z", "Z");
        assign(&mut moved, "D", "D");
        assert!(moved.is_empty(), "{moved:?}");
        assert_eq!(translate(KeyCode::KeyD), Some(KeyCode::KeyD));
        set(&BTreeMap::new());
    }
}
