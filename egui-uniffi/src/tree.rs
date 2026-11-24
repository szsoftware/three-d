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
    /// What it reacts to besides its widget: clicks, the pointer, dragging, gestures, drag and drop.
    #[uniffi(default)]
    pub reactions: Option<UiReactions>,
    /// The look of the node and the nodes inside it (Compose's nested `MaterialTheme`): what it leaves out stays as
    /// around it. Windows, menus and popups that open from inside take the look of the whole overlay.
    #[uniffi(default)]
    pub theme: Option<crate::UiTheme>,
}

/// What a node reacts to besides its own widget, as Compose's `clickable`, `combinedClickable`, `hoverable`,
/// `draggable`, `transformable` and drag and drop: any node (a text, an image, a frame with nodes inside), the nodes
/// inside first (a button in a clickable frame takes its own clicks).
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct UiReactions {
    /// A click reports [crate::UiEvent::Clicked].
    #[uniffi(default)]
    pub click: bool,
    /// A double click reports [crate::UiEvent::DoubleClicked] (after the [crate::UiEvent::Clicked] of its first click).
    #[uniffi(default)]
    pub double_click: bool,
    /// A click with the right button, or a long touch, reports [crate::UiEvent::SecondaryClicked].
    #[uniffi(default)]
    pub secondary_click: bool,
    /// Holding the button (or a finger) still on it for half a second reports [crate::UiEvent::LongPressed].
    #[uniffi(default)]
    pub long_press: bool,
    /// The pointer coming and going reports [crate::UiEvent::HoverChanged].
    #[uniffi(default)]
    pub hover: bool,
    /// Dragging it reports [crate::UiEvent::Dragged]: its start, each move, its end.
    #[uniffi(default)]
    pub drag: bool,
    /// Two fingers (zoom, turn, move) or Ctrl and the wheel over it report [crate::UiEvent::Gesture].
    #[uniffi(default)]
    pub gestures: bool,
    /// The pointer over it, as Compose's `pointerHoverIcon`.
    #[uniffi(default)]
    pub cursor: Option<crate::UiCursor>,
    /// It can be dragged onto a node with [UiReactions::drop_kinds] that takes this kind: it follows the pointer while
    /// dragged; `None` not.
    #[uniffi(default)]
    pub drag_kind: Option<String>,
    /// Nodes of these kinds ([UiReactions::drag_kind]) can be dropped onto it (empty: any), which reports
    /// [crate::UiEvent::Dropped]; it shows that while one is dragged over it. `None` not.
    #[uniffi(default)]
    pub drop_kinds: Option<Vec<String>>,
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

/// Where a [Widget::Popup] opens next to its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PopupSide {
    /// Under it.
    Below,
    /// Over it.
    Above,
    /// At its left.
    Left,
    /// At its right.
    Right,
}

/// Which clicks close a [Widget::Popup].
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PopupClose {
    /// Any click, also on its nodes: a list to choose from.
    Click,
    /// A click beside it: a popup with settings.
    ClickOutside,
    /// None, only Escape and Kotlin.
    Never,
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
        /// Its form, `None` round.
        #[uniffi(default)]
        shape: Option<PointShape>,
        /// Filled, else an outline.
        #[uniffi(default = true)]
        filled: bool,
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
    /// A dashed line from bottom to top at each value of [PlotSeries::x], for example when the scene changed;
    /// [PlotSeries::y] stays empty.
    VLine,
    /// The points joined to a closed shape, filled lightly in its color.
    Polygon {
        /// Of the outline, in points.
        width: f32,
    },
    /// Each of [PlotSeries::labels] at its point, for example the name of a peak.
    Text {
        /// Which side of the text lies on the point, `None` its middle.
        #[uniffi(default)]
        anchor: Option<crate::Anchor>,
    },
    /// An arrow from each point to the point of [PlotSeries::x2] and [PlotSeries::y2] at the same index.
    Arrows {
        /// The length of the tips in points, `None` egui_plot's.
        #[uniffi(default)]
        tip_length: Option<f32>,
    },
    /// The area between [PlotSeries::y] below and [PlotSeries::y2] above, at the values of [PlotSeries::x]: the spread
    /// of frame times around their mean.
    Area,
    /// A band across the whole plot from one value to the next of each pair in [PlotSeries::y] (or [PlotSeries::x] if
    /// `vertical`), for example the range of 30 to 60 frames per second.
    Span {
        /// From bottom to top between values of x, else from left to right between values of y.
        #[uniffi(default)]
        vertical: bool,
    },
    /// A grid of colored tiles, the values of [PlotSeries::y] row by row from the bottom, its lower left corner at the
    /// first values of [PlotSeries::x] and [PlotSeries::y2] (else 0), a tile 1 by 1: render times per tile of a picture.
    Heatmap {
        /// Tiles per row.
        columns: u32,
        /// The colors from the lowest value to the highest, `None` egui_plot's.
        #[uniffi(default)]
        palette: Vec<u32>,
        /// The values at the ends of the palette, `None` the lowest and highest.
        #[uniffi(default)]
        min: Option<f64>,
        /// See [SeriesKind::Heatmap::min].
        #[uniffi(default)]
        max: Option<f64>,
        /// Each value written on its tile.
        #[uniffi(default)]
        show_values: bool,
    },
    /// An image given to the overlay ([crate::EguiOverlay::set_image]), its middle at the first point, for example
    /// a picture under its measurements.
    Image {
        /// The key of the image.
        image: String,
        /// In units of the x axis.
        width: f64,
        /// In units of the y axis.
        height: f64,
    },
}

