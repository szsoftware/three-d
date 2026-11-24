//! The tree drawn with egui, in immediate mode, every frame.

use crate::state::Focus;
use crate::state::{Local, UiRect, UiState};
use crate::{
    Align, Anchor, Arrangement, ColumnWidth, NoticeLevel, PlotSeries, PopupClose, PopupSide,
    ScrollDirection, SeriesKind, Side, SliderClamping, SliderHandle, StripSize, TableColumn,
    TableSelection, TableSort, TextSpan, TextStyle, Tree, UiDate, UiEvent, UiInsets, UiModifier,
    UiNode, Widget,
};
use std::collections::HashMap;
use three_d::egui::{self, Color32, Id, Layout, Response, RichText, Ui, UiBuilder};

/// Draws the whole tree onto the root `ui` of a frame and notes what the user did in `state`.
pub(crate) fn draw(ui: &mut Ui, state: &mut UiState) {
    let tree = std::mem::take(&mut state.tree);
    state.previous = std::mem::take(&mut state.bounds);
    state.screen_bounds.clear();
    state.panels.clear();
    crate::pictures::hand_images(ui.ctx(), state);
    if state.focus == Some(Focus::Clear) {
        state.focus = None;
        ui.ctx().memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }
    let mut frame = Frame {
        tree: &tree,
        state,
        watched: Vec::new(),
        scenes: 0,
    };
    for &root in &tree.roots {
        let node = &tree.nodes[root];
        if matches!(node.widget, Widget::Shortcut { .. }) {
            frame.shortcut(ui.ctx(), node);
        } else {
            frame.root(ui, node);
        }
    }
    let watched = frame.watched;
    // Who has the keyboard after all nodes are drawn: one may have taken it from another drawn before
    let focused = ui.ctx().memory(|m| {
        watched
            .iter()
            .find(|(_, id)| m.has_focus(*id))
            .map(|(node, _)| *node)
    });
    if focused != state.focused {
        if let Some(id) = state.focused {
            state.push(UiEvent::FocusChanged { id, focused: false });
        }
        if let Some(id) = focused {
            state.push(UiEvent::FocusChanged { id, focused: true });
        }
        state.focused = focused;
    }
    state.tree = tree;
}

/// ARGB of an egui color.
fn to_argb(c: Color32) -> u32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    u32::from_be_bytes([a, r, g, b])
}

fn argb(c: u32) -> Color32 {
    let [a, r, g, b] = c.to_be_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub(crate) fn align2(anchor: Anchor) -> egui::Align2 {
    match anchor {
        Anchor::TopLeft => egui::Align2::LEFT_TOP,
        Anchor::Top => egui::Align2::CENTER_TOP,
        Anchor::TopRight => egui::Align2::RIGHT_TOP,
        Anchor::Left => egui::Align2::LEFT_CENTER,
        Anchor::Center => egui::Align2::CENTER_CENTER,
        Anchor::Right => egui::Align2::RIGHT_CENTER,
        Anchor::BottomLeft => egui::Align2::LEFT_BOTTOM,
        Anchor::Bottom => egui::Align2::CENTER_BOTTOM,
        Anchor::BottomRight => egui::Align2::RIGHT_BOTTOM,
    }
}

/// Where a popup opens next to its parent.
fn rect_align(side: Option<PopupSide>, align: Option<Align>) -> egui::RectAlign {
    use egui::RectAlign as R;
    let pick = |start, center, end| match align.unwrap_or(Align::Start) {
        Align::Start => start,
        Align::Center => center,
        Align::End => end,
    };
    match side.unwrap_or(PopupSide::Below) {
        PopupSide::Below => pick(R::BOTTOM_START, R::BOTTOM, R::BOTTOM_END),
        PopupSide::Above => pick(R::TOP_START, R::TOP, R::TOP_END),
        PopupSide::Left => pick(R::LEFT_START, R::LEFT, R::LEFT_END),
        PopupSide::Right => pick(R::RIGHT_START, R::RIGHT, R::RIGHT_END),
    }
}

fn node_id(node: &UiNode) -> Id {
    Id::new(("three-d-egui", node.id))
}

fn tooltip(response: Response, node: &UiNode) -> Response {
    match &node.tooltip {
        Some(text) if node.enabled => response.on_hover_text(text),
        Some(text) => response.on_disabled_hover_text(text),
        None => response,
    }
}

struct Frame<'a> {
    tree: &'a Tree,
    state: &'a mut UiState,
    /// The nodes that report focus ([UiModifier::focus_events]) with the ids of their responses.
    watched: Vec<(u64, Id)>,
    /// How many scenes the node drawn now is in: its rectangle is not where it is seen.
    scenes: usize,
}

