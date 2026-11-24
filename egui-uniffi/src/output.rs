//! What egui asks of the platform: the clipboard, links to open, the look of the pointer, where text is typed.

use crate::UiRect;
use three_d::egui;

/// The look of the pointer over the overlay, egui's `CursorIcon`; the comment names the CSS cursor of each (Compose
/// desktop: `PointerIcon(java.awt.Cursor)`, Android: `PointerIcon`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UiCursor {
    /// CSS `default`: the application decides.
    #[default]
    Default,
    /// CSS `none`.
    None,
    /// CSS `context-menu`.
    ContextMenu,
    /// CSS `help`.
    Help,
    /// CSS `pointer`.
    PointingHand,
    /// CSS `progress`.
    Progress,
    /// CSS `wait`.
    Wait,
    /// CSS `cell`.
    Cell,
    /// CSS `crosshair`.
    Crosshair,
    /// CSS `text`.
    Text,
    /// CSS `vertical-text`.
    VerticalText,
    /// CSS `alias`.
    Alias,
    /// CSS `copy`.
    Copy,
    /// CSS `move`.
    Move,
    /// CSS `no-drop`.
    NoDrop,
    /// CSS `not-allowed`.
    NotAllowed,
    /// CSS `grab`.
    Grab,
    /// CSS `grabbing`.
    Grabbing,
    /// CSS `all-scroll`.
    AllScroll,
    /// CSS `ew-resize`.
    ResizeHorizontal,
    /// CSS `nesw-resize`.
    ResizeNeSw,
    /// CSS `nwse-resize`.
    ResizeNwSe,
    /// CSS `ns-resize`.
    ResizeVertical,
    /// CSS `e-resize`.
    ResizeEast,
    /// CSS `se-resize`.
    ResizeSouthEast,
    /// CSS `s-resize`.
    ResizeSouth,
    /// CSS `sw-resize`.
    ResizeSouthWest,
    /// CSS `w-resize`.
    ResizeWest,
    /// CSS `nw-resize`.
    ResizeNorthWest,
    /// CSS `n-resize`.
    ResizeNorth,
    /// CSS `ne-resize`.
    ResizeNorthEast,
    /// CSS `col-resize`.
    ResizeColumn,
    /// CSS `row-resize`.
    ResizeRow,
    /// CSS `zoom-in`.
    ZoomIn,
    /// CSS `zoom-out`.
    ZoomOut,
}

impl From<egui::CursorIcon> for UiCursor {
    fn from(icon: egui::CursorIcon) -> Self {
        match icon {
            egui::CursorIcon::Default => Self::Default,
            egui::CursorIcon::None => Self::None,
            egui::CursorIcon::ContextMenu => Self::ContextMenu,
            egui::CursorIcon::Help => Self::Help,
            egui::CursorIcon::PointingHand => Self::PointingHand,
            egui::CursorIcon::Progress => Self::Progress,
            egui::CursorIcon::Wait => Self::Wait,
            egui::CursorIcon::Cell => Self::Cell,
            egui::CursorIcon::Crosshair => Self::Crosshair,
            egui::CursorIcon::Text => Self::Text,
            egui::CursorIcon::VerticalText => Self::VerticalText,
            egui::CursorIcon::Alias => Self::Alias,
            egui::CursorIcon::Copy => Self::Copy,
            egui::CursorIcon::Move => Self::Move,
            egui::CursorIcon::NoDrop => Self::NoDrop,
            egui::CursorIcon::NotAllowed => Self::NotAllowed,
            egui::CursorIcon::Grab => Self::Grab,
            egui::CursorIcon::Grabbing => Self::Grabbing,
            egui::CursorIcon::AllScroll => Self::AllScroll,
            egui::CursorIcon::ResizeHorizontal => Self::ResizeHorizontal,
            egui::CursorIcon::ResizeNeSw => Self::ResizeNeSw,
            egui::CursorIcon::ResizeNwSe => Self::ResizeNwSe,
            egui::CursorIcon::ResizeVertical => Self::ResizeVertical,
            egui::CursorIcon::ResizeEast => Self::ResizeEast,
            egui::CursorIcon::ResizeSouthEast => Self::ResizeSouthEast,
            egui::CursorIcon::ResizeSouth => Self::ResizeSouth,
            egui::CursorIcon::ResizeSouthWest => Self::ResizeSouthWest,
            egui::CursorIcon::ResizeWest => Self::ResizeWest,
            egui::CursorIcon::ResizeNorthWest => Self::ResizeNorthWest,
            egui::CursorIcon::ResizeNorth => Self::ResizeNorth,
            egui::CursorIcon::ResizeNorthEast => Self::ResizeNorthEast,
            egui::CursorIcon::ResizeColumn => Self::ResizeColumn,
            egui::CursorIcon::ResizeRow => Self::ResizeRow,
            egui::CursorIcon::ZoomIn => Self::ZoomIn,
            egui::CursorIcon::ZoomOut => Self::ZoomOut,
        }
    }
}

