use crate::UiError;
use std::collections::{HashMap, HashSet};

/// One node of the tree Kotlin describes: a panel, a container or a widget. The tree is a flat list: every node names its
/// parent, siblings keep the order of the list.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UiNode {
    /// Chosen by Kotlin and stable from one tree to the next (the DSL derives it from the position and `key`), so that egui
    /// keeps focus, scroll positions and open sections. Events name it.
    pub id: u64,
    /// The container this node is in; `None` for the panels, windows and areas at the top.
    pub parent: Option<u64>,
    /// What it is.
    pub widget: Widget,
    /// `false` greys it out (with everything inside); it shows its [UiNode::tooltip] all the same.
    pub enabled: bool,
    /// Shown while the pointer rests on it, for example why it is greyed out.
    pub tooltip: Option<String>,
    /// Size, frame and place of the node in its container, as Compose's `Modifier`; `None` as egui places it.
    #[uniffi(default)]
    pub modifier: Option<UiModifier>,
}

/// Where a node sits across its container (Compose's `Alignment`): in a [Widget::Column] left, centered or right, in a
/// [Widget::Row] at the top, in the middle or at the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Align {
    /// Left, or at the top.
    Start,
    /// Centered.
    Center,
    /// Right, or at the bottom.
    End,
}

impl From<Align> for three_d::egui::Align {
    fn from(a: Align) -> Self {
        match a {
            Align::Start => Self::Min,
            Align::Center => Self::Center,
            Align::End => Self::Max,
        }
    }
}

/// Where the nodes of a [Widget::Row] or [Widget::Column] gather along it (Compose's `Arrangement`), in their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Arrangement {
    /// At the start: left, or at the top.
    Start,
    /// In the middle.
    Center,
    /// At the end: right, or at the bottom.
    End,
}

/// What a [Widget::Text] does when it is wider than its room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TextWrap {
    /// Breaks into lines.
    Wrap,
    /// One line, cut with "…".
    Truncate,
    /// One line, as wide as it is (its container grows).
    Extend,
}

impl From<TextWrap> for three_d::egui::TextWrapMode {
    fn from(w: TextWrap) -> Self {
        match w {
            TextWrap::Wrap => Self::Wrap,
            TextWrap::Truncate => Self::Truncate,
            TextWrap::Extend => Self::Extend,
        }
    }
}

/// Which way a [Widget::ScrollArea] scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ScrollDirection {
    /// Up and down.
    Vertical,
    /// Left and right.
    Horizontal,
    /// Both.
    Both,
}

/// Room on the four sides, in points.
#[derive(Debug, Clone, Copy, Default, PartialEq, uniffi::Record)]
pub struct UiInsets {
    /// Left.
    #[uniffi(default)]
    pub left: f32,
    /// Top.
    #[uniffi(default)]
    pub top: f32,
    /// Right.
    #[uniffi(default)]
    pub right: f32,
    /// Bottom.
    #[uniffi(default)]
    pub bottom: f32,
}

/// A line around a node.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct UiStroke {
    /// In points.
    pub width: f32,
    /// As ARGB.
    pub color: u32,
}

/// A shadow under a node.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct UiShadow {
    /// How far it falls to the right, in points.
    #[uniffi(default)]
    pub dx: f32,
    /// How far it falls down, in points.
    #[uniffi(default)]
    pub dy: f32,
    /// How soft its edge is, in points.
    pub blur: f32,
    /// How much larger than the node it is, in points.
    #[uniffi(default)]
    pub spread: f32,
    /// As ARGB, usually black with some alpha.
    pub color: u32,
}

///
/// Size, frame and place of a node, as Compose's `Modifier`. A node with a background, a border, a shadow or padding
/// stands in a frame of egui: [UiModifier::margin] lies outside the frame, [UiModifier::padding] inside. Sizes count
/// the padding and the border, not the margin (`Modifier.width(120.dp).background(…).padding(8.dp)` of Compose).
///
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct UiModifier {
    /// Exactly this wide; a button or a field fills it.
    #[uniffi(default)]
    pub width: Option<f32>,
    /// Exactly this high.
    #[uniffi(default)]
    pub height: Option<f32>,
    /// At least this wide.
    #[uniffi(default)]
    pub min_width: Option<f32>,
    /// At least this high.
    #[uniffi(default)]
    pub min_height: Option<f32>,
    /// At most this wide: longer text wraps.
    #[uniffi(default)]
    pub max_width: Option<f32>,
    /// At most this high.
    #[uniffi(default)]
    pub max_height: Option<f32>,
    /// All the width of its container (Compose's `fillMaxWidth`).
    #[uniffi(default)]
    pub fill_width: bool,
    /// All the height of its container that is left (`fillMaxHeight`); in a scrolling container the height of the node.
    #[uniffi(default)]
    pub fill_height: bool,
    /// Room around the frame.
    #[uniffi(default)]
    pub margin: Option<UiInsets>,
    /// Room between the frame and the node.
    #[uniffi(default)]
    pub padding: Option<UiInsets>,
    /// Fills the frame, as ARGB.
    #[uniffi(default)]
    pub background: Option<u32>,
    /// A line around the frame.
    #[uniffi(default)]
    pub border: Option<UiStroke>,
    /// Rounds the corners of the frame, in points.
    #[uniffi(default)]
    pub corner_radius: f32,
    /// A shadow under the frame.
    #[uniffi(default)]
    pub shadow: Option<UiShadow>,
    /// Where it sits across its container, `None` where the container puts its nodes.
    #[uniffi(default)]
    pub align: Option<Align>,
    /// From 0 (not to be seen) to 1, with everything inside.
    #[uniffi(default)]
    pub opacity: Option<f32>,
    /// Keeps its room but is neither drawn nor reacts (Compose's `alpha(0f)` without the clicks).
    #[uniffi(default)]
    pub invisible: bool,
    /// Reports when it gets and loses the keyboard: [crate::UiEvent::FocusChanged] (Compose's `onFocusChanged`).
    #[uniffi(default)]
    pub focus_events: bool,
}

