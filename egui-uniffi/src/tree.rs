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

/// What a node is. The first five stand at the top of the tree (no parent), the containers hold other nodes, the rest
/// are widgets. Sizes are in points of egui (pixels divided by [crate::EguiOverlay::set_scale]).
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
    },
    /// A bar along the top edge; its nodes stand side by side.
    TopBar,
    /// A bar along the bottom edge; its nodes stand side by side.
    BottomBar,
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
    Row,
    /// Its nodes one below the other (as in a panel).
    Column,
    /// Its nodes in a table, row by row: a new row after every `columns` nodes.
    Grid {
        /// Nodes per row, at least 1.
        columns: u32,
        /// Every other row on a darker background.
        striped: bool,
    },
    /// A heading that folds its nodes in and out; egui remembers whether it is open.
    Collapsible {
        /// The heading.
        title: String,
        /// Open the first time it is shown.
        open: bool,
    },
    /// Its nodes scroll up and down.
    ScrollArea {
        /// The height it may take at most, `None` all there is.
        max_height: Option<f32>,
    },
    /// Its nodes in a frame.
    Group,
    /// A line of text.
    Text {
        /// What it says.
        text: String,
        /// How it looks.
        style: TextStyle,
        /// Its color as ARGB (Compose's `Color.toArgb()`), `None` the color of the style.
        color: Option<u32>,
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
    },
    /// A box to tick: [crate::UiEvent::Checked].
    Checkbox {
        /// Its text.
        text: String,
        /// Ticked.
        checked: bool,
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
                | Self::TopBar
                | Self::BottomBar
                | Self::Window { .. }
                | Self::Area { .. }
        )
    }

    /// May have nodes inside.
    pub(crate) fn is_container(&self) -> bool {
        self.is_root()
            || matches!(
                self,
                Self::Row
                    | Self::Column
                    | Self::Grid { .. }
                    | Self::Collapsible { .. }
                    | Self::ScrollArea { .. }
                    | Self::Group
            )
    }

    fn name(&self) -> &'static str {
        match self {
            Self::SidePanel { .. } => "SidePanel",
            Self::TopBar => "TopBar",
            Self::BottomBar => "BottomBar",
            Self::Window { .. } => "Window",
            Self::Area { .. } => "Area",
            Self::Row => "Row",
            Self::Column => "Column",
            Self::Grid { .. } => "Grid",
            Self::Collapsible { .. } => "Collapsible",
            Self::ScrollArea { .. } => "ScrollArea",
            Self::Group => "Group",
            Self::Text { .. } => "Text",
            Self::Separator => "Separator",
            Self::Spacer { .. } => "Spacer",
            Self::Button { .. } => "Button",
            Self::Checkbox { .. } => "Checkbox",
            Self::Radio { .. } => "Radio",
            Self::ComboBox { .. } => "ComboBox",
            Self::SliderInt { .. } => "SliderInt",
            Self::SliderFloat { .. } => "SliderFloat",
            Self::DragValue { .. } => "DragValue",
            Self::TextField { .. } => "TextField",
            Self::ProgressBar { .. } => "ProgressBar",
            Self::Spinner => "Spinner",
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
        match self {
            Self::Grid { columns: 0, .. } => Some("a grid needs at least one column".into()),
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
            if let Some(why) = node.widget.check() {
                return Err(invalid(format!("{}: {why}", what())));
            }
            match node.parent {
                None if node.widget.is_root() => roots.push(index),
                None => {
                    return Err(invalid(format!(
                        "{} has no parent: only SidePanel, TopBar, BottomBar, Window and Area stand at the top",
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