impl From<UiCursor> for egui::CursorIcon {
    fn from(cursor: UiCursor) -> Self {
        match cursor {
            UiCursor::Default => Self::Default,
            UiCursor::None => Self::None,
            UiCursor::ContextMenu => Self::ContextMenu,
            UiCursor::Help => Self::Help,
            UiCursor::PointingHand => Self::PointingHand,
            UiCursor::Progress => Self::Progress,
            UiCursor::Wait => Self::Wait,
            UiCursor::Cell => Self::Cell,
            UiCursor::Crosshair => Self::Crosshair,
            UiCursor::Text => Self::Text,
            UiCursor::VerticalText => Self::VerticalText,
            UiCursor::Alias => Self::Alias,
            UiCursor::Copy => Self::Copy,
            UiCursor::Move => Self::Move,
            UiCursor::NoDrop => Self::NoDrop,
            UiCursor::NotAllowed => Self::NotAllowed,
            UiCursor::Grab => Self::Grab,
            UiCursor::Grabbing => Self::Grabbing,
            UiCursor::AllScroll => Self::AllScroll,
            UiCursor::ResizeHorizontal => Self::ResizeHorizontal,
            UiCursor::ResizeNeSw => Self::ResizeNeSw,
            UiCursor::ResizeNwSe => Self::ResizeNwSe,
            UiCursor::ResizeVertical => Self::ResizeVertical,
            UiCursor::ResizeEast => Self::ResizeEast,
            UiCursor::ResizeSouthEast => Self::ResizeSouthEast,
            UiCursor::ResizeSouth => Self::ResizeSouth,
            UiCursor::ResizeSouthWest => Self::ResizeSouthWest,
            UiCursor::ResizeWest => Self::ResizeWest,
            UiCursor::ResizeNorthWest => Self::ResizeNorthWest,
            UiCursor::ResizeNorth => Self::ResizeNorth,
            UiCursor::ResizeNorthEast => Self::ResizeNorthEast,
            UiCursor::ResizeColumn => Self::ResizeColumn,
            UiCursor::ResizeRow => Self::ResizeRow,
            UiCursor::ZoomIn => Self::ZoomIn,
            UiCursor::ZoomOut => Self::ZoomOut,
        }
    }
}

/// A link the user clicked, for the application to open.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UiLink {
    /// The address.
    pub url: String,
    /// In a new tab of the browser (or window), else in this one.
    pub new_tab: bool,
}

/// A text field has the keyboard: where it is, for the box of an input method and the keyboard of a touch screen.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct UiTextInput {
    /// The field, in pixels from the top left.
    pub area: UiRect,
    /// The text cursor in it: a thin rectangle.
    pub cursor: UiRect,
}

/// What egui wants of the platform since the last [crate::EguiOverlay::take_output].
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct UiOutput {
    /// Text the user copied or cut, for the clipboard; `None` nothing new.
    pub copy: Option<String>,
    /// The user wants to paste (Control or Command with V): answer with [crate::UiInput::Paste] and the clipboard's text.
    pub paste: bool,
    /// Links the user clicked, oldest first.
    pub open: Vec<UiLink>,
    /// The look of the pointer now.
    pub cursor: UiCursor,
    /// A text field has the keyboard, `None` none: show the keyboard of a touch screen, place the box of an input method.
    pub text_input: Option<UiTextInput>,
}

impl UiOutput {
    /// Adds egui's output of a frame; rectangles from points to pixels.
    pub(crate) fn add(&mut self, output: egui::PlatformOutput, pixels_per_point: f32) {
        for command in output.commands {
            match command {
                egui::OutputCommand::CopyText(text) => self.copy = Some(text),
                egui::OutputCommand::OpenUrl(open) => self.open.push(UiLink {
                    url: open.url,
                    new_tab: open.new_tab,
                }),
                egui::OutputCommand::CopyImage(_) => {}
            }
        }
        self.cursor = output.cursor_icon.into();
        let pixels = |r: egui::Rect| UiRect::from(r * pixels_per_point);
        self.text_input = output.ime.map(|ime| UiTextInput {
            area: pixels(ime.rect),
            cursor: pixels(ime.cursor_rect),
        });
    }

    /// What is new since the last call; the cursor and the text field stay as they are.
    pub(crate) fn take(&mut self) -> Self {
        Self {
            copy: self.copy.take(),
            paste: std::mem::take(&mut self.paste),
            open: std::mem::take(&mut self.open),
            cursor: self.cursor,
            text_input: self.text_input,
        }
    }
}