/// The form of the dots of [SeriesKind::Points] (egui_plot's `MarkerShape`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PointShape {
    /// Round.
    Circle,
    /// A square on its tip.
    Diamond,
    /// A square.
    Square,
    /// An ×.
    Cross,
    /// A +.
    Plus,
    /// A triangle pointing up.
    Up,
    /// A triangle pointing down.
    Down,
    /// A triangle pointing left.
    Left,
    /// A triangle pointing right.
    Right,
    /// A star of lines.
    Asterisk,
}

/// How the line of a [PlotSeries] is drawn.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum PlotLineStyle {
    /// Without gaps.
    Solid,
    /// Dashes of `length` points with gaps as long.
    Dashed {
        /// In points.
        length: f32,
    },
    /// Dots `spacing` points apart.
    Dotted {
        /// In points.
        spacing: f32,
    },
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
    /// The x values of the tips of [SeriesKind::Arrows].
    #[uniffi(default)]
    pub x2: Vec<f64>,
    /// The y values of the tips of [SeriesKind::Arrows], the upper edge of an [SeriesKind::Area], the bottom of a
    /// [SeriesKind::Heatmap].
    #[uniffi(default)]
    pub y2: Vec<f64>,
    /// The texts of [SeriesKind::Text], one per point.
    #[uniffi(default)]
    pub labels: Vec<String>,
    /// How its line is drawn (lines, polygons, lines across), `None` as egui_plot draws it.
    #[uniffi(default)]
    pub style: Option<PlotLineStyle>,
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

/// How wide a column of a [Widget::Table] is (egui_extras' `Column`).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum ColumnWidth {
    /// As wide as its widest cell that was drawn (egui's default). A column the user may resize takes the width of
    /// the first frame only, the user widens it then: with many rows only those in sight were drawn.
    Auto,
    /// This wide at first; the user may change it if the column is resizable.
    Initial {
        /// In points.
        width: f32,
    },
    /// Always this wide, what does not fit is cut.
    Exact {
        /// In points.
        width: f32,
    },
    /// What the other columns leave, shared with the other columns of this kind. Columns the user may resize keep
    /// their width of the first frame, in which [ColumnWidth::Auto] ones take a guess: only the last column takes all
    /// that is left in every frame.
    Remainder,
}

/// A column of a [Widget::Table].
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TableColumn {
    /// In the header; a table whose columns have no titles and no [Widget::TableRow] header has no header.
    #[uniffi(default)]
    pub title: String,
    /// How wide, `None` [ColumnWidth::Auto].
    #[uniffi(default)]
    pub width: Option<ColumnWidth>,
    /// At least this wide, in points.
    #[uniffi(default)]
    pub min_width: Option<f32>,
    /// At most this wide, in points.
    #[uniffi(default)]
    pub max_width: Option<f32>,
    /// The user may drag its right edge, `None` as the table says ([Widget::Table::resizable]).
    #[uniffi(default)]
    pub resizable: Option<bool>,
    /// Cuts what is wider than the column instead of letting it run into the next one.
    #[uniffi(default)]
    pub clip: bool,
    /// Where the cells sit across the column, `None` at the start.
    #[uniffi(default)]
    pub align: Option<Align>,
    /// A click on the title sorts by this column ([crate::UiEvent::SortChanged]); the title shows an arrow while the
    /// table is sorted by it ([Widget::Table::sort]).
    #[uniffi(default)]
    pub sortable: bool,
    /// The style of its text from [crate::EguiOverlay::set_table_data], `None` [TextStyle::Body].
    #[uniffi(default)]
    pub style: Option<TextStyle>,
}

/// How many rows of a [Widget::Table] the user may select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TableSelection {
    /// One: a click selects its row.
    Single,
    /// Any: a click selects its row, with Ctrl (Command on a Mac) it adds or removes it, with Shift it selects the rows
    /// from the last clicked one to it.
    Multiple,
}

/// The column a [Widget::Table] is sorted by, as Kotlin sorted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct TableSort {
    /// Its index.
    pub column: u32,
    /// Smallest first.
    pub ascending: bool,
}

/// How an [Widget::Image] fills the width and height it is given (Compose's `ContentScale`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ImageFit {
    /// Whole and as large as fits, keeping its shape (Compose's `Fit`).
    Contain,
    /// All of the room, keeping its shape: what is too much is cut off on both sides (Compose's `Crop`).
    Cover,
    /// Stretched to the room (Compose's `FillBounds`).
    Fill,
}

/// The line of a shape on a [Widget::Canvas].
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct CanvasStroke {
    /// In points, whatever the view.
    pub width: f32,
    /// ARGB.
    pub color: u32,
    /// Dashes of this length in points with gaps as long, for lines and polygons; `None` without gaps.
    #[uniffi(default)]
    pub dash: Option<f32>,
}