impl UiModifier {
    /// Stands in a frame of egui.
    pub(crate) fn has_frame(&self) -> bool {
        self.margin.is_some()
            || self.padding.is_some()
            || self.background.is_some()
            || self.border.is_some()
            || self.shadow.is_some()
    }

    /// Has a size of its own.
    pub(crate) fn has_size(&self) -> bool {
        self.width.is_some()
            || self.height.is_some()
            || self.min_width.is_some()
            || self.min_height.is_some()
            || self.max_width.is_some()
            || self.max_height.is_some()
            || self.fill_width
            || self.fill_height
    }

    /// Why its values do not fit together, if they do not.
    fn check(&self) -> Option<String> {
        let sizes = [
            ("width", self.width),
            ("height", self.height),
            ("min_width", self.min_width),
            ("min_height", self.min_height),
            ("max_width", self.max_width),
            ("max_height", self.max_height),
        ];
        if let Some((name, v)) = sizes.iter().find_map(|(name, v)| {
            v.filter(|v| !(v.is_finite() && *v >= 0.0))
                .map(|v| (name, v))
        }) {
            return Some(format!(
                "the modifier's {name} is {v}, it needs a size of 0 or more"
            ));
        }
        let pairs = [
            ("width", self.min_width, self.max_width),
            ("height", self.min_height, self.max_height),
        ];
        if let Some((name, min, max)) = pairs.iter().find_map(|(name, min, max)| match (min, max) {
            (Some(min), Some(max)) if min > max => Some((name, min, max)),
            _ => None,
        }) {
            return Some(format!(
                "the modifier's min_{name} {min} > max_{name} {max}"
            ));
        }
        self.opacity
            .filter(|o| !(0.0..=1.0).contains(o))
            .map(|o| format!("the modifier's opacity is {o}, it goes from 0 to 1"))
    }
}

/// A part of a [Widget::Text] in its own style (a span of Compose's `AnnotatedString`). What it leaves out comes from
/// the text: its style and its color.
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct TextSpan {
    /// What it says.
    pub text: String,
    /// Heading, bold, small …, `None` that of the text.
    #[uniffi(default)]
    pub style: Option<TextStyle>,
    /// As ARGB, `None` that of the text.
    #[uniffi(default)]
    pub color: Option<u32>,
    /// Behind the letters, as ARGB.
    #[uniffi(default)]
    pub background: Option<u32>,
    /// The height of the letters in points, `None` that of the style.
    #[uniffi(default)]
    pub size: Option<f32>,
    /// Slanted.
    #[uniffi(default)]
    pub italic: bool,
    /// With a line under it.
    #[uniffi(default)]
    pub underline: bool,
    /// With a line through it.
    #[uniffi(default)]
    pub strikethrough: bool,
    /// As code: fixed width on a background.
    #[uniffi(default)]
    pub code: bool,
    /// Small and raised, as an exponent.
    #[uniffi(default)]
    pub raised: bool,
}

/// Which side of the picture a [Widget::SidePanel] is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Side {
    /// The left edge.
    Left,
    /// The right edge.
    Right,
}

/// Where a [Widget::Area] is pinned: a corner, the middle of an edge or the center of the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Anchor {
    /// Top left corner.
    TopLeft,
    /// Middle of the top edge.
    Top,
    /// Top right corner.
    TopRight,
    /// Middle of the left edge.
    Left,
    /// Center of the picture.
    Center,
    /// Middle of the right edge.
    Right,
    /// Bottom left corner.
    BottomLeft,
    /// Middle of the bottom edge.
    Bottom,
    /// Bottom right corner.
    BottomRight,
}

/// How a [Widget::Text] looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TextStyle {
    /// Normal text.
    Body,
    /// A heading.
    Heading,
    /// Bold.
    Strong,
    /// Smaller.
    Small,
    /// Fainter.
    Weak,
    /// Fixed width, for numbers in columns (frame times).
    Monospace,
}

/// How urgent a [Widget::Notice] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NoticeLevel {
    /// In the normal text color.
    Info,
    /// In the warning color, for example "takes effect after a restart".
    Warning,
    /// In the error color.
    Error,
}

/// How a [PlotSeries] is drawn.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum SeriesKind {
    /// The points joined by a line.
    Line {
        /// In points.
        width: f32,
        /// Shades the area between the line and 0.
        fill: bool,
    },
    /// Each point as a dot.
    Points {
        /// In points.
        radius: f32,
    },
    /// A bar per value, standing on 0.
    Bars {
        /// In units of the x axis (1 is as wide as the step from one category to the next).
        width: f64,
        /// A group: the bars of the series with the same group stand on each other in the order of the series (bar i
        /// on bar i), as the parts of a frame time.
        stack: Option<String>,
    },
    /// A box per five values of [PlotSeries::y] (lower whisker, first quartile, median, third quartile, upper
    /// whisker), at one value of [PlotSeries::x] each: how frame times spread.
    Boxes {
        /// In units of the x axis.
        width: f64,
    },
    /// A dashed line across the whole plot at each value of [PlotSeries::y], for example the 16.7 ms of 60 frames per
    /// second; [PlotSeries::x] stays empty.
    HLine,
}

