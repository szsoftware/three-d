//!
//! egui over a three-d picture, described from Kotlin (or any other language uniffi generates bindings for).
//!
//! Kotlin describes panels, windows and their widgets as a flat list of [UiNode]s, each with an id and the id of its
//! parent, and hands it to an [EguiOverlay] when its state changed. Rust draws that tree with egui in every frame, in the
//! same picture as the 3D view (so it lies over it on every platform, also where Compose cannot draw over a canvas), and
//! notes what the user did as [UiEvent]s with the ids of the nodes. Kotlin takes them after the frame and calls its
//! lambdas; how that reads in Kotlin (`SidePanel { Checkbox("Shadows", checked) { … } }`) is the business of a DSL on top
//! of the generated bindings.
//!
//! A value the user changes shows at once: the overlay keeps it until Kotlin took the event and sent its next tree, then
//! Kotlin's value counts. So Kotlin owns the state, as in Compose, and may refuse a change by sending the old value.
//!
//! The application paints the overlay over its picture with [EguiOverlay::paint] from Rust, on the thread of its three-d
//! context: an application crate does it after its own drawing, three-d-raytrace-uniffi's view will do it itself. Both
//! three-d backends work: OpenGL/WebGL with egui_glow, wgpu with three-d's own painter.
//!

uniffi::setup_scaffolding!();

mod draw;
mod error;
mod input;
mod output;
mod overlay;
mod pictures;
mod state;
#[cfg(test)]
mod tests;
mod tree;

pub use error::*;
pub use input::UiInput;
pub use output::{UiCursor, UiLink, UiOutput, UiTextInput};
pub use overlay::*;
pub use state::{UiEvent, UiPoint, UiRect};
pub(crate) use tree::Tree;
pub use tree::{
    Align, Anchor, Arrangement, CanvasShape, CanvasStroke, ColumnWidth, ImageFit, NoticeLevel,
    PlotLineStyle, PlotSeries, PointShape, PopupClose, PopupSide, ScrollDirection, SeriesKind,
    Side, SliderClamping, SliderHandle, StripSize, TableColumn, TableSelection, TableSort,
    TextSpan, TextStyle, TextWrap, UiDate, UiInsets, UiModifier, UiNode, UiShadow, UiStroke,
    Widget,
};
