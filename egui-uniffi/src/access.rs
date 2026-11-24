//! Accessibility: egui's AccessKit tree as records for Kotlin, and what assistive technology does with it back into egui.
//!
//! egui describes every frame as an AccessKit tree when it is asked to ([crate::EguiOverlay::set_accessibility]). The
//! overlay turns it into a flat list of [UiAccessNode]s without the many containers egui makes for its layout, and
//! Kotlin hands that on to the platform: Compose's semantics (Android, desktop), an ARIA mirror in the browser. A click,
//! a new value or the focus from a screen reader comes back with [crate::EguiOverlay::access_action] and its siblings
//! and reaches egui as AccessKit's action requests, so the nodes report their usual events.

use crate::state::UiState;
use crate::{NoticeLevel, TextStyle, UiNode, UiRect, Widget};
use std::collections::{HashMap, HashSet};
use three_d::egui::{self, accesskit};

/// What a node is for assistive technology: AccessKit's roles that egui and the DSL give. The comment names the ARIA
/// role a browser mirror uses and Compose's `Role` where there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AccessRole {
    /// Text (ARIA: none, the text itself).
    Label,
    /// A heading (ARIA `heading`, Compose `Modifier.semantics { heading() }`).
    Heading,
    /// ARIA `button`, Compose `Role.Button`.
    Button,
    /// ARIA `link`.
    Link,
    /// ARIA `checkbox`, Compose `Role.Checkbox`.
    CheckBox,
    /// ARIA `switch`, Compose `Role.Switch`.
    Switch,
    /// ARIA `radio`, Compose `Role.RadioButton`.
    RadioButton,
    /// ARIA `radiogroup`.
    RadioGroup,
    /// ARIA `combobox`, Compose `Role.DropdownList`.
    ComboBox,
    /// ARIA `slider`, Compose's `setProgress`.
    Slider,
    /// ARIA `spinbutton`, Compose `Role.ValuePicker`.
    SpinButton,
    /// One line to type into (ARIA `textbox`).
    TextInput,
    /// Several lines to type into (ARIA `textbox` with `aria-multiline`).
    MultilineTextInput,
    /// A password (ARIA `textbox`, an `<input type=password>`).
    PasswordInput,
    /// ARIA `img`, Compose `Role.Image`.
    Image,
    /// A color to pick (ARIA `button`).
    ColorWell,
    /// ARIA `progressbar`.
    ProgressIndicator,
    /// ARIA `tab`, Compose `Role.Tab`.
    Tab,
    /// ARIA `tablist`.
    TabList,
    /// An egui window (ARIA `dialog` without `aria-modal`).
    Window,
    /// A modal dialog (ARIA `dialog` with `aria-modal`).
    Dialog,
    /// A panel (ARIA `region`).
    Pane,
    /// Nodes that belong together under a name (ARIA `group`).
    Group,
    /// ARIA `list`.
    List,
    /// ARIA `listitem`.
    ListItem,
    /// ARIA `menu`.
    Menu,
    /// ARIA `menubar`.
    MenuBar,
    /// ARIA `menuitem`.
    MenuItem,
    /// ARIA `tooltip`.
    Tooltip,
    /// ARIA `table`.
    Table,
    /// ARIA `row`.
    Row,
    /// ARIA `cell`.
    Cell,
    /// A message that is read when it changes, politely (ARIA `status`).
    Status,
    /// A message that is read at once (ARIA `alert`).
    Alert,
    /// A drawing (ARIA `img`).
    Canvas,
    /// ARIA `scrollbar`.
    ScrollBar,
    /// The edge of a panel or window to drag (ARIA `separator`).
    Splitter,
    /// None of these.
    Unknown,
}

/// What assistive technology can do with a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AccessAction {
    /// Clicks it, as a press and release of the pointer would.
    Click,
    /// Gives it the keyboard.
    Focus,
    /// One step up (a slider, a number).
    Increment,
    /// One step down.
    Decrement,
    /// A new value: [crate::EguiOverlay::access_set_value] for numbers, [crate::EguiOverlay::access_set_text] for text.
    SetValue,
    /// Scrolls the areas it is in until it is seen.
    ScrollIntoView,
}

