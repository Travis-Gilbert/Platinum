//! GPUI keystrokes to the DOM's `key` and `code`.
//!
//! SOURCING: hand-rolled table. GPUI hands the embedder a named key plus the
//! typed character, never the hardware key code, so no crate converts it to
//! `keyboard-types`; servoshell does the same from winit's names. The table
//! follows Theorem's `space_servo_web.rs` and adds the keys that one skips.
//! Ported unchanged from Theorem UI's desktop host (`desktop/src/keys.rs`).

use std::str::FromStr as _;

use gpui_kit::Keystroke;
use servo::{Code, Key, KeyState, KeyboardEvent, Location, Modifiers};

/// The Servo keyboard event for a GPUI keystroke.
pub fn keyboard_event(keystroke: &Keystroke, state: KeyState, repeat: bool) -> KeyboardEvent {
    let key = dom_key(keystroke);
    let code = dom_code(&keystroke.key);
    KeyboardEvent::new_without_event(
        state,
        Key::from_str(&key).unwrap_or(Key::Character(key)),
        Code::from_str(code).unwrap_or(Code::Unidentified),
        Location::Standard,
        modifiers(&keystroke.modifiers),
        repeat,
        false,
    )
}

pub fn modifiers(modifiers: &gpui_kit::Modifiers) -> Modifiers {
    let mut out = Modifiers::empty();
    out.set(Modifiers::SHIFT, modifiers.shift);
    out.set(Modifiers::CONTROL, modifiers.control);
    out.set(Modifiers::ALT, modifiers.alt);
    out.set(Modifiers::META, modifiers.platform);
    out
}

/// Key events for the modifier keys that went down or up between two states,
/// since GPUI reports modifier changes rather than their key presses.
pub fn modifier_events(old: &gpui_kit::Modifiers, new: &gpui_kit::Modifiers) -> Vec<KeyboardEvent> {
    [
        ("shift", old.shift, new.shift),
        ("control", old.control, new.control),
        ("alt", old.alt, new.alt),
        ("platform", old.platform, new.platform),
    ]
    .into_iter()
    .filter(|(_, was, is)| was != is)
    .map(|(key, _, down)| {
        let keystroke = Keystroke {
            modifiers: *new,
            key: key.into(),
            key_char: None,
        };
        let state = if down { KeyState::Down } else { KeyState::Up };
        keyboard_event(&keystroke, state, false)
    })
    .collect()
}

/// The DOM `key` value: a named key, or the character the keystroke types.
fn dom_key(keystroke: &Keystroke) -> String {
    let named = match keystroke.key.as_str() {
        "enter" => "Enter",
        "tab" => "Tab",
        "escape" => "Escape",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "insert" => "Insert",
        "space" => " ",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "shift" => "Shift",
        "control" => "Control",
        "alt" => "Alt",
        "platform" => "Meta",
        "capslock" => "CapsLock",
        key if function_key(key).is_some() => return key.to_ascii_uppercase(),
        _ => "",
    };
    if !named.is_empty() {
        return named.to_string();
    }
    if let Some(typed) = &keystroke.key_char {
        return typed.clone();
    }
    // With Command or Control down GPUI reports no typed character, and gives
    // the unshifted key with `shift` set; browsers report the shifted letter.
    let key = &keystroke.key;
    if keystroke.modifiers.shift && key.len() == 1 && key.chars().all(|c| c.is_ascii_lowercase()) {
        return key.to_ascii_uppercase();
    }
    key.clone()
}

/// The DOM `code` value, for a US layout: GPUI does not report the physical key.
fn dom_code(key: &str) -> &'static str {
    if let Some(code) = function_key(key) {
        return code;
    }
    match key {
        "enter" => "Enter",
        "tab" => "Tab",
        "escape" => "Escape",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "insert" => "Insert",
        "space" | " " => "Space",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "shift" => "ShiftLeft",
        "control" => "ControlLeft",
        "alt" => "AltLeft",
        "platform" => "MetaLeft",
        "capslock" => "CapsLock",
        "-" | "_" => "Minus",
        "=" | "+" => "Equal",
        "[" | "{" => "BracketLeft",
        "]" | "}" => "BracketRight",
        ";" | ":" => "Semicolon",
        "'" | "\"" => "Quote",
        "," | "<" => "Comma",
        "." | ">" => "Period",
        "/" | "?" => "Slash",
        "\\" | "|" => "Backslash",
        "`" | "~" => "Backquote",
        "!" => "Digit1",
        "@" => "Digit2",
        "#" => "Digit3",
        "$" => "Digit4",
        "%" => "Digit5",
        "^" => "Digit6",
        "&" => "Digit7",
        "*" => "Digit8",
        "(" => "Digit9",
        ")" => "Digit0",
        _ => letter_or_digit(key).unwrap_or("Unidentified"),
    }
}

