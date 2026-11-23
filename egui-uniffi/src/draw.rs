//! The tree drawn with egui, in immediate mode, every frame.

use crate::state::{Local, UiState};
use crate::{
    Anchor, NoticeLevel, PlotSeries, SeriesKind, Side, TextStyle, Tree, UiEvent, UiNode, Widget,
};
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
            | Widget::TopBar
            | Widget::BottomBar
            | Widget::Window { .. }
            | Widget::Area { .. } => return None,
        })
    }
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
                        s.y.chunks_exact(5)
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