/// The state of a checkbox, a switch, a toggled button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AccessToggle {
    Off,
    On,
    /// Neither: a checkbox for some of several things.
    Mixed,
}

/// How a change of a node is announced (Compose's `LiveRegionMode`, ARIA's `aria-live`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AccessLive {
    /// When the reader is done with what it reads.
    Polite,
    /// At once.
    Assertive,
}

/// What a node tells assistive technology beyond its widget, as Compose's `Modifier.semantics`: put on
/// [crate::UiModifier::semantics].
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct UiSemantics {
    /// Its name, read instead of its text (Compose's `contentDescription`): an icon button "Close", an image.
    #[uniffi(default)]
    pub label: Option<String>,
    /// Read after the name (a hint, ARIA's `aria-description`).
    #[uniffi(default)]
    pub description: Option<String>,
    /// Its state in words, read instead of the state of its widget (Compose's `stateDescription`): "3 of 5 done".
    #[uniffi(default)]
    pub state: Option<String>,
    /// What it is, `None` what its widget is.
    #[uniffi(default)]
    pub role: Option<AccessRole>,
    /// Whether it is chosen (a tab, an item of a list).
    #[uniffi(default)]
    pub selected: Option<bool>,
    /// Changes of it are read out without the user going there (Compose's `liveRegion`): a status line, a toast.
    #[uniffi(default)]
    pub live: Option<AccessLive>,
    /// It and everything inside count as one node whose name is the text inside (Compose's `mergeDescendants`): a card
    /// read as "Laternen, 12 Objekte" instead of two texts.
    #[uniffi(default)]
    pub merge: bool,
    /// It and everything inside are left out (Compose's `hideFromAccessibility`): decoration, a picture with its text
    /// beside it.
    #[uniffi(default)]
    pub hidden: bool,
}

/// A node of the accessibility tree.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UiAccessNode {
    /// Its id for [crate::EguiOverlay::access_action]; it stays the same from frame to frame as long as the node does.
    pub id: u64,
    /// The node it is in (an earlier one of [UiAccessTree::nodes]), `None` at the top.
    pub parent: Option<u64>,
    /// The id of the [UiNode] it is, if it is one (and not a part of a widget, a cell of a table's data).
    pub node: Option<u64>,
    pub role: AccessRole,
    /// What it is called: the text of a button, a label, a heading; the title of a window.
    pub label: Option<String>,
    pub description: Option<String>,
    /// Its value as text: what a text field holds, a slider's number as shown.
    pub value: Option<String>,
    /// Its value as a number (a slider, a drag value, a progress bar).
    pub numeric: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub toggled: Option<AccessToggle>,
    pub selected: Option<bool>,
    /// Whether it is open (a section, a combo box), `None` if it does not open.
    pub expanded: Option<bool>,
    /// Its state in words ([UiSemantics::state]).
    pub state: Option<String>,
    /// What an empty text field shows.
    pub placeholder: Option<String>,
    pub live: Option<AccessLive>,
    pub disabled: bool,
    pub read_only: bool,
    /// Whether it has the keyboard.
    pub focused: bool,
    /// Where it is, in pixels from the top left as [crate::EguiOverlay::bounds].
    pub bounds: UiRect,
    pub actions: Vec<AccessAction>,
}

/// The accessibility tree of a frame: the nodes in reading order, each after the node it is in.
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct UiAccessTree {
    pub nodes: Vec<UiAccessNode>,
    /// The node with the keyboard, if any.
    pub focus: Option<u64>,
}