/// What a [Widget::Canvas] draws, in its coordinates ([Widget::Canvas::view]). Points come as x and y after each other.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum CanvasShape {
    /// A line through the points.
    Line {
        /// x and y after each other, at least two points.
        points: Vec<f32>,
        /// How it is drawn.
        stroke: CanvasStroke,
    },
    /// A closed shape through the points, also a concave one (a room of a floor plan), as long as its edges do not
    /// cross.
    Polygon {
        /// x and y after each other, at least three points.
        points: Vec<f32>,
        /// ARGB, `None` not filled.
        #[uniffi(default)]
        fill: Option<u32>,
        /// Its outline, `None` none.
        #[uniffi(default)]
        stroke: Option<CanvasStroke>,
    },
    /// A rectangle from (x, y) on.
    Rect {
        /// A corner.
        x: f32,
        /// A corner.
        y: f32,
        /// Along x.
        width: f32,
        /// Along y.
        height: f32,
        /// ARGB, `None` not filled.
        #[uniffi(default)]
        fill: Option<u32>,
        /// Its outline, `None` none.
        #[uniffi(default)]
        stroke: Option<CanvasStroke>,
        /// Rounded corners, in points.
        #[uniffi(default)]
        corner_radius: f32,
    },
    /// A circle around (x, y).
    Circle {
        /// Its middle.
        x: f32,
        /// Its middle.
        y: f32,
        /// In the units of the view.
        radius: f32,
        /// ARGB, `None` not filled.
        #[uniffi(default)]
        fill: Option<u32>,
        /// Its outline, `None` none.
        #[uniffi(default)]
        stroke: Option<CanvasStroke>,
    },
    /// An ellipse around (x, y).
    Ellipse {
        /// Its middle.
        x: f32,
        /// Its middle.
        y: f32,
        /// Along x, in the units of the view.
        radius_x: f32,
        /// Along y.
        radius_y: f32,
        /// ARGB, `None` not filled.
        #[uniffi(default)]
        fill: Option<u32>,
        /// Its outline, `None` none.
        #[uniffi(default)]
        stroke: Option<CanvasStroke>,
    },
    /// A Bézier curve: three points quadratic (start, control, end), four cubic (start, two controls, end).
    Curve {
        /// x and y after each other, three or four points.
        points: Vec<f32>,
        /// Back to the start; filled if it has a `fill`.
        #[uniffi(default)]
        closed: bool,
        /// ARGB, `None` not filled.
        #[uniffi(default)]
        fill: Option<u32>,
        /// Its line, `None` none.
        #[uniffi(default)]
        stroke: Option<CanvasStroke>,
    },
    /// An arrow from (x, y) to (to_x, to_y), its tip a quarter as long.
    Arrow {
        /// Where it starts.
        x: f32,
        /// Where it starts.
        y: f32,
        /// Where its tip is.
        to_x: f32,
        /// Where its tip is.
        to_y: f32,
        /// How it is drawn.
        stroke: CanvasStroke,
    },
    /// A text at (x, y).
    Text {
        /// Where it is.
        x: f32,
        /// Where it is.
        y: f32,
        /// What it says.
        text: String,
        /// In points, `None` egui's size of text.
        #[uniffi(default)]
        size: Option<f32>,
        /// ARGB, `None` egui's color of text.
        #[uniffi(default)]
        color: Option<u32>,
        /// Which side of the text lies on the point, `None` its top left corner.
        #[uniffi(default)]
        anchor: Option<crate::Anchor>,
        /// In egui's monospace font.
        #[uniffi(default)]
        monospace: bool,
    },
    /// An image given to the overlay ([crate::EguiOverlay::set_image]) in the rectangle from (x, y) on: a symbol on a
    /// map.
    Image {
        /// The key of the image.
        image: String,
        /// A corner.
        x: f32,
        /// A corner.
        y: f32,
        /// Along x.
        width: f32,
        /// Along y.
        height: f32,
        /// Multiplied with its colors as ARGB.
        #[uniffi(default)]
        tint: Option<u32>,
    },
}