impl Frame<'_> {
    fn children(&mut self, ui: &mut Ui, node: &UiNode) {
        for &child in self.tree.children(node.id) {
            self.node(ui, &self.tree.nodes[child]);
        }
    }

    /// The nodes inside `node`, greyed out with it.
    fn inside(&mut self, ui: &mut Ui, node: &UiNode) {
        if node.enabled {
            self.children(ui, node);
        } else {
            ui.add_enabled_ui(false, |ui| self.children(ui, node));
        }
    }

    fn root(&mut self, ui: &mut Ui, node: &UiNode) {
        let id = node_id(node);
        let response = match &node.widget {
            Widget::SidePanel {
                side,
                title,
                width,
                resizable,
                min_width,
                max_width,
                expanded,
            } => {
                let panel = match side {
                    Side::Left => egui::Panel::left(id),
                    Side::Right => egui::Panel::right(id),
                };
                // Not resizable: exactly as wide, else egui shrinks it to its content
                let mut panel = if *resizable {
                    panel.resizable(true).default_size(*width)
                } else {
                    panel.resizable(false).exact_size(*width)
                };
                if *resizable {
                    if let Some(min) = min_width {
                        panel = panel.min_size(*min);
                    }
                    if let Some(max) = max_width {
                        panel = panel.max_size(*max);
                    }
                }
                // Folded away (after its animation) it is not there at all
                let Some(r) = panel.show_animated_inside(ui, *expanded, |ui| {
                    if let Some(title) = title {
                        ui.heading(title);
                    }
                    self.inside(ui, node);
                }) else {
                    return;
                };
                self.state.panels.push(r.response.rect.into());
                r.response
            }
            Widget::TopBar { expanded } | Widget::BottomBar { expanded } => {
                let panel = if matches!(node.widget, Widget::TopBar { .. }) {
                    egui::Panel::top(id)
                } else {
                    egui::Panel::bottom(id)
                };
                let Some(r) = panel.show_animated_inside(ui, *expanded, |ui| {
                    ui.horizontal(|ui| self.inside(ui, node));
                }) else {
                    return;
                };
                self.state.panels.push(r.response.rect.into());
                r.response
            }
            Widget::CentralPanel { frame } => {
                let panel = if *frame {
                    egui::CentralPanel::default()
                } else {
                    egui::CentralPanel::no_frame()
                };
                let r = panel.show_inside(ui, |ui| self.inside(ui, node));
                if *frame {
                    self.state.panels.push(r.response.rect.into());
                } else {
                    // Without a background only its nodes cover the picture: the rest goes to the application
                    for &child in self.tree.children(node.id) {
                        if let Some(&rect) = self.state.bounds.get(&self.tree.nodes[child].id) {
                            self.state.panels.push(rect);
                        }
                    }
                }
                r.response
            }
            Widget::Window {
                title,
                x,
                y,
                width,
                closable,
                collapsible,
                resizable,
                height,
                anchor,
                movable,
                title_bar,
                fixed_size,
                scroll,
                open: kotlin_open,
            } => {
                if self.state.local(node.id) == Some(&Local::Closed) {
                    return;
                }
                let mut open = *kotlin_open;
                let mut window = egui::Window::new(title.as_str())
                    .id(id)
                    .collapsible(*collapsible)
                    .resizable(*resizable && !*fixed_size)
                    .movable(*movable)
                    .title_bar(*title_bar)
                    .vscroll(*scroll);
                window = match anchor {
                    Some(anchor) => window.anchor(align2(*anchor), [*x, *y]),
                    None => window.default_pos([*x, *y]),
                };
                if *fixed_size {
                    if let Some(w) = width {
                        window = window.min_width(*w).max_width(*w);
                    }
                    if let Some(h) = height {
                        window = window.min_height(*h).max_height(*h);
                    }
                } else {
                    if let Some(w) = width {
                        window = window.default_width(*w);
                    }
                    if let Some(h) = height {
                        window = window.default_height(*h);
                    }
                }
                // Closed by Kotlin it fades out, as when its close button is clicked
                if *closable || !*kotlin_open {
                    window = window.open(&mut open);
                }
                let shown = window.show(ui.ctx(), |ui| {
                    // egui sizes a window that cannot be resized by its content: its inside takes the size
                    if *fixed_size {
                        if let Some(w) = width {
                            ui.set_width(*w);
                        }
                        if let Some(h) = height {
                            ui.set_height(*h);
                        }
                    }
                    self.inside(ui, node)
                });
                if *kotlin_open && !open {
                    self.state
                        .change(Local::Closed, UiEvent::Closed { id: node.id });
                }
                match shown {
                    Some(r) => r.response,
                    None => return,
                }
            }
            Widget::Area {
                anchor,
                dx,
                dy,
                frame,
            } => {
                let r = egui::Area::new(id)
                    .anchor(align2(*anchor), [*dx, *dy])
                    .show(ui.ctx(), |ui| {
                        // An area is as wide as its content: a frame time near the edge does not wrap
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        if *frame {
                            egui::Frame::popup(ui.style()).show(ui, |ui| self.inside(ui, node));
                        } else {
                            self.inside(ui, node);
                        }
                    });
                r.response
            }
            Widget::Modal { dismissible } => {
                if self.state.local(node.id) == Some(&Local::Closed) {
                    return;
                }
                let r = egui::Modal::new(id).show(ui.ctx(), |ui| self.inside(ui, node));
                if *dismissible && r.should_close() {
                    self.state
                        .change(Local::Closed, UiEvent::Closed { id: node.id });
                }
                r.response
            }
            // Tree::new keeps everything else away from the top
            _ => return,
        };
        self.state.bounds.insert(node.id, response.rect.into());
        self.attached(&response, node);
    }

    /// The menus, tooltips and popups that belong to `node`, at its `response`.
    fn attached(&mut self, response: &Response, node: &UiNode) {
        let Some(attached) = self.tree.attached.get(&node.id) else {
            return;
        };
        for &index in attached {
            let a = &self.tree.nodes[index];
            match &a.widget {
                Widget::ContextMenu => {
                    response.context_menu(|ui| self.inside(ui, a));
                }
                Widget::Tooltip if node.enabled => {
                    response.clone().on_hover_ui(|ui| self.inside(ui, a));
                }
                Widget::Tooltip => {
                    response
                        .clone()
                        .on_disabled_hover_ui(|ui| self.inside(ui, a));
                }
                Widget::Popup {
                    open,
                    side,
                    align,
                    close,
                    width,
                } => {
                    let shown = *open && self.state.local(a.id) != Some(&Local::Closed);
                    let mut still = shown;
                    let mut popup = egui::Popup::from_response(response)
                        .id(node_id(a))
                        .open_bool(&mut still)
                        .align(rect_align(*side, *align))
                        .close_behavior(match close.unwrap_or(PopupClose::ClickOutside) {
                            PopupClose::Click => egui::PopupCloseBehavior::CloseOnClick,
                            PopupClose::ClickOutside => {
                                egui::PopupCloseBehavior::CloseOnClickOutside
                            }
                            PopupClose::Never => egui::PopupCloseBehavior::IgnoreClicks,
                        });
                    if let Some(w) = width {
                        popup = popup.width(*w);
                    }
                    if let Some(r) = popup.show(|ui| self.inside(ui, a)) {
                        self.state.bounds.insert(a.id, r.response.rect.into());
                    }
                    if shown && !still {
                        self.state
                            .change(Local::Closed, UiEvent::Closed { id: a.id });
                    }
                }
                _ => {}
            }
        }
    }

    /// A node inside a container: in a scope of its own id, so that egui keeps its state when nodes before it come and go.
    fn node(&mut self, ui: &mut Ui, node: &UiNode) {
        // A key takes no room
        if matches!(node.widget, Widget::Shortcut { .. }) {
            self.shortcut(ui.ctx(), node);
            return;
        }
        let mut builder = UiBuilder::new().id_salt(("three-d-egui", node.id));
        if !node.enabled {
            builder = builder.disabled();
        }
        let modifier = node.modifier.as_ref();
        if modifier.is_some_and(|m| m.invisible) {
            builder = builder.invisible();
        }
        let mut align = modifier.and_then(|m| m.align).map(egui::Align::from);
        let placed = self.across(ui, node);
        if let Some((rect, _)) = placed {
            // Placed across already: from the top of its room on
            builder = builder
                .max_rect(rect)
                .layout(ui.layout().with_cross_align(egui::Align::Min));
            align = None;
        }
        let scope = ui.scope_builder(builder, |ui| match modifier {
            // A button wider than its text has it in the middle, as in Compose
            Some(m) => modified(
                ui,
                m,
                matches!(
                    node.widget,
                    Widget::Button { .. }
                        | Widget::SelectableLabel { .. }
                        | Widget::ToggleButton { .. }
                ),
                align,
                |ui| self.widget(ui, node),
            ),
            None => self.widget(ui, node),
        });
        // With a modifier the node is all of it, its frame too
        let rect = match (modifier, &scope.inner) {
            (None, Some(r)) => r.rect,
            _ => scope.response.rect,
        };
        if placed.is_some_and(|(_, last)| (rect.height() - last).abs() > 0.5) {
            ui.ctx()
                .request_discard("a node in a row changed its height");
        }
        self.state.bounds.insert(node.id, rect.into());
        if self.scenes > 0 {
            // Where it is seen: through the zoom and the move of the scenes it is in
            let seen = ui
                .ctx()
                .layer_transform_to_global(ui.layer_id())
                .map_or(rect, |t| t * rect);
            self.state.screen_bounds.insert(node.id, seen.into());
        }
        let response = scope.inner.unwrap_or(scope.response);
        if self.state.focus == Some(Focus::Node(node.id)) {
            self.state.focus = None;
            response.request_focus();
        }
        if modifier.is_some_and(|m| m.focus_events) {
            self.watched.push((node.id, response.id));
        }
        let response = tooltip(response, node);
        self.attached(&response, node);
    }

    /// A [Widget::Table]: the header from a header row or the titles of the columns, then the rows from nodes or from
    /// the table's data; only the rows that are seen are drawn.
    fn table(&mut self, ui: &mut Ui, node: &UiNode) {
        let Widget::Table {
            columns,
            data,
            striped,
            resizable,
            selection,
            selected,
            sort,
            row_height,
            max_height,
            stick_to_end,
            scroll_to,
        } = &node.widget
        else {
            return;
        };
        let tree = self.tree;
        let rows = tree.children(node.id);
        let (header_row, body_rows) = match rows.first() {
            Some(&first)
                if matches!(
                    tree.nodes[first].widget,
                    Widget::TableRow { header: true, .. }
                ) =>
            {
                (Some(&tree.nodes[first]), &rows[1..])
            }
            _ => (None, rows),
        };
        let line = ui
            .text_style_height(&egui::TextStyle::Body)
            .max(ui.spacing().interact_size.y);
        let height = row_height.unwrap_or(line);
        let row_height_of = |row: &UiNode| match row.widget {
            Widget::TableRow {
                height: Some(h), ..
            } => h,
            _ => height,
        };
        let (selected, sort) = match self.state.local(node.id) {
            Some(Local::Rows(rows)) => (rows.clone(), *sort),
            Some(Local::Sort(s)) => (selected.clone(), Some(*s)),
            _ => (selected.clone(), *sort),
        };
        let id = node_id(node);
        // Scrolls when Kotlin names another row, not in every frame (the user scrolls too)
        let scroll_key = id.with("scroll_to");
        let scroll = ui.data_mut(|d| {
            let last = d.get_temp::<Option<u64>>(scroll_key);
            d.insert_temp(scroll_key, *scroll_to);
            if last == Some(*scroll_to) {
                None
            } else {
                *scroll_to
            }
        });
        // egui_extras takes 200 points at least once rows scroll, also in a smaller cell: the rows take what the
        // container leaves them, in one that shrinks to its content what is seen below the header
        let titled = columns.iter().any(|c| !c.title.is_empty());
        let header_height = match header_row {
            Some(header) => row_height_of(header) + ui.spacing().item_spacing.y,
            None if titled => line + ui.spacing().item_spacing.y,
            None => 0.0,
        };
        let least = if ui.available_height() < 1.0 {
            (room_below(ui) - header_height).max(0.0)
        } else {
            0.0
        };
        // In egui's sizing pass (the first frame of a window) egui_extras makes each column as wide as its content,
        // which resizable columns would keep: they take their widths anew in the first frame after it
        let sizing = ui.is_sizing_pass();
        let sizing_key = id.with("sizing");
        let after_sizing = ui.data_mut(|d| {
            let was = d.get_temp::<bool>(sizing_key).unwrap_or(false);
            d.insert_temp(sizing_key, sizing);
            was
        });
        let mut builder = egui_extras::TableBuilder::new(ui)
            .id_salt(id)
            .striped(*striped)
            .resizable(*resizable)
            .stick_to_bottom(*stick_to_end)
            .max_scroll_height(max_height.unwrap_or(f32::INFINITY))
            .min_scrolled_height(max_height.map_or(least, |max| least.min(max)))
            .cell_layout(Layout::left_to_right(egui::Align::Center));
        if after_sizing && !sizing {
            builder.reset();
        }
        if selection.is_some() {
            builder = builder.sense(egui::Sense::click());
        }
        if let Some(row) = scroll {
            builder = builder.scroll_to_row(row as usize, Some(egui::Align::Center));
        }
        for c in columns {
            builder = builder.column(column(c));
        }
        // What the user did, applied after the table let go of the frame
        let mut clicked: Option<u64> = None;
        let mut sorted: Option<TableSort> = None;
        let this = std::cell::RefCell::new(&mut *self);
        let body = |body: egui_extras::TableBody<'_>| {
            let mut react = |row: &mut egui_extras::TableRow<'_, '_>| {
                if selection.is_some() {
                    let r = row.response();
                    if r.clicked() {
                        clicked = Some(row.index() as u64);
                    }
                }
            };
            match data {
                Some(key) => {
                    let this = this.borrow();
                    let Some(data) = this.state.tables.get(key) else {
                        return;
                    };
                    let shown = data.columns.min(columns.len());
                    body.rows(height, data.rows(), |mut row| {
                        let index = row.index();
                        row.set_selected(selected.contains(&(index as u64)));
                        for (c, column) in columns.iter().enumerate().take(shown) {
                            row.col(|ui| {
                                aligned(ui, column.align, |ui| {
                                    let text = styled(
                                        data.cell(index, c),
                                        column.style.unwrap_or(TextStyle::Body),
                                        None,
                                    );
                                    // A column that grows with its cells needs their whole text, others cut it
                                    let label = egui::Label::new(text).selectable(false);
                                    let grows = column.width.unwrap_or(ColumnWidth::Auto)
                                        == ColumnWidth::Auto
                                        && !column.resizable.unwrap_or(*resizable);
                                    ui.add(if grows {
                                        label.extend()
                                    } else {
                                        label.truncate()
                                    });
                                });
                            });
                        }
                        react(&mut row);
                    });
                }
                None => {
                    body.heterogeneous_rows(
                        body_rows.iter().map(|&r| row_height_of(&tree.nodes[r])),
                        |mut row| {
                            let index = row.index();
                            let row_node = &tree.nodes[body_rows[index]];
                            row.set_selected(selected.contains(&(index as u64)));
                            this.borrow_mut().cells(
                                &mut row,
                                row_node,
                                columns,
                                selection.is_some(),
                            );
                            react(&mut row);
                        },
                    );
                }
            }
        };
        match header_row {
            Some(header) => builder
                .header(row_height_of(header), |mut row| {
                    this.borrow_mut().cells(&mut row, header, columns, false)
                })
                .body(body),
            None if titled => builder
                .header(line, |mut row| {
                    for (i, c) in columns.iter().enumerate() {
                        row.col(|ui| {
                            let cell = ui.max_rect();
                            let mut text = c.title.clone();
                            let by_it = sort.filter(|s| s.column as usize == i);
                            if let Some(s) = by_it {
                                text.push_str(if s.ascending { " ⏶" } else { " ⏷" });
                            }
                            // The whole cell of a sortable title answers, lit while the pointer is on it
                            let hovered = c.sortable && ui.rect_contains_pointer(cell);
                            let mut text = RichText::new(text).strong();
                            if hovered {
                                text = text.color(ui.visuals().widgets.hovered.fg_stroke.color);
                            }
                            aligned(ui, c.align, |ui| {
                                ui.add(egui::Label::new(text).selectable(false))
                            });
                            if c.sortable
                                && ui
                                    .interact(cell, ui.id().with(("sort", i)), egui::Sense::click())
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                            {
                                sorted = Some(TableSort {
                                    column: i as u32,
                                    ascending: !by_it.is_some_and(|s| s.ascending),
                                });
                            }
                        });
                    }
                })
                .body(body),
            None => builder.body(body),
        };
        if let Some(sort) = sorted {
            self.state.change(
                Local::Sort(sort),
                UiEvent::SortChanged { id: node.id, sort },
            );
        }
        if let (Some(row), Some(selection)) = (clicked, selection) {
            let anchor_key = id.with("anchor");
            let modifiers = ui.input(|i| i.modifiers);
            let anchor = ui.data(|d| d.get_temp::<u64>(anchor_key));
            let rows = select_rows(*selection, &selected, row, modifiers, anchor);
            if !(*selection == TableSelection::Multiple && modifiers.shift) {
                ui.data_mut(|d| d.insert_temp(anchor_key, row));
            }
            if rows != selected {
                self.state.change(
                    Local::Rows(rows.clone()),
                    UiEvent::RowsSelected { id: node.id, rows },
                );
            }
            // The second click on the same row within egui's time for a double click opens it, a third does not
            // again. egui's own count takes a click within 0.6 s of the one before the last for a third one, wherever
            // that was, so a double click right after a click elsewhere would not count
            let now = ui.input(|i| i.time);
            let delay = ui.ctx().options(|o| o.input_options.max_double_click_delay);
            let clicks_key = id.with("clicks");
            let count = ui.data_mut(|d| {
                let count = match d.get_temp::<(u64, f64, u32)>(clicks_key) {
                    Some((last, at, n)) if last == row && now - at < delay => n + 1,
                    _ => 1,
                };
                d.insert_temp(clicks_key, (row, now, count));
                count
            });
            if count == 2 {
                self.state.push(UiEvent::RowActivated { id: node.id, row });
            }
        }
    }

    /// The nodes of a [Widget::TableRow] in its cells, each in the place its column says. In a row that is selected
    /// with a click, text is not selectable: a click on it selects the row.
    fn cells(
        &mut self,
        row: &mut egui_extras::TableRow<'_, '_>,
        node: &UiNode,
        columns: &[TableColumn],
        selectable: bool,
    ) {
        let cells = self.tree.children(node.id);
        if cells.is_empty() {
            // A row needs a cell to answer clicks
            row.col(|_| {});
        }
        for (&cell, column) in cells.iter().zip(columns) {
            row.col(|ui| {
                if selectable {
                    ui.style_mut().interaction.selectable_labels = false;
                }
                aligned(ui, column.align, |ui| self.node(ui, &self.tree.nodes[cell]))
            });
        }
        self.state
            .bounds
            .insert(node.id, row.response().rect.into());
    }

    /// A [Widget::Scene]: its nodes moved and zoomed by the user, from Kotlin's view when that changes.
    fn scene(&mut self, ui: &mut Ui, node: &UiNode) -> Response {
        let Widget::Scene {
            height,
            width,
            min_zoom,
            max_zoom,
            view,
            report_view,
        } = &node.widget
        else {
            unreachable!("only for scenes")
        };
        let id = node_id(node);
        let (shown_key, view_key) = (id.with("scene"), id.with("scene_view"));
        // Nothing yet: egui fits the nodes in
        let mut shown = ui
            .data(|d| d.get_temp::<egui::Rect>(shown_key))
            .unwrap_or(egui::Rect::ZERO);
        let wanted = view.map(|v| [v.x, v.y, v.width, v.height]);
        let last = ui.data(|d| d.get_temp::<Option<[f32; 4]>>(view_key));
        if last != Some(wanted) {
            ui.data_mut(|d| d.insert_temp(view_key, wanted));
            if let Some(v) = view {
                shown =
                    egui::Rect::from_min_size(egui::pos2(v.x, v.y), egui::vec2(v.width, v.height));
            }
        }
        let before = shown;
        let zoom = egui::Rangef::new(min_zoom.unwrap_or(f32::EPSILON), max_zoom.unwrap_or(1.0));
        let size = egui::vec2(width.unwrap_or_else(|| ui.available_width()), *height);
        self.scenes += 1;
        // Its place on the screen: the response of egui's Scene is in the points of its nodes
        let r = ui.allocate_ui(size, |ui| {
            egui::Scene::new()
                .zoom_range(zoom)
                .show(ui, &mut shown, |ui| self.inside(ui, node));
        });
        self.scenes -= 1;
        ui.data_mut(|d| d.insert_temp(shown_key, shown));
        if *report_view && before != egui::Rect::ZERO && shown != before {
            self.state.push(UiEvent::ViewChanged {
                id: node.id,
                view: shown.into(),
            });
        }
        r.response
    }

    /// A [Widget::Strip]: a cell per size, a node in each.
    fn strip(
        &mut self,
        ui: &mut Ui,
        node: &UiNode,
        sizes: &[StripSize],
        vertical: bool,
        clip: bool,
    ) -> Response {
        let tree = self.tree;
        let children = tree.children(node.id);
        let mut draw = |ui: &mut Ui| {
            let mut builder = egui_extras::StripBuilder::new(ui).clip(clip);
            for &size in sizes {
                builder = builder.size(strip_size(size));
            }
            let cells = |mut strip: egui_extras::Strip<'_, '_>| {
                for i in 0..sizes.len() {
                    match children.get(i) {
                        Some(&child) => strip.cell(|ui| self.node(ui, &tree.nodes[child])),
                        None => strip.empty(),
                    }
                }
            };
            if vertical {
                builder.vertical(cells)
            } else {
                builder.horizontal(cells)
            }
        };
        // A strip divides the room its container leaves it; one that shrinks to its content leaves none, the strip
        // then divides what is seen below it
        if ui.available_height() < 1.0 {
            let room = egui::vec2(ui.available_width(), room_below(ui));
            ui.allocate_ui(room, |ui| draw(ui)).inner
        } else {
            draw(ui)
        }
    }

    /// A [Widget::Shortcut]: [UiEvent::Clicked] when its keys are pressed. Without Ctrl or Alt it waits while a text
    /// field has the keyboard, which needs the keys itself.
    fn shortcut(&mut self, ctx: &egui::Context, node: &UiNode) {
        let Widget::Shortcut {
            key,
            ctrl,
            shift,
            alt,
        } = &node.widget
        else {
            return;
        };
        let Some(key) = crate::input::key_named(key) else {
            return;
        };
        if !node.enabled || (!ctrl && !alt && ctx.text_edit_focused()) {
            return;
        }
        let modifiers = egui::Modifiers {
            alt: *alt,
            shift: *shift,
            command: *ctrl,
            ..Default::default()
        };
        let shortcut = egui::KeyboardShortcut::new(modifiers, key);
        if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
            self.state.push(UiEvent::Clicked { id: node.id });
        }
    }

    /// Where a node goes in a row that has its nodes in the middle or at the bottom, and its height of the last pass.
    ///
    /// egui puts a node across before the node has its height, which it knows beforehand only for a single widget: a
    /// button lands in the middle, but a column, a frame or a button with a size would start at the middle and hang
    /// down, as a field of several lines does. They go where their height of the last pass puts them (the first pass is
    /// redone). `None`: egui places it.
    fn across(&self, ui: &Ui, node: &UiNode) -> Option<(egui::Rect, f32)> {
        use egui::emath::GuiRounding;
        let layout = ui.layout();
        let m = node.modifier.as_ref();
        let cross = m
            .and_then(|m| m.align)
            .map_or(layout.cross_align, Into::into);
        // What fills the height takes the row as it is
        let measured = (node.widget.is_container()
            || node.widget.grows_down()
            || m.is_some_and(|m| m.has_frame() || m.has_size()))
            && !m.is_some_and(|m| m.fill_height);
        if !layout.is_horizontal() || layout.cross_justify || cross == egui::Align::Min || !measured
        {
            return None;
        }
        let Some(last) = self.state.previous.get(&node.id).map(|r| r.height) else {
            ui.ctx()
                .request_discard("a node in a row has no height yet");
            return None;
        };
        let row = ui.available_rect_before_wrap();
        let room = row.height() - last;
        let top = row.min.y
            + match cross {
                egui::Align::Center => room / 2.0,
                _ => room,
            }
            .max(0.0);
        // Down to the bottom of the row at least, so that it has the room it had
        let rect = egui::Rect::from_min_max(
            egui::pos2(row.min.x, top),
            egui::pos2(row.max.x, row.max.y.max(top + last)),
        );
        Some((rect.round_ui(), last))
    }

    /// The nodes of a row or a column in their order, gathered at the start, in the middle or at the end.
    #[allow(clippy::too_many_arguments)]
    fn flow(
        &mut self,
        ui: &mut Ui,
        node: &UiNode,
        horizontal: bool,
        wrap: bool,
        cross: egui::Align,
        arrangement: Option<Arrangement>,
        spacing: Option<f32>,
    ) -> Response {
        let available = ui.available_size_before_wrap();
        let along = if horizontal { available.x } else { available.y };
        // The middle and the end need the room along it, which a scroll area does not have; lines that wrap fill it
        let arrangement = match arrangement {
            Some(Arrangement::Center) if wrap => Arrangement::Start,
            Some(a) if along.is_finite() => a,
            _ => Arrangement::Start,
        };
        let end = arrangement == Arrangement::End;
        let layout = match (horizontal, end) {
            (true, false) => Layout::left_to_right(cross),
            (true, true) => Layout::right_to_left(cross),
            (false, false) => Layout::top_down(cross),
            (false, true) => Layout::bottom_up(cross),
        }
        .with_main_wrap(wrap);
        // As egui's horizontal and vertical: a row as high as a button to start with, a column as wide as its container;
        // at the bottom a column needs all the height. A row that does not wrap is as high as its highest node of the
        // last pass, so that the nodes before it are in the middle as well
        let button = ui.spacing().interact_size.y;
        let tree = self.tree;
        let tallest = |bounds: &HashMap<u64, UiRect>| {
            tree.children(node.id)
                .iter()
                .filter_map(|&c| bounds.get(&tree.nodes[c].id))
                .fold(button, |h, r| h.max(r.height))
        };
        let high = if horizontal && !wrap {
            tallest(&self.state.previous)
        } else {
            button
        };
        let initial = match (horizontal, end) {
            (true, _) => egui::vec2(available.x, high),
            (false, false) => egui::vec2(available.x, 0.0),
            (false, true) => available,
        };
        // In the middle the length of the last pass decides where it starts: egui lays out in one go
        let measured = node_id(node).with("length");
        let before = if arrangement == Arrangement::Center {
            ui.data(|d| d.get_temp::<f32>(measured))
        } else {
            None
        };
        let r = ui.allocate_ui_with_layout(initial, layout, |ui| {
            if let Some(s) = spacing {
                if horizontal {
                    ui.spacing_mut().item_spacing.x = s;
                } else {
                    ui.spacing_mut().item_spacing.y = s;
                }
            }
            if let Some(length) = before {
                ui.add_space(((along - length) / 2.0).max(0.0));
            }
            let start = if horizontal {
                ui.cursor().min.x
            } else {
                ui.cursor().min.y
            };
            let children = tree.children(node.id);
            // From the end in reverse, so that they keep their order
            let order: Box<dyn Iterator<Item = &usize>> = if end {
                Box::new(children.iter().rev())
            } else {
                Box::new(children.iter())
            };
            for &child in order {
                let id = tree.nodes[child].id;
                // A node draws in a scope of its own, which egui cannot wrap before it knows its size: the size of
                // the last frame decides whether it starts a new line
                if wrap {
                    let at_start = ui.cursor().min.x <= ui.max_rect().min.x + 0.5;
                    match self.state.previous.get(&id) {
                        Some(last)
                            if !at_start && last.width > ui.available_size_before_wrap().x =>
                        {
                            ui.end_row()
                        }
                        Some(_) => {}
                        None => ui
                            .ctx()
                            .request_discard("a node in a row that wraps has no size yet"),
                    }
                }
                self.node(ui, &tree.nodes[child]);
            }
            let rect = ui.min_rect();
            if horizontal {
                rect.max.x - start
            } else {
                rect.max.y - start
            }
        });
        if horizontal && !wrap && (tallest(&self.state.bounds) - high).abs() > 0.5 {
            ui.ctx().request_discard("a row changed its height");
        }
        if arrangement == Arrangement::Center {
            let length = r.inner;
            if before.is_none_or(|b| (b - length).abs() > 0.5) {
                ui.data_mut(|d| d.insert_temp(measured, length));
                ui.ctx()
                    .request_discard("a row or column in the middle measured its length");
            }
        }
        r.response
    }

    /// Draws one widget, returns its response (`None`: the scope's).
    fn widget(&mut self, ui: &mut Ui, node: &UiNode) -> Option<Response> {
        let id = node.id;
        let state = &mut *self.state;
        Some(match &node.widget {
            Widget::Row {
                wrap,
                align,
                arrangement,
                spacing,
            } => {
                let cross = align.map_or(egui::Align::Center, Into::into);
                self.flow(ui, node, true, *wrap, cross, *arrangement, *spacing)
            }
            Widget::Column {
                align,
                arrangement,
                spacing,
            } => {
                let cross = align.map_or(egui::Align::Min, Into::into);
                self.flow(ui, node, false, false, cross, *arrangement, *spacing)
            }
            Widget::Columns => {
                let tree = self.tree;
                let children = tree.children(node.id);
                if children.is_empty() {
                    return None;
                }
                ui.scope(|ui| {
                    ui.columns(children.len(), |columns| {
                        for (column, &child) in columns.iter_mut().zip(children) {
                            self.node(column, &tree.nodes[child]);
                        }
                    })
                })
                .response
            }
            Widget::Sides => {
                // Tree::new made sure of two
                let tree = self.tree;
                let children = tree.children(node.id);
                let (left, right) = (&tree.nodes[children[0]], &tree.nodes[children[1]]);
                // egui calls one side after the other
                let this = std::cell::RefCell::new(&mut *self);
                ui.scope(|ui| {
                    egui::Sides::new().show(
                        ui,
                        |ui| this.borrow_mut().node(ui, left),
                        |ui| this.borrow_mut().node(ui, right),
                    )
                })
                .response
            }
            Widget::Indent => {
                ui.indent(node_id(node), |ui| self.children(ui, node))
                    .response
            }
            Widget::Resize { width, height } => {
                ui.scope(|ui| {
                    egui::Resize::default()
                        .id_salt(node_id(node))
                        .default_size([*width, *height])
                        .show(ui, |ui| self.children(ui, node))
                })
                .response
            }
            Widget::Grid {
                columns,
                striped,
                column_spacing,
                row_spacing,
                min_column_width,
            } => {
                let columns = *columns as usize;
                let mut grid = egui::Grid::new(node_id(node)).striped(*striped);
                if column_spacing.is_some() || row_spacing.is_some() {
                    let s = ui.spacing().item_spacing;
                    grid =
                        grid.spacing([column_spacing.unwrap_or(s.x), row_spacing.unwrap_or(s.y)]);
                }
                if let Some(w) = min_column_width {
                    grid = grid.min_col_width(*w);
                }
                grid.show(ui, |ui| {
                    for (i, &child) in self.tree.children(node.id).iter().enumerate() {
                        self.node(ui, &self.tree.nodes[child]);
                        if (i + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                })
                .response
            }
            Widget::Collapsible { title, open } => {
                egui::CollapsingHeader::new(title.as_str())
                    .id_salt(node_id(node))
                    .default_open(*open)
                    .show(ui, |ui| self.children(ui, node))
                    .header_response
            }
            Widget::ScrollArea {
                max_height,
                direction,
                max_width,
                stick_to_end,
            } => {
                let [x, y] = match direction.unwrap_or(ScrollDirection::Vertical) {
                    ScrollDirection::Vertical => [false, true],
                    ScrollDirection::Horizontal => [true, false],
                    ScrollDirection::Both => [true, true],
                };
                egui::ScrollArea::new([x, y])
                    .id_salt(node_id(node))
                    .max_height(max_height.unwrap_or(f32::INFINITY))
                    .max_width(max_width.unwrap_or(f32::INFINITY))
                    .stick_to_bottom(*stick_to_end && y)
                    .stick_to_right(*stick_to_end && x)
                    .show(ui, |ui| self.children(ui, node));
                return None;
            }
            Widget::Group => ui.group(|ui| self.children(ui, node)).response,
            Widget::Table { .. } => {
                self.table(ui, node);
                return None;
            }
            // Drawn by their table
            Widget::TableRow { .. } => return None,
            Widget::Strip {
                sizes,
                vertical,
                clip,
            } => self.strip(ui, node, sizes, *vertical, *clip),
            Widget::MenuBar => {
                egui::MenuBar::new()
                    .ui(ui, |ui| self.children(ui, node))
                    .response
            }
            Widget::Menu { text } => {
                ui.menu_button(text.as_str(), |ui| self.children(ui, node))
                    .response
            }
            Widget::Text {
                text,
                style,
                color,
                spans,
                wrap,
                selectable,
            } => {
                let mut label = if spans.is_empty() {
                    egui::Label::new(styled(text, *style, *color))
                } else {
                    // The parts after one another in one layout, each in its own format
                    let mut job = egui::text::LayoutJob::default();
                    let style_of_ui = ui.style().clone();
                    let mut append = |rich: RichText| {
                        rich.append_to(
                            &mut job,
                            &style_of_ui,
                            egui::FontSelection::Default,
                            egui::Align::Center,
                        )
                    };
                    if !text.is_empty() {
                        append(styled(text, *style, *color));
                    }
                    for span in spans {
                        append(span_text(span, *style, *color));
                    }
                    egui::Label::new(job)
                };
                // A row of a table that is selected with a click turns selecting text off
                label = label.selectable(*selectable && ui.style().interaction.selectable_labels);
                if let Some(wrap) = wrap {
                    label = label.wrap_mode((*wrap).into());
                }
                ui.add(label)
            }
            Widget::Separator => ui.separator(),
            Widget::Spacer { size } => {
                ui.add_space(*size);
                return None;
            }
            Widget::Button {
                text,
                small,
                frame,
                selected,
                shortcut,
                fill,
                image,
                image_size,
            } => {
                let picture = image
                    .as_ref()
                    .and_then(|key| crate::pictures::image_source(state, key))
                    .map(|source| {
                        let height = image_size
                            .unwrap_or_else(|| ui.text_style_height(&egui::TextStyle::Button));
                        egui::Image::new(source)
                            .fit_to_exact_size(egui::vec2(f32::INFINITY, height))
                            .show_loading_spinner(true)
                    });
                let button = match picture {
                    Some(p) if text.is_empty() => egui::Button::image(p),
                    Some(p) => egui::Button::image_and_text(p, text.as_str()),
                    None => egui::Button::new(text.as_str()),
                };
                let mut button = button.frame(*frame).selected(*selected);
                if *small {
                    button = button.small();
                }
                if let Some(shortcut) = shortcut {
                    button = button.shortcut_text(shortcut.as_str());
                }
                if let Some(fill) = fill {
                    button = button.fill(argb(*fill));
                }
                let r = ui.add(button);
                if r.clicked() {
                    state.push(UiEvent::Clicked { id });
                }
                r
            }
            Widget::SelectableLabel { text, selected } => {
                let r = ui.selectable_label(*selected, text.as_str());
                if r.clicked() {
                    state.push(UiEvent::Clicked { id });
                }
                r
            }
            Widget::RadioButton { text, selected } => {
                let r = ui.radio(*selected, text.as_str());
                if r.clicked() {
                    state.push(UiEvent::Clicked { id });
                }
                r
            }
            Widget::ToggleButton { text, checked } | Widget::Switch { text, checked } => {
                let mut value = match state.local(id) {
                    Some(Local::Bool(v)) => *v,
                    _ => *checked,
                };
                let r = if matches!(node.widget, Widget::Switch { .. }) {
                    switch(ui, &mut value, text)
                } else {
                    ui.toggle_value(&mut value, text.as_str())
                };
                if r.changed() {
                    state.change(Local::Bool(value), UiEvent::Checked { id, checked: value });
                }
                r
            }
            Widget::Checkbox {
                text,
                checked,
                indeterminate,
            } => {
                let mut value = match state.local(id) {
                    Some(Local::Bool(v)) => *v,
                    _ => *checked,
                };
                // Until Kotlin answers a click, the box shows what the user chose
                let indeterminate = *indeterminate && state.local(id).is_none();
                let r = ui.add(
                    egui::Checkbox::new(&mut value, text.as_str()).indeterminate(indeterminate),
                );
                if r.changed() {
                    state.change(Local::Bool(value), UiEvent::Checked { id, checked: value });
                }
                r
            }
            Widget::Radio {
                options,
                selected,
                horizontal,
            } => {
                let mut value = match state.local(id) {
                    Some(Local::Index(v)) => *v,
                    _ => *selected,
                };
                let before = value;
                let mut add = |ui: &mut Ui| {
                    for (i, option) in options.iter().enumerate() {
                        ui.radio_value(&mut value, i as u32, option.as_str());
                    }
                };
                let r = if *horizontal {
                    ui.horizontal(|ui| add(ui)).response
                } else {
                    ui.vertical(|ui| add(ui)).response
                };
                if value != before {
                    state.change(Local::Index(value), UiEvent::Selected { id, index: value });
                }
                r
            }
            Widget::ComboBox {
                label,
                options,
                selected,
                width,
            } => {
                let mut value = match state.local(id) {
                    Some(Local::Index(v)) => *v,
                    _ => *selected,
                };
                let before = value;
                let r = ui
                    .horizontal(|ui| {
                        let mut combo = egui::ComboBox::from_id_salt(node_id(node))
                            .selected_text(options.get(value as usize).map_or("", String::as_str));
                        if let Some(width) = width {
                            combo = combo.width(*width);
                        }
                        let r = combo
                            .show_ui(ui, |ui| {
                                for (i, option) in options.iter().enumerate() {
                                    ui.selectable_value(&mut value, i as u32, option.as_str());
                                }
                            })
                            .response;
                        if !label.is_empty() {
                            ui.label(label.as_str());
                        }
                        r
                    })
                    .inner;
                if value != before {
                    state.change(Local::Index(value), UiEvent::Selected { id, index: value });
                }
                r
            }
            Widget::SliderInt {
                label,
                value,
                min,
                max,
                logarithmic,
                suffix,
                prefix,
                step,
                vertical,
                clamping,
                show_value,
                trailing_fill,
                handle,
            } => {
                let mut v = match state.local(id) {
                    Some(Local::Int(v)) => *v,
                    _ => *value,
                };
                let slider = egui::Slider::new(&mut v, *min..=*max)
                    .text(label.as_str())
                    .logarithmic(*logarithmic)
                    .suffix(suffix.as_str());
                let options = SliderOptions {
                    prefix,
                    step: *step,
                    vertical: *vertical,
                    clamping: *clamping,
                    show_value: *show_value,
                    trailing_fill: *trailing_fill,
                    handle: *handle,
                };
                let r = ui.add(options.apply(slider));
                if r.changed() {
                    state.change(Local::Int(v), UiEvent::IntChanged { id, value: v });
                }
                r
            }
            Widget::SliderFloat {
                label,
                value,
                min,
                max,
                logarithmic,
                decimals,
                suffix,
                prefix,
                step,
                vertical,
                clamping,
                show_value,
                trailing_fill,
                handle,
            } => {
                let mut v = match state.local(id) {
                    Some(Local::Float(v)) => *v,
                    _ => *value,
                };
                let mut slider = egui::Slider::new(&mut v, *min..=*max)
                    .text(label.as_str())
                    .logarithmic(*logarithmic)
                    .suffix(suffix.as_str());
                if let Some(d) = decimals {
                    slider = slider.fixed_decimals(*d as usize);
                }
                let options = SliderOptions {
                    prefix,
                    step: *step,
                    vertical: *vertical,
                    clamping: *clamping,
                    show_value: *show_value,
                    trailing_fill: *trailing_fill,
                    handle: *handle,
                };
                let r = ui.add(options.apply(slider));
                if r.changed() {
                    state.change(Local::Float(v), UiEvent::FloatChanged { id, value: v });
                }
                r
            }
            Widget::DragValue {
                label,
                value,
                speed,
                min,
                max,
                decimals,
                suffix,
                prefix,
                hexadecimal,
                angle,
            } => {
                let mut v = match state.local(id) {
                    Some(Local::Float(v)) => *v,
                    _ => *value,
                };
                // An angle is dragged in degrees and kept in radians, as egui's drag_angle
                let unit = if *angle { 1f64.to_degrees() } else { 1.0 };
                let mut shown = v * unit;
                let r = ui
                    .horizontal(|ui| {
                        let suffix = if *angle && suffix.is_empty() {
                            "°"
                        } else {
                            suffix.as_str()
                        };
                        let mut drag = egui::DragValue::new(&mut shown)
                            .speed(*speed)
                            .range(
                                min.map_or(f64::NEG_INFINITY, |m| m * unit)
                                    ..=max.map_or(f64::INFINITY, |m| m * unit),
                            )
                            .prefix(prefix.as_str())
                            .suffix(suffix);
                        if let Some(d) = decimals {
                            drag = drag.fixed_decimals(*d as usize);
                        }
                        if *hexadecimal {
                            drag = drag.hexadecimal(1, false, true);
                        }
                        let r = ui.add(drag);
                        if !label.is_empty() {
                            ui.label(label.as_str());
                        }
                        r
                    })
                    .inner;
                if r.changed() {
                    v = shown / unit;
                    state.change(Local::Float(v), UiEvent::FloatChanged { id, value: v });
                }
                r
            }
            Widget::TextField {
                label,
                value,
                hint,
                password,
                lines,
                code,
                language,
                char_limit,
                align,
                read_only,
            } => {
                let mut text = match state.local(id) {
                    Some(Local::Text(v)) => v.clone(),
                    _ => value.clone(),
                };
                let before = text.clone();
                let edit_id = node_id(node).with("text");
                // Several lines take Enter for a new line: Ctrl+Enter is done
                let submit = lines.is_some()
                    && ui.memory(|m| m.has_focus(edit_id))
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
                // A size of the modifier is the field's
                let fill = node
                    .modifier
                    .as_ref()
                    .is_some_and(|m| m.width.is_some() || m.fill_width);
                let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, wrap: f32| {
                    let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(
                        ui.ctx(),
                        ui.style(),
                    );
                    let mut job = egui_extras::syntax_highlighting::highlight(
                        ui.ctx(),
                        ui.style(),
                        &theme,
                        buffer.as_str(),
                        language.as_deref().unwrap_or_default(),
                    );
                    job.wrap.max_width = wrap;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let r = ui
                    .horizontal(|ui| {
                        if !label.is_empty() {
                            ui.label(label.as_str());
                        }
                        let mut shown: &str = &text.clone();
                        let buffer: &mut dyn egui::TextBuffer =
                            if *read_only { &mut shown } else { &mut text };
                        let mut edit = match lines {
                            Some(rows) => {
                                egui::TextEdit::multiline(buffer).desired_rows(*rows as usize)
                            }
                            None => egui::TextEdit::singleline(buffer),
                        }
                        .id(edit_id)
                        .hint_text(hint.as_str())
                        .password(*password);
                        if *code {
                            edit = edit.code_editor();
                        }
                        if let Some(limit) = char_limit {
                            edit = edit.char_limit(*limit as usize);
                        }
                        if let Some(align) = align {
                            edit = edit.horizontal_align((*align).into());
                        }
                        if fill {
                            edit = edit.desired_width(f32::INFINITY);
                        }
                        if language.is_some() {
                            edit = edit.layouter(&mut layouter);
                        }
                        ui.add(edit)
                    })
                    .inner;
                // egui reports a change for a key that changes nothing (Delete at the end, typing into a field to read)
                if r.changed() && text != before {
                    state.change(
                        Local::Text(text.clone()),
                        UiEvent::TextChanged {
                            id,
                            value: text.clone(),
                        },
                    );
                }
                if submit
                    || (lines.is_none()
                        && r.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    state.push(UiEvent::Submitted { id, value: text });
                }
                r
            }
            Widget::ColorPicker {
                label,
                color,
                alpha,
                inline,
            } => {
                let mut c = match state.local(id) {
                    Some(Local::Color(v)) => argb(*v),
                    _ => argb(*color),
                };
                let alpha = if *alpha {
                    egui::color_picker::Alpha::OnlyBlend
                } else {
                    egui::color_picker::Alpha::Opaque
                };
                let r = ui
                    .horizontal(|ui| {
                        let mut r = if *inline {
                            let mut changed = false;
                            let mut r = ui
                                .vertical(|ui| {
                                    changed =
                                        egui::color_picker::color_picker_color32(ui, &mut c, alpha)
                                })
                                .response;
                            if changed {
                                r.mark_changed();
                            }
                            r
                        } else {
                            egui::color_picker::color_edit_button_srgba(ui, &mut c, alpha)
                        };
                        if !label.is_empty() {
                            r = r.union(ui.label(label.as_str()));
                        }
                        r
                    })
                    .inner;
                if r.changed() {
                    let color = to_argb(c);
                    state.change(Local::Color(color), UiEvent::ColorChanged { id, color });
                }
                r
            }
            Widget::DatePicker {
                label,
                date,
                format,
                calendar_week,
                highlight_weekends,
                first_year,
                last_year,
            } => {
                let shown = match state.local(id) {
                    Some(Local::Date(d)) => *d,
                    _ => *date,
                };
                // Checked with the tree
                let mut day = shown.to_jiff().unwrap_or_default();
                let salt = format!("three-d-egui-date-{id}");
                let r = ui
                    .horizontal(|ui| {
                        let mut picker = egui_extras::DatePickerButton::new(&mut day)
                            .id_salt(&salt)
                            .calendar_week(*calendar_week)
                            .highlight_weekends(*highlight_weekends);
                        if let Some(format) = format {
                            picker = picker.format(format.as_str());
                        }
                        if first_year.is_some() || last_year.is_some() {
                            let now = jiff::Zoned::now().year() as i32;
                            let year = |y: i32| y.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                            picker = picker.start_end_years(
                                year(first_year.unwrap_or(now - 100))
                                    ..=year(last_year.unwrap_or(now + 10)),
                            );
                        }
                        let r = ui.add(picker);
                        if !label.is_empty() {
                            ui.label(label.as_str());
                        }
                        r
                    })
                    .inner;
                if r.changed() {
                    let date = UiDate::from_jiff(day);
                    state.change(Local::Date(date), UiEvent::DateChanged { id, date });
                }
                r
            }
            Widget::Link { text, url, new_tab } => {
                // egui's link is a label: selectable, it would show the text cursor while it is pressed
                ui.style_mut().interaction.selectable_labels = false;
                let r = match url {
                    Some(url) => ui.add(
                        egui::Hyperlink::from_label_and_url(text.as_str(), url)
                            .open_in_new_tab(*new_tab),
                    ),
                    None => ui.link(text.as_str()),
                };
                if r.clicked() {
                    state.push(UiEvent::Clicked { id });
                }
                r
            }
            // Handled before, in Frame::node
            Widget::Shortcut { .. } => return None,
            Widget::ProgressBar { progress, text } => {
                let mut bar = egui::ProgressBar::new(progress.clamp(0.0, 1.0));
                bar = match text {
                    Some(text) => bar.text(text.as_str()),
                    None => bar.show_percentage(),
                };
                ui.add(bar)
            }
            Widget::Spinner => ui.spinner(),
            Widget::Plot { .. } => plot(ui, state, node),
            Widget::Image { .. } => crate::pictures::image(ui, state, node),
            Widget::Canvas { .. } => crate::pictures::canvas(ui, state, node),
            Widget::Scene { .. } => self.scene(ui, node),
            Widget::Notice { text, level } => {
                let color = match level {
                    NoticeLevel::Info => ui.visuals().text_color(),
                    NoticeLevel::Warning => ui.visuals().warn_fg_color,
                    NoticeLevel::Error => ui.visuals().error_fg_color,
                };
                ui.colored_label(color, text.as_str())
            }
            // Drawn by the node they belong to
            Widget::ContextMenu | Widget::Tooltip | Widget::Popup { .. } => return None,
            // Tree::new keeps them at the top
            Widget::SidePanel { .. }
            | Widget::TopBar { .. }
            | Widget::BottomBar { .. }
            | Widget::CentralPanel { .. }
            | Widget::Window { .. }
            | Widget::Area { .. }
            | Widget::Modal { .. } => return None,
        })
    }
}

/// A text in its style and color.
/// A column of egui_extras for `c`.
fn column(c: &TableColumn) -> egui_extras::Column {
    let mut column = match c.width.unwrap_or(ColumnWidth::Auto) {
        ColumnWidth::Auto => egui_extras::Column::auto(),
        ColumnWidth::Initial { width } => egui_extras::Column::initial(width),
        ColumnWidth::Exact { width } => egui_extras::Column::exact(width),
        ColumnWidth::Remainder => egui_extras::Column::remainder(),
    };
    if let Some(min) = c.min_width {
        column = column.at_least(min);
    }
    if let Some(max) = c.max_width {
        column = column.at_most(max);
    }
    if let Some(resizable) = c.resizable {
        column = column.resizable(resizable);
    }
    if c.clip {
        column = column.clip(true);
    }
    column
}

/// A size of egui_extras for `size`.
fn strip_size(size: StripSize) -> egui_extras::Size {
    let (mut s, min, max) = match size {
        StripSize::Exact { points } => (egui_extras::Size::exact(points), None, None),
        StripSize::Initial { points, min, max } => (egui_extras::Size::initial(points), min, max),
        StripSize::Relative { fraction, min, max } => {
            (egui_extras::Size::relative(fraction), min, max)
        }
        StripSize::Remainder { min, max } => (egui_extras::Size::remainder(), min, max),
    };
    if let Some(min) = min {
        s = s.at_least(min);
    }
    if let Some(max) = max {
        s = s.at_most(max);
    }
    s
}

/// `add` at the start, in the middle or at the end of a cell, in the middle of its height.
fn aligned<R>(ui: &mut Ui, align: Option<Align>, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layout = match align.unwrap_or(Align::Start) {
        Align::Start => return add(ui),
        Align::Center => Layout::left_to_right(egui::Align::Center)
            .with_main_align(egui::Align::Center)
            .with_main_justify(true),
        Align::End => Layout::right_to_left(egui::Align::Center),
    };
    ui.with_layout(layout, add).inner
}

/// The room below the cursor for what takes all there is (a table, a strip) in a container that shrinks to its
/// content (a column, a frame), which leaves it none: down to the bottom of what is seen.
fn room_below(ui: &Ui) -> f32 {
    (ui.clip_rect().bottom() - ui.cursor().top()).max(0.0)
}

/// The rows selected after a click on `row`: alone, with Ctrl (Command) added or taken away, with Shift from the row
/// clicked before on.
fn select_rows(
    selection: TableSelection,
    selected: &[u64],
    row: u64,
    modifiers: egui::Modifiers,
    anchor: Option<u64>,
) -> Vec<u64> {
    match selection {
        TableSelection::Multiple if modifiers.shift => {
            let from = anchor.unwrap_or(row);
            let mut rows: Vec<u64> = (from.min(row)..=from.max(row)).collect();
            if modifiers.command {
                rows.extend(
                    selected
                        .iter()
                        .filter(|r| !rows.contains(r))
                        .copied()
                        .collect::<Vec<_>>(),
                );
                rows.sort_unstable();
            }
            rows
        }
        TableSelection::Multiple if modifiers.command => {
            let mut rows: Vec<u64> = selected.iter().copied().filter(|&r| r != row).collect();
            if !selected.contains(&row) {
                rows.push(row);
                rows.sort_unstable();
            }
            rows
        }
        _ => vec![row],
    }
}

fn styled(text: &str, style: TextStyle, color: Option<u32>) -> RichText {
    let rich = RichText::new(text);
    let rich = match style {
        TextStyle::Body => rich,
        TextStyle::Heading => rich.heading(),
        TextStyle::Strong => rich.strong(),
        TextStyle::Small => rich.small(),
        TextStyle::Weak => rich.weak(),
        TextStyle::Monospace => rich.monospace(),
    };
    match color {
        Some(color) => rich.color(argb(color)),
        None => rich,
    }
}

/// A part of a text: what it leaves out from the text.
fn span_text(span: &TextSpan, style: TextStyle, color: Option<u32>) -> RichText {
    let mut rich = styled(
        &span.text,
        span.style.unwrap_or(style),
        span.color.or(color),
    );
    if let Some(background) = span.background {
        rich = rich.background_color(argb(background));
    }
    if let Some(size) = span.size {
        rich = rich.size(size);
    }
    if span.italic {
        rich = rich.italics();
    }
    if span.underline {
        rich = rich.underline();
    }
    if span.strikethrough {
        rich = rich.strikethrough();
    }
    if span.code {
        rich = rich.code();
    }
    if span.raised {
        rich = rich.small_raised();
    }
    rich
}

/// A node with its modifier: its opacity, its place across its container, its frame and its size.
/// `centered`: a fixed size puts the content in the middle (a button's text).
/// `align`: its place across, unless it was placed already.
fn modified<R>(
    ui: &mut Ui,
    m: &UiModifier,
    centered: bool,
    align: Option<egui::Align>,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    if let Some(opacity) = m.opacity {
        ui.multiply_opacity(opacity);
    }
    match align {
        // The container's direction with another place across it
        Some(align) => {
            let layout = ui.layout().with_cross_align(align);
            ui.with_layout(layout, |ui| framed(ui, m, centered, add))
                .inner
        }
        None => framed(ui, m, centered, add),
    }
}

/// Points of a margin, which egui keeps as whole numbers.
fn margin(insets: Option<UiInsets>) -> egui::Margin {
    let p = |v: f32| v.round().clamp(i8::MIN as f32, i8::MAX as f32) as i8;
    insets.map_or(egui::Margin::ZERO, |i| egui::Margin {
        left: p(i.left),
        right: p(i.right),
        top: p(i.top),
        bottom: p(i.bottom),
    })
}

/// The frame of a modifier with a background, a border, a shadow or room around.
fn framed<R>(ui: &mut Ui, m: &UiModifier, centered: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    if !m.has_frame() {
        return sized(ui, m, centered, add);
    }
    let mut frame = egui::Frame::new()
        .outer_margin(margin(m.margin))
        .inner_margin(margin(m.padding))
        .corner_radius(m.corner_radius);
    if let Some(background) = m.background {
        frame = frame.fill(argb(background));
    }
    if let Some(border) = m.border {
        frame = frame.stroke(egui::Stroke::new(border.width, argb(border.color)));
    }
    if let Some(s) = m.shadow {
        let i = |v: f32| v.round().clamp(i8::MIN as f32, i8::MAX as f32) as i8;
        let u = |v: f32| v.round().clamp(0.0, u8::MAX as f32) as u8;
        frame = frame.shadow(egui::Shadow {
            offset: [i(s.dx), i(s.dy)],
            blur: u(s.blur),
            spread: u(s.spread),
            color: argb(s.color),
        });
    }
    frame.show(ui, |ui| sized(ui, m, centered, add)).inner
}

/// The size of a modifier, padding and border included: a button or a field fills it.
fn sized<R>(ui: &mut Ui, m: &UiModifier, centered: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    if !m.has_size() {
        return add(ui);
    }
    let pad = m.padding.unwrap_or_default();
    let border = m.border.map_or(0.0, |b| b.width);
    let (dx, dy) = (
        pad.left + pad.right + 2.0 * border,
        pad.top + pad.bottom + 2.0 * border,
    );
    let inner = |v: f32, d: f32| (v - d).max(0.0);
    let finite = |v: f32| v.is_finite().then_some(v);
    // Filling takes what is left inside the frame already
    let width = m
        .width
        .map(|w| inner(w, dx))
        .or_else(|| m.fill_width.then(|| finite(ui.available_width())).flatten());
    let height = m.height.map(|h| inner(h, dy)).or_else(|| {
        m.fill_height
            .then(|| finite(ui.available_height()))
            .flatten()
    });
    // The room it may take: the container places it there (across as its other nodes), its widgets fill it where the
    // size is fixed, and what is not fixed shrinks to the content
    let room = egui::vec2(
        width.unwrap_or_else(|| {
            let available = ui.available_size_before_wrap().x;
            m.max_width
                .map_or(available, |max| inner(max, dx).min(available))
        }),
        height.unwrap_or_else(|| m.max_height.map_or(0.0, |max| inner(max, dy))),
    );
    let cross = if centered {
        egui::Align::Center
    } else if ui.layout().is_vertical() {
        ui.layout().cross_align
    } else {
        egui::Align::Min
    };
    let layout = Layout::top_down(cross)
        .with_cross_justify(width.is_some())
        .with_main_justify(height.is_some());
    ui.allocate_ui_with_layout(room, layout, |ui| {
        match width {
            Some(w) => ui.set_min_width(w),
            None => {
                if let Some(min) = m.min_width {
                    ui.set_min_width(inner(min, dx));
                }
            }
        }
        match height {
            Some(h) => ui.set_min_height(h),
            None => {
                if let Some(min) = m.min_height {
                    ui.set_min_height(inner(min, dy));
                }
            }
        }
        add(ui)
    })
    .inner
}

/// The plot of a [Widget::Plot] node, with its series in their order.
fn plot(ui: &mut Ui, state: &mut UiState, node: &UiNode) -> Response {
    let Widget::Plot {
        series,
        height,
        width,
        x_label,
        y_label,
        x_unit,
        y_unit,
        categories,
        legend,
        interactive,
        link,
        include_x,
        include_y,
        clickable,
        report_pointer,
    } = &node.widget
    else {
        unreachable!("only for plots")
    };
    // The textures of its images, decoded before the plot draws
    let textures: HashMap<&str, egui::TextureId> = series
        .iter()
        .filter_map(|s| match &s.kind {
            SeriesKind::Image { image, .. } => {
                crate::pictures::texture(ui, state, image, egui::vec2(256.0, 256.0))
                    .map(|t| (image.as_str(), t.id))
            }
            _ => None,
        })
        .collect();
    let places = Places {
        categories: categories.clone(),
        label: x_label.clone(),
        unit: x_unit.clone(),
    };
    let places = &places;
    let mut plot = egui_plot::Plot::new(node_id(node))
        .id(node_id(node))
        .height(*height)
        .allow_zoom(*interactive)
        .allow_drag(*interactive)
        .allow_scroll(*interactive)
        .allow_boxed_zoom(*interactive)
        .allow_double_click_reset(*interactive)
        .allow_axis_zoom_drag(*interactive)
        // Values only near a series: the coordinates of an empty spot say little in a benchmark
        .label_formatter(move |name, point| {
            if name.is_empty() {
                return String::new();
            }
            format!(
                "{name}\n{}: {}{y_unit}",
                places.name(point.x),
                number(point.y)
            )
        });
    if let Some(width) = width {
        plot = plot.width(*width);
    }
    // egui_plot labels an axis only at powers of ten and wants 60 points between labels on the x axis: an axis of 0 to
    // 17 ms would read 0 and 10. Steps of 1, 2 and 5 and closer labels give 0, 5, 10, 15.
    plot = plot
        .grid_spacing(12.0..=300.0)
        .y_grid_spacer(egui_plot::uniform_grid_spacer(nice_steps))
        .custom_y_axes(vec![egui_plot::AxisHints::new_y()
            .label(y_label.as_str())
            .label_spacing(18.0..=22.0)
            .formatter(axis_number)]);
    if categories.is_empty() {
        plot = plot
            .x_grid_spacer(egui_plot::uniform_grid_spacer(nice_steps))
            .custom_x_axes(vec![egui_plot::AxisHints::new_x()
                .label(x_label.as_str())
                .label_spacing(30.0..=50.0)
                .formatter(axis_number)]);
    } else {
        // A name under each bar instead of numbers: a mark at every category, labelled as long as the names fit, and
        // no lines between the bars
        let count = categories.len();
        plot = plot
            .x_grid_spacer(move |input| {
                let (min, max) = input.bounds;
                (0..count)
                    .map(|i| i as f64)
                    .filter(|x| (min..=max).contains(x))
                    .map(|value| egui_plot::GridMark {
                        value,
                        step_size: 1.0,
                    })
                    .collect()
            })
            .show_grid([false, true])
            .custom_x_axes(vec![egui_plot::AxisHints::new_x()
                .label(x_label.as_str())
                .label_spacing(8.0..=16.0)
                .formatter(move |mark, _| {
                    places.category(mark.value).cloned().unwrap_or_default()
                })]);
    }
    if *legend {
        plot = plot.legend(egui_plot::Legend::default().position(egui_plot::Corner::RightTop));
    }
    if let Some(link) = link {
        let group = Id::new(("three-d-egui-plot-link", link.as_str()));
        plot = plot
            .link_axis(group, [true, false])
            .link_cursor(group, [true, false]);
    }
    for x in include_x {
        plot = plot.include_x(*x);
    }
    for y in include_y {
        plot = plot.include_y(*y);
    }
    let shown = plot.show(ui, |p| {
        // Per stack group and bar: how high the positive bars so far reach and how low the negative ones
        let mut stacks: Vec<(&str, Vec<(f64, f64)>)> = Vec::new();
        for s in series {
            let x = |i: usize| s.x.get(i).copied().unwrap_or(i as f64);
            let color = s.color.map(argb);
            match &s.kind {
                SeriesKind::Line { width, fill } => {
                    let points: Vec<[f64; 2]> =
                        s.y.iter().enumerate().map(|(i, y)| [x(i), *y]).collect();
                    let mut line = egui_plot::Line::new(s.name.as_str(), points).width(*width);
                    if *fill {
                        line = line.fill(0.0_f32);
                    }
                    if let Some(style) = s.style {
                        line = line.style(line_style(style));
                    }
                    if let Some(c) = color {
                        line = line.color(c);
                    }
                    p.line(line);
                }
                SeriesKind::Points {
                    radius,
                    shape,
                    filled,
                } => {
                    let points: Vec<[f64; 2]> =
                        s.y.iter().enumerate().map(|(i, y)| [x(i), *y]).collect();
                    let mut dots = egui_plot::Points::new(s.name.as_str(), points)
                        .radius(*radius)
                        .filled(*filled);
                    if let Some(shape) = shape {
                        dots = dots.shape(marker(*shape));
                    }
                    if let Some(c) = color {
                        dots = dots.color(c);
                    }
                    p.points(dots);
                }
                SeriesKind::Bars { width, stack } => {
                    // Bar i of a stack group stands on bar i of the series of the group before it: positive values on
                    // the top of the positive ones, negative values under the negative ones
                    let mut tops = stack.as_ref().map(|group| {
                        let at = stacks.iter().position(|(g, _)| *g == group.as_str());
                        let at = at.unwrap_or_else(|| {
                            stacks.push((group.as_str(), Vec::new()));
                            stacks.len() - 1
                        });
                        &mut stacks[at].1
                    });
                    let bars =
                        s.y.iter()
                            .enumerate()
                            .map(|(i, &y)| {
                                let mut bar = egui_plot::Bar::new(x(i), y).width(*width);
                                if let Some(tops) = tops.as_mut() {
                                    if tops.len() <= i {
                                        tops.resize(i + 1, (0.0, 0.0));
                                    }
                                    let (up, down) = &mut tops[i];
                                    let base = if y >= 0.0 { up } else { down };
                                    bar = bar.base_offset(*base);
                                    *base += y;
                                }
                                bar
                            })
                            .collect();
                    let mut chart = egui_plot::BarChart::new(s.name.as_str(), bars)
                        .element_formatter(bar_text(s, places, y_unit));
                    if let Some(c) = color {
                        chart = chart.color(c);
                    }
                    p.bar_chart(chart);
                }
                SeriesKind::Boxes { width } => {
                    let boxes =
                        s.y.as_chunks::<5>()
                            .0
                            .iter()
                            .enumerate()
                            .map(|(i, v)| {
                                egui_plot::BoxElem::new(
                                    x(i),
                                    egui_plot::BoxSpread::new(v[0], v[1], v[2], v[3], v[4]),
                                )
                                .box_width(*width)
                                .whisker_width(*width / 2.0)
                            })
                            .collect();
                    let mut plot = egui_plot::BoxPlot::new(s.name.as_str(), boxes)
                        .element_formatter(box_text(s, places, y_unit));
                    if let Some(c) = color {
                        plot = plot.color(c);
                    }
                    p.box_plot(plot);
                }
                SeriesKind::HLine => {
                    for y in &s.y {
                        let mut line = egui_plot::HLine::new(s.name.as_str(), *y).style(
                            s.style
                                .map_or(egui_plot::LineStyle::dashed_loose(), line_style),
                        );
                        if let Some(c) = color {
                            line = line.color(c);
                        }
                        p.hline(line);
                    }
                }
                SeriesKind::VLine => {
                    for x in &s.x {
                        let mut line = egui_plot::VLine::new(s.name.as_str(), *x).style(
                            s.style
                                .map_or(egui_plot::LineStyle::dashed_loose(), line_style),
                        );
                        if let Some(c) = color {
                            line = line.color(c);
                        }
                        p.vline(line);
                    }
                }
                SeriesKind::Polygon { width } => {
                    let points: Vec<[f64; 2]> =
                        s.y.iter().enumerate().map(|(i, y)| [x(i), *y]).collect();
                    let mut polygon =
                        egui_plot::Polygon::new(s.name.as_str(), points).width(*width);
                    if let Some(style) = s.style {
                        polygon = polygon.style(line_style(style));
                    }
                    if let Some(c) = color {
                        polygon = polygon
                            .stroke(egui::Stroke::new(*width, c))
                            .fill_color(c.gamma_multiply(0.25));
                    }
                    p.polygon(polygon);
                }
                SeriesKind::Text { anchor } => {
                    for (i, (y, label)) in s.y.iter().zip(&s.labels).enumerate() {
                        let mut text = egui_plot::Text::new(
                            s.name.as_str(),
                            egui_plot::PlotPoint::new(x(i), *y),
                            label.as_str(),
                        );
                        if let Some(anchor) = anchor {
                            text = text.anchor(align2(*anchor));
                        }
                        if let Some(c) = color {
                            text = text.color(c);
                        }
                        p.text(text);
                    }
                }
                SeriesKind::Arrows { tip_length } => {
                    let from: Vec<[f64; 2]> =
                        s.y.iter().enumerate().map(|(i, y)| [x(i), *y]).collect();
                    let to: Vec<[f64; 2]> = s.x2.iter().zip(&s.y2).map(|(x, y)| [*x, *y]).collect();
                    let mut arrows = egui_plot::Arrows::new(s.name.as_str(), from, to);
                    if let Some(tip) = tip_length {
                        arrows = arrows.tip_length(*tip);
                    }
                    if let Some(c) = color {
                        arrows = arrows.color(c);
                    }
                    p.arrows(arrows);
                }
                SeriesKind::Area => {
                    let xs: Vec<f64> = (0..s.y.len()).map(x).collect();
                    let mut area = egui_plot::FilledArea::new(s.name.as_str(), &xs, &s.y, &s.y2);
                    if let Some(c) = color {
                        area = area
                            .fill_color(c.gamma_multiply(0.35))
                            .stroke(egui::Stroke::new(1.0_f32, c));
                    }
                    p.add(area);
                }
                SeriesKind::Span { vertical } => {
                    let values = if *vertical { &s.x } else { &s.y };
                    for pair in values.as_chunks::<2>().0 {
                        let mut span = egui_plot::Span::new(s.name.as_str(), pair[0]..=pair[1])
                            .axis(if *vertical {
                                egui_plot::Axis::X
                            } else {
                                egui_plot::Axis::Y
                            });
                        if let Some(c) = color {
                            span = span.fill(c.gamma_multiply(0.25)).border_color(c);
                        }
                        p.add(span);
                    }
                }
                SeriesKind::Heatmap {
                    columns,
                    palette,
                    min,
                    max,
                    show_values,
                } => {
                    let mut heatmap = egui_plot::Heatmap::new(s.y.clone(), *columns as usize)
                        .name(s.name.as_str())
                        .show_labels(*show_values)
                        .at(egui_plot::PlotPoint::new(
                            s.x.first().copied().unwrap_or(0.0),
                            s.y2.first().copied().unwrap_or(0.0),
                        ))
                        .tile_size(1.0, 1.0);
                    if !palette.is_empty() {
                        let colors: Vec<Color32> = palette.iter().map(|c| argb(*c)).collect();
                        heatmap = heatmap.palette(&colors);
                    }
                    let (low, high) =
                        s.y.iter()
                            .fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
                    if min.is_some() || max.is_some() {
                        heatmap = heatmap.range(min.unwrap_or(low), max.unwrap_or(high));
                    }
                    p.add(heatmap);
                }
                SeriesKind::Image {
                    image,
                    width,
                    height,
                } => {
                    if let Some(texture) = textures.get(image.as_str()) {
                        let mut picture = egui_plot::PlotImage::new(
                            s.name.as_str(),
                            *texture,
                            egui_plot::PlotPoint::new(x(0), s.y[0]),
                            egui::vec2(*width as f32, *height as f32),
                        );
                        if let Some(c) = color {
                            picture = picture.tint(c);
                        }
                        p.image(picture);
                    }
                }
            }
        }
    });
    let to_values = |pos: egui::Pos2| {
        let v = shown.transform.value_from_position(pos);
        crate::UiPoint { x: v.x, y: v.y }
    };
    crate::pictures::pointer(
        ui,
        state,
        node,
        &shown.response,
        to_values,
        *clickable,
        *report_pointer,
    );
    shown.response
}

/// egui_plot's form of a dot.
fn marker(shape: crate::PointShape) -> egui_plot::MarkerShape {
    use crate::PointShape as P;
    use egui_plot::MarkerShape as M;
    match shape {
        P::Circle => M::Circle,
        P::Diamond => M::Diamond,
        P::Square => M::Square,
        P::Cross => M::Cross,
        P::Plus => M::Plus,
        P::Up => M::Up,
        P::Down => M::Down,
        P::Left => M::Left,
        P::Right => M::Right,
        P::Asterisk => M::Asterisk,
    }
}

/// egui_plot's style of a line.
fn line_style(style: crate::PlotLineStyle) -> egui_plot::LineStyle {
    match style {
        crate::PlotLineStyle::Solid => egui_plot::LineStyle::Solid,
        crate::PlotLineStyle::Dashed { length } => egui_plot::LineStyle::Dashed { length },
        crate::PlotLineStyle::Dotted { spacing } => egui_plot::LineStyle::Dotted { spacing },
    }
}

/// A number as the plot shows it: whole numbers without a point, the rest with up to three digits after it.
fn number(v: f64) -> String {
    let digits = if v.abs() >= 100.0 {
        1
    } else if v.abs() >= 1.0 {
        2
    } else {
        3
    };
    egui_plot::format_number(v, digits)
}

/// Grid steps of 1, 2 or 5 times a power of ten, the smallest at least `base_step_size`.
fn nice_steps(input: egui_plot::GridInput) -> [f64; 3] {
    let base = input.base_step_size;
    if !(base.is_finite() && base > 0.0) {
        return [1.0, 10.0, 100.0];
    }
    let nice = |v: f64| {
        let power = 10f64.powf(v.log10().floor());
        [1.0, 2.0, 5.0, 10.0]
            .into_iter()
            .map(|m| m * power)
            .find(|step| *step >= v * (1.0 - 1e-9))
            .unwrap_or(10.0 * power)
    };
    let small = nice(base);
    let middle = nice(small * 1.5);
    [small, middle, nice(middle * 1.5)]
}

/// A mark on an axis with as many digits as its step needs (0.5, not 0 or 1).
fn axis_number(mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>) -> String {
    let digits = (-mark.step_size.log10()).ceil().max(0.0) as usize;
    let value = if mark.value.abs() < mark.step_size * 1e-6 {
        0.0
    } else {
        mark.value
    };
    format!("{value:.digits$}")
}

/// How the pointer names a place on the x axis: by its category, else by the number with the axis' label and unit.
#[derive(Clone)]
struct Places {
    categories: Vec<String>,
    label: String,
    unit: String,
}

impl Places {
    fn category(&self, x: f64) -> Option<&String> {
        let i = x.round();
        ((x - i).abs() < 1e-9 && i >= 0.0)
            .then(|| self.categories.get(i as usize))
            .flatten()
    }

    /// "Cloister", "Frame 120", "12.5 s".
    fn name(&self, x: f64) -> String {
        match self.category(x) {
            Some(name) => name.clone(),
            None if self.label.is_empty() => format!("{}{}", number(x), self.unit),
            None => format!("{} {}{}", self.label, number(x), self.unit),
        }
    }
}

type BarText = Box<dyn Fn(&egui_plot::Bar, &egui_plot::BarChart) -> String>;
type BoxText = Box<dyn Fn(&egui_plot::BoxElem, &egui_plot::BoxPlot) -> String>;

/// The text on a bar: series, place and value (a stacked bar its own part, not the sum).
fn bar_text(s: &PlotSeries, places: &Places, unit: &str) -> BarText {
    let (name, places, unit) = (s.name.clone(), places.clone(), unit.to_owned());
    Box::new(move |bar, _| {
        format!(
            "{name}\n{}: {}{unit}",
            places.name(bar.argument),
            number(bar.value)
        )
    })
}

/// The text on a box: series, place and its five values.
fn box_text(s: &PlotSeries, places: &Places, unit: &str) -> BoxText {
    let (name, places, unit) = (s.name.clone(), places.clone(), unit.to_owned());
    Box::new(move |b, _| {
        let v = &b.spread;
        format!(
            "{name}\n{}\nmax {}{unit}\nQ3 {}{unit}\nmedian {}{unit}\nQ1 {}{unit}\nmin {}{unit}",
            places.name(b.argument),
            number(v.upper_whisker),
            number(v.quartile3),
            number(v.median),
            number(v.quartile1),
            number(v.lower_whisker)
        )
    })
}

/// The options both sliders have.
struct SliderOptions<'a> {
    prefix: &'a str,
    step: Option<f64>,
    vertical: bool,
    clamping: Option<SliderClamping>,
    show_value: bool,
    trailing_fill: Option<bool>,
    handle: Option<SliderHandle>,
}

impl SliderOptions<'_> {
    fn apply<'s>(&self, mut slider: egui::Slider<'s>) -> egui::Slider<'s> {
        slider = slider.prefix(self.prefix).show_value(self.show_value);
        if let Some(step) = self.step {
            slider = slider.step_by(step);
        }
        if self.vertical {
            slider = slider.vertical();
        }
        if let Some(clamping) = self.clamping {
            slider = slider.clamping(match clamping {
                SliderClamping::Never => egui::SliderClamping::Never,
                SliderClamping::Edits => egui::SliderClamping::Edits,
                SliderClamping::Always => egui::SliderClamping::Always,
            });
        }
        if let Some(fill) = self.trailing_fill {
            slider = slider.trailing_fill(fill);
        }
        if let Some(handle) = self.handle {
            slider = slider.handle_shape(match handle {
                SliderHandle::Circle => egui::style::HandleShape::Circle,
                SliderHandle::Rect { aspect_ratio } => {
                    egui::style::HandleShape::Rect { aspect_ratio }
                }
            });
        }
        slider
    }
}

/// A switch as Compose's `Switch` (egui's demo has it as `toggle_switch`): a track with a knob that slides over when it is
/// clicked, in the accent color when on; the text beside it is clicked as well.
fn switch(ui: &mut Ui, on: &mut bool, text: &str) -> Response {
    ui.horizontal(|ui| {
        let size = ui.spacing().interact_size.y * egui::vec2(2.0, 1.0);
        let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
        if !text.is_empty() {
            response = response.union(
                ui.add(egui::Label::new(text).sense(egui::Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand),
            );
        }
        if response.clicked() {
            *on = !*on;
            response.mark_changed();
        }
        let checked = *on;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), checked, text)
        });
        if ui.is_rect_visible(rect) {
            let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
            let visuals = ui.style().interact_selectable(&response, *on);
            let rect = rect.expand(visuals.expansion);
            let radius = 0.5 * rect.height();
            ui.painter().rect(
                rect,
                radius,
                visuals.bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
            let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
            ui.painter().circle(
                egui::pos2(x, rect.center().y),
                0.75 * radius,
                visuals.bg_fill,
                visuals.fg_stroke,
            );
        }
        response
    })
    .inner
}