/// The AccessKit role of an [AccessRole].
fn kit_role(role: AccessRole) -> accesskit::Role {
    use accesskit::Role as R;
    match role {
        AccessRole::Label => R::Label,
        AccessRole::Heading => R::Heading,
        AccessRole::Button => R::Button,
        AccessRole::Link => R::Link,
        AccessRole::CheckBox => R::CheckBox,
        AccessRole::Switch => R::Switch,
        AccessRole::RadioButton => R::RadioButton,
        AccessRole::RadioGroup => R::RadioGroup,
        AccessRole::ComboBox => R::ComboBox,
        AccessRole::Slider => R::Slider,
        AccessRole::SpinButton => R::SpinButton,
        AccessRole::TextInput => R::TextInput,
        AccessRole::MultilineTextInput => R::MultilineTextInput,
        AccessRole::PasswordInput => R::PasswordInput,
        AccessRole::Image => R::Image,
        AccessRole::ColorWell => R::ColorWell,
        AccessRole::ProgressIndicator => R::ProgressIndicator,
        AccessRole::Tab => R::Tab,
        AccessRole::TabList => R::TabList,
        AccessRole::Window => R::Window,
        AccessRole::Dialog => R::Dialog,
        AccessRole::Pane => R::Pane,
        AccessRole::Group => R::Group,
        AccessRole::List => R::List,
        AccessRole::ListItem => R::ListItem,
        AccessRole::Menu => R::Menu,
        AccessRole::MenuBar => R::MenuBar,
        AccessRole::MenuItem => R::MenuItem,
        AccessRole::Tooltip => R::Tooltip,
        AccessRole::Table => R::Table,
        AccessRole::Row => R::Row,
        AccessRole::Cell => R::Cell,
        AccessRole::Status => R::Status,
        AccessRole::Alert => R::Alert,
        AccessRole::Canvas => R::Canvas,
        AccessRole::ScrollBar => R::ScrollBar,
        AccessRole::Splitter => R::Splitter,
        AccessRole::Unknown => R::Unknown,
    }
}

/// The [AccessRole] of an AccessKit role; `None` for the containers egui makes for its layout.
fn role(role: accesskit::Role) -> Option<AccessRole> {
    use accesskit::Role as R;
    Some(match role {
        R::Label | R::Paragraph => AccessRole::Label,
        R::Heading => AccessRole::Heading,
        R::Button | R::DefaultButton => AccessRole::Button,
        R::Link => AccessRole::Link,
        R::CheckBox => AccessRole::CheckBox,
        R::Switch => AccessRole::Switch,
        R::RadioButton => AccessRole::RadioButton,
        R::RadioGroup => AccessRole::RadioGroup,
        R::ComboBox | R::EditableComboBox => AccessRole::ComboBox,
        R::Slider => AccessRole::Slider,
        R::SpinButton => AccessRole::SpinButton,
        R::TextInput
        | R::SearchInput
        | R::EmailInput
        | R::NumberInput
        | R::PhoneNumberInput
        | R::UrlInput => AccessRole::TextInput,
        R::MultilineTextInput => AccessRole::MultilineTextInput,
        R::PasswordInput => AccessRole::PasswordInput,
        R::Image => AccessRole::Image,
        R::ColorWell => AccessRole::ColorWell,
        R::ProgressIndicator | R::Meter => AccessRole::ProgressIndicator,
        R::Tab => AccessRole::Tab,
        R::TabList => AccessRole::TabList,
        R::Window => AccessRole::Window,
        R::Dialog | R::AlertDialog => AccessRole::Dialog,
        R::Pane | R::Region => AccessRole::Pane,
        R::Group => AccessRole::Group,
        R::List => AccessRole::List,
        R::ListItem => AccessRole::ListItem,
        R::Menu => AccessRole::Menu,
        R::MenuBar => AccessRole::MenuBar,
        R::MenuItem | R::MenuItemCheckBox | R::MenuItemRadio => AccessRole::MenuItem,
        R::Tooltip => AccessRole::Tooltip,
        R::Table | R::Grid => AccessRole::Table,
        R::Row => AccessRole::Row,
        R::Cell | R::GridCell => AccessRole::Cell,
        R::Status => AccessRole::Status,
        R::Alert => AccessRole::Alert,
        R::Canvas => AccessRole::Canvas,
        R::ScrollBar => AccessRole::ScrollBar,
        R::Splitter => AccessRole::Splitter,
        R::GenericContainer | R::Unknown => return None,
        _ => AccessRole::Unknown,
    })
}