/// The room of a cell of a [Widget::Strip] along it (egui_extras' `Size`).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum StripSize {
    /// Always this many points.
    Exact {
        /// In points.
        points: f32,
    },
    /// This many points at first (a strip does not let the user change it, so it is as [StripSize::Exact] with
    /// bounds).
    Initial {
        /// In points.
        points: f32,
        /// At least this many points.
        #[uniffi(default)]
        min: Option<f32>,
        /// At most this many points.
        #[uniffi(default)]
        max: Option<f32>,
    },
    /// This part of the whole strip (not of what the exact cells leave), 0.25 for a quarter.
    Relative {
        /// From 0 to 1.
        fraction: f32,
        /// At least this many points.
        #[uniffi(default)]
        min: Option<f32>,
        /// At most this many points.
        #[uniffi(default)]
        max: Option<f32>,
    },
    /// What the other cells leave, shared with the other cells of this kind.
    Remainder {
        /// At least this many points.
        #[uniffi(default)]
        min: Option<f32>,
        /// At most this many points.
        #[uniffi(default)]
        max: Option<f32>,
    },
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
    /// A dialog in the middle over everything, the rest dimmed and blocked until it is gone (egui's `Modal`). It is
    /// shown while it is in the tree; with `dismissible` a click beside it or Escape reports [crate::UiEvent::Closed]
    /// and hides it until the next tree.
    Modal {
        /// Escape and a click on the dimmed rest close it.
        #[uniffi(default = true)]
        dismissible: bool,
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
    /// A table of rows and columns (egui_extras' `TableBuilder`): a header from the columns' titles (a click on a
    /// sortable one sorts) or from a header [Widget::TableRow], rows that scroll under it, every other one striped, rows
    /// the user selects, columns the user makes wider. The rows are either [Widget::TableRow] nodes (a few dozen with
    /// widgets in their cells) or the text of [crate::EguiOverlay::set_table_data] under [Widget::Table::data] (tens of
    /// thousands, of which only those seen are drawn).
    Table {
        /// Its columns, at least one.
        columns: Vec<TableColumn>,
        /// The key of the rows' text given with [crate::EguiOverlay::set_table_data], instead of row nodes.
        #[uniffi(default)]
        data: Option<String>,
        /// Every other row on a darker background.
        #[uniffi(default = true)]
        striped: bool,
        /// The user may drag the edges of the columns (each column may say otherwise).
        #[uniffi(default)]
        resizable: bool,
        /// Whether and how many rows the user may select ([crate::UiEvent::RowsSelected]), and a double click on a row
        /// reports it ([crate::UiEvent::RowActivated]); `None` the rows do not react.
        #[uniffi(default)]
        selection: Option<TableSelection>,
        /// The selected rows, by their index.
        #[uniffi(default)]
        selected: Vec<u64>,
        /// The column the rows are sorted by, as Kotlin sorted them.
        #[uniffi(default)]
        sort: Option<TableSort>,
        /// The height of a row in points, `None` a line of text; a [Widget::TableRow] may have its own.
        #[uniffi(default)]
        row_height: Option<f32>,
        /// The height the rows take at most before they scroll, `None` all there is.
        #[uniffi(default)]
        max_height: Option<f32>,
        /// Stays at the end while rows come at the end, as a log does, until the user scrolls away.
        #[uniffi(default)]
        stick_to_end: bool,
        /// Scrolls to the row with this index when the value changes.
        #[uniffi(default)]
        scroll_to: Option<u64>,
    },
    /// A row of a [Widget::Table]: one node per column, the first in the first column (a [Widget::Row] or
    /// [Widget::Column] for several in one cell). Fewer nodes than columns leave the last cells empty.
    TableRow {
        /// The header of the table instead of a row (only its first node may be one); the columns' titles are not shown
        /// then.
        #[uniffi(default)]
        header: bool,
        /// The height of this row in points, `None` the table's.
        #[uniffi(default)]
        height: Option<f32>,
    },
    /// Its nodes side by side (or one under the other) in cells of the given sizes (egui_extras' `StripBuilder`): a
    /// sidebar of 200 points and the rest, three thirds. One node per cell (a [Widget::Column] for several), fewer
    /// nodes leave the last cells empty; a strip in a cell divides that cell.
    Strip {
        /// The size of each cell along the strip.
        sizes: Vec<StripSize>,
        /// One under the other instead of side by side.
        #[uniffi(default)]
        vertical: bool,
        /// Cuts what does not fit into its cell.
        #[uniffi(default)]
        clip: bool,
    },
    /// A bar of [Widget::Menu]s, as at the top of an application window: a click opens one, the pointer moves on to the
    /// next.
    MenuBar,
    /// A button with `text` that opens its nodes in a menu (egui's `menu_button`): buttons with a `shortcut` as items,
    /// checkboxes, separators. Inside a menu it is a submenu that opens to the side. A click on an item closes the
    /// menu.
    Menu {
        /// The text of the button.
        text: String,
    },
    /// Its nodes as the menu of a right click (or a long touch) on its parent, which may be any node.
    ContextMenu,
    /// Its nodes in a tooltip while the pointer rests on its parent, which may be any node: a tooltip with a picture,
    /// a value in color, several lines. Instead of [UiNode::tooltip].
    Tooltip,
    /// Its nodes in a popup next to its parent, which may be any node, while `open` (Compose's `DropdownMenu`). When
    /// the user closes it (Escape, a click beside it) it reports [crate::UiEvent::Closed] and stays closed until the
    /// next tree.
    Popup {
        /// Shown.
        open: bool,
        /// Where next to the parent, `None` below it.
        #[uniffi(default)]
        side: Option<PopupSide>,
        /// Along that side at the start, in the middle or at the end, `None` at the start.
        #[uniffi(default)]
        align: Option<Align>,
        /// Which clicks close it, `None` [PopupClose::ClickOutside].
        #[uniffi(default)]
        close: Option<PopupClose>,
        /// Its width in points, `None` that of its nodes.
        #[uniffi(default)]
        width: Option<f32>,
    },
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
        /// An image before the text, by its key ([Widget::Image]); with an empty text it is egui's image button.
        #[uniffi(default)]
        image: Option<String>,
        /// The height of the image in points, `None` as high as the text.
        #[uniffi(default)]
        image_size: Option<f32>,
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
        /// A click that does not drag reports [crate::UiEvent::PointClicked] with the values there.
        #[uniffi(default)]
        clickable: bool,
        /// The values under the pointer come as [crate::UiEvent::PointerAt] while it moves over the plot.
        #[uniffi(default)]
        report_pointer: bool,
    },
    /// An image given to the overlay under a key ([crate::EguiOverlay::set_image] for PNG, JPEG, GIF, WebP and SVG,
    /// [crate::EguiOverlay::set_image_rgba] for pixels), as Compose's `Image`. Without a width and height as large as it
    /// has pixels (a pixel a point), at most as wide as there is room; an SVG is drawn sharp at any size, a GIF or WebP
    /// moves. A spinner while it is decoded, nothing for a key without an image.
    Image {
        /// The key of the image.
        image: String,
        /// In points, `None` from the height and the image's shape (or its own size).
        #[uniffi(default)]
        width: Option<f32>,
        /// In points, `None` from the width and the image's shape (or its own size).
        #[uniffi(default)]
        height: Option<f32>,
        /// How it fills width and height when both are given, `None` [ImageFit::Contain].
        #[uniffi(default)]
        fit: Option<ImageFit>,
        /// Multiplied with its colors as ARGB: a white icon in any color.
        #[uniffi(default)]
        tint: Option<u32>,
        /// Rounded corners, in points.
        #[uniffi(default)]
        corner_radius: Option<f32>,
        /// Turned clockwise around its middle, in degrees.
        #[uniffi(default)]
        rotation: Option<f32>,
        /// What it shows, for screen readers.
        #[uniffi(default)]
        description: Option<String>,
    },
    /// A surface to draw on, as Compose's `Canvas`: lines, rectangles, circles, polygons of any shape, Bézier curves,
    /// arrows, text and images in its own coordinates (see `view`), cut off at its edges. With `clickable` a click
    /// reports [crate::UiEvent::PointClicked], with `report_pointer` the pointer over it comes as
    /// [crate::UiEvent::PointerAt], both in its coordinates.
    Canvas {
        /// What it draws, in this order, over the shapes of `data`.
        shapes: Vec<CanvasShape>,
        /// In points.
        height: f32,
        /// In points, `None` all the width there is.
        #[uniffi(default)]
        width: Option<f32>,
        /// The part of its coordinates it shows, `None` points from its top left corner.
        #[uniffi(default)]
        view: Option<crate::UiRect>,
        /// The view stretched to the canvas; else it keeps its shape, in the middle.
        #[uniffi(default)]
        stretch: bool,
        /// y grows upwards, as on a plan; else downwards, as on the screen.
        #[uniffi(default)]
        y_up: bool,
        /// The key of shapes given with [crate::EguiOverlay::set_canvas_shapes], drawn first: many shapes that seldom
        /// change, as a floor plan under a moving marker.
        #[uniffi(default)]
        data: Option<String>,
        /// A click reports [crate::UiEvent::PointClicked].
        #[uniffi(default)]
        clickable: bool,
        /// The pointer over it comes as [crate::UiEvent::PointerAt].
        #[uniffi(default)]
        report_pointer: bool,
    },
    /// Its nodes on a surface that the user moves (dragging, the wheel) and zooms (the wheel with Ctrl, a pinch), as
    /// egui's `Scene`: a large plan of widgets. It starts with all its nodes in sight.
    Scene {
        /// In points.
        height: f32,
        /// In points, `None` all the width there is.
        #[uniffi(default)]
        width: Option<f32>,
        /// How far it zooms out, `None` until all nodes fit.
        #[uniffi(default)]
        min_zoom: Option<f32>,
        /// How far it zooms in, `None` to their own size (egui's 1).
        #[uniffi(default)]
        max_zoom: Option<f32>,
        /// The part it shows, in the points of its nodes; applied when it changes, then the user moves it.
        #[uniffi(default)]
        view: Option<crate::UiRect>,
        /// The user moving it reports [crate::UiEvent::ViewChanged].
        #[uniffi(default)]
        report_view: bool,
    },
    /// A line in the color of its level: "takes effect after a restart".
    Notice {
        /// What it says.
        text: String,
        /// How urgent.
        level: NoticeLevel,
    },
    /// One of egui's own panels: its settings, its style editor (every value of the style, live), what it knows of the
    /// frame, its memory or its textures. For trying a look before it goes into a [crate::UiTheme], and for debugging.
    EguiTool {
        /// Which.
        tool: crate::EguiTool,
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
                | Self::Modal { .. }
        )
    }

    /// Belongs to its parent, which may be any node, instead of standing inside it: menus, tooltips and popups.
    pub(crate) fn is_attached(&self) -> bool {
        matches!(self, Self::ContextMenu | Self::Tooltip | Self::Popup { .. })
    }

    /// A widget that egui draws in a frame of its own, which starts at the height of a button and grows down: across a
    /// row it needs its height of the last pass, as a container does.
    pub(crate) fn grows_down(&self) -> bool {
        matches!(
            self,
            Self::TextField { lines: Some(_), .. } | Self::ColorPicker { inline: true, .. }
        )
    }

    /// May have nodes inside.
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
                    | Self::Table { .. }
                    | Self::TableRow { .. }
                    | Self::Strip { .. }
                    | Self::Scene { .. }
                    | Self::MenuBar
                    | Self::Menu { .. }
            )
            || self.is_attached()
    }

    fn name(&self) -> &'static str {
        match self {
            Self::SidePanel { .. } => "SidePanel",
            Self::TopBar { .. } => "TopBar",
            Self::BottomBar { .. } => "BottomBar",
            Self::CentralPanel { .. } => "CentralPanel",
            Self::Window { .. } => "Window",
            Self::Area { .. } => "Area",
            Self::Modal { .. } => "Modal",
            Self::MenuBar => "MenuBar",
            Self::Menu { .. } => "Menu",
            Self::ContextMenu => "ContextMenu",
            Self::Tooltip => "Tooltip",
            Self::Popup { .. } => "Popup",
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
            Self::Table { .. } => "Table",
            Self::TableRow { .. } => "TableRow",
            Self::Strip { .. } => "Strip",
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
            Self::Image { .. } => "Image",
            Self::Canvas { .. } => "Canvas",
            Self::Scene { .. } => "Scene",
            Self::Notice { .. } => "Notice",
            Self::EguiTool { .. } => "EguiTool",
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
            Self::Popup { width, .. } => negative("width", width),
            Self::Table { columns, .. } if columns.is_empty() => {
                Some("a table needs at least one column".into())
            }
            Self::Table {
                columns,
                selection,
                selected,
                sort,
                row_height,
                max_height,
                ..
            } => columns
                .iter()
                .enumerate()
                .find_map(|(i, c)| c.check().map(|why| format!("column {i}: {why}")))
                .or_else(|| match (selection, selected.len()) {
                    (None, n) if n > 0 => Some(format!(
                        "{n} rows are selected, but the table has no selection"
                    )),
                    (Some(TableSelection::Single), n) if n > 1 => {
                        Some(format!("{n} rows are selected, but the table selects one"))
                    }
                    _ => None,
                })
                .or_else(|| {
                    sort.filter(|s| s.column as usize >= columns.len())
                        .map(|s| {
                            format!(
                                "it is sorted by column {}, it has {}",
                                s.column,
                                columns.len()
                            )
                        })
                })
                .or_else(|| above_zero("row_height", row_height))
                .or_else(|| negative("max_height", max_height)),
            Self::TableRow { height, .. } => above_zero("height", height),
            Self::Strip { sizes, .. } if sizes.is_empty() => {
                Some("a strip needs at least one size".into())
            }
            Self::Strip { sizes, .. } => sizes
                .iter()
                .enumerate()
                .find_map(|(i, size)| size.check().map(|why| format!("size {i}: {why}"))),
            Self::Button { image_size, .. } => above_zero("image_size", image_size),
            Self::Image {
                width,
                height,
                corner_radius,
                rotation,
                ..
            } => above_zero("width", width)
                .or_else(|| above_zero("height", height))
                .or_else(|| negative("corner_radius", corner_radius))
                .or_else(|| {
                    rotation
                        .filter(|r| !r.is_finite())
                        .map(|r| format!("the rotation is {r}, it needs a number"))
                }),
            Self::Canvas {
                shapes,
                height,
                width,
                view,
                ..
            } => above_zero("height", &Some(*height))
                .or_else(|| above_zero("width", width))
                .or_else(|| view.and_then(|v| view_check(&v)))
                .or_else(|| {
                    shapes
                        .iter()
                        .enumerate()
                        .find_map(|(i, s)| s.check().map(|why| format!("shape {i}: {why}")))
                }),
            Self::Scene {
                height,
                width,
                min_zoom,
                max_zoom,
                view,
                ..
            } => above_zero("height", &Some(*height))
                .or_else(|| above_zero("width", width))
                .or_else(|| above_zero("min_zoom", min_zoom))
                .or_else(|| above_zero("max_zoom", max_zoom))
                .or_else(|| match (min_zoom, max_zoom) {
                    (Some(min), Some(max)) if min > max => {
                        Some(format!("min_zoom {min} > max_zoom {max}"))
                    }
                    _ => None,
                })
                .or_else(|| view.and_then(|v| view_check(&v))),
            _ => None,
        }
    }
}

