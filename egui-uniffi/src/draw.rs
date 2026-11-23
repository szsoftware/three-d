//! The tree drawn with egui, in immediate mode, every frame.

use crate::state::{Local, UiState};
use crate::{Anchor, NoticeLevel, Side, TextStyle, Tree, UiEvent, UiNode, Widget};
use three_d::egui::{self, Color32, Id, Response, RichText, Ui, UiBuilder};

/// Draws the whole tree onto the root `ui` of a frame and notes what the user did in `state`.
pub(crate) fn draw(ui: &mut Ui, state: &mut UiState) {
    let tree = std::mem::take(&mut state.tree);
    state.bounds.clear();
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
            } => {
                let panel = match side {
                    Side::Left => egui::Panel::left(id),
                    Side::Right => egui::Panel::right(id),
                };
                // Not resizable: exactly as wide, else egui shrinks it to its content
                let panel = if *resizable {
                    panel.resizable(true).default_size(*width)
                } else {
                    panel.resizable(false).exact_size(*width)
                };
                let r = panel.show_inside(ui, |ui| {
                    if let Some(title) = title {
                        ui.heading(title);
                    }
                    self.inside(ui, node);
                });
                self.state.panels.push(r.response.rect.into());
                r.response.rect
            }
            Widget::TopBar | Widget::BottomBar => {
                let panel = if matches!(node.widget, Widget::TopBar) {
                    egui::Panel::top(id)
                } else {
                    egui::Panel::bottom(id)
                };
                let r = panel.show_inside(ui, |ui| {
                    ui.horizontal(|ui| self.inside(ui, node));
                });
                self.state.panels.push(r.response.rect.into());
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
            } => {
                if self.state.local(node.id) == Some(&Local::Closed) {
                    return;
                }
                let mut open = true;
                let mut window = egui::Window::new(title.as_str())
                    .id(id)
                    .default_pos([*x, *y])
                    .collapsible(*collapsible)
                    .resizable(*resizable);
                if let Some(width) = width {
                    window = window.default_width(*width);
                }
                if *closable {
                    window = window.open(&mut open);
                }
                let shown = window.show(ui.ctx(), |ui| self.inside(ui, node));
                if !open {
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
                let align = match anchor {
                    Anchor::TopLeft => egui::Align2::LEFT_TOP,
                    Anchor::Top => egui::Align2::CENTER_TOP,
                    Anchor::TopRight => egui::Align2::RIGHT_TOP,
                    Anchor::Left => egui::Align2::LEFT_CENTER,
                    Anchor::Center => egui::Align2::CENTER_CENTER,
                    Anchor::Right => egui::Align2::RIGHT_CENTER,
                    Anchor::BottomLeft => egui::Align2::LEFT_BOTTOM,
                    Anchor::Bottom => egui::Align2::CENTER_BOTTOM,
                    Anchor::BottomRight => egui::Align2::RIGHT_BOTTOM,
                };
                let r = egui::Area::new(id)
                    .anchor(align, [*dx, *dy])
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
        let scope = ui.scope_builder(builder, |ui| self.widget(ui, node));
        let response = scope.inner.unwrap_or(scope.response);
        self.state.bounds.insert(node.id, response.rect.into());
        tooltip(response, node);
    }

    /// Draws one widget, returns its response (`None`: the scope's).
    fn widget(&mut self, ui: &mut Ui, node: &UiNode) -> Option<Response> {
        let id = node.id;
        let state = &mut *self.state;
        Some(match &node.widget {
            Widget::Row => ui.horizontal(|ui| self.children(ui, node)).response,
            Widget::Column => ui.vertical(|ui| self.children(ui, node)).response,
            Widget::Grid { columns, striped } => {
                let columns = *columns as usize;
                egui::Grid::new(node_id(node))
                    .striped(*striped)
                    .show(ui, |ui| {
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
            Widget::ScrollArea { max_height } => {
                egui::ScrollArea::vertical()
                    .id_salt(node_id(node))
                    .max_height(max_height.unwrap_or(f32::INFINITY))
                    .show(ui, |ui| self.children(ui, node));
                return None;
            }
            Widget::Group => ui.group(|ui| self.children(ui, node)).response,
            Widget::Text { text, style, color } => {
                let mut rich = RichText::new(text.as_str());
                rich = match style {
                    TextStyle::Body => rich,
                    TextStyle::Heading => rich.heading(),
                    TextStyle::Strong => rich.strong(),
                    TextStyle::Small => rich.small(),
                    TextStyle::Weak => rich.weak(),
                    TextStyle::Monospace => rich.monospace(),
                };
                if let Some(color) = color {
                    rich = rich.color(argb(*color));
                }
                ui.label(rich)
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
            | Widget::TopBar
            | Widget::BottomBar
            | Widget::Window { .. }
            | Widget::Area { .. } => return None,
        })
    }
}