/// The role the DSL gives a node where egui's is too vague (a heading, a switch, a dialog, a notice) or missing (a
/// panel), with how its changes are read and a name.
fn dsl_role(node: &UiNode) -> Option<(accesskit::Role, Option<accesskit::Live>, Option<&str>)> {
    use accesskit::{Live, Role};
    Some(match &node.widget {
        // Its text as its name, as egui has it as the value of a label
        Widget::Text {
            style: TextStyle::Heading,
            text,
            spans,
            ..
        } => (
            Role::Heading,
            None,
            Some(text.as_str()).filter(|t| !t.is_empty() && spans.is_empty()),
        ),
        Widget::Switch { .. } => (Role::Switch, None, None),
        Widget::Modal { .. } => (Role::Dialog, None, None),
        Widget::Notice {
            level: NoticeLevel::Error,
            ..
        } => (Role::Alert, Some(Live::Assertive), None),
        Widget::Notice { .. } => (Role::Status, Some(Live::Polite), None),
        Widget::SidePanel { title, .. } => (Role::Pane, None, title.as_deref()),
        Widget::TopBar { .. } | Widget::BottomBar { .. } | Widget::CentralPanel { .. } => {
            (Role::Pane, None, None)
        }
        // Drawings: not buttons, though they sense the pointer
        Widget::Plot { .. } | Widget::Canvas { .. } => (Role::Canvas, None, None),
        _ => return None,
    })
}

/// Notes the AccessKit node `id` of `node` (its response, or the ui of the nodes inside a panel or window) and puts its
/// semantics and the DSL's role onto it; `clicks` says whether it senses clicks.
pub(crate) fn node(
    ctx: &egui::Context,
    state: &mut UiState,
    node: &UiNode,
    id: egui::Id,
    clicks: bool,
) {
    let key = id.value();
    state.access_nodes.insert(key, node.id);
    let semantics = node.modifier.as_ref().and_then(|m| m.semantics.as_ref());
    let clickable = node
        .modifier
        .as_ref()
        .and_then(|m| m.reactions.as_ref())
        .is_some_and(|r| r.click || r.double_click)
        && clicks;
    let dsl = dsl_role(node);
    if semantics.is_none() && dsl.is_none() && !clickable {
        return;
    }
    if semantics.is_some_and(|s| s.merge) {
        state.access_merged.insert(key);
    }
    ctx.accesskit_node_builder(id, |n| {
        // A frame that is clicked (Modifier.clickable) is clicked by a screen reader as well
        if clickable {
            n.add_action(accesskit::Action::Click);
            n.add_action(accesskit::Action::Focus);
            if matches!(
                n.role(),
                accesskit::Role::GenericContainer | accesskit::Role::Unknown
            ) {
                n.set_role(accesskit::Role::Button);
            }
        }
        if let Some((role, live, label)) = dsl {
            n.set_role(role);
            if let Some(live) = live {
                n.set_live(live);
            }
            if let Some(label) = label {
                n.set_label(label);
            }
        }
        let Some(s) = semantics else {
            return;
        };
        if s.hidden {
            n.set_hidden();
        }
        if let Some(role) = s.role {
            n.set_role(kit_role(role));
        }
        if let Some(label) = &s.label {
            n.set_label(label.as_str());
        }
        if let Some(text) = &s.description {
            n.set_description(text.as_str());
        }
        if let Some(text) = &s.state {
            n.set_state_description(text.as_str());
        }
        if let Some(selected) = s.selected {
            n.set_selected(selected);
        }
        match s.live {
            Some(AccessLive::Polite) => n.set_live(accesskit::Live::Polite),
            Some(AccessLive::Assertive) => n.set_live(accesskit::Live::Assertive),
            None => {}
        }
    });
}

/// The AccessKit node of `id` is the heading `text`: the title of a panel.
pub(crate) fn heading(ctx: &egui::Context, id: egui::Id, text: &str) {
    ctx.accesskit_node_builder(id, |n| {
        n.set_role(accesskit::Role::Heading);
        n.set_label(text);
    });
}

