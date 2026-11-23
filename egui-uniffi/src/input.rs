use three_d::{Event, Key, Modifiers, MouseButton, PhysicalPoint};

/// A pointer, key or text event of the application for [crate::EguiOverlay::input], in pixels from the top left of the
/// picture as in Compose.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum UiInput {
    /// A button went down: 0 left, 1 right, 2 middle.
    Press {
        /// x in pixels from the left.
        x: f32,
        /// y in pixels from the top.
        y: f32,
        /// 0 left, 1 right, 2 middle.
        button: u32,
    },
    /// A button went up.
    Release {
        /// x in pixels from the left.
        x: f32,
        /// y in pixels from the top.
        y: f32,
        /// 0 left, 1 right, 2 middle.
        button: u32,
    },
    /// The pointer moved.
    Move {
        /// x in pixels from the left.
        x: f32,
        /// y in pixels from the top.
        y: f32,
    },
    /// The wheel turned, in steps of the wheel; positive `dy` away from the user (the content moves down).
    Wheel {
        /// x of the pointer in pixels from the left.
        x: f32,
        /// y of the pointer in pixels from the top.
        y: f32,
        /// Steps to the side.
        dx: f32,
        /// Steps up and down.
        dy: f32,
    },
    /// A key went down or up, named as in Compose and in three-d-raytrace-uniffi: "A" to "Z", "0" to "9", "Enter",
    /// "Backspace", "Delete", "Tab", "Escape", "Space", "ArrowLeft", "Home", "End", "PageUp", "F1" and so on. "Shift",
    /// "Control", "Alt" and "Meta" are the modifiers.
    Key {
        /// The key.
        key: String,
        /// Down or up.
        down: bool,
    },
    /// Characters typed, after the keyboard layout: what a text field takes.
    Text {
        /// The characters.
        text: String,
    },
    /// The pointer left the picture.
    Leave,
}

/// The modifiers held, from the key events.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Held {
    shift: bool,
    control: bool,
    alt: bool,
    meta: bool,
}

impl Held {
    fn modifiers(&self) -> Modifiers {
        Modifiers {
            alt: self.alt,
            ctrl: self.control,
            shift: self.shift,
            command: if cfg!(target_os = "macos") {
                self.meta
            } else {
                self.control
            },
        }
    }

    /// Takes a modifier key, `true` if it was one.
    fn key(&mut self, key: &str, down: bool) -> bool {
        let held = match key {
            "Shift" | "ShiftLeft" | "ShiftRight" => &mut self.shift,
            "Control" | "Ctrl" | "CtrlLeft" | "CtrlRight" => &mut self.control,
            "Alt" | "AltLeft" | "AltRight" => &mut self.alt,
            "Meta" | "MetaLeft" | "MetaRight" | "Command" => &mut self.meta,
            _ => return false,
        };
        *held = down;
        true
    }

    /// The three-d events of `input`, at a picture of `height` pixels (three-d counts from the bottom).
    pub fn events(&mut self, input: UiInput, height: f32) -> Vec<Event> {
        let point = |x: f32, y: f32| PhysicalPoint { x, y: height - y };
        let button = |b: u32| match b {
            1 => MouseButton::Right,
            2 => MouseButton::Middle,
            _ => MouseButton::Left,
        };
        let modifiers = self.modifiers();
        let event = match input {
            UiInput::Press { x, y, button: b } => Event::MousePress {
                button: button(b),
                position: point(x, y),
                modifiers,
                handled: false,
            },
            UiInput::Release { x, y, button: b } => Event::MouseRelease {
                button: button(b),
                position: point(x, y),
                modifiers,
                handled: false,
            },
            UiInput::Move { x, y } => Event::MouseMotion {
                button: None,
                delta: (0.0, 0.0),
                position: point(x, y),
                modifiers,
                handled: false,
            },
            // egui-winit takes 50 points per step of the wheel; three-d's GUI hands the delta over in points
            UiInput::Wheel { x, y, dx, dy } => Event::MouseWheel {
                delta: (dx * 50.0, dy * 50.0),
                position: point(x, y),
                modifiers,
                handled: false,
            },
            UiInput::Key { key, down } => {
                if self.key(&key, down) {
                    return vec![Event::ModifiersChange {
                        modifiers: self.modifiers(),
                    }];
                }
                let Some(kind) = key_named(&key) else {
                    return Vec::new();
                };
                if down {
                    Event::KeyPress {
                        kind,
                        modifiers,
                        handled: false,
                    }
                } else {
                    Event::KeyRelease {
                        kind,
                        modifiers,
                        handled: false,
                    }
                }
            }
            UiInput::Text { text } => Event::Text(text),
            UiInput::Leave => Event::MouseLeave,
        };
        vec![event]
    }
}

/// three-d's key of a Compose key name.
pub(crate) fn key_named(name: &str) -> Option<Key> {
    use Key::*;
    let letters = [
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    ];
    let digits = [Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9];
    let functions = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphabetic() {
            return Some(letters[(c.to_ascii_uppercase() as u8 - b'A') as usize]);
        }
        if let Some(d) = c.to_digit(10) {
            return Some(digits[d as usize]);
        }
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<usize>().ok()) {
        return functions.get(n.wrapping_sub(1)).copied();
    }
    if let Some(n) = name
        .strip_prefix("Num")
        .or_else(|| name.strip_prefix("Digit"))
        .and_then(|n| n.parse::<usize>().ok())
    {
        return digits.get(n).copied();
    }
    Some(match name {
        "ArrowDown" | "DirectionDown" => ArrowDown,
        "ArrowLeft" | "DirectionLeft" => ArrowLeft,
        "ArrowRight" | "DirectionRight" => ArrowRight,
        "ArrowUp" | "DirectionUp" => ArrowUp,
        "Escape" => Escape,
        "Tab" => Tab,
        "Backspace" => Backspace,
        "Enter" | "NumPadEnter" => Enter,
        "Space" | " " => Space,
        "Insert" => Insert,
        "Delete" => Delete,
        "Home" | "MoveHome" => Home,
        "End" | "MoveEnd" => End,
        "PageUp" => PageUp,
        "PageDown" => PageDown,
        "Copy" => Copy,
        "Paste" => Paste,
        "Cut" => Cut,
        "Minus" => Minus,
        "Plus" => Plus,
        "Equals" => Equals,
        _ => return None,
    })
}