/// One line, set of dots, bars or boxes of a [Widget::Plot].
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PlotSeries {
    /// In the legend and when the pointer rests on it; unique within the plot (the legend hides series by name).
    pub name: String,
    /// How it is drawn.
    pub kind: SeriesKind,
    /// Where on the x axis, empty for 0, 1, 2 … (the categories of [Widget::Plot]).
    pub x: Vec<f64>,
    /// The values; five per box for [SeriesKind::Boxes].
    pub y: Vec<f64>,
    /// As ARGB, `None` the next color of egui_plot.
    pub color: Option<u32>,
}

/// Where a slider keeps its value while the user types another or Kotlin sends one outside its range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SliderClamping {
    /// Never: values outside stay as they are.
    Never,
    /// What the user types is brought into the range, values from Kotlin stay.
    Edits,
    /// Always into the range (egui's default).
    Always,
}

/// The handle of a slider.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum SliderHandle {
    /// Round (egui's default).
    Circle,
    /// A bar, `aspect_ratio` as wide as high (0.5 is half as wide).
    Rect {
        /// Width by height.
        aspect_ratio: f32,
    },
}

/// A day of the calendar, for [Widget::DatePicker].
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct UiDate {
    /// The year, for example 2026.
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    /// 1 to 31.
    pub day: u32,
}