/// Leaves the AccessKit node of `id` out of the tree: a label that is the name of the field beside it.
pub(crate) fn hide(ctx: &egui::Context, id: egui::Id) {
    ctx.accesskit_node_builder(id, |n| n.set_hidden());
}

/// The text new for a text field from assistive technology in this frame (an ARIA mirror's `<input>`, a reader's edit).
pub(crate) fn new_text(ui: &egui::Ui, id: egui::Id) -> Option<String> {
    ui.input(|i| {
        i.accesskit_action_requests(id, accesskit::Action::SetValue)
            .filter_map(|r| match &r.data {
                Some(accesskit::ActionData::Value(text)) => Some(text.to_string()),
                _ => None,
            })
            .last()
    })
}

/// A text field takes a new value from assistive technology: AccessKit lists the action for it.
pub(crate) fn takes_text(ctx: &egui::Context, id: egui::Id, label: &str) {
    ctx.accesskit_node_builder(id, |n| {
        n.add_action(accesskit::Action::SetValue);
        if !label.is_empty() && n.label().is_none() {
            n.set_label(label);
        }
    });
}

/// The request of `action` for AccessKit node `id`.
pub(crate) fn request(
    id: u64,
    action: AccessAction,
    data: Option<accesskit::ActionData>,
) -> egui::Event {
    egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
        action: match action {
            AccessAction::Click => accesskit::Action::Click,
            AccessAction::Focus => accesskit::Action::Focus,
            AccessAction::Increment => accesskit::Action::Increment,
            AccessAction::Decrement => accesskit::Action::Decrement,
            AccessAction::SetValue => accesskit::Action::SetValue,
            AccessAction::ScrollIntoView => accesskit::Action::ScrollIntoView,
        },
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(id),
        data,
    })
}

/// What [tree] left out or named, for the tree that goes to a platform's adapter unchanged otherwise (AT-SPI).
#[cfg_attr(not(all(target_os = "linux", feature = "atspi")), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Fix {
    /// A node that only repeats the one before.
    Hide(u64),
    /// A node named by the text after it.
    Name(u64, String),
}

/// Applies `fixes` to egui's tree as it is.
#[cfg_attr(not(all(target_os = "linux", feature = "atspi")), allow(dead_code))]
pub(crate) fn fix(update: &mut accesskit::TreeUpdate, fixes: &[Fix]) {
    for f in fixes {
        let (Fix::Hide(id) | Fix::Name(id, _)) = f;
        if let Some((_, n)) = update.nodes.iter_mut().find(|(i, _)| i.0 == *id) {
            match f {
                Fix::Hide(_) => n.set_hidden(),
                Fix::Name(_, name) => n.set_label(name.as_str()),
            }
        }
    }
}

/// Turns egui's AccessKit tree of a frame into the nodes for Kotlin; `scale` is pixels per point.
#[cfg(test)]
pub(crate) fn tree(update: &accesskit::TreeUpdate, scale: f32, state: &UiState) -> UiAccessTree {
    tree_and_fixes(update, scale, state).0
}

/// [tree] with what it left out or named of egui's tree.
pub(crate) fn tree_and_fixes(
    update: &accesskit::TreeUpdate,
    scale: f32,
    state: &UiState,
) -> (UiAccessTree, Vec<Fix>) {
    let nodes: HashMap<accesskit::NodeId, &accesskit::Node> =
        update.nodes.iter().map(|(id, n)| (*id, n)).collect();
    let mut walk = Walk {
        nodes: &nodes,
        scale: scale as f64,
        focus: update.focus,
        dsl: &state.access_nodes,
        merged: &state.access_merged,
        out: Vec::new(),
    };
    if let Some(tree) = &update.tree {
        if let Some(root) = nodes.get(&tree.root) {
            for &child in root.children() {
                walk.node(child, None);
            }
        }
    }
    let mut fixes = Vec::new();
    let out = without_repeats(walk.out, &mut fixes);
    let focus = out.iter().find(|n| n.focused).map(|n| n.id);
    (UiAccessTree { nodes: out, focus }, fixes)
}