fn letter_or_digit(key: &str) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK",
        "KeyL", "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT", "KeyU", "KeyV",
        "KeyW", "KeyX", "KeyY", "KeyZ",
    ];
    const DIGITS: [&str; 10] = [
        "Digit0", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8",
        "Digit9",
    ];
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    match c.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(LETTERS[(c as u8 - b'a') as usize]),
        c @ '0'..='9' => Some(DIGITS[(c as u8 - b'0') as usize]),
        _ => None,
    }
}

fn function_key(key: &str) -> Option<&'static str> {
    const KEYS: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14",
        "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];
    let n: usize = key.strip_prefix('f')?.parse().ok()?;
    KEYS.get(n.checked_sub(1)?).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(key: &str, key_char: Option<&str>, modifiers: gpui_kit::Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.into(),
            key_char: key_char.map(Into::into),
        }
    }

    fn none() -> gpui_kit::Modifiers {
        gpui_kit::Modifiers::default()
    }

    #[test]
    fn letters_use_the_typed_character_and_the_physical_code() {
        let event = keyboard_event(&stroke("a", Some("A"), none()), KeyState::Down, false);
        assert_eq!(event.event.key, Key::Character("A".into()));
        assert_eq!(event.event.code, Code::KeyA);
    }

    #[test]
    fn command_shortcuts_report_the_shifted_letter() {
        let mods = gpui_kit::Modifiers {
            platform: true,
            shift: true,
            ..Default::default()
        };
        let event = keyboard_event(&stroke("z", None, mods), KeyState::Down, false);
        assert_eq!(event.event.key, Key::Character("Z".into()));
        assert!(
            event
                .event
                .modifiers
                .contains(Modifiers::META | Modifiers::SHIFT)
        );
    }

    #[test]
    fn named_keys_map_to_dom_names() {
        for (gpui, dom, code) in [
            ("enter", Key::Named(servo::NamedKey::Enter), Code::Enter),
            (
                "left",
                Key::Named(servo::NamedKey::ArrowLeft),
                Code::ArrowLeft,
            ),
            (
                "pagedown",
                Key::Named(servo::NamedKey::PageDown),
                Code::PageDown,
            ),
            ("f13", Key::Named(servo::NamedKey::F13), Code::F13),
        ] {
            let event = keyboard_event(&stroke(gpui, None, none()), KeyState::Down, false);
            assert_eq!(event.event.key, dom, "{gpui}");
            assert_eq!(event.event.code, code, "{gpui}");
        }
    }

    #[test]
    fn space_is_a_character_with_the_space_code() {
        let event = keyboard_event(&stroke("space", Some(" "), none()), KeyState::Down, false);
        assert_eq!(event.event.key, Key::Character(" ".into()));
        assert_eq!(event.event.code, Code::Space);
    }

    #[test]
    fn modifier_changes_become_key_presses() {
        let shift = gpui_kit::Modifiers {
            shift: true,
            ..Default::default()
        };
        let command = gpui_kit::Modifiers {
            platform: true,
            ..Default::default()
        };
        let events = modifier_events(&shift, &command);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.key, Key::Named(servo::NamedKey::Shift));
        assert_eq!(events[0].event.state, KeyState::Up);
        assert_eq!(events[1].event.key, Key::Named(servo::NamedKey::Meta));
        assert_eq!(events[1].event.code, Code::MetaLeft);
        assert_eq!(events[1].event.state, KeyState::Down);
        assert!(events[1].event.modifiers.contains(Modifiers::META));
        assert!(modifier_events(&shift, &shift).is_empty());
    }

    #[test]
    fn shifted_symbols_keep_their_physical_key() {
        let event = keyboard_event(&stroke("!", Some("!"), none()), KeyState::Down, false);
        assert_eq!(event.event.key, Key::Character("!".into()));
        assert_eq!(event.event.code, Code::Digit1);
    }
}