/// What a node is. The panels, windows and areas stand at the top of the tree (no parent), the containers hold other
/// nodes, the rest are widgets. Sizes are in points of egui (pixels divided by [crate::EguiOverlay::set_scale]).
///
/// Fields added later have defaults, so that Kotlin code written before them still builds.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Widget {
    /// A panel along the left or right edge over the full height.
    SidePanel {
        /// Left or right.
        side: Side,
        /// A heading at the top, if any.
        title: Option<String>,
        /// The width to start with.
        width: f32,
        /// Whether the user may drag its edge.
        resizable: bool,
        /// The narrowest the user may drag it to.
        #[uniffi(default)]
        min_width: Option<f32>,
        /// The widest the user may drag it to.
        #[uniffi(default)]
        max_width: Option<f32>,
        /// `false` folds it away to the edge, animated; it comes back with `true`.
        #[uniffi(default = true)]
        expanded: bool,
    },
    /// A bar along the top edge; its nodes stand side by side.
    TopBar {
        /// `false` folds it away to the edge, animated.
        #[uniffi(default = true)]
        expanded: bool,
    },
    /// A bar along the bottom edge; its nodes stand side by side.
    BottomBar {
        /// `false` folds it away to the edge, animated.
        #[uniffi(default = true)]
        expanded: bool,
    },
    /// All the room the bars and side panels leave, for an interface without a 3D view behind it (or with one in a
    /// [Widget::Window]). Drawn after them, at most one.
    CentralPanel {
        /// The background of a panel; `false` leaves the picture visible, and only its nodes cover it.
        #[uniffi(default = true)]
        frame: bool,
    },
    /// A window the user can move.
    Window {
        /// Its title bar.
        title: String,
        /// Where it starts, from the left.
        x: f32,
        /// Where it starts, from the top.
        y: f32,
        /// The width to start with, `None` as wide as its content.
        width: Option<f32>,
        /// A close button in the title bar, which sends [crate::UiEvent::Closed] and hides the window until the next tree.
        closable: bool,
        /// The title bar folds it up.
        collapsible: bool,
        /// The user may drag its edges.
        resizable: bool,
        /// The height to start with, `None` as high as its content.
        #[uniffi(default)]
        height: Option<f32>,
        /// Pins it to a corner, an edge or the center, with [Widget::Window::x] and [Widget::Window::y] as the distance
        /// from there; it cannot be moved then.
        #[uniffi(default)]
        anchor: Option<Anchor>,
        /// The user may drag it by its title bar.
        #[uniffi(default = true)]
        movable: bool,
        /// With a title bar; without, it has no close button and does not fold.
        #[uniffi(default = true)]
        title_bar: bool,
        /// Exactly [Widget::Window::width] wide and [Widget::Window::height] high, whatever its content.
        #[uniffi(default)]
        fixed_size: bool,
        /// Its content scrolls up and down when the window is lower than it.
        #[uniffi(default)]
        scroll: bool,
        /// `false` hides it, fading out, and keeps where it was and how large; `true` fades it in again.
        #[uniffi(default = true)]
        open: bool,
    },
    /// Nodes pinned to a corner, an edge or the center, over everything else: a frame time, a hint.
    Area {
        /// Where.
        anchor: Anchor,
        /// Moves it to the right (negative: left).
        dx: f32,
        /// Moves it down (negative: up).
        dy: f32,
        /// A background with a border, as a popup has.
        frame: bool,
    },
    /// Its nodes side by side.
    Row {
        /// Goes on in a new line where the room ends (egui's `horizontal_wrapped`), as words do.
        #[uniffi(default)]
        wrap: bool,
        /// Where its nodes sit across it, `None` in the middle.
        #[uniffi(default)]
        align: Option<Align>,
        /// Where its nodes gather along it, `None` at the start.
        #[uniffi(default)]
        arrangement: Option<Arrangement>,
        /// The room between its nodes in points, `None` egui's.
        #[uniffi(default)]
        spacing: Option<f32>,
    },
    /// Its nodes one below the other (as in a panel).
    Column {
        /// Where its nodes sit across it, `None` at the start (left).
        #[uniffi(default)]
        align: Option<Align>,
        /// Where its nodes gather along it, `None` at the top; the middle and the end only where the height is known
        /// (a panel, a sized node), not in a [Widget::ScrollArea].
        #[uniffi(default)]
        arrangement: Option<Arrangement>,
        /// The room between its nodes in points, `None` egui's.
        #[uniffi(default)]
        spacing: Option<f32>,
    },
    /// Each of its nodes in a column of its own, all as wide (egui's `columns`).
    Columns,
    /// Two nodes, the first at the left and the second at the right edge, with the room between them (egui's `Sides`):
    /// a title and its buttons.
    Sides,
    /// Its nodes one below the other, moved to the right as under a heading.
    Indent,
    /// Its nodes in a box whose lower right corner the user can drag.
    Resize {
        /// The width to start with.
        width: f32,
        /// The height to start with.
        height: f32,
    },
    /// Its nodes in a table, row by row: a new row after every `columns` nodes.
    Grid {
        /// Nodes per row, at least 1.
        columns: u32,
        /// Every other row on a darker background.
        striped: bool,
        /// The room between the columns in points, `None` egui's.
        #[uniffi(default)]
        column_spacing: Option<f32>,
        /// The room between the rows in points, `None` egui's.
        #[uniffi(default)]
        row_spacing: Option<f32>,
        /// Each column at least this wide.
        #[uniffi(default)]
        min_column_width: Option<f32>,
    },
    /// A heading that folds its nodes in and out; egui remembers whether it is open.
    Collapsible {
        /// The heading.
        title: String,
        /// Open the first time it is shown.
        open: bool,
    },
    /// Its nodes scroll up and down (or sideways).
    ScrollArea {
        /// The height it may take at most, `None` all there is.
        max_height: Option<f32>,
        /// Which way it scrolls, `None` up and down.
        #[uniffi(default)]
        direction: Option<ScrollDirection>,
        /// The width it may take at most, `None` all there is.
        #[uniffi(default)]
        max_width: Option<f32>,
        /// Stays at the end while new nodes come at the end, as a log does, until the user scrolls away.
        #[uniffi(default)]
        stick_to_end: bool,
    },
    /// Its nodes in a frame.
    Group,
    /// Text, in one style or in parts of their own styles.
    Text {
        /// What it says; [Widget::Text::spans] follow it.
        text: String,
        /// How it looks.
        style: TextStyle,
        /// Its color as ARGB (Compose's `Color.toArgb()`), `None` the color of the style.
        color: Option<u32>,
        /// Parts in their own styles after [Widget::Text::text], as Compose's `AnnotatedString`: "frame time **12.9 ms**".
        #[uniffi(default)]
        spans: Vec<TextSpan>,
        /// What happens when it is wider than its room, `None` what its container does (a panel wraps, an area extends).
        #[uniffi(default)]
        wrap: Option<TextWrap>,
        /// The user can select it with the pointer and copy it.
        #[uniffi(default = true)]
        selectable: bool,
    },
    /// A line between the nodes before and after it.
    Separator,
    /// Empty space.
    Spacer {
        /// How much, in points.
        size: f32,
    },
    /// A button: [crate::UiEvent::Clicked].
    Button {
        /// Its text.
        text: String,
        /// Smaller, without room around the text, for toolbars.
        #[uniffi(default)]
        small: bool,
        /// `false` without a background until the pointer is on it, as Compose's `TextButton`.
        #[uniffi(default = true)]
        frame: bool,
        /// Drawn as chosen, as a pressed button of a group.
        #[uniffi(default)]
        selected: bool,
        /// A key on its right, faint, for example "Ctrl+S" (the key itself is a [Widget::Shortcut]).
        #[uniffi(default)]
        shortcut: Option<String>,
        /// Its background as ARGB, `None` egui's.
        #[uniffi(default)]
        fill: Option<u32>,
    },
    /// A text that is chosen or not, as a tab or an entry of a list: [crate::UiEvent::Clicked].
    SelectableLabel {
        /// Its text.
        text: String,
        /// Drawn as chosen.
        selected: bool,
    },
    /// A button that stays pressed until it is clicked again (egui's `toggle_value`): [crate::UiEvent::Checked].
    ToggleButton {
        /// Its text.
        text: String,
        /// Pressed.
        checked: bool,
    },
    /// One round button of a choice, the others are their own nodes (Compose's `RadioButton`): [crate::UiEvent::Clicked].
    RadioButton {
        /// Its text.
        text: String,
        /// The chosen one.
        selected: bool,
    },
    /// A switch that slides on and off, as Compose's `Switch`: [crate::UiEvent::Checked].
    Switch {
        /// Beside it, empty for none.
        text: String,
        /// On.
        checked: bool,
    },
    /// A box to tick: [crate::UiEvent::Checked].
    Checkbox {
        /// Its text.
        text: String,
        /// Ticked.
        checked: bool,
        /// Neither ticked nor not, with a dash: for a box over others of which some are ticked. A click ticks it.
        #[uniffi(default)]
        indeterminate: bool,
    },
    /// One of several options with round buttons: [crate::UiEvent::Selected].
    Radio {
        /// The options.
        options: Vec<String>,
        /// The index of the chosen one.
        selected: u32,
        /// Side by side instead of one below the other.
        horizontal: bool,
    },
    /// One of several options in a list that drops down: [crate::UiEvent::Selected].
    ComboBox {
        /// The text beside it, empty for none.
        label: String,
        /// The options.
        options: Vec<String>,
        /// The index of the chosen one.
        selected: u32,
        /// Its width, `None` egui's.
        width: Option<f32>,
    },
    /// A whole number on a slider: [crate::UiEvent::IntChanged].
    SliderInt {
        /// The text beside it, empty for none.
        label: String,
        /// The number.
        value: i64,
        /// The smallest.
        min: i64,
        /// The largest.
        max: i64,
        /// Logarithmic, for ranges over several powers of ten.
        #[uniffi(default)]
        logarithmic: bool,
        /// After the number, for example " px".
        #[uniffi(default)]
        suffix: String,
        /// Before the number, for example "x ".
        #[uniffi(default)]
        prefix: String,
        /// The value goes in steps of this size, `None` freely.
        #[uniffi(default)]
        step: Option<f64>,
        /// Upright, the smallest value at the bottom.
        #[uniffi(default)]
        vertical: bool,
        /// Where values outside the range go, `None` [SliderClamping::Always].
        #[uniffi(default)]
        clamping: Option<SliderClamping>,
        /// The number beside the slider, to type into.
        #[uniffi(default = true)]
        show_value: bool,
        /// The track up to the handle in the accent color, `None` as egui's style says.
        #[uniffi(default)]
        trailing_fill: Option<bool>,
        /// The handle, `None` round.
        #[uniffi(default)]
        handle: Option<SliderHandle>,
    },
    /// A number on a slider: [crate::UiEvent::FloatChanged].
    SliderFloat {
        /// The text beside it, empty for none.
        label: String,
        /// The number.
        value: f64,
        /// The smallest.
        min: f64,
        /// The largest.
        max: f64,
        /// Logarithmic, for ranges over several powers of ten.
        logarithmic: bool,
        /// Digits after the point, `None` as many as egui finds useful.
        decimals: Option<u32>,
        /// After the number, for example " ms".
        suffix: String,
        /// Before the number, for example "x ".
        #[uniffi(default)]
        prefix: String,
        /// The value goes in steps of this size, `None` freely.
        #[uniffi(default)]
        step: Option<f64>,
        /// Upright, the smallest value at the bottom.
        #[uniffi(default)]
        vertical: bool,
        /// Where values outside the range go, `None` [SliderClamping::Always].
        #[uniffi(default)]
        clamping: Option<SliderClamping>,
        /// The number beside the slider, to type into.
        #[uniffi(default = true)]
        show_value: bool,
        /// The track up to the handle in the accent color, `None` as egui's style says.
        #[uniffi(default)]
        trailing_fill: Option<bool>,
        /// The handle, `None` round.
        #[uniffi(default)]
        handle: Option<SliderHandle>,
    },
    /// A number that changes by dragging it or typing into it: [crate::UiEvent::FloatChanged].
    DragValue {
        /// The text beside it, empty for none.
        label: String,
        /// The number.
        value: f64,
        /// How much it changes per point dragged.
        speed: f64,
        /// The smallest, `None` no limit.
        min: Option<f64>,
        /// The largest, `None` no limit.
        max: Option<f64>,
        /// Digits after the point, `None` as many as egui finds useful.
        decimals: Option<u32>,
        /// After the number, for example " °".
        suffix: String,
        /// Before the number.
        #[uniffi(default)]
        prefix: String,
        /// As a whole number in hexadecimal, for colors and bit masks.
        #[uniffi(default)]
        hexadecimal: bool,
        /// An angle: the value in radians, shown and dragged in degrees (egui's `drag_angle`).
        #[uniffi(default)]
        angle: bool,
    },
    /// One line to type into: [crate::UiEvent::TextChanged] while typing, [crate::UiEvent::Submitted] on Enter.
    TextField {
        /// The text beside it, empty for none.
        label: String,
        /// What it holds.
        value: String,
        /// Shown faintly while it is empty.
        hint: String,
        /// Shows dots instead of the characters.
        password: bool,
        /// Several lines, this many high to start with; Enter makes a new line, Ctrl+Enter reports
        /// [crate::UiEvent::Submitted]. `None` one line.
        #[uniffi(default)]
        lines: Option<u32>,
        /// Code: fixed width, Tab indents.
        #[uniffi(default)]
        code: bool,
        /// Colors the code as this language (egui_extras: "rs", "toml", "py", "c", "cpp"; more with the feature
        /// `syntect`), `None` plain.
        #[uniffi(default)]
        language: Option<String>,
        /// At most this many characters.
        #[uniffi(default)]
        char_limit: Option<u32>,
        /// The text at the start, in the middle or at the end, `None` at the start.
        #[uniffi(default)]
        align: Option<Align>,
        /// To read and select, not to change.
        #[uniffi(default)]
        read_only: bool,
    },
    /// A color to choose: a button with the color that opens egui's picker (hue, saturation, value, alpha):
    /// [crate::UiEvent::ColorChanged].
    ColorPicker {
        /// Beside it, empty for none.
        label: String,
        /// ARGB.
        color: u32,
        /// With alpha, else always opaque.
        #[uniffi(default)]
        alpha: bool,
        /// The picker itself in the panel instead of a button.
        #[uniffi(default)]
        inline: bool,
    },
    /// A day to choose: a button with the date that opens a calendar (egui_extras): [crate::UiEvent::DateChanged].
    DatePicker {
        /// Beside it, empty for none.
        label: String,
        /// The day.
        date: UiDate,
        /// How the button shows it, as strftime ("%d.%m.%Y"), `None` "%Y-%m-%d".
        #[uniffi(default)]
        format: Option<String>,
        /// Numbers of the weeks in the calendar.
        #[uniffi(default = true)]
        calendar_week: bool,
        /// Saturdays and Sundays in another color.
        #[uniffi(default = true)]
        highlight_weekends: bool,
        /// The first year to choose from, `None` 100 years ago.
        #[uniffi(default)]
        first_year: Option<i32>,
        /// The last year to choose from, `None` in 10 years.
        #[uniffi(default)]
        last_year: Option<i32>,
    },
    /// A text that works as a link: [crate::UiEvent::Clicked]; with a `url` the application opens it too
    /// ([crate::UiOutput::open]).
    Link {
        /// Its text.
        text: String,
        /// The address, `None` a link inside the application.
        #[uniffi(default)]
        url: Option<String>,
        /// In a new tab.
        #[uniffi(default)]
        new_tab: bool,
    },
    /// A key with modifiers that works while the node is drawn (in an open window, a shown panel, or at the top of
    /// the tree always): [crate::UiEvent::Clicked]. It draws nothing.
    Shortcut {
        /// The key, named as in [crate::UiInput::Key] ("S", "F5", "Delete").
        key: String,
        /// Control, Command on macOS.
        #[uniffi(default)]
        ctrl: bool,
        /// Shift.
        #[uniffi(default)]
        shift: bool,
        /// Alt, Option on macOS.
        #[uniffi(default)]
        alt: bool,
    },
    /// A bar that fills up.
    ProgressBar {
        /// From 0 to 1.
        progress: f32,
        /// Inside the bar, `None` the percentage.
        text: Option<String>,
    },
    /// A small turning circle: something is busy.
    Spinner,
    /// A diagram of lines, dots, bars and boxes (egui_plot), for benchmarks: frame times over time, a bar per scene.
    /// The pointer on a series shows its values; with [Widget::Plot::interactive] dragging moves it, the wheel with
    /// Ctrl (or a pinch) zooms, and a double click shows everything again.
    Plot {
        /// What it shows, drawn in this order.
        series: Vec<PlotSeries>,
        /// In points.
        height: f32,
        /// In points, `None` all the width there is.
        width: Option<f32>,
        /// Under the x axis, empty for none.
        x_label: String,
        /// Beside the y axis, empty for none.
        y_label: String,
        /// After the values the pointer shows, for example " ms" (the axes leave it out, so say it in the label).
        x_unit: String,
        /// After the values the pointer shows, for example " ms".
        y_unit: String,
        /// Names for x = 0, 1, 2 … on the x axis: the scenes of a bar chart. Empty for numbers.
        categories: Vec<String>,
        /// The names of the series in a corner; a click on one hides it.
        legend: bool,
        /// Dragging, zooming and the double click; `false` keeps it showing all of its series (still with values).
        interactive: bool,
        /// Plots with the same link show the same part of the x axis and the pointer's position together.
        link: Option<String>,
        /// Values the x axis always shows.
        include_x: Vec<f64>,
        /// Values the y axis always shows, for example 0 so that frame times start at the bottom.
        include_y: Vec<f64>,
    },
    /// A line in the color of its level: "takes effect after a restart".
    Notice {
        /// What it says.
        text: String,
        /// How urgent.
        level: NoticeLevel,
    },
}

