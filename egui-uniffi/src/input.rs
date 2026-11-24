use three_d::egui;

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
    ///
    /// Control (Command on macOS) with C or X copies or cuts, as Copy and Cut do; with V the overlay asks for the
    /// clipboard ([crate::UiOutput::paste]).
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
    /// Copy the selected text: it comes back in [crate::UiOutput::copy].
    Copy,
    /// Cut the selected text: it comes back in [crate::UiOutput::copy].
    Cut,
    /// Text from the clipboard for the text field with the keyboard.
    Paste {
        /// The text.
        text: String,
    },
    /// An input method (for accents with dead keys, Chinese, Japanese) starts composing.
    ImeStart,
    /// The text the input method composes so far, shown underlined in the field.
    ImeCompose {
        /// The text.
        text: String,
    },
    /// The input method is done: the field takes the text.
    ImeCommit {
        /// The text.
        text: String,
    },
    /// The input method stopped.
    ImeEnd,
}

/// What the overlay asks of the platform beyond egui's events: the clipboard's text.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct Wanted {
    pub paste: bool,
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
    pub fn modifiers(&self) -> egui::Modifiers {
        egui::Modifiers {
            alt: self.alt,
            ctrl: self.control,
            shift: self.shift,
            mac_cmd: cfg!(target_os = "macos") && self.meta,
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

    /// egui's events of `input` at `pixels_per_point`; what it asks of the platform goes to `wanted`.
    pub fn events(
        &mut self,
        input: UiInput,
        pixels_per_point: f32,
        wanted: &mut Wanted,
    ) -> Vec<egui::Event> {
        let pos = |x: f32, y: f32| egui::pos2(x / pixels_per_point, y / pixels_per_point);
        let button = |b: u32| match b {
            1 => egui::PointerButton::Secondary,
            2 => egui::PointerButton::Middle,
            _ => egui::PointerButton::Primary,
        };
        let modifiers = self.modifiers();
        let event = match input {
            UiInput::Press { x, y, button: b } | UiInput::Release { x, y, button: b } => {
                egui::Event::PointerButton {
                    pos: pos(x, y),
                    button: button(b),
                    pressed: matches!(input, UiInput::Press { .. }),
                    modifiers,
                }
            }
            UiInput::Move { x, y } => egui::Event::PointerMoved(pos(x, y)),
            // egui-winit takes 50 points per step of the wheel
            UiInput::Wheel { dx, dy, .. } => egui::Event::MouseWheel {
                delta: egui::vec2(dx * 50.0, dy * 50.0),
                unit: egui::MouseWheelUnit::Point,
                modifiers,
                phase: egui::TouchPhase::Move,
            },
            UiInput::Key { key, down } => {
                if self.key(&key, down) {
                    return Vec::new();
                }
                let Some(key) = key_named(&key) else {
                    return Vec::new();
                };
                // The clipboard keys as egui-winit has them
                if down {
                    use egui::Key;
                    let command = modifiers.command;
                    if key == Key::Cut
                        || (command && key == Key::X)
                        || (modifiers.shift && key == Key::Delete)
                    {
                        return vec![egui::Event::Cut];
                    }
                    if key == Key::Copy || (command && (key == Key::C || key == Key::Insert)) {
                        return vec![egui::Event::Copy];
                    }
                    if key == Key::Paste
                        || (command && key == Key::V)
                        || (modifiers.shift && key == Key::Insert)
                    {
                        wanted.paste = true;
                        return Vec::new();
                    }
                }
                egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: down,
                    repeat: false,
                    modifiers,
                }
            }
            UiInput::Text { text } => egui::Event::Text(text),
            UiInput::Leave => egui::Event::PointerGone,
            UiInput::Copy => egui::Event::Copy,
            UiInput::Cut => egui::Event::Cut,
            UiInput::Paste { text } => egui::Event::Paste(text),
            UiInput::ImeStart => egui::Event::Ime(egui::ImeEvent::Enabled),
            UiInput::ImeCompose { text } => egui::Event::Ime(egui::ImeEvent::Preedit(text)),
            UiInput::ImeCommit { text } => egui::Event::Ime(egui::ImeEvent::Commit(text)),
            UiInput::ImeEnd => egui::Event::Ime(egui::ImeEvent::Disabled),
        };
        vec![event]
    }
}

/// egui's key of a Compose key name.
pub(crate) fn key_named(name: &str) -> Option<egui::Key> {
    use egui::Key::*;
    let letters = [
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    ];
    let digits = [Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9];
    let functions = [
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18, F19, F20,
    ];
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
        "Comma" | "," => Comma,
        "Period" | "." => Period,
        "Slash" | "/" => Slash,
        "Backslash" | "\\" => Backslash,
        "Semicolon" | ";" => Semicolon,
        "Quote" | "'" => Quote,
        "Backtick" | "`" => Backtick,
        "OpenBracket" | "[" => OpenBracket,
        "CloseBracket" | "]" => CloseBracket,
        _ => return None,
    })
}