/// Leaves out what only repeats the control before it (the text beside a slider or a switch that is its name, the
/// number box of a slider, which takes the same values) and makes the text after a control without a name its name
/// (egui puts the label of a drag value or a combo box after it).
fn without_repeats(nodes: Vec<UiAccessNode>, fixes: &mut Vec<Fix>) -> Vec<UiAccessNode> {
    let mut out: Vec<UiAccessNode> = Vec::with_capacity(nodes.len());
    for n in nodes {
        let before = out.iter_mut().rev().find(|b| b.parent == n.parent);
        if let Some(b) = before {
            let control = !matches!(
                b.role,
                AccessRole::Label
                    | AccessRole::Heading
                    | AccessRole::Group
                    | AccessRole::Pane
                    | AccessRole::Status
                    | AccessRole::Alert
            );
            if n.role == AccessRole::Label && control && n.label.is_some() {
                let beside = (n.bounds.y - b.bounds.y).abs() < 0.5 * b.bounds.height.max(1.0);
                if b.label.is_none()
                    && beside
                    && matches!(b.role, AccessRole::SpinButton | AccessRole::ComboBox)
                {
                    fixes.push(Fix::Name(b.id, n.label.clone().unwrap_or_default()));
                    fixes.push(Fix::Hide(n.id));
                    b.label = n.label;
                    continue;
                }
                if n.label == b.label {
                    fixes.push(Fix::Hide(n.id));
                    continue;
                }
            }
            if n.role == AccessRole::SpinButton
                && b.role == AccessRole::Slider
                && n.label.is_none()
                && n.numeric == b.numeric
            {
                fixes.push(Fix::Hide(n.id));
                continue;
            }
        }
        out.push(n);
    }
    out
}

/// The rectangle around both.
fn union(a: UiRect, b: UiRect) -> UiRect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    UiRect {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}

struct Walk<'a> {
    nodes: &'a HashMap<accesskit::NodeId, &'a accesskit::Node>,
    scale: f64,
    focus: accesskit::NodeId,
    dsl: &'a HashMap<u64, u64>,
    merged: &'a HashSet<u64>,
    out: Vec<UiAccessNode>,
}