impl Widget {
    /// Stands at the top of the tree, without a parent.
    pub(crate) fn is_root(&self) -> bool {
        matches!(
            self,
            Self::SidePanel { .. }
                | Self::TopBar { .. }
                | Self::BottomBar { .. }
                | Self::CentralPanel { .. }
                | Self::Window { .. }
                | Self::Area { .. }
        )
    }

    /// May have nodes inside.
    /// A widget that egui draws in a frame of its own, which starts at the height of a button and grows down: across a
    /// row it needs its height of the last pass, as a container does.
    pub(crate) fn grows_down(&self) -> bool {
        matches!(
            self,
            Self::TextField { lines: Some(_), .. } | Self::ColorPicker { inline: true, .. }
        )
    }

    pub(crate) fn is_container(&self) -> bool {
        self.is_root()
            || matches!(
                self,
                Self::Row { .. }
                    | Self::Column { .. }
                    | Self::Columns
                    | Self::Sides
                    | Self::Indent
                    | Self::Resize { .. }
                    | Self::Grid { .. }
                    | Self::Collapsible { .. }
                    | Self::ScrollArea { .. }
                    | Self::Group
            )
    }

    fn name(&self) -> &'static str {
        match self {
            Self::SidePanel { .. } => "SidePanel",
            Self::TopBar { .. } => "TopBar",
            Self::BottomBar { .. } => "BottomBar",
            Self::CentralPanel { .. } => "CentralPanel",
            Self::Window { .. } => "Window",
            Self::Area { .. } => "Area",
            Self::Row { .. } => "Row",
            Self::Column { .. } => "Column",
            Self::Columns => "Columns",
            Self::Sides => "Sides",
            Self::Indent => "Indent",
            Self::Resize { .. } => "Resize",
            Self::Grid { .. } => "Grid",
            Self::Collapsible { .. } => "Collapsible",
            Self::ScrollArea { .. } => "ScrollArea",
            Self::Group => "Group",
            Self::Text { .. } => "Text",
            Self::Separator => "Separator",
            Self::Spacer { .. } => "Spacer",
            Self::Button { .. } => "Button",
            Self::SelectableLabel { .. } => "SelectableLabel",
            Self::ToggleButton { .. } => "ToggleButton",
            Self::RadioButton { .. } => "RadioButton",
            Self::Switch { .. } => "Switch",
            Self::Checkbox { .. } => "Checkbox",
            Self::Radio { .. } => "Radio",
            Self::ComboBox { .. } => "ComboBox",
            Self::SliderInt { .. } => "SliderInt",
            Self::SliderFloat { .. } => "SliderFloat",
            Self::DragValue { .. } => "DragValue",
            Self::TextField { .. } => "TextField",
            Self::ColorPicker { .. } => "ColorPicker",
            Self::DatePicker { .. } => "DatePicker",
            Self::Link { .. } => "Link",
            Self::Shortcut { .. } => "Shortcut",
            Self::ProgressBar { .. } => "ProgressBar",
            Self::Spinner => "Spinner",
            Self::Plot { .. } => "Plot",
            Self::Notice { .. } => "Notice",
        }
    }

    /// Why the values of the widget do not fit together, if they do not.
    fn check(&self) -> Option<String> {
        let selected = |options: &[String], selected: u32| {
            (!options.is_empty() && selected as usize >= options.len()).then(|| {
                format!(
                    "selected {selected} is not one of the {} options",
                    options.len()
                )
            })
        };
        let negative = |name: &str, v: &Option<f32>| {
            v.filter(|v| !(v.is_finite() && *v >= 0.0))
                .map(|v| format!("the {name} is {v}, it needs 0 or more"))
        };
        match self {
            Self::SidePanel {
                min_width: Some(min),
                max_width: Some(max),
                ..
            } if min > max => Some(format!("min_width {min} > max_width {max}")),
            Self::Row { spacing, .. } | Self::Column { spacing, .. } => {
                negative("spacing", spacing)
            }
            Self::Resize { width, height } if !(*width > 0.0 && *height > 0.0) => Some(format!(
                "a resize box needs a size above 0, it has {width} x {height}"
            )),
            Self::Grid { columns: 0, .. } => Some("a grid needs at least one column".into()),
            Self::Grid {
                column_spacing,
                row_spacing,
                min_column_width,
                ..
            } => negative("column_spacing", column_spacing)
                .or_else(|| negative("row_spacing", row_spacing))
                .or_else(|| negative("min_column_width", min_column_width)),
            Self::Text { spans, .. } => spans.iter().find_map(|span| {
                span.size
                    .filter(|size| !(size.is_finite() && *size > 0.0))
                    .map(|size| {
                        format!(
                            "the part '{}' has the size {size}, it needs one above 0",
                            span.text
                        )
                    })
            }),
            Self::Radio {
                options,
                selected: s,
                ..
            }
            | Self::ComboBox {
                options,
                selected: s,
                ..
            } => selected(options, *s),
            Self::SliderInt { min, max, .. } if min > max => Some(format!("min {min} > max {max}")),
            Self::SliderInt {
                min,
                logarithmic: true,
                ..
            } if *min < 0 => Some(format!("a logarithmic slider needs min >= 0, it has {min}")),
            Self::SliderInt { step, handle, .. } | Self::SliderFloat { step, handle, .. }
                if slider_options(*step, *handle).is_some() =>
            {
                slider_options(*step, *handle)
            }
            Self::SliderFloat { min, max, .. } if out_of_order(*min, *max) => {
                Some(format!("min {min} > max {max}"))
            }
            Self::SliderFloat {
                min,
                logarithmic: true,
                ..
            } if *min < 0.0 => Some(format!("a logarithmic slider needs min >= 0, it has {min}")),
            Self::DragValue {
                min: Some(min),
                max: Some(max),
                ..
            } if out_of_order(*min, *max) => Some(format!("min {min} > max {max}")),
            Self::Plot { height, .. } if height.is_nan() || *height <= 0.0 => {
                Some(format!("a plot needs a height above 0, it has {height}"))
            }
            Self::Plot { series, .. } => series.iter().find_map(PlotSeries::check),
            Self::TextField { lines: Some(0), .. } => {
                Some("a text field needs at least one line".into())
            }
            Self::DatePicker {
                date,
                first_year,
                last_year,
                ..
            } => date.check().or_else(|| match (first_year, last_year) {
                (Some(first), Some(last)) if first > last => {
                    Some(format!("first_year {first} > last_year {last}"))
                }
                _ => None,
            }),
            Self::Shortcut { key, .. } if crate::input::key_named(key).is_none() => Some(format!(
                "the key '{key}' is none egui knows (\"S\", \"F5\", \"Delete\", \"ArrowUp\")"
            )),
            _ => None,
        }
    }
}

