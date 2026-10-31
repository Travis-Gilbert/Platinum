//! Window-local hold gesture. GPUI owns key parsing/events; Core Graphics only
//! supplies the Command side that GPUI's combined modifier flag cannot express.
use gpui_kit::{KeybindingKeystroke, Keystroke, Modifiers};

pub const DEFAULT: &str = "left-cmd";
pub const HOLD_DELAY: std::time::Duration = std::time::Duration::from_millis(350);

#[derive(Clone, Debug)]
pub enum Shortcut {
    LeftCommand,
    Chord(Keystroke),
}

impl Shortcut {
    pub fn matches(&self, key: &Keystroke) -> bool {
        matches!(self, Self::Chord(binding) if key.should_match(&KeybindingKeystroke::from_keystroke(binding.clone())))
    }
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case(DEFAULT) {
            return Ok(Self::LeftCommand);
        }
        anyhow::ensure!(value.len() <= 64, "Shortcut is too long");
        let key = Keystroke::parse(value)
            .map_err(|_| anyhow::anyhow!("Use left-cmd or a shortcut such as ctrl-shift-space"))?;
        let function_key = key
            .key
            .strip_prefix('f')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=20).contains(&n));
        let named_key = matches!(
            key.key.as_str(),
            "space"
                | "tab"
                | "enter"
                | "backspace"
                | "delete"
                | "up"
                | "down"
                | "left"
                | "right"
                | "home"
                | "end"
                | "pageup"
                | "pagedown"
        );
        anyhow::ensure!(
            key.key_char.is_none()
                && (function_key || named_key || key.key.chars().count() == 1)
                && (function_key
                    || key.modifiers.control
                    || key.modifiers.alt
                    || key.modifiers.platform),
            "Use left-cmd, a function key, or a shortcut with Control, Option or Command. Escape is reserved for canceling."
        );
        Ok(Self::Chord(key))
    }

    pub fn setting(&self) -> String {
        match self {
            Self::LeftCommand => DEFAULT.into(),
            Self::Chord(key) => key.unparse(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::LeftCommand => "Left Command".into(),
            Self::Chord(key) => key.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Arm,
    Finish,
    Cancel,
}

#[derive(Default)]
pub struct Hold {
    pub active: bool,
    left_down: bool,
    pressed: Option<Keystroke>,
}

impl Hold {
    pub fn cancel(&mut self) -> Effect {
        self.pressed = None;
        if std::mem::take(&mut self.active) {
            Effect::Cancel
        } else {
            Effect::None
        }
    }

    pub fn focus_changed(&mut self, left_down: bool) {
        self.active = false;
        self.pressed = None;
        self.left_down = left_down;
    }

    fn finish(&mut self) -> Effect {
        self.pressed = None;
        if std::mem::take(&mut self.active) {
            Effect::Finish
        } else {
            Effect::None
        }
    }

    pub fn modifiers(
        &mut self,
        shortcut: &Shortcut,
        modifiers: Modifiers,
        left: bool,
        right: bool,
    ) -> Effect {
        let pressed = left && !self.left_down;
        self.left_down = left;
        match shortcut {
            Shortcut::LeftCommand => {
                if !left {
                    return self.finish();
                }
                let only_left = !right
                    && modifiers
                        == Modifiers {
                            platform: true,
                            ..Modifiers::none()
                        };
                if !only_left {
                    return self.cancel();
                }
                if pressed {
                    self.active = true;
                    return Effect::Arm;
                }
            }
            Shortcut::Chord(key)
                if self.active
                    && modifiers
                        != self
                            .pressed
                            .as_ref()
                            .map_or(key.modifiers, |key| key.modifiers) =>
            {
                // Releasing any required modifier finishes; adding another
                // modifier cancels rather than turning a new shortcut into speech.
                let required = self
                    .pressed
                    .as_ref()
                    .map_or(key.modifiers, |key| key.modifiers);
                if (required.control && !modifiers.control)
                    || (required.alt && !modifiers.alt)
                    || (required.shift && !modifiers.shift)
                    || (required.platform && !modifiers.platform)
                    || (required.function && !modifiers.function)
                {
                    return self.finish();
                }
                return self.cancel();
            }
            _ => {}
        }
        Effect::None
    }

    pub fn key_down(
        &mut self,
        shortcut: &Shortcut,
        key: &Keystroke,
        repeat: bool,
    ) -> (bool, Effect) {
        if shortcut.matches(key) {
            if !repeat && !self.active {
                self.active = true;
                self.pressed = Some(key.clone());
                return (true, Effect::Arm);
            }
            return (true, Effect::None);
        }
        (false, self.cancel())
    }

    pub fn key_up(&mut self, shortcut: &Shortcut, key: &Keystroke) -> Effect {
        if let Shortcut::Chord(binding) = shortcut
            && self
                .pressed
                .as_ref()
                .map_or(&binding.key, |pressed| &pressed.key)
                == &key.key
        {
            return self.finish();
        }
        Effect::None
    }
}

pub fn command_keys() -> (bool, bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2_core_graphics::{CGEventSource, CGEventSourceStateID};
        // macOS virtual key codes kVK_Command and kVK_RightCommand.
        let state = CGEventSourceStateID::CombinedSessionState;
        (
            CGEventSource::key_state(state, 0x37),
            CGEventSource::key_state(state, 0x36),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        (false, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_command_only_and_shortcut_chords_do_not_rearm() {
        let shortcut = Shortcut::LeftCommand;
        let command = Modifiers {
            platform: true,
            ..Modifiers::none()
        };
        let mut hold = Hold::default();
        assert_eq!(
            hold.modifiers(&shortcut, command, false, true),
            Effect::None
        );
        assert_eq!(hold.modifiers(&shortcut, command, true, false), Effect::Arm);
        assert_eq!(
            hold.key_down(&shortcut, &Keystroke::parse("cmd-c").unwrap(), false),
            (false, Effect::Cancel)
        );
        assert_eq!(
            hold.modifiers(&shortcut, command, true, false),
            Effect::None
        );
        assert_eq!(
            hold.modifiers(&shortcut, Modifiers::none(), false, false),
            Effect::None
        );
        assert_eq!(hold.modifiers(&shortcut, command, true, false), Effect::Arm);
        assert_eq!(
            hold.modifiers(&shortcut, command, false, true),
            Effect::Finish
        );
        hold.focus_changed(true);
        assert_eq!(
            hold.modifiers(&shortcut, command, true, false),
            Effect::None
        );
    }

    #[test]
    fn custom_chord_finishes_on_either_release_and_ignores_repeat() {
        let shortcut = Shortcut::parse("ctrl-shift-space").unwrap();
        assert_eq!(shortcut.setting(), "ctrl-shift-space");
        assert!(Shortcut::parse(&shortcut.setting()).is_ok());
        let legacy: crate::spoken::Preferences =
            serde_json::from_str(r#"{"voice":null,"words_per_minute":180}"#).unwrap();
        assert_eq!(legacy.dictation_hotkey, DEFAULT);
        let key = Keystroke::parse("ctrl-shift-space").unwrap();
        let mut hold = Hold::default();
        assert_eq!(hold.key_down(&shortcut, &key, false), (true, Effect::Arm));
        assert_eq!(hold.key_down(&shortcut, &key, true), (true, Effect::None));
        assert_eq!(
            hold.modifiers(&shortcut, Modifiers::none(), false, false),
            Effect::Finish
        );
        assert_eq!(hold.key_up(&shortcut, &key), Effect::None);
        assert_eq!(hold.key_down(&shortcut, &key, true), (true, Effect::None));
        assert_eq!(hold.key_down(&shortcut, &key, false), (true, Effect::Arm));
        assert_eq!(hold.key_up(&shortcut, &key), Effect::Finish);
        for invalid in [
            "escape",
            "a",
            "shift-a",
            "cmd",
            "ctrl-shift",
            "ctrl-unknown",
            "ctrl-a->x",
        ] {
            assert!(Shortcut::parse(invalid).is_err(), "{invalid}");
        }
        // Reuse GPUI's layout-aware matching, then release the actual key.
        let shortcut = Shortcut::parse("cmd-$").unwrap();
        let mut typed = Keystroke::parse("cmd-alt-ç").unwrap();
        typed.key_char = Some("$".into());
        assert_eq!(hold.key_down(&shortcut, &typed, false), (true, Effect::Arm));
        assert_eq!(hold.key_up(&shortcut, &typed), Effect::Finish);
    }
}