/// Why `v` is not above 0, if it is not.
fn above_zero(name: &str, v: &Option<f32>) -> Option<String> {
    v.filter(|v| !(v.is_finite() && *v > 0.0))
        .map(|v| format!("the {name} is {v}, it needs one above 0"))
}

/// Why a view of a canvas or scene shows nothing, if it does not.
fn view_check(v: &crate::UiRect) -> Option<String> {
    let finite = [v.x, v.y, v.width, v.height].iter().all(|c| c.is_finite());
    (!finite || v.width <= 0.0 || v.height <= 0.0).then(|| {
        format!(
            "the view is {} x {} at {}, {}: it needs a size above 0",
            v.width, v.height, v.x, v.y
        )
    })
}

impl CanvasShape {
    /// Why its values do not fit together, if they do not.
    pub(crate) fn check(&self) -> Option<String> {
        let points = |name: &str, p: &Vec<f32>, least: usize| {
            if !p.len().is_multiple_of(2) {
                Some(format!(
                    "a {name} has {} coordinates, it needs x and y for each point",
                    p.len()
                ))
            } else if p.len() / 2 < least {
                Some(format!(
                    "a {name} has {} points, it needs {least}",
                    p.len() / 2
                ))
            } else {
                p.iter()
                    .find(|v| !v.is_finite())
                    .map(|v| format!("a {name} has the coordinate {v}"))
            }
        };
        let stroke = |s: &Option<CanvasStroke>| {
            s.and_then(|s| {
                (!(s.width.is_finite() && s.width >= 0.0))
                    .then(|| format!("the line is {} wide, it needs 0 or more", s.width))
                    .or_else(|| above_zero("dash", &s.dash))
            })
        };
        let finite = |name: &str, values: &[f32]| {
            values
                .iter()
                .find(|v| !v.is_finite())
                .map(|v| format!("a {name} has the value {v}"))
        };
        match self {
            Self::Line {
                points: p,
                stroke: s,
            } => points("line", p, 2).or_else(|| stroke(&Some(*s))),
            Self::Polygon {
                points: p,
                stroke: s,
                ..
            } => points("polygon", p, 3).or_else(|| stroke(s)),
            Self::Rect {
                x,
                y,
                width,
                height,
                stroke: s,
                corner_radius,
                ..
            } => finite("rectangle", &[*x, *y, *width, *height, *corner_radius])
                .or_else(|| {
                    (*corner_radius < 0.0).then(|| {
                        format!("the corner radius is {corner_radius}, it needs 0 or more")
                    })
                })
                .or_else(|| stroke(s)),
            Self::Circle {
                x,
                y,
                radius,
                stroke: s,
                ..
            } => finite("circle", &[*x, *y, *radius])
                .or_else(|| {
                    (*radius < 0.0).then(|| format!("the radius is {radius}, it needs 0 or more"))
                })
                .or_else(|| stroke(s)),
            Self::Ellipse {
                x,
                y,
                radius_x,
                radius_y,
                stroke: s,
                ..
            } => finite("ellipse", &[*x, *y, *radius_x, *radius_y])
                .or_else(|| {
                    (*radius_x < 0.0 || *radius_y < 0.0).then(|| {
                        format!("the radii are {radius_x} and {radius_y}, they need 0 or more")
                    })
                })
                .or_else(|| stroke(s)),
            Self::Curve {
                points: p,
                stroke: s,
                ..
            } => points("curve", p, 3)
                .or_else(|| {
                    (p.len() > 8).then(|| {
                        format!("a curve has {} points, it needs three or four", p.len() / 2)
                    })
                })
                .or_else(|| stroke(s)),
            Self::Arrow {
                x,
                y,
                to_x,
                to_y,
                stroke: s,
            } => finite("arrow", &[*x, *y, *to_x, *to_y]).or_else(|| stroke(&Some(*s))),
            Self::Text { x, y, size, .. } => {
                finite("text", &[*x, *y]).or_else(|| above_zero("size", size))
            }
            Self::Image {
                x,
                y,
                width,
                height,
                ..
            } => finite("image", &[*x, *y, *width, *height]),
        }
    }
}