/// Why the step or the handle of a slider does not fit, if it does not.
fn slider_options(step: Option<f64>, handle: Option<SliderHandle>) -> Option<String> {
    if let Some(step) = step.filter(|s| !(s.is_finite() && *s > 0.0)) {
        return Some(format!("the step is {step}, it needs one above 0"));
    }
    match handle {
        Some(SliderHandle::Rect { aspect_ratio })
            if !(aspect_ratio.is_finite() && aspect_ratio > 0.0) =>
        {
            Some(format!(
                "the handle's aspect_ratio is {aspect_ratio}, it needs one above 0"
            ))
        }
        _ => None,
    }
}

impl UiDate {
    /// Why it is no day of the calendar, if it is none.
    fn check(&self) -> Option<String> {
        self.to_jiff().err().map(|e| {
            format!(
                "{}-{:02}-{:02} is no day of the calendar: {e}",
                self.year, self.month, self.day
            )
        })
    }

    pub(crate) fn to_jiff(self) -> Result<jiff::civil::Date, jiff::Error> {
        let small = |v: i64| i16::try_from(v).unwrap_or(i16::MAX);
        jiff::civil::Date::new(
            small(self.year as i64),
            i8::try_from(self.month).unwrap_or(i8::MAX),
            i8::try_from(self.day).unwrap_or(i8::MAX),
        )
    }

