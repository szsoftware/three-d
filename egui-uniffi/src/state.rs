use crate::{TableSort, Tree, UiDate, UiError, UiNode};
use std::collections::HashMap;

/// What the user did in a frame, for Kotlin to take with [crate::EguiOverlay::take_events]: the DSL calls the lambda of
/// the node with that id. Values that changed several times before Kotlin took them come as the last one.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum UiEvent {
    /// A [crate::Widget::Button] was clicked.
    Clicked {
        /// The node.
        id: u64,
    },
    /// A [crate::Widget::Checkbox] was ticked or unticked.
    Checked {
        /// The node.
        id: u64,
        /// Ticked now.
        checked: bool,
    },
    /// Another option of a [crate::Widget::Radio] or [crate::Widget::ComboBox] was chosen.
    Selected {
        /// The node.
        id: u64,
        /// The index of the option.
        index: u32,
    },
    /// A [crate::Widget::SliderInt] has another value.
    IntChanged {
        /// The node.
        id: u64,
        /// The value.
        value: i64,
    },
    /// A [crate::Widget::SliderFloat] or [crate::Widget::DragValue] has another value.
    FloatChanged {
        /// The node.
        id: u64,
        /// The value.
        value: f64,
    },
    /// The text of a [crate::Widget::TextField] changed.
    TextChanged {
        /// The node.
        id: u64,
        /// The whole text.
        value: String,
    },
    /// Enter was pressed in a [crate::Widget::TextField].
    Submitted {
        /// The node.
        id: u64,
        /// The whole text.
        value: String,
    },
    /// The close button of a [crate::Widget::Window] was clicked; the window stays hidden until a tree without it or
    /// with it again (after the events were taken).
    Closed {
        /// The node.
        id: u64,
    },
    /// Another color was chosen in a [crate::Widget::ColorPicker].
    ColorChanged {
        /// The node.
        id: u64,
        /// ARGB.
        color: u32,
    },
    /// Another day was chosen in a [crate::Widget::DatePicker].
    DateChanged {
        /// The node.
        id: u64,
        /// The day.
        date: UiDate,
    },
    /// A node with [crate::UiModifier::focus_events] got or lost the keyboard.
    FocusChanged {
        /// The node.
        id: u64,
        /// Has it now.
        focused: bool,
    },
    /// The user selected other rows of a [crate::Widget::Table].
    RowsSelected {
        /// The table.
        id: u64,
        /// The indices of all selected rows, in order.
        rows: Vec<u64>,
    },
    /// The user double-clicked a row of a [crate::Widget::Table] with a selection: open it.
    RowActivated {
        /// The table.
        id: u64,
        /// Its index.
        row: u64,
    },
    /// The user clicked the title of a sortable column of a [crate::Widget::Table]: Kotlin sorts the rows so and sends
    /// the table with this sort (the arrow shows it until then).
    SortChanged {
        /// The table.
        id: u64,
        /// The column, ascending unless it was sorted ascending by it before.
        sort: TableSort,
    },
}

impl UiEvent {
    pub(crate) fn id(&self) -> u64 {
        match self {
            Self::Clicked { id }
            | Self::Checked { id, .. }
            | Self::Selected { id, .. }
            | Self::IntChanged { id, .. }
            | Self::FloatChanged { id, .. }
            | Self::TextChanged { id, .. }
            | Self::Submitted { id, .. }
            | Self::Closed { id }
            | Self::ColorChanged { id, .. }
            | Self::DateChanged { id, .. }
            | Self::FocusChanged { id, .. }
            | Self::RowsSelected { id, .. }
            | Self::RowActivated { id, .. }
            | Self::SortChanged { id, .. } => *id,
        }
    }

    /// Only the last value counts: a newer event of the same kind and node replaces it.
    fn is_value(&self) -> bool {
        matches!(
            self,
            Self::Checked { .. }
                | Self::Selected { .. }
                | Self::IntChanged { .. }
                | Self::FloatChanged { .. }
                | Self::TextChanged { .. }
                | Self::ColorChanged { .. }
                | Self::DateChanged { .. }
                | Self::RowsSelected { .. }
                | Self::SortChanged { .. }
        )
    }
}

