//! Modifier state belongs to each input event, rather than the frame's final key state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Modifiers(pub u8);
impl Modifiers {
    pub const CONTROL_LEFT: u8 = 1;
    pub const CONTROL_RIGHT: u8 = 2;
    pub const SUPER_LEFT: u8 = 4;
    pub const SUPER_RIGHT: u8 = 8;
    pub const SHIFT_LEFT: u8 = 16;
    pub const SHIFT_RIGHT: u8 = 32;
    pub const ALT_LEFT: u8 = 64;
    pub const ALT_RIGHT: u8 = 128;
    pub fn command(self) -> bool {
        self.0 & 15 != 0
    }
    pub fn shift(self) -> bool {
        self.0 & 48 != 0
    }
    pub fn alt(self) -> bool {
        self.0 & 192 != 0
    }
}
#[derive(Clone, Copy)]
pub(super) struct Input {
    pub modifier: Option<u8>,
    pub pressed: bool,
    pub repeat: bool,
}
pub(super) fn states(final_state: Modifiers, events: &[Input]) -> Vec<Modifiers> {
    let mut held = final_state.0;
    // Reconstruct the frame's initial state without retaining modifiers across focus loss.
    for event in events.iter().rev() {
        if !event.repeat
            && let Some(bit) = event.modifier
        {
            if event.pressed {
                held &= !bit;
            } else {
                held |= bit;
            }
        }
    }
    events
        .iter()
        .map(|event| {
            if let Some(bit) = event.modifier {
                if event.pressed {
                    held |= bit;
                } else {
                    held &= !bit;
                }
            }
            Modifiers(held)
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn key(pressed: bool) -> Input {
        Input {
            modifier: None,
            pressed,
            repeat: false,
        }
    }
    fn modifier(bit: u8, pressed: bool) -> Input {
        Input {
            modifier: Some(bit),
            pressed,
            repeat: false,
        }
    }
    #[test]
    fn fast_shortcut_keeps_command_for_z_but_not_following_plain_text() {
        // All events can arrive before one render frame; no modifiers remain held afterward.
        let events = [
            modifier(Modifiers::SUPER_LEFT, true),
            key(true),
            key(false),
            modifier(Modifiers::SUPER_LEFT, false),
            key(true),
        ];
        let at = states(Modifiers(0), &events);
        assert!(
            at[1].command(),
            "the first z is Cmd+Z, even after Super is released in the same frame"
        );
        assert!(!at[4].command(), "the following z must be ordinary text");
    }
    #[test]
    fn redo_selection_and_alt_movement_use_modifiers_at_the_keypress() {
        let events = [
            modifier(Modifiers::CONTROL_LEFT, true),
            modifier(Modifiers::SHIFT_RIGHT, true),
            key(true),
            modifier(Modifiers::CONTROL_LEFT, false),
            key(true),
            modifier(Modifiers::SHIFT_RIGHT, false),
            modifier(Modifiers::ALT_LEFT, true),
            key(true),
            modifier(Modifiers::ALT_LEFT, false),
        ];
        let at = states(Modifiers(0), &events);
        assert!(
            at[2].command() && at[2].shift(),
            "Ctrl+Shift+Z keeps both modifiers"
        );
        assert!(
            !at[4].command() && at[4].shift(),
            "Shift+Arrow keeps selection after Ctrl release"
        );
        assert!(
            at[7].alt() && !at[7].shift(),
            "Alt+Arrow keeps word movement"
        );
    }
    #[test]
    fn modifiers_held_from_previous_frames_and_repeat_are_preserved() {
        let mut repeated = modifier(Modifiers::CONTROL_RIGHT, true);
        repeated.repeat = true;
        let at = states(Modifiers(Modifiers::CONTROL_RIGHT), &[repeated, key(true)]);
        assert!(at[1].command());
        let at = states(
            Modifiers(0),
            &[key(true), modifier(Modifiers::SHIFT_LEFT, false)],
        );
        assert!(
            at[0].shift(),
            "initial state is reconstructed from the later release"
        );
        assert!(!at[1].shift());
    }
}
