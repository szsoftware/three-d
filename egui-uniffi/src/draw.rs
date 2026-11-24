//! The tree drawn with egui, in immediate mode, every frame.

use crate::state::{Local, UiRect, UiState};
use crate::{
    Anchor, Arrangement, NoticeLevel, PlotSeries, ScrollDirection, SeriesKind, Side, TextSpan,
    TextStyle, Tree, UiEvent, UiInsets, UiModifier, UiNode, Widget,
};
use std::collections::HashMap;
use three_d::egui::{self, Color32, Id, Layout, Response, RichText, Ui, UiBuilder};

/// Draws the whole tree onto the root `ui` of a frame and notes what the user did in `state`.
pub(crate) fn draw(ui: &mut Ui, state: &mut UiState) {
    let tree = std::mem::take(&mut state.tree);
    state.previous = std::mem::take(&mut state.bounds);
    state.panels.clear();
    let mut frame = Frame { tree: &tree, state };
    for &root in &tree.roots {
        frame.root(ui, &tree.nodes[root]);
    }
    state.tree = tree;
}

fn argb(c: u32) -> Color32 {
    let [a, r, g, b] = c.to_be_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn align2(anchor: Anchor) -> egui::Align2 {
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
        let rect = match &node.widget {
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
                r.response.rect
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
                r.response.rect
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
                r.response.rect
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
                    Some(r) => r.response.rect,
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
                r.response.rect
            }
            // Tree::new keeps everything else away from the top
            _ => return,
        };
        self.state.bounds.insert(node.id, rect.into());
    }

    /// A node inside a container: in a scope of its own id, so that egui keeps its state when nodes before it come and go.
    fn node(&mut self, ui: &mut Ui, node: &UiNode) {
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
                matches!(node.widget, Widget::Button { .. }),
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
        tooltip(scope.inner.unwrap_or(scope.response), node);
    }

    /// Where a node goes in a row that has its nodes in the middle or at the bottom, and its height of the last pass.
    ///
    /// egui puts a node across before the node has its height, which it knows beforehand only for a single widget: a
    /// button lands in the middle, but a column, a frame or a button with a size would start at the middle and hang
    /// down. They go where their height of the last pass puts them (the first pass is redone). `None`: egui places it.
    fn across(&self, ui: &Ui, node: &UiNode) -> Option<(egui::Rect, f32)> {
        use egui::emath::GuiRounding;
        let layout = ui.layout();
        let m = node.modifier.as_ref();
        let cross = m
            .and_then(|m| m.align)
            .map_or(layout.cross_align, Into::into);
        // What fills the height takes the row as it is
        let measured = (node.widget.is_container()
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
                label = label.selectable(*selectable);
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
            Widget::Button { text } => {
                let r = ui.button(text.as_str());
                if r.clicked() {
                    state.push(UiEvent::Clicked { id });
                }
                r
            }
            Widget::Checkbox { text, checked } => {
                let mut value = match state.local(id) {
                    Some(Local::Bool(v)) => *v,
                    _ => *checked,
                };
                let r = ui.checkbox(&mut value, text.as_str());
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
            } => {
                let mut v = match state.local(id) {
                    Some(Local::Int(v)) => *v,
                    _ => *value,
                };
                let r = ui.add(egui::Slider::new(&mut v, *min..=*max).text(label.as_str()));
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
                let r = ui.add(slider);
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
            } => {
                let mut v = match state.local(id) {
                    Some(Local::Float(v)) => *v,
                    _ => *value,
                };
                let r = ui
                    .horizontal(|ui| {
                        let mut drag = egui::DragValue::new(&mut v)
                            .speed(*speed)
                            .range(min.unwrap_or(f64::NEG_INFINITY)..=max.unwrap_or(f64::INFINITY))
                            .suffix(suffix.as_str());
                        if let Some(d) = decimals {
                            drag = drag.fixed_decimals(*d as usize);
                        }
                        let r = ui.add(drag);
                        if !label.is_empty() {
                            ui.label(label.as_str());
                        }
                        r
                    })
                    .inner;
                if r.changed() {
                    state.change(Local::Float(v), UiEvent::FloatChanged { id, value: v });
                }
                r
            }
            Widget::TextField {
                label,
                value,
                hint,
                password,
            } => {
                let mut text = match state.local(id) {
                    Some(Local::Text(v)) => v.clone(),
                    _ => value.clone(),
                };
                let r = ui
                    .horizontal(|ui| {
                        if !label.is_empty() {
                            ui.label(label.as_str());
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .hint_text(hint.as_str())
                                .password(*password),
                        )
                    })
                    .inner;
                if r.changed() {
                    state.change(
                        Local::Text(text.clone()),
                        UiEvent::TextChanged {
                            id,
                            value: text.clone(),
                        },
                    );
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    state.push(UiEvent::Submitted { id, value: text });
                }
                r
            }
            Widget::ProgressBar { progress, text } => {
                let mut bar = egui::ProgressBar::new(progress.clamp(0.0, 1.0));
                bar = match text {
                    Some(text) => bar.text(text.as_str()),
                    None => bar.show_percentage(),
                };
                ui.add(bar)
            }
            Widget::Spinner => ui.spinner(),
            Widget::Plot { .. } => plot(ui, node),
            Widget::Notice { text, level } => {
                let color = match level {
                    NoticeLevel::Info => ui.visuals().text_color(),
                    NoticeLevel::Warning => ui.visuals().warn_fg_color,
                    NoticeLevel::Error => ui.visuals().error_fg_color,
                };
                ui.colored_label(color, text.as_str())
            }
            // Tree::new keeps them at the top
            Widget::SidePanel { .. }
            | Widget::TopBar { .. }
            | Widget::BottomBar { .. }
            | Widget::CentralPanel { .. }
            | Widget::Window { .. }
            | Widget::Area { .. } => return None,
        })
    }
}

/// A text in its style and color.
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
fn plot(ui: &mut Ui, node: &UiNode) -> Response {
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
    } = &node.widget
    else {
        unreachable!("only for plots")
    };
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
    plot.show(ui, |p| {
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
                    if let Some(c) = color {
                        line = line.color(c);
                    }
                    p.line(line);
                }
                SeriesKind::Points { radius } => {
                    let points: Vec<[f64; 2]> =
                        s.y.iter().enumerate().map(|(i, y)| [x(i), *y]).collect();
                    let mut dots = egui_plot::Points::new(s.name.as_str(), points)
                        .radius(*radius)
                        .filled(true);
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
                        let mut line = egui_plot::HLine::new(s.name.as_str(), *y)
                            .style(egui_plot::LineStyle::dashed_loose());
                        if let Some(c) = color {
                            line = line.color(c);
                        }
                        p.hline(line);
                    }
                }
            }
        }
    })
    .response
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