/// A value the user gave a widget, shown until Kotlin answers with a tree after taking the event: so a box is ticked in
/// the same frame, and Kotlin's value wins in the end (also when Kotlin does not take the change).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Local {
    Bool(bool),
    Index(u32),
    Int(i64),
    Float(f64),
    Text(String),
    Color(u32),
    Date(UiDate),
    Closed,
    Rows(Vec<u64>),
    Sort(TableSort),
}

/// Who gets the keyboard with the next frame ([crate::EguiOverlay::request_focus]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    /// This node.
    Node(u64),
    /// Nobody.
    Clear,
}

#[derive(Debug, Clone)]
struct LocalValue {
    value: Local,
    /// Kotlin took the event of this value: the next tree replaces it.
    taken: bool,
}

/// A rectangle in egui's points, from the top left.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct UiRect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl UiRect {
    pub(crate) fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }
}

impl From<three_d::egui::Rect> for UiRect {
    fn from(r: three_d::egui::Rect) -> Self {
        Self {
            x: r.min.x,
            y: r.min.y,
            width: r.width(),
            height: r.height(),
        }
    }
}

/// The text of the rows of tables ([crate::EguiOverlay::set_table_data]), row by row.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct TableData {
    /// Cells per row, at least 1.
    pub columns: usize,
    pub cells: Vec<String>,
}

impl TableData {
    pub fn rows(&self) -> usize {
        self.cells.len() / self.columns
    }

    pub fn cell(&self, row: usize, column: usize) -> &str {
        &self.cells[row * self.columns + column]
    }
}

/// The tree, the values the user gave and the events not taken yet: everything that is no business of the painter's thread.
#[derive(Debug, Default)]
pub(crate) struct UiState {
    pub tree: Tree,
    local: HashMap<u64, LocalValue>,
    events: Vec<UiEvent>,
    /// Where each node was drawn last frame, in points.
    pub bounds: HashMap<u64, UiRect>,
    /// [UiState::bounds] of the frame before, while a frame is drawn: a row that wraps needs the sizes before it
    /// draws.
    pub previous: HashMap<u64, UiRect>,
    /// The panels of last frame: they lie on egui's background layer, which has no area of its own to hit.
    pub panels: Vec<UiRect>,
    /// Something changed that the picture does not show yet.
    pub dirty: bool,
    /// Who gets the keyboard when it is drawn next.
    pub focus: Option<Focus>,
    /// The node with [crate::UiModifier::focus_events] that has the keyboard, as reported.
    pub focused: Option<u64>,
    /// The rows of tables by their key.
    pub tables: HashMap<String, TableData>,
}

impl UiState {
    /// Without nodes, to be drawn.
    pub fn new() -> Self {
        Self {
            dirty: true,
            ..Self::default()
        }
    }

    /// A new tree from Kotlin. Values whose events Kotlin took make way for the tree's; those it has not seen stay.
    pub fn set_tree(&mut self, nodes: Vec<UiNode>) -> Result<(), UiError> {
        let tree = Tree::new(nodes)?;
        let before = self.local.len();
        self.local
            .retain(|id, local| !local.taken && tree.ids.contains(id));
        if matches!(self.focus, Some(Focus::Node(id)) if !tree.ids.contains(&id)) {
            self.focus = None;
        }
        if tree != self.tree || self.local.len() != before {
            self.dirty = true;
        }
        self.tree = tree;
        Ok(())
    }

    pub fn take_events(&mut self) -> Vec<UiEvent> {
        for local in self.local.values_mut() {
            local.taken = true;
        }
        std::mem::take(&mut self.events)
    }

    pub fn local(&self, id: u64) -> Option<&Local> {
        self.local.get(&id).map(|l| &l.value)
    }

    /// The user gave node `event.id()` the value `value`.
    pub fn change(&mut self, value: Local, event: UiEvent) {
        self.local.insert(
            event.id(),
            LocalValue {
                value,
                taken: false,
            },
        );
        self.push(event);
    }

    pub fn push(&mut self, event: UiEvent) {
        if event.is_value() {
            if let Some(old) = self.events.iter_mut().find(|e| {
                e.id() == event.id() && std::mem::discriminant(*e) == std::mem::discriminant(&event)
            }) {
                *old = event;
                return;
            }
        }
        self.events.push(event);
    }
}