    pub(crate) fn from_jiff(date: jiff::civil::Date) -> Self {
        Self {
            year: date.year() as i32,
            month: date.month() as u32,
            day: date.day() as u32,
        }
    }
}

impl PlotSeries {
    /// Why its values do not fit together, if they do not.
    fn check(&self) -> Option<String> {
        let name = &self.name;
        let (x, y) = (self.x.len(), self.y.len());
        match self.kind {
            SeriesKind::Boxes { .. } if y % 5 != 0 => Some(format!(
                "the boxes '{name}' need five values each, they have {y}"
            )),
            SeriesKind::Boxes { .. } if x != 0 && x * 5 != y => Some(format!(
                "the boxes '{name}' have {x} places for {} boxes",
                y / 5
            )),
            SeriesKind::HLine if x != 0 => {
                Some(format!("the lines across '{name}' take no x values"))
            }
            SeriesKind::Line { .. } | SeriesKind::Points { .. } | SeriesKind::Bars { .. }
                if x != 0 && x != y =>
            {
                Some(format!("'{name}' has {x} x values for {y} y values"))
            }
            _ => None,
        }
    }
}

/// `min` above `max`, or one of them not a number.
fn out_of_order(min: f64, max: f64) -> bool {
    matches!(
        min.partial_cmp(&max),
        None | Some(std::cmp::Ordering::Greater)
    )
}