/// Why `min` and `max` do not fit, if they do not.
fn bounds(min: Option<f32>, max: Option<f32>) -> Option<String> {
    let bad = |v: f32| !(v.is_finite() && v >= 0.0);
    match (min, max) {
        (Some(v), _) | (_, Some(v)) if bad(v) => {
            Some(format!("a bound is {v}, it needs 0 or more"))
        }
        (Some(min), Some(max)) if min > max => Some(format!("min {min} > max {max}")),
        _ => None,
    }
}

impl TableColumn {
    /// Why its values do not fit together, if they do not.
    fn check(&self) -> Option<String> {
        match self.width {
            Some(ColumnWidth::Initial { width } | ColumnWidth::Exact { width })
                if !(width.is_finite() && width > 0.0) =>
            {
                Some(format!("the width is {width}, it needs one above 0"))
            }
            _ => bounds(self.min_width, self.max_width),
        }
    }
}

impl StripSize {
    /// Why its values do not fit together, if they do not.
    fn check(&self) -> Option<String> {
        match *self {
            Self::Exact { points } | Self::Initial { points, .. }
                if !(points.is_finite() && points >= 0.0) =>
            {
                Some(format!("{points} points, it needs 0 or more"))
            }
            Self::Relative { fraction, .. } if !(0.0..=1.0).contains(&fraction) => Some(format!(
                "the fraction is {fraction}, it needs one from 0 to 1"
            )),
            Self::Exact { .. } => None,
            Self::Initial { min, max, .. }
            | Self::Relative { min, max, .. }
            | Self::Remainder { min, max } => bounds(min, max),
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
        match &self.kind {
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
            SeriesKind::VLine if y != 0 => Some(format!(
                "the lines from bottom to top '{name}' take no y values"
            )),
            SeriesKind::Polygon { .. } | SeriesKind::Area if x != y => {
                Some(format!("'{name}' has {x} x values for {y} y values"))
            }
            SeriesKind::Area if self.y2.len() != y => Some(format!(
                "the area '{name}' has {} upper values for {y} lower ones",
                self.y2.len()
            )),
            SeriesKind::Text { .. } if x != y || self.labels.len() != y => Some(format!(
                "the texts '{name}' have {x} x values, {y} y values and {} labels",
                self.labels.len()
            )),
            SeriesKind::Arrows { .. } if x != y || self.x2.len() != y || self.y2.len() != y => {
                Some(format!(
                    "the arrows '{name}' have {x} x and {y} y values, but {} and {} for their tips",
                    self.x2.len(),
                    self.y2.len()
                ))
            }
            SeriesKind::Span { vertical, .. } => {
                let (axis, values) = if *vertical { ("x", x) } else { ("y", y) };
                (values % 2 != 0).then(|| {
                    format!("the bands '{name}' need pairs of {axis} values, they have {values}")
                })
            }
            SeriesKind::Heatmap { columns: 0, .. } => {
                Some(format!("the heatmap '{name}' needs at least one column"))
            }
            SeriesKind::Heatmap {
                columns, min, max, ..
            } => {
                let columns = *columns as usize;
                if y == 0 || y % columns != 0 {
                    Some(format!(
                        "the heatmap '{name}' has {y} values for rows of {columns}"
                    ))
                } else {
                    match (min, max) {
                        (Some(min), Some(max)) if out_of_order(*min, *max) => {
                            Some(format!("the heatmap '{name}' has min {min} > max {max}"))
                        }
                        _ => None,
                    }
                }
            }
            SeriesKind::Image { width, height, .. } => {
                if x != 1 || y != 1 {
                    Some(format!(
                        "the image '{name}' needs one point, it has {x} x and {y} y values"
                    ))
                } else if !(*width > 0.0 && *height > 0.0) {
                    Some(format!(
                        "the image '{name}' needs a size above 0, it has {width} x {height}"
                    ))
                } else {
                    None
                }
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

/// Why `node` does not fit as a table, a row of one or a strip, if it does not: rows only in tables, tables only of
/// rows, no more cells than columns or sizes.
fn tables_and_strips(
    node: &UiNode,
    nodes: &[UiNode],
    widgets: &HashMap<u64, &Widget>,
    children: &HashMap<u64, Vec<usize>>,
) -> Option<String> {
    let inside = children.get(&node.id).map_or(&[][..], Vec::as_slice);
    match &node.widget {
        Widget::TableRow { .. } => match node.parent.and_then(|p| widgets.get(&p)) {
            Some(Widget::Table { .. }) => None,
            parent => Some(format!(
                "TableRow {} is inside {}: rows belong to a Table",
                node.id,
                parent.map_or("nothing", |p| p.name())
            )),
        },
        Widget::Table { columns, data, .. } => {
            for (i, &child) in inside.iter().enumerate() {
                let row = &nodes[child];
                let Widget::TableRow { header, .. } = row.widget else {
                    return Some(format!(
                        "{} {} is inside Table {}, which holds TableRows",
                        row.widget.name(),
                        row.id,
                        node.id
                    ));
                };
                if header && i > 0 {
                    return Some(format!(
                        "TableRow {} is a header, but not the first row of Table {}",
                        row.id, node.id
                    ));
                }
                if !header && data.is_some() {
                    return Some(format!(
                        "TableRow {} is a row of Table {}, which has its rows from data: one of them",
                        row.id, node.id
                    ));
                }
                let cells = children.get(&row.id).map_or(0, Vec::len);
                if cells > columns.len() {
                    return Some(format!(
                        "TableRow {} has {cells} cells, Table {} has {} columns",
                        row.id,
                        node.id,
                        columns.len()
                    ));
                }
            }
            None
        }
        Widget::Strip { sizes, .. } if inside.len() > sizes.len() => Some(format!(
            "Strip {} holds {} nodes in {} sizes: one node per size",
            node.id,
            inside.len(),
            sizes.len()
        )),
        _ => None,
    }
}

/// A checked tree: the nodes, where each one is and which are inside which.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Tree {
    pub nodes: Vec<UiNode>,
    /// The panels, windows and areas, in the order of the list.
    pub roots: Vec<usize>,
    /// The nodes inside each container, in the order of the list.
    pub children: HashMap<u64, Vec<usize>>,
    /// The menus, tooltips and popups that belong to each node ([Widget::is_attached]), in the order of the list.
    pub attached: HashMap<u64, Vec<usize>>,
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
        let mut attached: HashMap<u64, Vec<usize>> = HashMap::new();
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
                None if node.widget.is_attached() => {
                    return Err(invalid(format!(
                        "{} has no parent: it belongs to the node it opens on",
                        what()
                    )))
                }
                None => {
                    return Err(invalid(format!(
                        "{} has no parent: only SidePanel, TopBar, BottomBar, CentralPanel, Window, Area, Modal and Shortcut stand at the top",
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
                    Some(p) if node.widget.is_attached() => {
                        let kind = std::mem::discriminant(&node.widget);
                        let mine = attached.entry(parent).or_default();
                        if p.is_attached() || matches!(p, Widget::Shortcut { .. } | Widget::Modal { .. }) {
                            return Err(invalid(format!(
                                "{} belongs to {} {parent}, which cannot open it",
                                what(),
                                p.name()
                            )));
                        }
                        let popup = matches!(node.widget, Widget::Popup { .. });
                        if !popup && mine.iter().any(|&i| std::mem::discriminant(&nodes[i].widget) == kind) {
                            return Err(invalid(format!(
                                "{} is the second {} of {parent}: a node has one",
                                what(),
                                node.widget.name()
                            )));
                        }
                        let text = nodes.iter().find(|n| n.id == parent).and_then(|n| n.tooltip.as_ref());
                        if matches!(node.widget, Widget::Tooltip) && text.is_some() {
                            return Err(invalid(format!(
                                "{} belongs to {parent}, which has a tooltip text as well: one of them",
                                what()
                            )));
                        }
                        mine.push(index);
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
            let id = nodes[index].id;
            stack.extend(
                children
                    .get(&id)
                    .into_iter()
                    .chain(attached.get(&id))
                    .flatten(),
            );
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
        if let Some(why) = nodes
            .iter()
            .find_map(|n| tables_and_strips(n, &nodes, &widgets, &children))
        {
            return Err(invalid(why));
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
            attached,
        })
    }

    pub fn children(&self, id: u64) -> &[usize] {
        self.children.get(&id).map_or(&[], Vec::as_slice)
    }
}