impl Walk<'_> {
    fn node(&mut self, id: accesskit::NodeId, parent: Option<u64>) {
        let Some(n) = self.nodes.get(&id).copied() else {
            return;
        };
        // A run of text is a part of its label, for the selection of text
        if n.is_hidden() || n.role() == accesskit::Role::TextRun {
            return;
        }
        let named = n.label().is_some() || n.description().is_some() || n.live().is_some();
        let merged = self.merged.contains(&id.0);
        let role = match role(n.role()) {
            Some(r) => r,
            // A container with a name is a group; the others only lay out
            None if named || merged => AccessRole::Group,
            None => {
                for &child in n.children() {
                    self.node(child, parent);
                }
                return;
            }
        };
        let mut access = self.access(id, n, role, parent);
        if merged {
            // One node: the texts inside are its name, unless it has one
            if access.label.is_none() {
                let mut texts = Vec::new();
                self.texts(n, &mut texts);
                if !texts.is_empty() {
                    access.label = Some(texts.join(", "));
                }
            }
            if n.bounds().is_none() {
                access.bounds = self.within(n).unwrap_or_default();
            }
            self.out.push(access);
            return;
        }
        let at = self.out.len();
        self.out.push(access);
        for &child in n.children() {
            self.node(child, Some(id.0));
        }
        // egui gives the containers of its layout no place: a container with a role (a tab list, a group) is where the
        // nodes inside are
        if n.bounds().is_none() {
            let inside = self.out[at + 1..]
                .iter()
                .map(|c| c.bounds)
                .filter(|b| b.width > 0.0 || b.height > 0.0)
                .reduce(union);
            if let Some(b) = inside {
                self.out[at].bounds = b;
            }
        }
    }

    /// Where the nodes inside `n` are, in pixels.
    fn within(&self, n: &accesskit::Node) -> Option<UiRect> {
        n.children()
            .iter()
            .filter_map(|c| self.nodes.get(c).copied())
            .filter_map(|c| {
                c.bounds()
                    .map(|b| UiRect {
                        x: (b.x0 * self.scale) as f32,
                        y: (b.y0 * self.scale) as f32,
                        width: ((b.x1 - b.x0) * self.scale) as f32,
                        height: ((b.y1 - b.y0) * self.scale) as f32,
                    })
                    .or_else(|| self.within(c))
            })
            .reduce(union)
    }

    /// The texts inside `n` that are not hidden, in reading order.
    fn texts(&self, n: &accesskit::Node, texts: &mut Vec<String>) {
        for child in n.children() {
            let Some(c) = self.nodes.get(child).copied() else {
                continue;
            };
            if c.is_hidden() {
                continue;
            }
            if let Some(text) = c.label().or_else(|| {
                matches!(
                    role(c.role()),
                    Some(AccessRole::Label | AccessRole::Heading)
                )
                .then(|| c.value())
                .flatten()
            }) {
                if !text.is_empty() {
                    texts.push(text.to_owned());
                }
            }
            self.texts(c, texts);
        }
    }

    fn access(
        &self,
        id: accesskit::NodeId,
        n: &accesskit::Node,
        role: AccessRole,
        parent: Option<u64>,
    ) -> UiAccessNode {
        let s = self.scale;
        let bounds = n.bounds().map_or(UiRect::default(), |b| UiRect {
            x: (b.x0 * s) as f32,
            y: (b.y0 * s) as f32,
            width: ((b.x1 - b.x0) * s) as f32,
            height: ((b.y1 - b.y0) * s) as f32,
        });
        // egui has the text of a label as its value: here it is its name, as in ARIA
        let text = matches!(
            role,
            AccessRole::Label | AccessRole::Heading | AccessRole::Status | AccessRole::Alert
        );
        let (label, value) = match (n.label(), n.value()) {
            (Some(l), v) => (Some(l.to_owned()), v.filter(|_| !text).map(str::to_owned)),
            (None, Some(v)) if text => (Some(v.to_owned()), None),
            (None, v) => (None, v.map(str::to_owned)),
        };
        // A text senses clicks for its selection: for a screen reader it is only read
        let actions = [
            (accesskit::Action::Click, AccessAction::Click),
            (accesskit::Action::Focus, AccessAction::Focus),
            (accesskit::Action::Increment, AccessAction::Increment),
            (accesskit::Action::Decrement, AccessAction::Decrement),
            (accesskit::Action::SetValue, AccessAction::SetValue),
        ]
        .into_iter()
        .filter(|(kit, _)| n.supports_action(*kit))
        .map(|(_, action)| action)
        .filter(|a| !text || !matches!(a, AccessAction::Click | AccessAction::Focus))
        .collect();
        UiAccessNode {
            id: id.0,
            parent,
            node: self.dsl.get(&id.0).copied(),
            role,
            label,
            description: n.description().map(str::to_owned),
            value,
            numeric: n.numeric_value(),
            min: n.min_numeric_value(),
            max: n.max_numeric_value(),
            step: n.numeric_value_step(),
            toggled: n.toggled().map(|t| match t {
                accesskit::Toggled::False => AccessToggle::Off,
                accesskit::Toggled::True => AccessToggle::On,
                accesskit::Toggled::Mixed => AccessToggle::Mixed,
            }),
            selected: n.is_selected(),
            expanded: n.is_expanded(),
            state: n.state_description().map(str::to_owned),
            placeholder: n.placeholder().map(str::to_owned),
            live: n.live().and_then(|l| match l {
                accesskit::Live::Off => None,
                accesskit::Live::Polite => Some(AccessLive::Polite),
                accesskit::Live::Assertive => Some(AccessLive::Assertive),
            }),
            disabled: n.is_disabled(),
            read_only: n.is_read_only(),
            focused: id == self.focus,
            bounds,
            actions,
        }
    }
}