/// A checked tree: the nodes, where each one is and which are inside which.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Tree {
    pub nodes: Vec<UiNode>,
    /// The panels, windows and areas, in the order of the list.
    pub roots: Vec<usize>,
    /// The nodes inside each container, in the order of the list.
    pub children: HashMap<u64, Vec<usize>>,
    pub ids: HashSet<u64>,
}

impl Tree {
    /// Checks the list: unique ids, parents that are there and are containers, panels only at the top, values that fit.
    pub fn new(nodes: Vec<UiNode>) -> Result<Self, UiError> {
        let invalid = |why: String| UiError::InvalidTree { why };
        let mut widgets = HashMap::with_capacity(nodes.len());
        for node in &nodes {
            if widgets.insert(node.id, &node.widget).is_some() {
                return Err(invalid(format!("the id {} is there twice", node.id)));
            }
        }
        let mut roots = Vec::new();
        let mut children: HashMap<u64, Vec<usize>> = HashMap::new();
        for (index, node) in nodes.iter().enumerate() {
            let what = || format!("{} {}", node.widget.name(), node.id);
            if let Some(why) = node
                .widget
                .check()
                .or_else(|| node.modifier.as_ref().and_then(UiModifier::check))
            {
                return Err(invalid(format!("{}: {why}", what())));
            }
            match node.parent {
                None if node.widget.is_root() || matches!(node.widget, Widget::Shortcut { .. }) => {
                    roots.push(index)
                }
                None => {
                    return Err(invalid(format!(
                        "{} has no parent: only SidePanel, TopBar, BottomBar, CentralPanel, Window, Area and Shortcut stand at the top",
                        what()
                    )))
                }
                Some(_) if node.widget.is_root() => {
                    return Err(invalid(format!(
                        "{} stands at the top and cannot be inside another node",
                        what()
                    )))
                }
                Some(parent) => match widgets.get(&parent) {
                    None => {
                        return Err(invalid(format!(
                            "{} is inside {parent}, which is not in the tree",
                            what()
                        )))
                    }
                    Some(p) if !p.is_container() => {
                        return Err(invalid(format!(
                            "{} is inside {} {parent}, which holds no nodes",
                            what(),
                            p.name()
                        )))
                    }
                    Some(_) => children.entry(parent).or_default().push(index),
                },
            }
        }
        // Every node has one parent, so a node that cannot be reached from the top sits in a circle of parents
        let mut reached = 0;
        let mut stack: Vec<usize> = roots.clone();
        while let Some(index) = stack.pop() {
            reached += 1;
            if let Some(inside) = children.get(&nodes[index].id) {
                stack.extend(inside);
            }
        }
        if reached != nodes.len() {
            return Err(invalid(format!(
                "{} nodes are not reached from the top: their parents go round in a circle",
                nodes.len() - reached
            )));
        }
        let central = |&&i: &&usize| matches!(nodes[i].widget, Widget::CentralPanel { .. });
        if let Some(second) = roots.iter().filter(central).nth(1) {
            return Err(invalid(format!(
                "CentralPanel {} is the second one: there is room for one",
                nodes[*second].id
            )));
        }
        if let Some(sides) = nodes.iter().find(|n| {
            matches!(n.widget, Widget::Sides) && children.get(&n.id).map_or(0, Vec::len) != 2
        }) {
            return Err(invalid(format!(
                "Sides {} holds {} nodes: it takes two, the left one and the right one",
                sides.id,
                children.get(&sides.id).map_or(0, Vec::len)
            )));
        }
        // egui gives the room in the order of the panels: bars and side panels first, then the central panel with the
        // rest; windows and areas lie on layers of their own
        roots.sort_by_key(|&i| match nodes[i].widget {
            Widget::SidePanel { .. } | Widget::TopBar { .. } | Widget::BottomBar { .. } => 0,
            Widget::CentralPanel { .. } => 1,
            _ => 2,
        });
        Ok(Self {
            ids: widgets.keys().copied().collect(),
            nodes,
            roots,
            children,
        })
    }

    pub fn children(&self, id: u64) -> &[usize] {
        self.children.get(&id).map_or(&[], Vec::as_slice)
    }
}
