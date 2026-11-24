//! The tree, the values and the events with egui alone, without a graphics card: egui is run with made-up pointer events.

use crate::state::{Focus, UiState};
use crate::*;
use three_d::egui;

fn row() -> Widget {
    Widget::Row {
        wrap: false,
        align: None,
        arrangement: None,
        spacing: None,
    }
}

fn column() -> Widget {
    Widget::Column {
        align: None,
        arrangement: None,
        spacing: None,
    }
}

fn node(id: u64, parent: Option<u64>, widget: Widget) -> UiNode {
    UiNode {
        id,
        parent,
        widget,
        enabled: true,
        tooltip: None,
        modifier: None,
    }
}

fn panel() -> UiNode {
    node(
        1,
        None,
        Widget::SidePanel {
            side: Side::Left,
            title: Some("Options".into()),
            width: 200.0,
            resizable: false,
            min_width: None,
            max_width: None,
            expanded: true,
        },
    )
}

fn checkbox(id: u64, checked: bool) -> UiNode {
    node(
        id,
        Some(1),
        Widget::Checkbox {
            text: "Shadows".into(),
            checked,
            indeterminate: false,
        },
    )
}

/// egui with the state, frame by frame, 800 x 600 points.
struct Run {
    egui: egui::Context,
    state: UiState,
    time: f64,
    /// Held in every frame.
    modifiers: egui::Modifiers,
}

impl Run {
    fn new(nodes: Vec<UiNode>) -> Self {
        let mut state = UiState::new();
        state.set_tree(nodes).unwrap();
        let mut run = Self {
            egui: egui::Context::default(),
            state,
            time: 0.0,
            modifiers: egui::Modifiers::NONE,
        };
        run.frame(vec![]);
        run.frame(vec![]);
        run
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.time += 1.0 / 60.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            time: Some(self.time),
            events,
            modifiers: self.modifiers,
            ..Default::default()
        };
        let state = &mut self.state;
        self.egui.run_ui(input, |ui| crate::draw::draw(ui, state))
    }

    fn center(&self, id: u64) -> egui::Pos2 {
        let r = self.state.bounds[&id];
        egui::pos2(r.x + r.width / 2.0, r.y + r.height / 2.0)
    }

    fn click_at(&mut self, pos: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        self.frame(vec![egui::Event::PointerMoved(pos)]);
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
        self.frame(vec![]);
    }

    fn click(&mut self, id: u64) {
        self.click_at(self.center(id));
    }

    fn drag(&mut self, from: egui::Pos2, to: &[egui::Pos2]) {
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        self.frame(vec![egui::Event::PointerMoved(from)]);
        self.frame(vec![button(from, true)]);
        for &p in to {
            self.frame(vec![egui::Event::PointerMoved(p)]);
        }
        self.frame(vec![button(*to.last().unwrap(), false)]);
    }
}

fn invalid(nodes: Vec<UiNode>) -> String {
    match Tree::new(nodes) {
        Err(UiError::InvalidTree { why }) => why,
        other => panic!("expected an invalid tree, got {other:?}"),
    }
}

#[test]
fn trees_that_cannot_be_drawn_are_refused_with_the_reason() {
    assert!(invalid(vec![panel(), panel()]).contains("the id 1 is there twice"));
    assert!(invalid(vec![
        panel(),
        checkbox(2, true),
        checkbox(3, true),
        checkbox(2, false)
    ])
    .contains("the id 2 is there twice"));
    assert!(invalid(vec![checkbox(2, true)]).contains("inside 1, which is not in the tree"));
    assert!(invalid(vec![node(2, None, Widget::Separator)]).contains("Separator 2 has no parent"));
    let mut inner = panel();
    inner.id = 3;
    inner.parent = Some(1);
    assert!(invalid(vec![panel(), inner]).contains("SidePanel 3 stands at the top"));
    assert!(invalid(vec![
        panel(),
        checkbox(2, true),
        node(3, Some(2), Widget::Separator)
    ])
    .contains("inside Checkbox 2, which holds no nodes"));
    assert!(invalid(vec![
        panel(),
        node(2, Some(3), row()),
        node(3, Some(2), row())
    ])
    .contains("go round in a circle"));
    assert!(invalid(vec![
        panel(),
        node(
            2,
            Some(1),
            Widget::ComboBox {
                label: String::new(),
                options: vec!["a".into(), "b".into()],
                selected: 2,
                width: None
            }
        )
    ])
    .contains("ComboBox 2: selected 2 is not one of the 2 options"));
    assert!(invalid(vec![
        panel(),
        node(
            2,
            Some(1),
            Widget::SliderFloat {
                label: String::new(),
                value: 1.0,
                min: 2.0,
                max: 1.0,
                logarithmic: false,
                decimals: None,
                suffix: String::new(),
                prefix: String::new(),
                step: None,
                vertical: false,
                clamping: None,
                show_value: true,
                trailing_fill: None,
                handle: None,
            }
        )
    ])
    .contains("min 2 > max 1"));
    assert!(invalid(vec![
        panel(),
        node(
            2,
            Some(1),
            Widget::Grid {
                columns: 0,
                striped: false,
                column_spacing: None,
                row_spacing: None,
                min_column_width: None,
            }
        )
    ])
    .contains("Grid 2: a grid needs at least one column"));
    assert!(Tree::new(vec![
        panel(),
        node(
            2,
            Some(1),
            Widget::Grid {
                columns: 2,
                striped: true,
                column_spacing: None,
                row_spacing: None,
                min_column_width: None,
            }
        ),
        node(3, Some(2), Widget::Separator),
    ])
    .is_ok());
}

#[test]
fn a_click_ticks_the_box_at_once_and_reports_it() {
    let mut run = Run::new(vec![panel(), checkbox(2, false)]);
    run.click(2);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Checked {
            id: 2,
            checked: true
        }]
    );
    // Kotlin took it and answers with its state: ticked
    run.state
        .set_tree(vec![panel(), checkbox(2, true)])
        .unwrap();
    run.frame(vec![]);
    run.click(2);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Checked {
            id: 2,
            checked: false
        }]
    );
}

#[test]
fn kotlins_value_wins_after_it_took_the_event() {
    let mut run = Run::new(vec![panel(), checkbox(2, false)]);
    run.click(2);
    // A tree from before Kotlin saw the click (another change): the tick stays
    run.state
        .set_tree(vec![panel(), checkbox(2, false)])
        .unwrap();
    run.frame(vec![]);
    run.click(2);
    // Clicked twice: unticked again, and only the last value is reported
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Checked {
            id: 2,
            checked: false
        }]
    );
    run.click(2);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Checked {
            id: 2,
            checked: true
        }]
    );
    // Kotlin refuses the change: its tree still says unticked, and so the box is unticked again
    run.state
        .set_tree(vec![panel(), checkbox(2, false)])
        .unwrap();
    assert!(run.state.dirty);
    run.frame(vec![]);
    run.click(2);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Checked {
            id: 2,
            checked: true
        }]
    );
}

#[test]
fn buttons_options_and_sliders_report_their_ids() {
    let options = vec!["Auto".to_string(), "GPU".into(), "CPU".into()];
    let mut run = Run::new(vec![
        panel(),
        node(
            2,
            Some(1),
            Widget::Button {
                text: "Photo".into(),
                small: false,
                frame: true,
                selected: false,
                shortcut: None,
                fill: None,
                image: None,
                image_size: None,
            },
        ),
        node(
            3,
            Some(1),
            Widget::Radio {
                options: options.clone(),
                selected: 0,
                horizontal: false,
            },
        ),
        node(
            4,
            Some(1),
            Widget::SliderFloat {
                label: "Exposure".into(),
                value: 0.0,
                min: 0.0,
                max: 10.0,
                logarithmic: false,
                decimals: None,
                suffix: String::new(),
                prefix: String::new(),
                step: None,
                vertical: false,
                clamping: None,
                show_value: true,
                trailing_fill: None,
                handle: None,
            },
        ),
        node(5, Some(1), row()),
        node(
            6,
            Some(5),
            Widget::Button {
                text: "Cancel".into(),
                small: false,
                frame: true,
                selected: false,
                shortcut: None,
                fill: None,
                image: None,
                image_size: None,
            },
        ),
        node(
            7,
            Some(5),
            Widget::Button {
                text: "Cancel".into(),
                small: false,
                frame: true,
                selected: false,
                shortcut: None,
                fill: None,
                image: None,
                image_size: None,
            },
        ),
    ]);
    run.click(2);
    // The radio buttons stand one below the other: the third is in the lower third of the node
    let r = run.state.bounds[&3];
    run.click_at(egui::pos2(r.x + 6.0, r.y + r.height * 5.0 / 6.0));
    // Two buttons with the same text are still told apart
    run.click(7);
    // Dragging the slider's handle to the right: one event with the last value
    let s = run.state.bounds[&4];
    let from = egui::pos2(s.x + 4.0, s.y + s.height / 2.0);
    let to: Vec<_> = (1..=5)
        .map(|i| egui::pos2(from.x + 10.0 * i as f32, from.y))
        .collect();
    run.drag(from, &to);
    let events = run.state.take_events();
    assert_eq!(
        events[..3],
        [
            UiEvent::Clicked { id: 2 },
            UiEvent::Selected { id: 3, index: 2 },
            UiEvent::Clicked { id: 7 },
        ]
    );
    assert_eq!(events.len(), 4, "{events:?}");
    match events[3] {
        UiEvent::FloatChanged { id: 4, value } => assert!(value > 0.5, "{value}"),
        ref e => panic!("{e:?}"),
    }
}

#[test]
fn disabled_widgets_do_not_react() {
    let mut button = node(
        2,
        Some(1),
        Widget::Button {
            text: "GPU".into(),
            small: false,
            frame: true,
            selected: false,
            shortcut: None,
            fill: None,
            image: None,
            image_size: None,
        },
    );
    button.enabled = false;
    button.tooltip = Some("no compute shaders".into());
    let mut run = Run::new(vec![panel(), button]);
    run.click(2);
    assert_eq!(run.state.take_events(), vec![]);
    // A disabled panel greys out everything inside
    let mut panel = panel();
    panel.enabled = false;
    let mut run = Run::new(vec![panel, checkbox(2, false)]);
    run.click(2);
    assert_eq!(run.state.take_events(), vec![]);
}

#[test]
fn a_closed_window_reports_it_and_stays_hidden_until_kotlin_answers() {
    let window = || {
        node(
            1,
            None,
            Widget::Window {
                title: "Benchmark".into(),
                x: 300.0,
                y: 100.0,
                width: Some(200.0),
                closable: true,
                collapsible: false,
                resizable: false,
                height: None,
                anchor: None,
                movable: None,
                title_bar: true,
                fixed_size: false,
                scroll: false,
                open: true,
            },
        )
    };
    let mut run = Run::new(vec![window(), checkbox(2, false)]);
    let w = run.state.bounds[&1];
    // The close button is in the top right corner of the title bar
    run.click_at(egui::pos2(w.x + w.width - 12.0, w.y + 12.0));
    assert!(!run.state.bounds.contains_key(&1));
    assert_eq!(run.state.take_events(), vec![UiEvent::Closed { id: 1 }]);
    run.state
        .set_tree(vec![window(), checkbox(2, false)])
        .unwrap();
    run.frame(vec![]);
    assert!(run.state.bounds.contains_key(&1));
}

#[test]
fn text_fields_report_typing_and_enter() {
    let mut run = Run::new(vec![
        panel(),
        node(
            2,
            Some(1),
            Widget::TextField {
                label: "Name".into(),
                value: "Garden".into(),
                hint: String::new(),
                password: false,
                lines: None,
                code: false,
                language: None,
                char_limit: None,
                align: None,
                read_only: false,
            },
        ),
    ]);
    run.click(2);
    run.frame(vec![egui::Event::Text(" 2".into())]);
    let key = |key, pressed| egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Default::default(),
    };
    run.frame(vec![key(egui::Key::Enter, true)]);
    run.frame(vec![key(egui::Key::Enter, false)]);
    let events = run.state.take_events();
    assert!(
        events.contains(&UiEvent::TextChanged {
            id: 2,
            value: "Garden 2".into()
        }),
        "{events:?}"
    );
    assert_eq!(
        events.last(),
        Some(&UiEvent::Submitted {
            id: 2,
            value: "Garden 2".into()
        })
    );
}

/// A node of every kind, in panels, bars and a window.
fn every_widget() -> Vec<UiNode> {
    let s = String::new;
    let mut nodes = vec![
        panel(),
        node(10, None, Widget::TopBar { expanded: true }),
        node(
            11,
            Some(10),
            Widget::Button {
                text: "File".into(),
                small: false,
                frame: true,
                selected: false,
                shortcut: None,
                fill: None,
                image: None,
                image_size: None,
            },
        ),
        node(20, None, Widget::BottomBar { expanded: true }),
        node(21, Some(20), Widget::Spinner),
        node(
            30,
            None,
            Widget::Area {
                anchor: Anchor::TopRight,
                dx: -8.0,
                dy: 8.0,
                frame: true,
            },
        ),
        node(
            31,
            Some(30),
            Widget::Text {
                text: "16.7 ms".into(),
                style: TextStyle::Monospace,
                color: Some(0xFFFF8800),
                spans: vec![],
                wrap: None,
                selectable: true,
            },
        ),
    ];
    let widgets = vec![
        Widget::Text {
            text: "Heading".into(),
            style: TextStyle::Heading,
            color: None,
            spans: vec![],
            wrap: None,
            selectable: true,
        },
        Widget::Separator,
        Widget::Spacer { size: 4.0 },
        Widget::SliderInt {
            label: "Samples".into(),
            value: 4,
            min: 1,
            max: 64,
            logarithmic: false,
            suffix: String::new(),
            prefix: String::new(),
            step: None,
            vertical: false,
            clamping: None,
            show_value: true,
            trailing_fill: None,
            handle: None,
        },
        Widget::SliderFloat {
            label: "Haze".into(),
            value: 0.01,
            min: 0.001,
            max: 1.0,
            logarithmic: true,
            decimals: Some(3),
            suffix: " /m".into(),
            prefix: String::new(),
            step: None,
            vertical: false,
            clamping: None,
            show_value: true,
            trailing_fill: None,
            handle: None,
        },
        Widget::DragValue {
            label: "Angle".into(),
            value: 45.0,
            speed: 0.5,
            min: None,
            max: Some(90.0),
            decimals: Some(1),
            suffix: " °".into(),
            prefix: String::new(),
            hexadecimal: false,
            angle: false,
        },
        Widget::ComboBox {
            label: "Scene".into(),
            options: vec!["cloister".into(), "garden".into()],
            selected: 1,
            width: Some(120.0),
        },
        Widget::ProgressBar {
            progress: 0.4,
            text: None,
        },
        Widget::ProgressBar {
            progress: 2.0,
            text: Some("12 s".into()),
        },
        Widget::Notice {
            text: "Takes effect after a restart".into(),
            level: NoticeLevel::Warning,
        },
        Widget::Notice {
            text: "Failed".into(),
            level: NoticeLevel::Error,
        },
        Widget::TextField {
            label: s(),
            value: s(),
            hint: "Search".into(),
            password: true,
            lines: None,
            code: false,
            language: None,
            char_limit: None,
            align: None,
            read_only: false,
        },
        Widget::Group,
        Widget::Collapsible {
            title: "More".into(),
            open: true,
        },
        Widget::ScrollArea {
            max_height: Some(50.0),
            direction: None,
            max_width: None,
            stick_to_end: false,
        },
        Widget::Grid {
            columns: 2,
            striped: true,
            column_spacing: None,
            row_spacing: None,
            min_column_width: None,
        },
        column(),
        plot(vec![
            series("Frame", line(), vec![], vec![12.0, 14.0, 13.0]),
            series(
                "Points",
                SeriesKind::Points {
                    radius: 2.0,
                    shape: None,
                    filled: true,
                },
                vec![0.5, 1.5],
                vec![1.0, 2.0],
            ),
            series("Bars", bars(None), vec![], vec![3.0, 4.0, 5.0]),
            series("Boxes", boxes(), vec![], vec![1.0, 2.0, 3.0, 4.0, 5.0]),
            series("60 fps", SeriesKind::HLine, vec![], vec![16.7]),
        ]),
    ];
    let containers = [100 + 12, 100 + 13, 100 + 14, 100 + 15, 100 + 16];
    for (i, widget) in widgets.into_iter().enumerate() {
        nodes.push(node(100 + i as u64, Some(1), widget));
    }
    for (i, &c) in containers.iter().enumerate() {
        for j in 0..3 {
            nodes.push(node(
                1000 + 10 * i as u64 + j,
                Some(c),
                Widget::Text {
                    text: format!("{c}.{j}"),
                    style: TextStyle::Body,
                    color: None,
                    spans: vec![],
                    wrap: None,
                    selectable: true,
                },
            ));
        }
    }
    nodes
}

#[test]
fn every_widget_draws() {
    let nodes = every_widget();
    let ids: Vec<u64> = nodes.iter().map(|n| n.id).collect();
    let run = Run::new(nodes);
    for id in ids {
        // The scroll area hides what is below its height
        if (1020..1030).contains(&id) {
            continue;
        }
        let r = run
            .state
            .bounds
            .get(&id)
            .unwrap_or_else(|| panic!("node {id} was not drawn"));
        assert!(
            r.width > 0.0 || matches!(id, 102),
            "node {id} has no size: {r:?}"
        );
    }
    // The panels are noted for telling whether the pointer is on egui
    assert_eq!(run.state.panels.len(), 3);
}

#[test]
fn text_in_an_area_at_the_edge_does_not_wrap() {
    let tree = |text: &str| {
        vec![
            node(
                1,
                None,
                Widget::Area {
                    anchor: Anchor::TopRight,
                    dx: -8.0,
                    dy: 8.0,
                    frame: true,
                },
            ),
            node(
                2,
                Some(1),
                Widget::Text {
                    text: text.into(),
                    style: TextStyle::Monospace,
                    color: None,
                    spans: vec![],
                    wrap: None,
                    selectable: true,
                },
            ),
        ]
    };
    // egui keeps the size of an area from frame to frame: a longer text after a short one must not wrap
    let mut run = Run::new(tree("0.0 ms"));
    run.state.set_tree(tree("312.9 ms per frame")).unwrap();
    run.frame(vec![]);
    run.frame(vec![]);
    let line = run.state.bounds[&2];
    assert!(line.height < 20.0 && line.width > 100.0, "{line:?}");
}

fn series(name: &str, kind: SeriesKind, x: Vec<f64>, y: Vec<f64>) -> PlotSeries {
    PlotSeries {
        name: name.into(),
        kind,
        x,
        y,
        color: None,
        x2: vec![],
        y2: vec![],
        labels: vec![],
        style: None,
    }
}

fn line() -> SeriesKind {
    SeriesKind::Line {
        width: 1.5,
        fill: false,
    }
}

fn bars(stack: Option<&str>) -> SeriesKind {
    SeriesKind::Bars {
        width: 0.6,
        stack: stack.map(Into::into),
    }
}

fn boxes() -> SeriesKind {
    SeriesKind::Boxes { width: 0.5 }
}

fn plot(series: Vec<PlotSeries>) -> Widget {
    Widget::Plot {
        series,
        height: 150.0,
        width: None,
        x_label: String::new(),
        y_label: String::new(),
        x_unit: String::new(),
        y_unit: String::new(),
        categories: vec![],
        legend: true,
        interactive: true,
        link: None,
        include_x: vec![],
        include_y: vec![0.0],
        clickable: false,
        report_pointer: false,
    }
}

/// The texts egui painted in a frame (labels, axis marks, tooltips).
fn texts(output: &egui::FullOutput) -> Vec<String> {
    fn collect(shape: &egui::Shape, into: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => into.push(t.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, into)),
            _ => {}
        }
    }
    let mut into = Vec::new();
    for clipped in &output.shapes {
        collect(&clipped.shape, &mut into);
    }
    into
}

/// Where a value of the plot `id` is on the screen, as egui_plot drew it in the last frame.
fn on_screen(run: &Run, id: u64, x: f64, y: f64) -> egui::Pos2 {
    let memory = egui_plot::PlotMemory::load(&run.egui, egui::Id::new(("three-d-egui", id)))
        .expect("the plot was drawn");
    memory
        .transform()
        .position_from_point(&egui_plot::PlotPoint::new(x, y))
}

/// What the plot shows when the pointer rests at `pos`.
fn hover(run: &mut Run, pos: egui::Pos2) -> Vec<String> {
    run.frame(vec![egui::Event::PointerMoved(pos)]);
    let output = run.frame(vec![]);
    texts(&output)
}

#[test]
fn plots_refuse_series_whose_values_do_not_fit() {
    let tree = |widget| vec![panel(), node(2, Some(1), widget)];
    let one = |s| plot(vec![s]);
    assert!(
        invalid(tree(one(series("Spread", boxes(), vec![], vec![1.0; 7]))))
            .contains("Plot 2: the boxes 'Spread' need five values each, they have 7")
    );
    assert!(invalid(tree(one(series(
        "Spread",
        boxes(),
        vec![0.0, 1.0, 2.0],
        vec![1.0; 10]
    ))))
    .contains("the boxes 'Spread' have 3 places for 2 boxes"));
    assert!(invalid(tree(one(series(
        "Frame",
        line(),
        vec![0.0, 1.0],
        vec![1.0; 3]
    ))))
    .contains("'Frame' has 2 x values for 3 y values"));
    assert!(invalid(tree(one(series(
        "60 fps",
        SeriesKind::HLine,
        vec![1.0],
        vec![16.7]
    ))))
    .contains("the lines across '60 fps' take no x values"));
    let mut flat = plot(vec![]);
    if let Widget::Plot { height, .. } = &mut flat {
        *height = 0.0;
    }
    assert!(invalid(tree(flat)).contains("a plot needs a height above 0, it has 0"));
    assert!(Tree::new(tree(one(series(
        "Spread",
        boxes(),
        vec![0.0, 2.0],
        vec![1.0; 10]
    ))))
    .is_ok());
}

#[test]
fn a_bar_chart_names_its_categories_and_stacks_bars_of_a_group() {
    let mut chart = plot(vec![
        series("Trace", bars(Some("frame")), vec![], vec![10.0, 20.0]),
        series("Denoise", bars(Some("frame")), vec![], vec![5.0, 5.0]),
        series("Alone", bars(None), vec![2.0], vec![4.0]),
    ]);
    if let Widget::Plot {
        categories, y_unit, ..
    } = &mut chart
    {
        *categories = vec!["Cloister".into(), "Garden".into(), "Hall".into()];
        *y_unit = " ms".into();
    }
    let mut run = Run::new(vec![panel(), node(2, Some(1), chart)]);
    let shown = texts(&run.frame(vec![]));
    for name in ["Cloister", "Garden", "Hall"] {
        assert!(
            shown.iter().any(|t| t == name),
            "no {name} under the bars: {shown:?}"
        );
    }
    // The denoise part stands on the trace part: low in the column the pointer finds the trace, above it the denoise
    let low = on_screen(&run, 2, 0.0, 2.0);
    let high = on_screen(&run, 2, 0.0, 12.5);
    assert!(hover(&mut run, low)
        .iter()
        .any(|t| t == "Trace\nCloister: 10 ms"));
    assert!(hover(&mut run, high)
        .iter()
        .any(|t| t == "Denoise\nCloister: 5 ms"));
    let second = on_screen(&run, 2, 1.0, 22.0);
    assert!(hover(&mut run, second)
        .iter()
        .any(|t| t == "Denoise\nGarden: 5 ms"));
    let alone = on_screen(&run, 2, 2.0, 1.0);
    assert!(hover(&mut run, alone)
        .iter()
        .any(|t| t == "Alone\nHall: 4 ms"));
}

#[test]
fn the_pointer_on_a_line_or_a_box_shows_its_values() {
    let mut chart = plot(vec![
        series("Frame time", line(), vec![], vec![12.0, 12.5, 13.0, 12.0]),
        series(
            "Spread",
            boxes(),
            vec![6.0],
            vec![10.0, 11.0, 12.0, 13.0, 15.0],
        ),
    ]);
    if let Widget::Plot {
        x_label, y_unit, ..
    } = &mut chart
    {
        *x_label = "Frame".into();
        *y_unit = " ms".into();
    }
    let mut run = Run::new(vec![panel(), node(2, Some(1), chart)]);
    let point = on_screen(&run, 2, 1.0, 12.5);
    let shown = hover(&mut run, point);
    assert!(
        shown.iter().any(|t| t == "Frame time\nFrame 1: 12.50 ms"),
        "{shown:?}"
    );
    let median = on_screen(&run, 2, 6.0, 12.0);
    let shown = hover(&mut run, median);
    assert!(
        shown
            .iter()
            .any(|t| t.starts_with("Spread\nFrame 6\nmax 15 ms\nQ3 13 ms\nmedian 12 ms")),
        "{shown:?}"
    );
    // Away from the series the plot shows no values
    let empty = on_screen(&run, 2, 4.5, 2.0);
    let shown = hover(&mut run, empty);
    assert!(!shown.iter().any(|t| t.contains(" ms")), "{shown:?}");
}

#[test]
fn linked_plots_move_together_and_fixed_ones_stay() {
    let linked = |interactive| {
        let mut p = plot(vec![series(
            "Frame",
            line(),
            vec![],
            (0..100).map(f64::from).collect(),
        )]);
        if let Widget::Plot {
            link,
            interactive: i,
            ..
        } = &mut p
        {
            *link = Some("time".into());
            *i = interactive;
        }
        p
    };
    let mut fixed = linked(false);
    if let Widget::Plot { link, .. } = &mut fixed {
        *link = None;
    }
    let mut run = Run::new(vec![
        panel(),
        node(2, Some(1), linked(true)),
        node(3, Some(1), linked(true)),
        node(4, Some(1), fixed),
    ]);
    let x_range = |run: &Run, id: u64| {
        let memory =
            egui_plot::PlotMemory::load(&run.egui, egui::Id::new(("three-d-egui", id))).unwrap();
        let b = memory.bounds();
        (b.min()[0], b.max()[0])
    };
    let before = [x_range(&run, 2), x_range(&run, 3), x_range(&run, 4)];
    let drag = |run: &mut Run, id: u64| {
        let from = run.center(id);
        let to: Vec<egui::Pos2> = (1..=6)
            .map(|i| from + egui::vec2(10.0 * i as f32, 0.0))
            .collect();
        run.drag(from, &to);
        run.frame(vec![]);
    };
    drag(&mut run, 2);
    let after = [x_range(&run, 2), x_range(&run, 3), x_range(&run, 4)];
    assert!(
        after[0].0 < before[0].0 - 1.0,
        "dragging did not move the plot: {before:?} -> {after:?}"
    );
    assert!(
        (after[1].0 - after[0].0).abs() < 1e-6 && (after[1].1 - after[0].1).abs() < 1e-6,
        "{after:?}"
    );
    drag(&mut run, 4);
    assert_eq!(
        x_range(&run, 4),
        before[2],
        "the plot without interaction moved"
    );
}

// Phase 1: modifiers, layouts, panels, windows, scrolling, text in parts

fn text(id: u64, parent: u64, s: &str) -> UiNode {
    node(
        id,
        Some(parent),
        Widget::Text {
            text: s.into(),
            style: TextStyle::Body,
            color: None,
            spans: vec![],
            wrap: None,
            selectable: true,
        },
    )
}

fn button(id: u64, parent: u64, s: &str) -> UiNode {
    node(
        id,
        Some(parent),
        Widget::Button {
            text: s.into(),
            small: false,
            frame: true,
            selected: false,
            shortcut: None,
            fill: None,
            image: None,
            image_size: None,
        },
    )
}

fn with(mut node: UiNode, modifier: UiModifier) -> UiNode {
    node.modifier = Some(modifier);
    node
}

fn insets(v: f32) -> Option<UiInsets> {
    Some(UiInsets {
        left: v,
        top: v,
        right: v,
        bottom: v,
    })
}

fn central(id: u64) -> UiNode {
    node(id, None, Widget::CentralPanel { frame: true })
}

fn right(r: UiRect) -> f32 {
    r.x + r.width
}

fn bottom(r: UiRect) -> f32 {
    r.y + r.height
}

/// The galley of the text that says `text`.
fn galley(output: &egui::FullOutput, text: &str) -> std::sync::Arc<egui::Galley> {
    fn find(shape: &egui::Shape, text: &str) -> Option<std::sync::Arc<egui::Galley>> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some(t.galley.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|c| find(&c.shape, text))
        .unwrap_or_else(|| panic!("no text '{text}' among {:?}", texts(output)))
}

#[test]
fn modifiers_size_frame_and_place_a_node() {
    let mut run = Run::new(vec![
        panel(),
        button(2, 1, "Plain"),
        with(
            button(3, 1, "Wide"),
            UiModifier {
                width: Some(150.0),
                ..Default::default()
            },
        ),
        with(
            button(4, 1, "Full"),
            UiModifier {
                fill_width: true,
                ..Default::default()
            },
        ),
        with(
            button(5, 1, "Right"),
            UiModifier {
                align: Some(Align::End),
                ..Default::default()
            },
        ),
        with(
            text(6, 1, "Framed"),
            UiModifier {
                background: Some(0xFF336699),
                padding: insets(8.0),
                margin: insets(4.0),
                ..Default::default()
            },
        ),
        text(7, 1, "Framed"),
        with(
            button(8, 1, "Hidden"),
            UiModifier {
                invisible: true,
                ..Default::default()
            },
        ),
        with(
            button(9, 1, "Boxed"),
            UiModifier {
                width: Some(120.0),
                height: Some(40.0),
                padding: insets(4.0),
                border: Some(UiStroke {
                    width: 2.0,
                    color: 0xFFFFFFFF,
                }),
                ..Default::default()
            },
        ),
        with(
            text(10, 1, "Not wider than this, so it wraps into lines"),
            UiModifier {
                max_width: Some(90.0),
                ..Default::default()
            },
        ),
    ]);
    let b = |id: u64| run.state.bounds[&id];
    assert!((b(3).width - 150.0).abs() < 0.5, "{:?}", b(3));
    // All the width of the panel's inside, from where the other nodes start
    assert!((b(4).x - b(2).x).abs() < 0.5, "{:?} {:?}", b(4), b(2));
    assert!(b(4).width > 150.0 && right(b(4)) <= 200.0, "{:?}", b(4));
    assert!((right(b(5)) - right(b(4))).abs() < 1.0, "{:?}", b(5));
    // The frame: padding and margin on each side
    assert!(
        (b(6).width - (b(7).width + 24.0)).abs() < 1.0,
        "{:?} {:?}",
        b(6),
        b(7)
    );
    assert!((b(6).height - (b(7).height + 24.0)).abs() < 1.0);
    // The size counts padding and border
    assert!(
        (b(9).width - 120.0).abs() < 0.5 && (b(9).height - 40.0).abs() < 0.5,
        "{:?}",
        b(9)
    );
    assert!(
        b(10).width <= 90.0 && b(10).height > 2.0 * b(7).height,
        "{:?}",
        b(10)
    );
    // Invisible keeps its room but does not react
    assert!(b(8).width > 0.0 && b(8).y > bottom(b(7)));
    run.click(8);
    assert_eq!(run.state.take_events(), vec![]);
    run.click(9);
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 9 }]);

    let refused = |m: UiModifier| invalid(vec![panel(), with(button(2, 1, "x"), m)]);
    assert!(refused(UiModifier {
        width: Some(-1.0),
        ..Default::default()
    })
    .contains("Button 2: the modifier's width is -1"));
    assert!(refused(UiModifier {
        min_height: Some(30.0),
        max_height: Some(20.0),
        ..Default::default()
    })
    .contains("min_height 30 > max_height 20"));
    assert!(refused(UiModifier {
        opacity: Some(2.0),
        ..Default::default()
    })
    .contains("opacity is 2"));
}

fn flow_row(arrangement: Option<Arrangement>, wrap: bool, spacing: Option<f32>) -> Widget {
    Widget::Row {
        wrap,
        align: None,
        arrangement,
        spacing,
    }
}

#[test]
fn rows_gather_their_nodes_at_the_start_in_the_middle_or_at_the_end() {
    let mut nodes = vec![
        central(1),
        node(10, Some(1), flow_row(Some(Arrangement::End), false, None)),
        button(11, 10, "First"),
        button(12, 10, "Second"),
        node(
            20,
            Some(1),
            flow_row(Some(Arrangement::Center), false, None),
        ),
        button(21, 20, "First"),
        button(22, 20, "Second"),
        node(30, Some(1), flow_row(None, false, Some(20.0))),
        button(31, 30, "First"),
        button(32, 30, "Second"),
        node(40, Some(1), flow_row(None, true, None)),
    ];
    for i in 0..30 {
        nodes.push(button(41 + i, 40, &format!("Button {i}")));
    }
    let run = Run::new(nodes);
    let b = |id: u64| run.state.bounds[&id];
    let inside = b(1);
    // At the end, in their order
    assert!(b(11).x < b(12).x);
    assert!(
        right(b(12)) > right(inside) - 20.0,
        "{:?} {:?}",
        b(12),
        inside
    );
    // In the middle: as much room on the left as on the right
    let (left_gap, right_gap) = (b(21).x - inside.x, right(inside) - right(b(22)));
    assert!(
        left_gap > 100.0 && (left_gap - right_gap).abs() < 2.0,
        "{left_gap} {right_gap}"
    );
    assert!((b(32).x - right(b(31)) - 20.0).abs() < 0.5);
    // Lines that wrap
    assert!(b(70).y > bottom(b(41)), "{:?} {:?}", b(41), b(70));
    assert!(right(b(70)) <= right(inside));
}

#[test]
fn nodes_in_a_row_line_up_in_the_middle_or_at_the_bottom_whatever_they_are() {
    let bottom_row = Widget::Row {
        wrap: false,
        align: Some(Align::End),
        arrangement: None,
        spacing: None,
    };
    let nodes = vec![
        central(1),
        // A button with a size next to one without, at the end (the settings' Reset and Apply)
        node(10, Some(1), flow_row(Some(Arrangement::End), false, None)),
        button(11, 10, "Reset"),
        with(
            button(12, 10, "Apply"),
            UiModifier {
                width: Some(110.0),
                ..Default::default()
            },
        ),
        // A button before a column of two lines and a framed button
        node(20, Some(1), row()),
        button(21, 20, "Short"),
        node(22, Some(20), column()),
        text(23, 22, "One"),
        text(24, 22, "Two"),
        with(
            button(25, 20, "Framed"),
            UiModifier {
                background: Some(0xFF1F4E79),
                padding: insets(6.0),
                ..Default::default()
            },
        ),
        // At the bottom
        node(30, Some(1), bottom_row),
        button(31, 30, "Low"),
        node(32, Some(30), column()),
        text(33, 32, "One"),
        text(34, 32, "Two"),
    ];
    let run = Run::new(nodes);
    let b = |id: u64| run.state.bounds[&id];
    let middle = |id: u64| run.center(id).y;
    assert!((b(12).width - 110.0).abs() < 0.5, "{:?}", b(12));
    assert!(
        (b(11).y - b(12).y).abs() < 0.5 && (b(11).height - b(12).height).abs() < 0.5,
        "{:?} {:?}",
        b(11),
        b(12)
    );
    // The column is as high as two lines, the row as high as it: nothing hangs down
    assert!(b(22).height > 1.5 * b(21).height, "{:?}", b(22));
    assert!(
        (b(20).height - b(22).height).abs() < 0.5,
        "{:?} {:?}",
        b(20),
        b(22)
    );
    for id in [21, 25] {
        assert!(
            (middle(id) - middle(22)).abs() < 1.0,
            "{id}: {:?} {:?}",
            b(id),
            b(22)
        );
    }
    assert!(
        (bottom(b(31)) - bottom(b(32))).abs() < 1.0,
        "{:?} {:?}",
        b(31),
        b(32)
    );
    assert!(
        (b(30).height - b(32).height).abs() < 0.5,
        "{:?} {:?}",
        b(30),
        b(32)
    );
}

#[test]
fn columns_gather_their_nodes_across_and_along() {
    let column_with = |align, arrangement| Widget::Column {
        align,
        arrangement,
        spacing: None,
    };
    let tree = |align, arrangement| {
        vec![
            panel(),
            node(2, Some(1), column_with(align, arrangement)),
            button(3, 2, "One"),
            button(4, 2, "Two"),
        ]
    };
    let run = Run::new(tree(Some(Align::Center), None));
    let (one, two) = (run.state.bounds[&3], run.state.bounds[&4]);
    let middle = |r: UiRect| r.x + r.width / 2.0;
    assert!((middle(one) - 100.0).abs() < 2.0, "{one:?}");
    assert!((middle(two) - 100.0).abs() < 2.0, "{two:?}");
    // At the bottom of the panel, in their order
    let run = Run::new(tree(None, Some(Arrangement::End)));
    let (one, two) = (run.state.bounds[&3], run.state.bounds[&4]);
    assert!(one.y < two.y && bottom(two) > 580.0, "{one:?} {two:?}");
    let run = Run::new(tree(None, Some(Arrangement::Center)));
    let (one, two) = (run.state.bounds[&3], run.state.bounds[&4]);
    let middle = (one.y + bottom(two)) / 2.0;
    // The middle of what is left under the title
    assert!((middle - 310.0).abs() < 20.0, "{one:?} {two:?}");
}

#[test]
fn columns_sides_indents_and_resize_boxes_place_their_nodes() {
    let run = Run::new(vec![
        central(1),
        node(10, Some(1), Widget::Columns),
        text(11, 10, "a"),
        text(12, 10, "b"),
        text(13, 10, "c"),
        node(20, Some(1), Widget::Sides),
        text(21, 20, "Title"),
        button(22, 20, "Close"),
        node(30, Some(1), column()),
        text(31, 30, "Before"),
        node(32, Some(30), Widget::Indent),
        text(33, 32, "Inside"),
        node(
            40,
            Some(1),
            Widget::Resize {
                width: 300.0,
                height: 120.0,
            },
        ),
        text(41, 40, "Drag the corner"),
    ]);
    let b = |id: u64| run.state.bounds[&id];
    let inside = b(1);
    assert!(b(11).x < b(12).x && b(12).x < b(13).x);
    assert!(((b(12).x - b(11).x) - (b(13).x - b(12).x)).abs() < 1.0);
    assert!(b(12).x - b(11).x > 200.0, "{:?} {:?}", b(11), b(12));
    assert!(b(21).x < inside.x + 20.0);
    assert!(right(b(22)) > right(inside) - 20.0, "{:?}", b(22));
    assert!((b(21).y - b(22).y).abs() < 10.0);
    assert!(b(33).x > b(31).x + 5.0);
    assert!(
        (b(40).width - 300.0).abs() < 20.0 && (b(40).height - 120.0).abs() < 20.0,
        "{:?}",
        b(40)
    );
    assert!(invalid(vec![
        central(1),
        node(2, Some(1), Widget::Sides),
        text(3, 2, "x")
    ])
    .contains("Sides 2 holds 1 nodes"));
}

#[test]
fn panels_fold_away_and_the_central_panel_takes_the_rest() {
    let tree = |expanded| {
        vec![
            // Before the side panel in the list, but it gets what the panel leaves
            central(9),
            node(
                1,
                None,
                Widget::SidePanel {
                    side: Side::Left,
                    title: None,
                    width: 200.0,
                    resizable: false,
                    min_width: None,
                    max_width: None,
                    expanded,
                },
            ),
            text(2, 1, "Panel"),
            text(10, 9, "Rest"),
        ]
    };
    let mut run = Run::new(tree(true));
    let (side, rest) = (run.state.bounds[&1], run.state.bounds[&9]);
    assert!((rest.x - right(side)).abs() < 1.0, "{side:?} {rest:?}");
    assert!((right(rest) - 800.0).abs() < 1.0);
    assert_eq!(run.state.panels.len(), 2);
    run.state.set_tree(tree(false)).unwrap();
    for _ in 0..30 {
        run.frame(vec![]);
    }
    assert!(!run.state.bounds.contains_key(&1));
    assert!(run.state.bounds[&9].x < 1.0);
    // Without its background only its nodes cover the picture
    let run = Run::new(vec![
        node(1, None, Widget::CentralPanel { frame: false }),
        text(2, 1, "Over the picture"),
    ]);
    assert_eq!(run.state.panels, vec![run.state.bounds[&2]]);
    assert!(invalid(vec![central(1), central(2)]).contains("CentralPanel 2 is the second one"));
}

#[test]
fn windows_are_pinned_sized_without_a_title_and_closed_by_kotlin() {
    let window = |title_bar, open| {
        node(
            1,
            None,
            Widget::Window {
                title: "Stats".into(),
                x: -10.0,
                y: -10.0,
                width: Some(200.0),
                closable: true,
                collapsible: false,
                resizable: false,
                height: Some(100.0),
                anchor: Some(Anchor::BottomRight),
                movable: None,
                title_bar,
                fixed_size: true,
                scroll: false,
                open,
            },
        )
    };
    let long = "A line much longer than the two hundred points of the window, which wraps";
    let settle = |run: &mut Run| {
        for _ in 0..20 {
            run.frame(vec![]);
        }
    };
    let mut run = Run::new(vec![window(true, true), text(2, 1, long)]);
    settle(&mut run);
    let w = run.state.bounds[&1];
    assert!(
        (right(w) - 790.0).abs() < 1.0 && (bottom(w) - 590.0).abs() < 1.0,
        "{w:?}"
    );
    assert!((200.0..=230.0).contains(&w.width), "{w:?}");
    assert!(w.height >= 100.0, "{w:?}");
    let below_title = run.state.bounds[&2].y - w.y;
    let mut run = Run::new(vec![window(false, true), text(2, 1, long)]);
    settle(&mut run);
    let without = run.state.bounds[&2].y - run.state.bounds[&1].y;
    assert!(without + 15.0 < below_title, "{without} {below_title}");
    // Closed by Kotlin: it fades out without an event
    run.state
        .set_tree(vec![window(false, false), text(2, 1, long)])
        .unwrap();
    settle(&mut run);
    assert!(!run.state.bounds.contains_key(&1));
    assert_eq!(run.state.take_events(), vec![]);
}

#[test]
fn an_anchored_window_starts_at_its_corner_and_moves_when_it_may() {
    let window = |anchor, movable| {
        node(
            1,
            None,
            Widget::Window {
                title: "Benchmark".into(),
                x: 12.0,
                y: -12.0,
                width: Some(220.0),
                closable: false,
                collapsible: false,
                resizable: true,
                height: None,
                anchor,
                movable,
                title_bar: true,
                fixed_size: false,
                scroll: false,
                open: true,
            },
        )
    };
    let lines = |n: u64| (0..n).map(|i| text(2 + i, 1, &format!("Line {i}")));
    let settle = |run: &mut Run| {
        for _ in 0..20 {
            run.frame(vec![]);
        }
    };
    let drag_title = |run: &mut Run| {
        let w = run.state.bounds[&1];
        let from = egui::pos2(w.x + 40.0, w.y + 8.0);
        let to: Vec<_> = (1..=10)
            .map(|i| from + egui::vec2(10.0 * i as f32, -5.0 * i as f32))
            .collect();
        run.drag(from, &to);
        settle(run);
        (w, run.state.bounds[&1])
    };
    // Movable as well: it starts with its lower left corner 12 points from the corner of the picture, also while egui
    // still measures its height, and the user moves it from there
    let tree = |n| {
        let mut nodes = vec![window(Some(Anchor::BottomLeft), Some(true))];
        nodes.extend(lines(n));
        nodes
    };
    let mut run = Run::new(tree(12));
    settle(&mut run);
    let w = run.state.bounds[&1];
    assert!(
        (w.x - 12.0).abs() < 1.0 && (bottom(w) - 588.0).abs() < 1.0,
        "{w:?}"
    );
    let (before, after) = drag_title(&mut run);
    assert!(
        (after.x - before.x - 100.0).abs() < 2.0 && (before.y - after.y - 50.0).abs() < 2.0,
        "{before:?} {after:?}"
    );
    // Kotlin's next tree with more lines does not take it back to the corner
    run.state.set_tree(tree(14)).unwrap();
    settle(&mut run);
    let grown = run.state.bounds[&1];
    assert!((grown.x - after.x).abs() < 1.0, "{after:?} {grown:?}");
    // Only anchored it stays in the corner
    let mut nodes = vec![window(Some(Anchor::BottomLeft), None)];
    nodes.extend(lines(12));
    let mut run = Run::new(nodes);
    settle(&mut run);
    let (before, after) = drag_title(&mut run);
    assert_eq!(before, after);
    assert!((bottom(after) - 588.0).abs() < 1.0, "{after:?}");
    // Without an anchor movable is the default, and `Some(false)` keeps it where it is
    let mut nodes = vec![window(None, Some(false))];
    nodes.extend(lines(3));
    let mut run = Run::new(nodes);
    settle(&mut run);
    let (before, after) = drag_title(&mut run);
    assert_eq!(before, after);
}

#[test]
fn scroll_areas_scroll_sideways_and_stay_at_the_end() {
    let mut nodes = vec![
        central(1),
        node(
            10,
            Some(1),
            Widget::ScrollArea {
                max_height: Some(100.0),
                direction: None,
                max_width: None,
                stick_to_end: true,
            },
        ),
    ];
    for i in 0..50 {
        nodes.push(text(11 + i, 10, &format!("Line {i}")));
    }
    nodes.push(node(
        70,
        Some(1),
        Widget::ScrollArea {
            max_height: None,
            direction: Some(ScrollDirection::Horizontal),
            max_width: Some(120.0),
            stick_to_end: false,
        },
    ));
    nodes.push(node(71, Some(70), row()));
    for i in 0..9 {
        nodes.push(button(72 + i, 71, &format!("Button {i}")));
    }
    let mut run = Run::new(nodes);
    run.frame(vec![]);
    let b = |id: u64| run.state.bounds[&id];
    let area = b(10);
    assert!(area.height <= 100.5, "{area:?}");
    // The last line in sight, the first scrolled away above
    assert!(
        bottom(b(60)) <= bottom(area) + 1.0 && b(60).y >= area.y,
        "{area:?} {:?}",
        b(60)
    );
    assert!(b(11).y < area.y);
    let sideways = b(70);
    assert!(sideways.width <= 120.5, "{sideways:?}");
    assert!(b(80).x > right(sideways));
}

#[test]
fn texts_come_in_parts_wrap_or_are_cut_and_can_be_selected() {
    let long = "A text that is much too long for the panel it stands in";
    let text_with = |id, s: &str, spans, wrap, selectable| {
        node(
            id,
            Some(1),
            Widget::Text {
                text: s.into(),
                style: TextStyle::Body,
                color: None,
                spans,
                wrap,
                selectable,
            },
        )
    };
    let mut run = Run::new(vec![
        panel(),
        text_with(
            2,
            "Frame time ",
            vec![
                TextSpan {
                    text: "12.9 ms".into(),
                    style: Some(TextStyle::Strong),
                    ..Default::default()
                },
                TextSpan {
                    text: " (lavapipe)".into(),
                    italic: true,
                    color: Some(0xFF00FF00),
                    ..Default::default()
                },
            ],
            None,
            true,
        ),
        text_with(3, long, vec![], Some(TextWrap::Truncate), true),
        text_with(4, long, vec![], Some(TextWrap::Wrap), true),
        text_with(5, "Select me", vec![], None, true),
        text_with(6, "Not me", vec![], None, false),
    ]);
    let output = run.frame(vec![]);
    let parts = galley(&output, "Frame time 12.9 ms (lavapipe)");
    let sections = &parts.job.sections;
    assert_eq!(sections.len(), 3);
    assert_eq!(
        sections[1].format.color,
        run.egui.global_style().visuals.strong_text_color()
    );
    assert!(sections[2].format.italics);
    assert_eq!(sections[2].format.color, egui::Color32::GREEN);
    let line = run.state.bounds[&5].height;
    assert!((run.state.bounds[&3].height - line).abs() < 1.0);
    assert!(galley(&output, long).elided || texts(&output).iter().any(|t| t == long));
    assert!(run.state.bounds[&4].height > 1.5 * line);
    // A text that can be selected shows the text cursor
    let mut cursor = |id| {
        run.frame(vec![egui::Event::PointerMoved(run.center(id))]);
        run.frame(vec![]).platform_output.cursor_icon
    };
    assert_eq!(cursor(5), egui::CursorIcon::Text);
    assert_eq!(cursor(6), egui::CursorIcon::Default);

    assert!(invalid(vec![
        panel(),
        text_with(
            2,
            "",
            vec![TextSpan {
                text: "tiny".into(),
                size: Some(0.0),
                ..Default::default()
            }],
            None,
            true
        )
    ])
    .contains("the part 'tiny' has the size 0"));
}

#[test]
fn grids_keep_the_room_between_cells() {
    let grid = |row_spacing| {
        Run::new(vec![
            panel(),
            node(
                2,
                Some(1),
                Widget::Grid {
                    columns: 2,
                    striped: false,
                    column_spacing: Some(40.0),
                    row_spacing,
                    min_column_width: Some(50.0),
                },
            ),
            text(3, 2, "a"),
            text(4, 2, "b"),
            text(5, 2, "c"),
            text(6, 2, "d"),
        ])
    };
    let (spaced, tight) = (grid(Some(12.0)), grid(Some(0.0)));
    let b = |run: &Run, id: u64| run.state.bounds[&id];
    assert!(b(&spaced, 4).x - b(&spaced, 3).x >= 90.0 - 0.5);
    let pitch = |run: &Run| b(run, 5).y - b(run, 3).y;
    assert!((pitch(&spaced) - pitch(&tight) - 12.0).abs() < 0.5);
}

// Phase 2: every input of egui

fn plain_button(text: &str) -> Widget {
    Widget::Button {
        text: text.into(),
        small: false,
        frame: true,
        selected: false,
        shortcut: None,
        fill: None,
        image: None,
        image_size: None,
    }
}

fn field(value: &str) -> Widget {
    Widget::TextField {
        label: String::new(),
        value: value.into(),
        hint: String::new(),
        password: false,
        lines: None,
        code: false,
        language: None,
        char_limit: None,
        align: None,
        read_only: false,
    }
}

fn slider(value: f64, min: f64, max: f64) -> Widget {
    Widget::SliderFloat {
        label: String::new(),
        value,
        min,
        max,
        logarithmic: false,
        decimals: None,
        suffix: String::new(),
        prefix: String::new(),
        step: None,
        vertical: false,
        clamping: None,
        show_value: true,
        trailing_fill: None,
        handle: None,
    }
}

fn drag_value(value: f64) -> Widget {
    Widget::DragValue {
        label: String::new(),
        value,
        speed: 1.0,
        min: None,
        max: None,
        decimals: None,
        suffix: String::new(),
        prefix: String::new(),
        hexadecimal: false,
        angle: false,
    }
}

/// The middle of the text `text` as egui drew it.
fn text_center(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    fn find(shape: &egui::Shape, text: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => {
                Some(t.pos + t.galley.rect.center().to_vec2())
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|c| find(&c.shape, text))
        .unwrap_or_else(|| panic!("no text '{text}' among {:?}", texts(output)))
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    let event = |pressed| egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    };
    vec![event(true), event(false)]
}

impl Run {
    /// A click at `pos`, with the output of each frame.
    fn click_output(&mut self, pos: egui::Pos2) -> Vec<egui::FullOutput> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![
            self.frame(vec![egui::Event::PointerMoved(pos)]),
            self.frame(vec![button(true)]),
            self.frame(vec![button(false)]),
            self.frame(vec![]),
        ]
    }

    fn focus(&mut self, id: u64) {
        self.state.focus = Some(Focus::Node(id));
        self.frame(vec![]);
    }
}

#[test]
fn buttons_come_small_without_a_frame_chosen_and_with_their_key() {
    let mut small = plain_button("Small");
    if let Widget::Button { small: s, .. } = &mut small {
        *s = true;
    }
    let mut flat = plain_button("Flat");
    if let Widget::Button { frame, .. } = &mut flat {
        *frame = false;
    }
    let mut save = plain_button("Save");
    if let Widget::Button {
        shortcut, selected, ..
    } = &mut save
    {
        *shortcut = Some("Ctrl+S".into());
        *selected = true;
    }
    let mut run = Run::new(vec![
        central(1),
        node(2, Some(1), plain_button("Normal")),
        node(3, Some(1), small),
        node(4, Some(1), flat),
        node(5, Some(1), save),
    ]);
    let b = |run: &Run, id: u64| run.state.bounds[&id];
    assert!(b(&run, 3).height < b(&run, 2).height, "{:?}", b(&run, 3));
    // The key stands on the right of the text, in the same button
    let output = run.frame(vec![]);
    let (save_text, key_text) = (text_center(&output, "Save"), text_center(&output, "Ctrl+S"));
    assert!(key_text.x > save_text.x && b(&run, 5).contains(key_text.x, key_text.y));
    for id in [3, 4, 5] {
        run.click(id);
    }
    assert_eq!(
        run.state.take_events(),
        vec![
            UiEvent::Clicked { id: 3 },
            UiEvent::Clicked { id: 4 },
            UiEvent::Clicked { id: 5 }
        ]
    );
}

#[test]
fn labels_toggles_radio_buttons_and_switches_report_what_the_user_chose() {
    let mut run = Run::new(vec![
        central(1),
        node(
            2,
            Some(1),
            Widget::SelectableLabel {
                text: "Tab".into(),
                selected: false,
            },
        ),
        node(
            3,
            Some(1),
            Widget::ToggleButton {
                text: "Grid".into(),
                checked: false,
            },
        ),
        node(
            4,
            Some(1),
            Widget::RadioButton {
                text: "Low".into(),
                selected: true,
            },
        ),
        node(
            5,
            Some(1),
            Widget::Switch {
                text: "Shadows".into(),
                checked: false,
            },
        ),
        node(
            6,
            Some(1),
            Widget::Checkbox {
                text: "All".into(),
                checked: false,
                indeterminate: true,
            },
        ),
    ]);
    for id in [2, 3, 4, 6] {
        run.click(id);
    }
    // A switch is clicked on its track and on its text
    let switch = run.state.bounds[&5];
    run.click_at(egui::pos2(switch.x + 8.0, switch.y + switch.height / 2.0));
    assert_eq!(
        run.state.take_events(),
        vec![
            UiEvent::Clicked { id: 2 },
            UiEvent::Checked {
                id: 3,
                checked: true
            },
            UiEvent::Clicked { id: 4 },
            UiEvent::Checked {
                id: 6,
                checked: true
            },
            UiEvent::Checked {
                id: 5,
                checked: true
            },
        ]
    );
    let output = run.frame(vec![]);
    let label = text_center(&output, "Shadows");
    run.click_at(label);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Checked {
            id: 5,
            checked: false
        }]
    );
    // The switch is twice as wide as high, its text beside it
    assert!(switch.width > 2.0 * switch.height, "{switch:?}");
}

#[test]
fn sliders_stand_upright_go_in_steps_and_hide_their_value() {
    let mut upright = slider(5.0, 0.0, 10.0);
    let mut stepped = slider(5.0, 0.0, 10.0);
    let mut bare = slider(5.0, 0.0, 10.0);
    if let Widget::SliderFloat {
        vertical, prefix, ..
    } = &mut upright
    {
        *vertical = true;
        *prefix = "x ".into();
    }
    if let Widget::SliderFloat { step, handle, .. } = &mut stepped {
        *step = Some(2.5);
        *handle = Some(SliderHandle::Rect { aspect_ratio: 0.5 });
    }
    if let Widget::SliderFloat {
        show_value,
        clamping,
        ..
    } = &mut bare
    {
        *show_value = false;
        *clamping = Some(SliderClamping::Never);
    }
    // Kotlin's value outside the range stays without clamping
    if let Widget::SliderFloat { value, .. } = &mut bare {
        *value = 12.0;
    }
    let mut run = Run::new(vec![
        central(1),
        node(2, Some(1), upright),
        node(3, Some(1), stepped),
        node(4, Some(1), bare),
    ]);
    let upright = run.state.bounds[&2];
    assert!(upright.height > upright.width, "{upright:?}");
    let output = run.frame(vec![]);
    let shown = texts(&output);
    assert!(shown.iter().any(|t| t == "x "), "{shown:?}");
    // Two sliders show 5, the third nothing (not its 12 outside the range)
    assert_eq!(shown.iter().filter(|t| *t == "5.0").count(), 2, "{shown:?}");
    assert!(!shown.iter().any(|t| t.contains("12")), "{shown:?}");
    // Dragging goes in steps of 2.5
    let s = run.state.bounds[&3];
    let from = egui::pos2(s.x + s.width * 0.3, s.y + s.height / 2.0);
    run.drag(
        from,
        &[
            egui::pos2(s.x + s.width * 0.31, from.y),
            egui::pos2(s.x + 85.0, from.y),
        ],
    );
    let events = run.state.take_events();
    let Some(UiEvent::FloatChanged { value, .. }) = events.last() else {
        panic!("{events:?}")
    };
    assert_eq!(value % 2.5, 0.0, "{events:?}");
}

#[test]
fn drag_values_count_in_hexadecimal_and_in_degrees() {
    let mut hex = drag_value(255.0);
    if let Widget::DragValue {
        hexadecimal,
        prefix,
        ..
    } = &mut hex
    {
        *hexadecimal = true;
        *prefix = "#".into();
    }
    let mut angle = drag_value(std::f64::consts::FRAC_PI_2);
    if let Widget::DragValue { angle: a, .. } = &mut angle {
        *a = true;
    }
    let mut run = Run::new(vec![
        central(1),
        node(2, Some(1), hex),
        node(3, Some(1), angle),
    ]);
    let output = run.frame(vec![]);
    let shown = texts(&output);
    for part in ["#", "FF", "90", "°"] {
        assert!(shown.iter().any(|t| t == part), "{part}: {shown:?}");
    }
    // Dragging turns it in degrees (a degree per point, after a few points to start), reported in radians
    let a = run.state.bounds[&3];
    let from = egui::pos2(a.x + a.width / 2.0, a.y + a.height / 2.0);
    run.drag(
        from,
        &[
            egui::pos2(from.x + 5.0, from.y),
            egui::pos2(from.x + 10.0, from.y),
        ],
    );
    let events = run.state.take_events();
    let Some(UiEvent::FloatChanged { id: 3, value }) = events.last() else {
        panic!("{events:?}")
    };
    let degrees = value.to_degrees();
    assert!(degrees > 92.0 && degrees < 100.5, "{degrees}");
}

#[test]
fn text_fields_take_lines_code_and_limits() {
    let mut notes = field("");
    if let Widget::TextField { lines, .. } = &mut notes {
        *lines = Some(4);
    }
    let mut short = field("");
    if let Widget::TextField { char_limit, .. } = &mut short {
        *char_limit = Some(3);
    }
    let mut fixed = field("read me");
    if let Widget::TextField { read_only, .. } = &mut fixed {
        *read_only = true;
    }
    let mut rust = field("fn main() { let x = 1; }");
    if let Widget::TextField {
        code,
        language,
        lines,
        ..
    } = &mut rust
    {
        *code = true;
        *language = Some("rs".into());
        *lines = Some(2);
    }
    let mut run = Run::new(vec![
        central(1),
        node(2, Some(1), notes),
        node(3, Some(1), short),
        node(4, Some(1), fixed),
        node(5, Some(1), rust),
    ]);
    // Several lines: Enter makes a new one, Ctrl+Enter is done
    run.focus(2);
    run.frame(vec![egui::Event::Text("a".into())]);
    run.frame(key(egui::Key::Enter, egui::Modifiers::NONE));
    run.frame(vec![egui::Event::Text("b".into())]);
    run.frame(key(egui::Key::Enter, egui::Modifiers::COMMAND));
    let events = run.state.take_events();
    assert_eq!(
        events.last(),
        Some(&UiEvent::Submitted {
            id: 2,
            value: "a\nb".into()
        }),
        "{events:?}"
    );
    assert!(run.state.bounds[&2].height > 3.0 * run.state.bounds[&3].height);
    // At most three characters
    run.focus(3);
    run.frame(vec![egui::Event::Text("abcdef".into())]);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::TextChanged {
            id: 3,
            value: "abc".into()
        }]
    );
    // Read only: typing changes nothing
    run.focus(4);
    run.frame(vec![egui::Event::Text("x".into())]);
    assert_eq!(run.state.take_events(), vec![]);
    // The code in colors: several parts in other colors
    let output = run.frame(vec![]);
    let code = galley(&output, "fn main() { let x = 1; }");
    let colors: std::collections::HashSet<_> =
        code.job.sections.iter().map(|s| s.format.color).collect();
    assert!(colors.len() >= 3, "{:?}", code.job.sections);
}

#[test]
fn colors_and_days_are_chosen_in_their_pickers() {
    let mut run = Run::new(vec![
        central(1),
        node(
            2,
            Some(1),
            Widget::ColorPicker {
                label: "Light".into(),
                color: 0xFF336699,
                alpha: false,
                inline: true,
            },
        ),
        node(
            3,
            Some(1),
            Widget::DatePicker {
                label: "Day".into(),
                date: UiDate {
                    year: 2026,
                    month: 6,
                    day: 21,
                },
                format: Some("%d.%m.%Y".into()),
                calendar_week: true,
                highlight_weekends: true,
                first_year: None,
                last_year: None,
            },
        ),
    ]);
    // A click into the field of saturation and value of the inline picker
    let picker = run.state.bounds[&2];
    run.click_at(egui::pos2(picker.x + 20.0, picker.y + 80.0));
    let events = run.state.take_events();
    let Some(UiEvent::ColorChanged { id: 2, color }) = events.last() else {
        panic!("{events:?}")
    };
    assert_ne!(*color, 0xFF336699);
    assert_eq!(color >> 24, 0xFF, "opaque");
    // The button shows the day as asked; its calendar takes a click on the 15th and Save
    let output = run.frame(vec![]);
    let button = text_center(&output, "21.06.2026 📆");
    let outputs = run.click_output(button);
    let calendar = outputs.last().unwrap();
    let day = text_center(calendar, "15");
    run.click_at(day);
    let output = run.frame(vec![]);
    run.click_at(text_center(&output, "Save"));
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::DateChanged {
            id: 3,
            date: UiDate {
                year: 2026,
                month: 6,
                day: 15
            }
        }]
    );
}

#[test]
fn the_clipboard_copies_cuts_and_pastes() {
    let mut run = Run::new(vec![central(1), node(2, Some(1), field("garden"))]);
    run.focus(2);
    run.frame(key(egui::Key::A, egui::Modifiers::COMMAND));
    let output = run.frame(vec![egui::Event::Copy]);
    let mut out = UiOutput::default();
    out.add(output.platform_output, 2.0);
    assert_eq!(out.take().copy, Some("garden".into()));
    // Cut leaves the field empty, Paste fills it
    run.frame(vec![egui::Event::Cut]);
    run.frame(vec![egui::Event::Paste("cloister".into())]);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::TextChanged {
            id: 2,
            value: "cloister".into()
        }]
    );
    // The keys of the overlay: Control (Command on a Mac) with C and X copy and cut, with V the overlay asks for the
    // clipboard
    let mut held = crate::input::Held::default();
    let mut wanted = crate::input::Wanted::default();
    let mut press = |held: &mut crate::input::Held, key: &str, down| {
        held.events(
            UiInput::Key {
                key: key.into(),
                down,
            },
            1.0,
            &mut wanted,
        )
    };
    let command = if cfg!(target_os = "macos") {
        "Meta"
    } else {
        "Control"
    };
    press(&mut held, command, true);
    assert_eq!(press(&mut held, "C", true), vec![egui::Event::Copy]);
    assert_eq!(press(&mut held, "X", true), vec![egui::Event::Cut]);
    assert_eq!(press(&mut held, "V", true), vec![]);
    assert!(wanted.paste);
}

#[test]
fn an_input_method_composes_and_commits() {
    let mut run = Run::new(vec![central(1), node(2, Some(1), field(""))]);
    run.focus(2);
    let output = run.frame(vec![
        egui::Event::Ime(egui::ImeEvent::Enabled),
        egui::Event::Ime(egui::ImeEvent::Preedit("ni".into())),
    ]);
    // While it composes, the field tells where its cursor is, for the box of the input method
    let mut out = UiOutput::default();
    out.add(output.platform_output, 1.0);
    let input = out.text_input.expect("a text field has the keyboard");
    let f = run.state.bounds[&2];
    assert!(
        f.contains(input.cursor.x, input.cursor.y),
        "{input:?} {f:?}"
    );
    run.frame(vec![
        egui::Event::Ime(egui::ImeEvent::Commit("你".into())),
        egui::Event::Ime(egui::ImeEvent::Disabled),
    ]);
    let events = run.state.take_events();
    assert_eq!(
        events.last(),
        Some(&UiEvent::TextChanged {
            id: 2,
            value: "你".into()
        }),
        "{events:?}"
    );
}

#[test]
fn links_open_their_address_and_shortcuts_work_while_drawn() {
    let shortcut = |id, parent, key: &str, ctrl| {
        node(
            id,
            parent,
            Widget::Shortcut {
                key: key.into(),
                ctrl,
                shift: false,
                alt: false,
            },
        )
    };
    let mut run = Run::new(vec![
        central(1),
        node(
            2,
            Some(1),
            Widget::Link {
                text: "egui".into(),
                url: Some("https://www.egui.rs".into()),
                new_tab: true,
            },
        ),
        node(
            3,
            Some(1),
            Widget::Link {
                text: "Details".into(),
                url: None,
                new_tab: false,
            },
        ),
        node(4, Some(1), field("")),
        shortcut(10, None, "S", true),
        shortcut(11, None, "Delete", false),
        shortcut(12, Some(1), "F5", false),
    ]);
    let outputs = run.click_output(run.center(2));
    let mut out = UiOutput::default();
    for output in outputs {
        out.add(output.platform_output, 1.0);
    }
    assert_eq!(
        out.take().open,
        vec![crate::UiLink {
            url: "https://www.egui.rs".into(),
            new_tab: true
        }]
    );
    run.click(3);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Clicked { id: 2 }, UiEvent::Clicked { id: 3 }]
    );
    run.frame(key(egui::Key::S, egui::Modifiers::COMMAND));
    run.frame(key(egui::Key::F5, egui::Modifiers::NONE));
    run.frame(key(egui::Key::Delete, egui::Modifiers::NONE));
    assert_eq!(
        run.state.take_events(),
        vec![
            UiEvent::Clicked { id: 10 },
            UiEvent::Clicked { id: 12 },
            UiEvent::Clicked { id: 11 }
        ]
    );
    // While a text field has the keyboard, Delete is the field's, Ctrl+S still the shortcut's
    run.focus(4);
    run.frame(key(egui::Key::Delete, egui::Modifiers::NONE));
    run.frame(key(egui::Key::S, egui::Modifiers::COMMAND));
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 10 }]);
    // The pointer turns into a hand over a link and into a text cursor over a field
    let cursor = |run: &mut Run, id: u64| {
        let output = run.frame(vec![egui::Event::PointerMoved(run.center(id))]);
        let mut out = UiOutput::default();
        out.add(output.platform_output, 1.0);
        out.cursor
    };
    assert_eq!(cursor(&mut run, 2), crate::UiCursor::PointingHand);
    assert_eq!(cursor(&mut run, 4), crate::UiCursor::Text);
    // Also in the frames of a click: egui's label under the link would show the text cursor
    let at = run.center(2);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    for event in [button(true), button(false)] {
        let mut out = UiOutput::default();
        out.add(run.frame(vec![event]).platform_output, 1.0);
        assert_eq!(out.cursor, crate::UiCursor::PointingHand);
    }
}

#[test]
fn kotlin_gives_the_keyboard_and_hears_who_has_it() {
    let watched = UiModifier {
        focus_events: true,
        ..Default::default()
    };
    let mut run = Run::new(vec![
        central(1),
        with(node(2, Some(1), field("")), watched.clone()),
        with(node(3, Some(1), field("")), watched),
    ]);
    run.focus(2);
    run.frame(vec![egui::Event::Text("a".into())]);
    run.focus(3);
    run.state.focus = Some(Focus::Clear);
    run.frame(vec![]);
    run.frame(vec![]);
    assert_eq!(
        run.state.take_events(),
        vec![
            UiEvent::FocusChanged {
                id: 2,
                focused: true
            },
            UiEvent::TextChanged {
                id: 2,
                value: "a".into()
            },
            UiEvent::FocusChanged {
                id: 2,
                focused: false
            },
            UiEvent::FocusChanged {
                id: 3,
                focused: true
            },
            UiEvent::FocusChanged {
                id: 3,
                focused: false
            },
        ]
    );
}

#[test]
fn inputs_that_cannot_work_are_refused_with_the_reason() {
    let tree = |widget| vec![central(1), node(2, Some(1), widget)];
    let mut stepped = slider(1.0, 0.0, 2.0);
    if let Widget::SliderFloat { step, .. } = &mut stepped {
        *step = Some(0.0);
    }
    assert!(invalid(tree(stepped)).contains("the step is 0"));
    let mut lines = field("");
    if let Widget::TextField { lines: l, .. } = &mut lines {
        *l = Some(0);
    }
    assert!(invalid(tree(lines)).contains("at least one line"));
    let date = |month, day, first_year, last_year| Widget::DatePicker {
        label: String::new(),
        date: UiDate {
            year: 2026,
            month,
            day,
        },
        format: None,
        calendar_week: true,
        highlight_weekends: true,
        first_year,
        last_year,
    };
    assert!(invalid(tree(date(2, 30, None, None))).contains("2026-02-30 is no day"));
    assert!(invalid(tree(date(2, 3, Some(2030), Some(2020))))
        .contains("first_year 2030 > last_year 2020"));
    assert!(invalid(tree(Widget::Shortcut {
        key: "Hyper".into(),
        ctrl: true,
        shift: false,
        alt: false
    }))
    .contains("the key 'Hyper' is none egui knows"));
}

// Phase 3: menus, popups, dialogs

fn press(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Default::default(),
    }
}

impl Run {
    fn drawn(&self, id: u64) -> bool {
        self.state.bounds.contains_key(&id)
    }

    fn right_click(&mut self, id: u64) {
        let at = self.center(id);
        self.frame(vec![egui::Event::PointerMoved(at)]);
        self.frame(vec![press(at, egui::PointerButton::Secondary, true)]);
        self.frame(vec![press(at, egui::PointerButton::Secondary, false)]);
        self.frame(vec![]);
    }

    fn hover(&mut self, id: u64) {
        let at = self.center(id);
        self.frame(vec![egui::Event::PointerMoved(at)]);
        self.frame(vec![]);
    }
}

fn menu(id: u64, parent: u64, text: &str) -> UiNode {
    node(id, Some(parent), Widget::Menu { text: text.into() })
}

fn item(id: u64, parent: u64, text: &str, shortcut: Option<&str>) -> UiNode {
    node(
        id,
        Some(parent),
        Widget::Button {
            text: text.into(),
            small: false,
            frame: true,
            selected: false,
            shortcut: shortcut.map(Into::into),
            fill: None,
            image: None,
            image_size: None,
        },
    )
}

fn popup(id: u64, parent: u64, open: bool, side: Option<PopupSide>) -> UiNode {
    node(
        id,
        Some(parent),
        Widget::Popup {
            open,
            side,
            align: None,
            close: None,
            width: None,
        },
    )
}

#[test]
fn a_menu_bar_opens_menus_and_submenus_and_an_item_closes_them() {
    let mut run = Run::new(vec![
        node(1, None, Widget::TopBar { expanded: true }),
        node(2, Some(1), Widget::MenuBar),
        menu(3, 2, "Datei"),
        item(4, 3, "Speichern", Some("Ctrl+S")),
        node(5, Some(3), Widget::Separator),
        menu(6, 3, "Export"),
        item(7, 6, "PNG", None),
        node(
            8,
            Some(3),
            Widget::Checkbox {
                text: "Raster".into(),
                checked: false,
                indeterminate: false,
            },
        ),
        menu(9, 2, "Ansicht"),
        item(10, 9, "Oben", None),
    ]);
    assert!(run.drawn(3) && !run.drawn(4), "closed at the start");
    run.click(3);
    assert!(
        run.drawn(4) && run.drawn(6) && run.drawn(8),
        "the menu is open"
    );
    let (menu, item) = (run.state.bounds[&3], run.state.bounds[&4]);
    assert!(
        item.y >= menu.y + menu.height - 1.0,
        "{item:?} under {menu:?}"
    );
    // The submenu opens beside its button while the pointer is on it
    run.hover(6);
    run.frame(vec![]);
    assert!(run.drawn(7), "the submenu is open");
    let (export, png) = (run.state.bounds[&6], run.state.bounds[&7]);
    assert!(
        png.x >= export.x + export.width - 1.0,
        "{png:?} beside {export:?}"
    );
    run.click(7);
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 7 }]);
    run.frame(vec![]);
    assert!(!run.drawn(4) && !run.drawn(7), "an item closes the menu");
    // The other menu of the bar
    run.click(9);
    assert!(run.drawn(10) && !run.drawn(4));
    run.click(10);
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 10 }]);
}

#[test]
fn a_right_click_opens_the_context_menu_of_any_node() {
    let mut run = Run::new(vec![
        central(1),
        node(2, Some(1), plain_button("Säule")),
        node(3, Some(2), Widget::ContextMenu),
        item(4, 3, "Löschen", Some("Delete")),
        item(5, 3, "Kopieren", None),
        text(6, 1, "Boden"),
        node(7, Some(6), Widget::ContextMenu),
        item(8, 7, "Material", None),
    ]);
    assert!(!run.drawn(4));
    run.right_click(2);
    assert!(run.drawn(4) && run.drawn(5) && !run.drawn(8));
    run.click(4);
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 4 }]);
    run.frame(vec![]);
    assert!(!run.drawn(4), "the item closed the menu");
    // Also on a text, and Escape closes it
    run.right_click(6);
    assert!(run.drawn(8));
    run.frame(key(egui::Key::Escape, egui::Modifiers::NONE));
    run.frame(vec![]);
    assert!(!run.drawn(8));
    assert!(run.state.take_events().is_empty());
}

#[test]
fn a_tooltip_shows_nodes_while_the_pointer_rests() {
    let mut run = Run::new(vec![
        central(1),
        node(2, Some(1), plain_button("Sonne")),
        node(3, Some(2), Widget::Tooltip),
        text(4, 3, "Höhe 45°"),
        with(
            node(
                5,
                Some(3),
                Widget::ProgressBar {
                    progress: 0.5,
                    text: Some("Anteil".into()),
                },
            ),
            UiModifier {
                width: Some(160.0),
                ..Default::default()
            },
        ),
    ]);
    run.hover(2);
    for _ in 0..60 {
        run.frame(vec![]);
    }
    assert!(run.drawn(4) && run.drawn(5), "the tooltip shows its nodes");
    let (button, line) = (run.state.bounds[&2], run.state.bounds[&4]);
    assert!(line.y > button.y, "{line:?} under {button:?}");
    let bar = run.state.bounds[&5];
    assert!(
        (bar.width - 160.0).abs() < 0.5,
        "the bar keeps its width: {bar:?}"
    );
    run.frame(vec![egui::Event::PointerMoved(egui::pos2(700.0, 500.0))]);
    run.frame(vec![]);
    assert!(!run.drawn(4), "gone when the pointer leaves");
}

#[test]
fn kotlin_opens_a_popup_and_hears_when_the_user_closes_it() {
    let mut nodes = vec![
        central(1),
        node(2, Some(1), plain_button("Ebenen")),
        popup(3, 2, true, None),
        checkbox(4, false),
        popup(5, 2, false, Some(PopupSide::Right)),
        text(6, 5, "rechts"),
    ];
    nodes[3].parent = Some(3);
    let mut run = Run::new(nodes.clone());
    assert!(run.drawn(4) && !run.drawn(6));
    let (button, inside) = (run.state.bounds[&2], run.state.bounds[&4]);
    assert!(
        inside.y >= button.y + button.height,
        "{inside:?} below {button:?}"
    );
    // A click inside keeps it open
    run.click(4);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Checked {
            id: 4,
            checked: true
        }]
    );
    assert!(run.drawn(4));
    // A click beside it closes it until Kotlin sends a tree
    run.click_at(egui::pos2(600.0, 400.0));
    assert_eq!(run.state.take_events(), vec![UiEvent::Closed { id: 3 }]);
    run.frame(vec![]);
    assert!(!run.drawn(4));
    // Kotlin keeps it closed, then opens the other one at the right
    nodes[2] = popup(3, 2, false, None);
    nodes[4] = popup(5, 2, true, Some(PopupSide::Right));
    run.state.set_tree(nodes.clone()).unwrap();
    run.frame(vec![]);
    run.frame(vec![]);
    assert!(!run.drawn(4) && run.drawn(6));
    let (button, right) = (run.state.bounds[&2], run.state.bounds[&6]);
    assert!(
        right.x >= button.x + button.width,
        "{right:?} right of {button:?}"
    );
    run.frame(key(egui::Key::Escape, egui::Modifiers::NONE));
    assert_eq!(run.state.take_events(), vec![UiEvent::Closed { id: 5 }]);
}

#[test]
fn a_modal_blocks_the_rest_and_closes_on_escape_when_it_may() {
    let modal = |dismissible| {
        vec![
            central(1),
            node(2, Some(1), plain_button("Dahinter")),
            node(10, None, Widget::Modal { dismissible }),
            text(11, 10, "Szene verwerfen?"),
            node(12, Some(10), plain_button("Verwerfen")),
        ]
    };
    // The button behind is blocked; without dismissible neither a click beside it nor Escape closes it
    let mut run = Run::new(modal(false));
    assert!(run.drawn(11) && run.drawn(12));
    run.click(2);
    run.frame(key(egui::Key::Escape, egui::Modifiers::NONE));
    assert!(run.state.take_events().is_empty());
    run.click(12);
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 12 }]);
    assert!(run.drawn(11));
    // With dismissible a click beside it closes it (and does not reach the button behind), as Escape does
    let mut run = Run::new(modal(true));
    run.click(2);
    assert_eq!(run.state.take_events(), vec![UiEvent::Closed { id: 10 }]);
    run.frame(vec![]);
    assert!(!run.drawn(11), "hidden until the next tree");
    let mut run = Run::new(modal(true));
    run.frame(key(egui::Key::Escape, egui::Modifiers::NONE));
    assert_eq!(run.state.take_events(), vec![UiEvent::Closed { id: 10 }]);
}

#[test]
fn menus_and_popups_that_cannot_work_are_refused_with_the_reason() {
    let base = || vec![central(1), node(2, Some(1), plain_button("A"))];
    let with_nodes = |extra: Vec<UiNode>| {
        let mut all = base();
        all.extend(extra);
        all
    };
    let why = invalid(with_nodes(vec![node(3, None, Widget::ContextMenu)]));
    assert!(why.contains("belongs to the node"), "{why}");
    let why = invalid(with_nodes(vec![
        node(3, Some(2), Widget::Tooltip),
        node(4, Some(2), Widget::Tooltip),
    ]));
    assert!(why.contains("second Tooltip"), "{why}");
    let mut texted = base();
    texted[1].tooltip = Some("Text".into());
    texted.push(node(3, Some(2), Widget::Tooltip));
    let why = invalid(texted);
    assert!(why.contains("tooltip text as well"), "{why}");
    let why = invalid(with_nodes(vec![
        popup(3, 2, true, None),
        node(4, Some(3), Widget::ContextMenu),
    ]));
    assert!(why.contains("cannot open it"), "{why}");
    let why = invalid(with_nodes(vec![node(
        3,
        Some(2),
        Widget::Popup {
            open: true,
            side: None,
            align: None,
            close: None,
            width: Some(-5.0),
        },
    )]));
    assert!(why.contains("width is -5"), "{why}");
    let why = invalid(with_nodes(vec![node(
        3,
        Some(1),
        Widget::Modal { dismissible: true },
    )]));
    assert!(why.contains("stands at the top"), "{why}");
    // Two popups at one node are fine, a menu holds nodes
    assert!(Tree::new(with_nodes(vec![
        popup(3, 2, true, None),
        popup(4, 2, false, None),
        node(5, Some(1), Widget::MenuBar),
        menu(6, 5, "Datei"),
        item(7, 6, "Neu", None),
    ]))
    .is_ok());
}

// Phase 4: tables and strips

fn col(title: &str) -> TableColumn {
    TableColumn {
        title: title.into(),
        width: None,
        min_width: None,
        max_width: None,
        resizable: None,
        clip: false,
        align: None,
        sortable: false,
        style: None,
    }
}

fn table(
    columns: Vec<TableColumn>,
    data: Option<&str>,
    selection: Option<TableSelection>,
) -> Widget {
    Widget::Table {
        columns,
        data: data.map(Into::into),
        striped: true,
        resizable: false,
        selection,
        selected: vec![],
        sort: None,
        row_height: None,
        max_height: None,
        stick_to_end: false,
        scroll_to: None,
    }
}

fn table_row(id: u64, parent: u64) -> UiNode {
    node(
        id,
        Some(parent),
        Widget::TableRow {
            header: false,
            height: None,
        },
    )
}

/// `rows` rows of "Zeile i" and "i × 10" for the table data `key`.
fn rows_of(state: &mut UiState, key: &str, rows: usize) {
    let cells = (0..rows)
        .flat_map(|i| [format!("Zeile {i}"), format!("{} Punkte", i * 10)])
        .collect();
    state
        .tables
        .insert(key.into(), crate::state::TableData { columns: 2, cells });
}

impl Run {
    /// Long enough that the next click is no double click.
    fn pause(&mut self) {
        for _ in 0..30 {
            self.frame(vec![]);
        }
    }

    fn click_text(&mut self, text: &str) {
        let output = self.frame(vec![]);
        self.click_at(text_center(&output, text));
    }

    fn set(&mut self, f: impl FnOnce(&mut Widget), id: u64) {
        let mut nodes = self.state.tree.nodes.clone();
        f(&mut nodes.iter_mut().find(|n| n.id == id).unwrap().widget);
        self.state.set_tree(nodes).unwrap();
        self.frame(vec![]);
        self.frame(vec![]);
    }
}

#[test]
fn a_table_shows_titles_and_rows_of_nodes_in_their_columns() {
    let columns = vec![
        TableColumn {
            width: Some(ColumnWidth::Exact { width: 160.0 }),
            ..col("Objekt")
        },
        TableColumn {
            width: Some(ColumnWidth::Exact { width: 120.0 }),
            align: Some(Align::End),
            ..col("Polygone")
        },
        TableColumn {
            width: Some(ColumnWidth::Remainder),
            align: Some(Align::Center),
            ..col("Sichtbar")
        },
    ];
    let mut nodes = vec![central(1), node(10, Some(1), table(columns, None, None))];
    for (i, (name, polygons)) in [("Säule", "12400"), ("Brunnen", "48900")]
        .iter()
        .enumerate()
    {
        let row = 20 + 10 * i as u64;
        nodes.push(table_row(row, 10));
        nodes.push(text(row + 1, row, name));
        nodes.push(text(row + 2, row, polygons));
        nodes.push(node(
            row + 3,
            Some(row),
            Widget::Checkbox {
                text: String::new(),
                checked: i == 0,
                indeterminate: false,
            },
        ));
    }
    let mut run = Run::new(nodes);
    let output = run.frame(vec![]);
    let b = |id: u64| run.state.bounds[&id];
    for title in ["Objekt", "Polygone", "Sichtbar"] {
        assert!(
            texts(&output).contains(&title.to_string()),
            "{:?}",
            texts(&output)
        );
    }
    // Rows under the titles, one under the other, the cells side by side in their columns
    let title = text_center(&output, "Objekt");
    assert!(
        b(21).y > title.y && b(31).y > b(21).y,
        "{:?} {:?}",
        b(21),
        b(31)
    );
    assert!((b(21).y - b(22).y).abs() < 2.0 && b(22).x > right(b(21)));
    // Right in the column of 120 points, the box in the middle of the rest
    let table = b(10);
    let polygons_end = table.x + 160.0 + 120.0;
    assert!(
        (right(b(22)) - polygons_end).abs() < 12.0,
        "{:?} ends near {polygons_end}",
        b(22)
    );
    let rest = (polygons_end + right(table)) / 2.0;
    assert!(
        (run.center(23).x - rest).abs() < 12.0,
        "{:?} around {rest}",
        b(23)
    );
    // A row knows its room, a widget in a cell works as anywhere
    assert!(b(20).width > 300.0 && b(20).y <= b(21).y, "{:?}", b(20));
    run.click(33);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Checked {
            id: 33,
            checked: true
        }]
    );
}

#[test]
fn a_table_draws_only_the_rows_in_sight_of_its_data() {
    let columns = vec![
        col("Name"),
        TableColumn {
            align: Some(Align::End),
            ..col("Wert")
        },
    ];
    let mut widget = table(columns, Some("messung"), None);
    if let Widget::Table { max_height, .. } = &mut widget {
        *max_height = Some(200.0);
    }
    let mut run = Run::new(vec![central(1), node(10, Some(1), widget)]);
    rows_of(&mut run.state, "messung", 50_000);
    let started = std::time::Instant::now();
    run.frame(vec![]);
    let output = run.frame(vec![]);
    let frame = started.elapsed() / 2;
    let shown: Vec<String> = texts(&output)
        .into_iter()
        .filter(|t| t.starts_with("Zeile"))
        .collect();
    assert!(
        shown.contains(&"Zeile 0".to_string()) && shown.len() < 20,
        "{shown:?}"
    );
    assert!(
        run.state.bounds[&10].height <= 230.0,
        "{:?}",
        run.state.bounds[&10]
    );
    println!("a frame of a table with 50000 rows: {frame:?}");
    // Kotlin names a row: the table scrolls to it once, then the user scrolls as he likes
    run.set(
        |w| {
            if let Widget::Table { scroll_to, .. } = w {
                *scroll_to = Some(49_999)
            }
        },
        10,
    );
    for _ in 0..30 {
        run.frame(vec![]);
    }
    let output = run.frame(vec![]);
    assert!(texts(&output).contains(&"Zeile 49999".to_string()));
    assert!(texts(&output).contains(&"499990 Punkte".to_string()));
    assert!(!texts(&output).contains(&"Zeile 0".to_string()));
    // Rows at the end of a table that sticks to it come into sight
    run.set(
        |w| {
            if let Widget::Table { stick_to_end, .. } = w {
                *stick_to_end = true
            }
        },
        10,
    );
    rows_of(&mut run.state, "messung", 50_010);
    for _ in 0..5 {
        run.frame(vec![]);
    }
    let output = run.frame(vec![]);
    assert!(
        texts(&output).contains(&"Zeile 50009".to_string()),
        "{:?}",
        texts(&output)
    );
    // Without its data it shows its titles alone
    run.state.tables.clear();
    let output = run.frame(vec![]);
    assert!(texts(&output).contains(&"Name".to_string()));
    assert!(!texts(&output).iter().any(|t| t.starts_with("Zeile")));
}

#[test]
fn rows_are_selected_with_a_click_ctrl_and_shift_and_opened_with_a_double_click() {
    let columns = vec![col(""), col("")];
    let mut run = Run::new(vec![
        central(1),
        node(
            10,
            Some(1),
            table(columns, Some("liste"), Some(TableSelection::Multiple)),
        ),
    ]);
    rows_of(&mut run.state, "liste", 20);
    run.frame(vec![]);
    let select = |run: &mut Run, row: usize, modifiers: egui::Modifiers| {
        run.modifiers = modifiers;
        run.click_text(&format!("Zeile {row}"));
        run.modifiers = egui::Modifiers::NONE;
        run.pause();
        let events = run.state.take_events();
        // Kotlin answers with the rows it was told
        if let Some(UiEvent::RowsSelected { rows, .. }) = events.last() {
            let rows = rows.clone();
            run.set(
                |w| {
                    if let Widget::Table { selected, .. } = w {
                        *selected = rows
                    }
                },
                10,
            );
        }
        events
    };
    let selected = |rows: Vec<u64>| vec![UiEvent::RowsSelected { id: 10, rows }];
    assert_eq!(
        select(&mut run, 2, egui::Modifiers::NONE),
        selected(vec![2])
    );
    assert_eq!(
        select(&mut run, 4, egui::Modifiers::COMMAND),
        selected(vec![2, 4])
    );
    assert_eq!(
        select(&mut run, 7, egui::Modifiers::SHIFT),
        selected(vec![4, 5, 6, 7])
    );
    assert_eq!(
        select(&mut run, 5, egui::Modifiers::COMMAND),
        selected(vec![4, 6, 7])
    );
    assert_eq!(
        select(&mut run, 9, egui::Modifiers::NONE),
        selected(vec![9])
    );
    // A click on the one selected row changes nothing, a second one at once opens it
    let at = text_center(&run.frame(vec![]), "Zeile 9");
    run.click_at(at);
    assert_eq!(run.state.take_events(), vec![]);
    run.click_at(at);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::RowActivated { id: 10, row: 9 }]
    );
    // Also when the pointer rests between the clicks
    run.pause();
    let at = text_center(&run.frame(vec![]), "Zeile 12");
    run.frame(vec![egui::Event::PointerMoved(at)]);
    for _ in 0..2 {
        run.frame(vec![press(at, egui::PointerButton::Primary, true)]);
        run.frame(vec![press(at, egui::PointerButton::Primary, false)]);
    }
    assert_eq!(
        run.state.take_events(),
        vec![
            UiEvent::RowsSelected {
                id: 10,
                rows: vec![12]
            },
            UiEvent::RowActivated { id: 10, row: 12 }
        ]
    );
    run.set(
        |w| {
            if let Widget::Table { selected, .. } = w {
                *selected = vec![12]
            }
        },
        10,
    );
    // A double click right after a click elsewhere (egui counts it as a third click), a triple click opens once
    run.pause();
    let output = run.frame(vec![]);
    let (other, at) = (
        text_center(&output, "Zeile 1"),
        text_center(&output, "Zeile 12"),
    );
    let clicks = |run: &mut Run, at: egui::Pos2, n: usize| {
        run.frame(vec![egui::Event::PointerMoved(at)]);
        for _ in 0..n {
            run.frame(vec![press(at, egui::PointerButton::Primary, true)]);
            run.frame(vec![press(at, egui::PointerButton::Primary, false)]);
        }
    };
    clicks(&mut run, other, 1);
    clicks(&mut run, at, 2);
    assert_eq!(
        run.state.take_events(),
        // The selection of the first click is replaced by the newer one: Kotlin did not take it
        vec![
            UiEvent::RowsSelected {
                id: 10,
                rows: vec![12]
            },
            UiEvent::RowActivated { id: 10, row: 12 }
        ]
    );
    run.pause();
    clicks(&mut run, at, 3);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::RowActivated { id: 10, row: 12 }]
    );
    // Also when the first click selects the row and Kotlin answers in between
    run.pause();
    let at = text_center(&run.frame(vec![]), "Zeile 11");
    run.click_at(at);
    assert_eq!(run.state.take_events(), selected(vec![11]));
    run.set(
        |w| {
            if let Widget::Table { selected, .. } = w {
                *selected = vec![11]
            }
        },
        10,
    );
    run.click_at(at);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::RowActivated { id: 10, row: 11 }]
    );
    // One row at most
    run.pause();
    run.set(
        |w| {
            if let Widget::Table {
                selection,
                selected,
                ..
            } = w
            {
                *selection = Some(TableSelection::Single);
                *selected = vec![];
            }
        },
        10,
    );
    assert_eq!(
        select(&mut run, 3, egui::Modifiers::COMMAND),
        selected(vec![3])
    );
    assert_eq!(
        select(&mut run, 6, egui::Modifiers::SHIFT),
        selected(vec![6])
    );
    // A click on the text of a row of nodes selects the row, not the text
    let mut nodes = run.state.tree.nodes.clone();
    nodes[1].widget = table(vec![col("")], None, Some(TableSelection::Single));
    nodes.push(table_row(20, 10));
    nodes.push(text(21, 20, "Brunnen"));
    run.state.set_tree(nodes).unwrap();
    run.frame(vec![]);
    run.pause();
    run.click(21);
    assert_eq!(run.state.take_events(), selected(vec![0]));
    let mut nodes = run.state.tree.nodes[..2].to_vec();
    nodes[1].widget = table(
        vec![col(""), col("")],
        Some("liste"),
        Some(TableSelection::Single),
    );
    run.state.set_tree(nodes).unwrap();
    run.frame(vec![]);
    // Without a selection the rows do not react
    run.set(
        |w| {
            if let Widget::Table {
                selection,
                selected,
                ..
            } = w
            {
                *selection = None;
                *selected = vec![];
            }
        },
        10,
    );
    assert_eq!(select(&mut run, 1, egui::Modifiers::NONE), vec![]);
}

#[test]
fn a_click_on_a_sortable_title_asks_kotlin_to_sort() {
    let columns = vec![
        col("Name"),
        TableColumn {
            sortable: true,
            width: Some(ColumnWidth::Exact { width: 140.0 }),
            ..col("Wert")
        },
    ];
    let mut run = Run::new(vec![
        central(1),
        node(10, Some(1), table(columns, Some("t"), None)),
    ]);
    rows_of(&mut run.state, "t", 3);
    run.click_text("Name");
    assert_eq!(run.state.take_events(), vec![], "Name is not sortable");
    // Anywhere in the title's cell
    let output = run.frame(vec![]);
    let title = text_center(&output, "Wert");
    run.click_at(egui::pos2(title.x + 50.0, title.y));
    let ascending = TableSort {
        column: 1,
        ascending: true,
    };
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::SortChanged {
            id: 10,
            sort: ascending
        }]
    );
    // The arrow shows at once; Kotlin sorts and sends the table so
    assert!(texts(&run.frame(vec![])).contains(&"Wert ⏶".to_string()));
    run.set(
        |w| {
            if let Widget::Table { sort, .. } = w {
                *sort = Some(ascending)
            }
        },
        10,
    );
    run.pause();
    run.click_text("Wert ⏶");
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::SortChanged {
            id: 10,
            sort: TableSort {
                column: 1,
                ascending: false
            }
        }]
    );
    assert!(texts(&run.frame(vec![])).contains(&"Wert ⏷".to_string()));
}

#[test]
fn a_header_row_takes_widgets_and_strips_divide_their_room() {
    // A header of nodes instead of titles
    let mut nodes = vec![
        central(1),
        node(
            10,
            Some(1),
            table(vec![col("unused"), col("")], Some("t"), None),
        ),
        node(
            11,
            Some(10),
            Widget::TableRow {
                header: true,
                height: Some(30.0),
            },
        ),
        checkbox(12, false),
        text(13, 11, "Alle"),
    ];
    nodes[3].parent = Some(11);
    // A sidebar of 200 points, a quarter of the whole and the rest; the rest one above the other in halves
    let fill = |n: UiNode| {
        with(
            n,
            UiModifier {
                fill_width: true,
                fill_height: true,
                ..Default::default()
            },
        )
    };
    nodes.push(node(
        20,
        Some(1),
        Widget::Strip {
            sizes: vec![
                StripSize::Exact { points: 200.0 },
                StripSize::Relative {
                    fraction: 0.25,
                    min: None,
                    max: None,
                },
                StripSize::Remainder {
                    min: None,
                    max: None,
                },
            ],
            vertical: false,
            clip: false,
        },
    ));
    nodes.push(fill(button(21, 20, "Seite")));
    nodes.push(fill(button(22, 20, "Viertel")));
    nodes.push(node(
        23,
        Some(20),
        Widget::Strip {
            sizes: vec![
                StripSize::Relative {
                    fraction: 0.5,
                    min: None,
                    max: None,
                },
                StripSize::Remainder {
                    min: None,
                    max: None,
                },
            ],
            vertical: true,
            clip: false,
        },
    ));
    nodes.push(fill(button(24, 23, "Oben")));
    nodes.push(fill(button(25, 23, "Unten")));
    let mut run = Run::new(nodes);
    rows_of(&mut run.state, "t", 2);
    let output = run.frame(vec![]);
    let b = |id: u64| run.state.bounds[&id];
    assert!(
        !texts(&output).contains(&"unused".to_string()),
        "the header row replaces the titles"
    );
    assert!(
        run.drawn(12) && run.drawn(13) && (b(11).height - 30.0).abs() < 1.0,
        "{:?}",
        b(11)
    );
    assert!(text_center(&output, "Zeile 0").y > bottom(b(11)));
    let (side, quarter, top, below) = (b(21), b(22), b(24), b(25));
    assert!((side.width - 200.0).abs() < 1.0, "{side:?}");
    let whole = b(20).width;
    assert!(
        (quarter.width - whole / 4.0).abs() < 12.0,
        "{quarter:?} of {whole}"
    );
    assert!(top.x > right(quarter) && (top.x - below.x).abs() < 1.0 && below.y > bottom(top));
    assert!((top.height - below.height).abs() < 6.0, "{top:?} {below:?}");
    assert!(right(top) > right(b(20)) - 12.0);
    run.click(12);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Checked {
            id: 12,
            checked: true
        }]
    );
}

#[test]
fn tables_and_strips_in_a_column_take_the_room_below() {
    // A column shrinks to its content and leaves the nodes in it no room: a table and a strip take what is seen
    let mut nodes = vec![
        central(1),
        node(2, Some(1), column()),
        text(3, 2, "Materialien"),
        node(10, Some(2), table(vec![col("Name"), col("An")], None, None)),
    ];
    for (i, name) in ["Sandstein", "Marmor", "Bronze"].iter().enumerate() {
        let row = 20 + 10 * i as u64;
        nodes.push(table_row(row, 10));
        nodes.push(text(row + 1, row, name));
        nodes.push(checkbox(row + 2, true));
        nodes.last_mut().unwrap().parent = Some(row);
    }
    let fill = |n: UiNode| {
        with(
            n,
            UiModifier {
                fill_width: true,
                fill_height: true,
                ..Default::default()
            },
        )
    };
    nodes.push(node(4, Some(1), column()));
    nodes.push(node(
        50,
        Some(4),
        Widget::Strip {
            sizes: vec![
                StripSize::Exact { points: 60.0 },
                StripSize::Remainder {
                    min: None,
                    max: None,
                },
            ],
            vertical: true,
            clip: true,
        },
    ));
    nodes.push(fill(button(51, 50, "Oben")));
    nodes.push(fill(button(52, 50, "Unten")));
    let mut run = Run::new(nodes);
    let output = run.frame(vec![]);
    let b = |id: u64| run.state.bounds[&id];
    for name in ["Sandstein", "Marmor", "Bronze"] {
        assert!(
            texts(&output).contains(&name.to_string()),
            "{name} is drawn: {:?}",
            texts(&output)
        );
    }
    let title = text_center(&output, "Name");
    assert!(b(21).y > title.y && b(41).y > b(31).y, "{:?}", b(41));
    // The rows take what they need, not the whole room
    assert!(bottom(b(10)) < b(21).y + 4.0 * 30.0, "{:?}", b(10));
    // The strip divides the room below it down to the bottom
    let (top, rest) = (b(51), b(52));
    assert!((top.height - 60.0).abs() < 6.0, "{top:?}");
    assert!(rest.y > bottom(top) && rest.height > 100.0, "{rest:?}");
    run.click(42);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Checked {
            id: 42,
            checked: false
        }]
    );
}

#[test]
fn tables_and_strips_that_cannot_work_are_refused_with_the_reason() {
    let with_nodes = |more: Vec<UiNode>| {
        let mut nodes = vec![
            central(1),
            node(10, Some(1), table(vec![col("A"), col("B")], None, None)),
        ];
        nodes.extend(more);
        nodes
    };
    let why = invalid(vec![central(1), table_row(2, 1)]);
    assert!(
        why.contains("TableRow 2 is inside CentralPanel: rows belong to a Table"),
        "{why}"
    );
    let why = invalid(with_nodes(vec![text(11, 10, "loose")]));
    assert!(
        why.contains("Text 11 is inside Table 10, which holds TableRows"),
        "{why}"
    );
    let header = |id| {
        node(
            id,
            Some(10),
            Widget::TableRow {
                header: true,
                height: None,
            },
        )
    };
    let why = invalid(with_nodes(vec![table_row(11, 10), header(12)]));
    assert!(
        why.contains("TableRow 12 is a header, but not the first row"),
        "{why}"
    );
    let why = invalid(with_nodes(vec![
        table_row(11, 10),
        text(12, 11, "a"),
        text(13, 11, "b"),
        text(14, 11, "c"),
    ]));
    assert!(
        why.contains("TableRow 11 has 3 cells, Table 10 has 2 columns"),
        "{why}"
    );
    let mut nodes = with_nodes(vec![header(11), table_row(12, 10)]);
    nodes[1].widget = table(vec![col("A")], Some("t"), None);
    assert!(
        invalid(nodes).contains("TableRow 12 is a row of Table 10, which has its rows from data")
    );
    let lone = |widget| invalid(vec![central(1), node(10, Some(1), widget)]);
    assert!(lone(table(vec![], None, None)).contains("at least one column"));
    let mut selected = table(vec![col("A")], None, None);
    if let Widget::Table { selected: s, .. } = &mut selected {
        *s = vec![1];
    }
    assert!(lone(selected.clone()).contains("1 rows are selected, but the table has no selection"));
    if let Widget::Table {
        selected: s,
        selection,
        sort,
        ..
    } = &mut selected
    {
        *s = vec![1, 2];
        *selection = Some(TableSelection::Single);
        *sort = Some(TableSort {
            column: 3,
            ascending: true,
        });
    }
    assert!(lone(selected.clone()).contains("2 rows are selected, but the table selects one"));
    if let Widget::Table { selection, .. } = &mut selected {
        *selection = Some(TableSelection::Multiple);
    }
    assert!(lone(selected).contains("sorted by column 3, it has 1"));
    let why = lone(table(
        vec![TableColumn {
            width: Some(ColumnWidth::Exact { width: 0.0 }),
            ..col("A")
        }],
        None,
        None,
    ));
    assert!(why.contains("column 0: the width is 0"), "{why}");
    let why = lone(table(
        vec![TableColumn {
            min_width: Some(90.0),
            max_width: Some(40.0),
            ..col("A")
        }],
        None,
        None,
    ));
    assert!(why.contains("column 0: min 90 > max 40"), "{why}");
    let strip = |sizes| Widget::Strip {
        sizes,
        vertical: false,
        clip: false,
    };
    assert!(lone(strip(vec![])).contains("at least one size"));
    let why = lone(strip(vec![StripSize::Relative {
        fraction: 1.5,
        min: None,
        max: None,
    }]));
    assert!(why.contains("size 0: the fraction is 1.5"), "{why}");
    let why = invalid(vec![
        central(1),
        node(10, Some(1), strip(vec![StripSize::Exact { points: 10.0 }])),
        text(11, 10, "a"),
        text(12, 10, "b"),
    ]);
    assert!(why.contains("Strip 10 holds 2 nodes in 1 sizes"), "{why}");
    // The rows of data
    let overlay = EguiOverlay::new();
    let data_why = |r: Result<(), UiError>| match r {
        Err(UiError::InvalidData { why }) => why,
        other => panic!("expected invalid data, got {other:?}"),
    };
    assert!(data_why(overlay.set_table_data("t".into(), 0, vec![])).contains("at least one cell"));
    assert!(
        data_why(overlay.set_table_data("t".into(), 2, vec!["a".into()]))
            .contains("1 cells of 't' do not fill rows of 2")
    );
    assert!(data_why(overlay.append_table_rows("t".into(), vec![])).contains("'t' has no rows yet"));
    overlay
        .set_table_data("t".into(), 2, vec!["a".into(), "b".into()])
        .unwrap();
    overlay
        .append_table_rows("t".into(), vec!["c".into(), "d".into()])
        .unwrap();
    assert!(
        data_why(overlay.append_table_rows("t".into(), vec!["e".into()]))
            .contains("1 cells do not fill rows of 2")
    );
}

#[test]
fn a_resizable_table_in_a_new_window_takes_its_widths_after_the_first_frame() {
    let window = node(
        1,
        None,
        Widget::Window {
            title: "Messungen".into(),
            x: -12.0,
            y: 12.0,
            width: Some(600.0),
            closable: false,
            collapsible: false,
            resizable: false,
            height: Some(330.0),
            anchor: Some(Anchor::TopRight),
            movable: None,
            title_bar: true,
            fixed_size: true,
            scroll: false,
            open: true,
        },
    );
    let mut t = table(
        vec![
            col("Lauf"),
            TableColumn {
                width: Some(ColumnWidth::Initial { width: 110.0 }),
                ..col("Szene")
            },
            TableColumn {
                width: Some(ColumnWidth::Initial { width: 76.0 }),
                ..col("Backend")
            },
            TableColumn {
                width: Some(ColumnWidth::Remainder),
                ..col("ms")
            },
        ],
        Some("t"),
        None,
    );
    if let Widget::Table { resizable, .. } = &mut t {
        *resizable = true;
    }
    let nodes = vec![
        window,
        node(
            20,
            Some(1),
            Widget::Strip {
                sizes: vec![
                    StripSize::Exact { points: 200.0 },
                    StripSize::Remainder {
                        min: None,
                        max: None,
                    },
                ],
                vertical: false,
                clip: false,
            },
        ),
        text(21, 20, "links"),
        node(10, Some(20), t),
    ];
    let mut run = Run::new(nodes);
    let cells = (0..100)
        .flat_map(|i| {
            [
                format!("{i}"),
                "Garten".into(),
                "WebGPU".into(),
                "8.00".into(),
            ]
        })
        .collect();
    run.state
        .tables
        .insert("t".into(), crate::state::TableData { columns: 4, cells });
    for _ in 0..5 {
        run.frame(vec![]);
    }
    // egui's first frame of a window makes each column as wide as its content: the table fills its cell all the same
    let output = run.frame(vec![]);
    let (cell, table) = (run.state.bounds[&20], run.state.bounds[&10]);
    assert!(
        (right(table) - right(cell)).abs() < 12.0,
        "{table:?} in {cell:?}"
    );
    let (scene, backend) = (
        text_center(&output, "Szene"),
        text_center(&output, "Backend"),
    );
    assert!(backend.x - scene.x > 90.0, "{scene:?} {backend:?}");
}

// Phase 5: images, the canvas, the scene and the rest of egui_plot

fn image_node(
    id: u64,
    key: &str,
    width: Option<f32>,
    height: Option<f32>,
    fit: Option<ImageFit>,
) -> UiNode {
    node(
        id,
        Some(1),
        Widget::Image {
            image: key.into(),
            width,
            height,
            fit,
            tint: None,
            corner_radius: None,
            rotation: None,
            description: None,
        },
    )
}

/// A PNG file of one color.
#[cfg(feature = "images")]
fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 30, 255]));
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

#[cfg(feature = "images")]
impl Run {
    /// Frames until `done` (images are decoded on threads of egui_extras), at most a second.
    fn until(&mut self, done: impl Fn(&UiState) -> bool) {
        for _ in 0..200 {
            self.frame(vec![]);
            if done(&self.state) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

/// Every shape of the frame of the kind `pick` says.
fn count_shapes(output: &egui::FullOutput, pick: &dyn Fn(&egui::Shape) -> bool) -> usize {
    fn count(shape: &egui::Shape, pick: &dyn Fn(&egui::Shape) -> bool) -> usize {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().map(|s| count(s, pick)).sum(),
            s => usize::from(pick(s)),
        }
    }
    output.shapes.iter().map(|c| count(&c.shape, pick)).sum()
}

#[cfg(feature = "images")]
#[test]
fn images_come_from_files_and_pixels_in_their_size() {
    let svg = br##"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="#2e7d32"/></svg>"##;
    let mut with_image = button(15, 1, "Speichern");
    let mut image_only = button(16, 1, "");
    for b in [&mut with_image, &mut image_only] {
        if let Widget::Button { image, .. } = &mut b.widget {
            *image = Some("svg".into());
        }
    }
    let nodes = vec![
        central(1),
        image_node(10, "png", None, None, None),
        image_node(11, "svg", Some(100.0), None, None),
        image_node(
            12,
            "pixels",
            Some(40.0),
            Some(40.0),
            Some(ImageFit::Contain),
        ),
        image_node(13, "pixels", Some(40.0), Some(40.0), Some(ImageFit::Cover)),
        image_node(14, "missing", Some(30.0), Some(30.0), None),
        with_image,
        image_only,
        button(17, 1, "Speichern"),
    ];
    let mut run = Run::new(nodes);
    run.state.set_image("png".into(), png(32, 16)).unwrap();
    run.state.set_image("svg".into(), svg.to_vec()).unwrap();
    run.state
        .set_image_rgba("pixels".into(), 4, 2, vec![255; 32])
        .unwrap();
    let size = |s: &UiState, id: u64| (s.bounds[&id].width, s.bounds[&id].height);
    let near =
        |(w, h): (f32, f32), (ew, eh): (f32, f32)| (w - ew).abs() < 1.0 && (h - eh).abs() < 1.0;
    run.until(|s| near(size(s, 10), (32.0, 16.0)) && near(size(s, 11), (100.0, 50.0)));
    // Its own size, a width that gives the height, whole in a square, the square covered, the room of a missing one
    assert!(
        near(size(&run.state, 10), (32.0, 16.0)),
        "{:?}",
        size(&run.state, 10)
    );
    assert!(
        near(size(&run.state, 11), (100.0, 50.0)),
        "{:?}",
        size(&run.state, 11)
    );
    assert!(
        near(size(&run.state, 12), (40.0, 20.0)),
        "{:?}",
        size(&run.state, 12)
    );
    assert!(
        near(size(&run.state, 13), (40.0, 40.0)),
        "{:?}",
        size(&run.state, 13)
    );
    assert!(
        near(size(&run.state, 14), (30.0, 30.0)),
        "{:?}",
        size(&run.state, 14)
    );
    // An image before the text makes the button wider; an image alone makes a button as high as one with text
    assert!(
        size(&run.state, 15).0 > size(&run.state, 17).0 + 10.0,
        "{:?} {:?}",
        size(&run.state, 15),
        size(&run.state, 17)
    );
    assert!(
        size(&run.state, 16).0 < size(&run.state, 17).0
            && (size(&run.state, 16).1 - size(&run.state, 17).1).abs() < 2.0,
        "{:?} {:?}",
        size(&run.state, 16),
        size(&run.state, 17)
    );
    run.click(16);
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 16 }]);
    // Another file under the same key replaces it
    run.state.set_image("png".into(), png(8, 8)).unwrap();
    run.until(|s| near(size(s, 10), (8.0, 8.0)));
    assert!(
        near(size(&run.state, 10), (8.0, 8.0)),
        "{:?}",
        size(&run.state, 10)
    );
    assert!(run.state.forget.is_empty());
    // Bytes that are no image, pixels that do not fill the size
    let why = |r: Result<(), UiError>| r.unwrap_err().to_string();
    assert!(why(run.state.set_image("x".into(), vec![1, 2, 3]))
        .contains("no PNG, JPEG, GIF, WebP or SVG"));
    assert!(why(run.state.set_image_rgba("x".into(), 2, 2, vec![0; 3])).contains("it needs 16"));
}

#[test]
fn a_canvas_draws_in_its_coordinates_and_reports_clicks_and_the_pointer() {
    let thin = CanvasStroke {
        width: 1.0,
        color: 0xFF000000,
        dash: None,
    };
    let canvas = Widget::Canvas {
        shapes: vec![
            // An L: concave, cut into triangles
            CanvasShape::Polygon {
                points: vec![
                    0.0, 0.0, 20.0, 0.0, 20.0, 4.0, 8.0, 4.0, 8.0, 10.0, 0.0, 10.0,
                ],
                fill: Some(0xFF4C9A2A),
                stroke: Some(thin),
            },
            CanvasShape::Line {
                points: vec![0.0, 0.0, 20.0, 10.0],
                stroke: CanvasStroke {
                    dash: Some(3.0),
                    ..thin
                },
            },
            CanvasShape::Circle {
                x: 15.0,
                y: 7.0,
                radius: 1.0,
                fill: Some(0xFFFF0000),
                stroke: None,
            },
            CanvasShape::Ellipse {
                x: 15.0,
                y: 7.0,
                radius_x: 2.0,
                radius_y: 1.0,
                fill: None,
                stroke: Some(thin),
            },
            CanvasShape::Rect {
                x: 12.0,
                y: 6.0,
                width: 6.0,
                height: 3.0,
                fill: None,
                stroke: Some(thin),
                corner_radius: 2.0,
            },
            CanvasShape::Curve {
                points: vec![0.0, 0.0, 5.0, 10.0, 10.0, 0.0],
                closed: false,
                fill: None,
                stroke: Some(thin),
            },
            CanvasShape::Arrow {
                x: 2.0,
                y: 8.0,
                to_x: 6.0,
                to_y: 8.0,
                stroke: thin,
            },
            CanvasShape::Text {
                x: 10.0,
                y: 10.0,
                text: "Nord".into(),
                size: None,
                color: None,
                anchor: Some(Anchor::Top),
                monospace: false,
            },
        ],
        height: 100.0,
        width: Some(200.0),
        view: Some(UiRect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
        }),
        stretch: false,
        y_up: true,
        data: Some("plan".into()),
        clickable: true,
        report_pointer: true,
    };
    let mut run = Run::new(vec![central(1), node(10, Some(1), canvas)]);
    run.state.shapes.insert(
        "plan".into(),
        vec![CanvasShape::Text {
            x: 1.0,
            y: 1.0,
            text: "Plan".into(),
            size: Some(10.0),
            color: Some(0xFF808080),
            anchor: Some(Anchor::BottomLeft),
            monospace: true,
        }],
    );
    let output = run.frame(vec![]);
    let r = run.state.bounds[&10];
    assert!(
        (r.width - 200.0).abs() < 1.0 && (r.height - 100.0).abs() < 1.0,
        "{r:?}"
    );
    assert!(
        texts(&output).contains(&"Plan".to_string()),
        "{:?}",
        texts(&output)
    );
    // y grows upwards: y = 10 is the top, x = 10 the middle of 200 points
    let north = text_center(&output, "Nord");
    assert!(
        (north.x - (r.x + 100.0)).abs() < 1.0 && north.y > r.y && north.y < r.y + 15.0,
        "{north:?} in {r:?}"
    );
    let meshes = count_shapes(&output, &|s| matches!(s, egui::Shape::Mesh(_)));
    assert!(meshes >= 1, "the L is no mesh");
    // A click and the pointer in its coordinates
    run.click_at(egui::pos2(r.x + 50.0, r.y + 80.0));
    let events = run.state.take_events();
    assert!(
        events.contains(&UiEvent::PointClicked {
            id: 10,
            point: UiPoint { x: 5.0, y: 2.0 }
        }),
        "{events:?}"
    );
    run.frame(vec![egui::Event::PointerMoved(egui::pos2(
        r.x + 150.0,
        r.y + 50.0,
    ))]);
    run.frame(vec![egui::Event::PointerMoved(egui::pos2(
        r.x + 150.0,
        r.y + 300.0,
    ))]);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::PointerAt {
            id: 10,
            point: None
        }],
        "the last word on the pointer is that it left"
    );
    run.frame(vec![egui::Event::PointerMoved(egui::pos2(
        r.x + 150.0,
        r.y + 50.0,
    ))]);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::PointerAt {
            id: 10,
            point: Some(UiPoint { x: 15.0, y: 5.0 })
        }]
    );
}

#[test]
fn a_scene_shows_its_nodes_moved_and_zoomed() {
    let scene = Widget::Scene {
        height: 200.0,
        width: Some(300.0),
        min_zoom: Some(0.1),
        max_zoom: Some(4.0),
        view: None,
        report_view: true,
    };
    let mut run = Run::new(vec![
        central(1),
        node(10, Some(1), scene),
        button(11, 10, "Brunnen"),
        button(12, 10, "Säule"),
    ]);
    run.frame(vec![]);
    let seen = |run: &Run, id: u64| run.state.screen_bounds[&id];
    let area = run.state.bounds[&10];
    let fountain = seen(&run, 11);
    assert!(
        area.contains(fountain.x, fountain.y),
        "{fountain:?} in {area:?}"
    );
    // A click where it is seen reaches it
    run.click_at(egui::pos2(
        fountain.x + fountain.width / 2.0,
        fountain.y + fountain.height / 2.0,
    ));
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 11 }]);
    // Kotlin shows a part half as large: everything twice as large
    let laid = run.state.bounds[&11];
    let before = seen(&run, 11).width;
    run.set(
        |w| {
            if let Widget::Scene { view, .. } = w {
                *view = Some(UiRect {
                    x: laid.x,
                    y: laid.y,
                    width: 150.0,
                    height: 100.0,
                })
            }
        },
        10,
    );
    let after = seen(&run, 11).width;
    assert!(
        (after - 2.0 * laid.width).abs() < 2.0,
        "{before} -> {after} ({laid:?})"
    );
    run.state.take_events();
    // The user drags it: Kotlin hears where it went
    let from = egui::pos2(area.x + area.width - 10.0, area.y + area.height - 10.0);
    run.drag(
        from,
        &[from + egui::vec2(-20.0, 0.0), from + egui::vec2(-40.0, 0.0)],
    );
    let events = run.state.take_events();
    assert!(
        matches!(events.as_slice(), [UiEvent::ViewChanged { id: 10, view }] if (view.x - (laid.x + 20.0)).abs() < 1.0),
        "{events:?}"
    );
}

#[test]
fn plots_draw_every_item_and_report_clicks_and_the_pointer() {
    let with = |mut s: PlotSeries, f: &dyn Fn(&mut PlotSeries)| {
        f(&mut s);
        s
    };
    let mut widget = plot(vec![
        series(
            "Mittel",
            line(),
            vec![0.0, 1.0, 2.0, 3.0],
            vec![10.0, 12.0, 11.0, 13.0],
        ),
        with(
            series(
                "Streuung",
                SeriesKind::Area,
                vec![0.0, 1.0, 2.0, 3.0],
                vec![9.0, 10.0, 10.0, 12.0],
            ),
            &|s| s.y2 = vec![11.0, 14.0, 12.0, 14.0],
        ),
        series(
            "Zone",
            SeriesKind::Polygon { width: 1.0 },
            vec![0.0, 1.0, 1.0],
            vec![0.0, 0.0, 5.0],
        ),
        with(
            series(
                "Spitzen",
                SeriesKind::Text {
                    anchor: Some(Anchor::Bottom),
                },
                vec![3.0],
                vec![13.0],
            ),
            &|s| s.labels = vec!["Spitze".into()],
        ),
        with(
            series(
                "Wege",
                SeriesKind::Arrows { tip_length: None },
                vec![0.0],
                vec![1.0],
            ),
            &|s| {
                s.x2 = vec![2.0];
                s.y2 = vec![4.0];
            },
        ),
        series(
            "60 fps",
            SeriesKind::Span { vertical: false },
            vec![],
            vec![0.0, 16.7],
        ),
        series("Wechsel", SeriesKind::VLine, vec![1.5], vec![]),
        series(
            "Kacheln",
            SeriesKind::Heatmap {
                columns: 2,
                palette: vec![0xFF0000FF, 0xFFFF0000],
                min: None,
                max: None,
                show_values: true,
            },
            vec![4.0],
            vec![1.0, 2.0, 3.0, 4.0],
        ),
        with(
            series(
                "Punkte",
                SeriesKind::Points {
                    radius: 3.0,
                    shape: Some(PointShape::Diamond),
                    filled: false,
                },
                vec![1.0],
                vec![12.0],
            ),
            &|s| s.style = Some(PlotLineStyle::Dotted { spacing: 4.0 }),
        ),
    ]);
    if let Widget::Plot {
        clickable,
        report_pointer,
        interactive,
        ..
    } = &mut widget
    {
        *clickable = true;
        *report_pointer = true;
        *interactive = false;
    }
    let mut run = Run::new(vec![panel(), node(2, Some(1), widget)]);
    let output = run.frame(vec![]);
    assert!(
        texts(&output).contains(&"Spitze".to_string()),
        "{:?}",
        texts(&output)
    );
    let r = run.state.bounds[&2];
    let middle = egui::pos2(r.x + r.width / 2.0, r.y + r.height / 2.0);
    run.click_at(middle);
    let events = run.state.take_events();
    let clicked = events.iter().find_map(|e| match e {
        UiEvent::PointClicked { id: 2, point } => Some(*point),
        _ => None,
    });
    let pointer = events.iter().find_map(|e| match e {
        UiEvent::PointerAt { id: 2, point } => Some(*point),
        _ => None,
    });
    let clicked = clicked.unwrap_or_else(|| panic!("no click in {events:?}"));
    // The middle of the values the plot shows: x from about 0 to 4, y from 0 to about 17
    assert!(
        clicked.x > 0.5 && clicked.x < 3.5 && clicked.y > 3.0 && clicked.y < 14.0,
        "{clicked:?}"
    );
    assert_eq!(pointer, Some(Some(clicked)));
}

#[test]
fn images_canvases_scenes_and_plot_items_that_cannot_work_are_refused_with_the_reason() {
    let tree = |widget| vec![panel(), node(2, Some(1), widget)];
    let canvas = |shapes| Widget::Canvas {
        shapes,
        height: 50.0,
        width: None,
        view: None,
        stretch: false,
        y_up: false,
        data: None,
        clickable: false,
        report_pointer: false,
    };
    assert!(invalid(tree(canvas(vec![CanvasShape::Polygon {
        points: vec![0.0, 0.0, 1.0, 1.0],
        fill: None,
        stroke: None,
    }])))
    .contains("Canvas 2: shape 0: a polygon has 2 points, it needs 3"));
    assert!(invalid(tree(canvas(vec![CanvasShape::Line {
        points: vec![0.0, 0.0, 1.0],
        stroke: CanvasStroke {
            width: 1.0,
            color: 0,
            dash: None,
        },
    }])))
    .contains("a line has 3 coordinates, it needs x and y for each point"));
    let mut no_view = canvas(vec![]);
    if let Widget::Canvas { view, .. } = &mut no_view {
        *view = Some(UiRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 5.0,
        });
    }
    assert!(invalid(tree(no_view)).contains("the view is 0 x 5 at 0, 0: it needs a size above 0"));
    let mut flat = image_node(2, "x", Some(0.0), None, None);
    flat.parent = Some(1);
    assert!(invalid(vec![panel(), flat]).contains("Image 2: the width is 0, it needs one above 0"));
    let scene = Widget::Scene {
        height: 100.0,
        width: None,
        min_zoom: Some(2.0),
        max_zoom: Some(1.0),
        view: None,
        report_view: false,
    };
    assert!(invalid(tree(scene)).contains("Scene 2: min_zoom 2 > max_zoom 1"));
    let one = |s| plot(vec![s]);
    assert!(invalid(tree(one(series(
        "Wege",
        SeriesKind::Arrows { tip_length: None },
        vec![0.0],
        vec![1.0]
    ))))
    .contains("the arrows 'Wege' have 1 x and 1 y values, but 0 and 0 for their tips"));
    assert!(invalid(tree(one(series(
        "Kacheln",
        SeriesKind::Heatmap {
            columns: 3,
            palette: vec![],
            min: None,
            max: None,
            show_values: false
        },
        vec![],
        vec![1.0; 4]
    ))))
    .contains("the heatmap 'Kacheln' has 4 values for rows of 3"));
    assert!(invalid(tree(one(series(
        "Zonen",
        SeriesKind::Span { vertical: true },
        vec![1.0],
        vec![]
    ))))
    .contains("the bands 'Zonen' need pairs of x values, they have 1"));
    assert!(invalid(tree(one(series(
        "Bild",
        SeriesKind::Image {
            image: "x".into(),
            width: 1.0,
            height: 1.0
        },
        vec![],
        vec![]
    ))))
    .contains("the image 'Bild' needs one point, it has 0 x and 0 y values"));
}

/// A text that does not take clicks itself (selectable text does).
fn plain_text(id: u64, parent: u64, s: &str) -> UiNode {
    node(
        id,
        Some(parent),
        Widget::Text {
            text: s.into(),
            style: TextStyle::Body,
            color: None,
            spans: vec![],
            wrap: None,
            selectable: false,
        },
    )
}

fn reacting(node: UiNode, reactions: UiReactions) -> UiNode {
    with(
        node,
        UiModifier {
            reactions: Some(reactions),
            ..Default::default()
        },
    )
}

#[test]
fn any_node_reports_clicks_double_clicks_right_clicks_and_long_presses() {
    let card = with(
        node(2, Some(1), column()),
        UiModifier {
            padding: insets(12.0),
            reactions: Some(UiReactions {
                click: true,
                double_click: true,
                secondary_click: true,
                long_press: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let mut run = Run::new(vec![
        central(1),
        card,
        plain_text(3, 2, "Säule"),
        button(4, 2, "Löschen"),
    ]);
    // On its text and in its padding the card takes the click, its button its own
    run.click(3);
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 2 }]);
    let card = run.state.bounds[&2];
    run.click_at(egui::pos2(card.x + 4.0, card.y + 4.0));
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 2 }]);
    run.click(4);
    assert_eq!(run.state.take_events(), vec![UiEvent::Clicked { id: 4 }]);
    // Twice quickly: two clicks, the second also a double click
    run.time += 1.0;
    let at = run.center(3);
    run.frame(vec![egui::Event::PointerMoved(at)]);
    for _ in 0..2 {
        run.frame(vec![press(at, egui::PointerButton::Primary, true)]);
        run.frame(vec![press(at, egui::PointerButton::Primary, false)]);
    }
    assert_eq!(
        run.state.take_events(),
        vec![
            UiEvent::Clicked { id: 2 },
            UiEvent::Clicked { id: 2 },
            UiEvent::DoubleClicked { id: 2 }
        ]
    );
    // The right button, where it was in the card
    run.time += 1.0;
    run.frame(vec![press(at, egui::PointerButton::Secondary, true)]);
    run.frame(vec![press(at, egui::PointerButton::Secondary, false)]);
    match run.state.take_events().as_slice() {
        [UiEvent::SecondaryClicked { id: 2, point }] => assert!(
            (point.x - (at.x - card.x) as f64).abs() < 1.0
                && (point.y - (at.y - card.y) as f64).abs() < 1.0,
            "{point:?}"
        ),
        events => panic!("{events:?}"),
    }
    // Held still for more than half a second: once, and egui asks for the frame when the time is up
    run.time += 1.0;
    let out = run.frame(vec![press(at, egui::PointerButton::Primary, true)]);
    let asked = out.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    assert!(asked.as_secs_f64() < 0.6, "{asked:?}");
    for _ in 0..40 {
        run.frame(vec![]);
    }
    run.frame(vec![press(at, egui::PointerButton::Primary, false)]);
    let events = run.state.take_events();
    assert_eq!(
        events
            .iter()
            .filter(|e| **e == UiEvent::LongPressed { id: 2 })
            .count(),
        1,
        "{events:?}"
    );
}

#[test]
fn nodes_report_the_pointer_dragging_and_gestures_also_with_fingers() {
    let pad = reacting(
        with(
            node(2, Some(1), column()),
            UiModifier {
                width: Some(200.0),
                height: Some(120.0),
                ..Default::default()
            },
        ),
        UiReactions {
            hover: true,
            drag: true,
            gestures: true,
            cursor: Some(crate::UiCursor::Crosshair),
            ..Default::default()
        },
    );
    let mut run = Run::new(vec![central(1), pad, plain_text(3, 2, "Plan")]);
    let r = run.state.bounds[&2];
    let mid = run.center(2);
    let out = run.frame(vec![egui::Event::PointerMoved(mid)]);
    assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::Crosshair);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::HoverChanged {
            id: 2,
            hovered: true
        }]
    );
    // In and out before Kotlin took it: the newer value replaced the older one
    run.frame(vec![egui::Event::PointerGone]);
    run.frame(vec![egui::Event::PointerMoved(mid)]);
    run.frame(vec![egui::Event::PointerMoved(egui::pos2(700.0, 500.0))]);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::HoverChanged {
            id: 2,
            hovered: false
        }]
    );
    // A drag: its start, the moves with how far, its end
    run.drag(
        mid,
        &[mid + egui::vec2(10.0, 0.0), mid + egui::vec2(30.0, 5.0)],
    );
    let drags: Vec<_> = run
        .state
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            UiEvent::Dragged {
                phase,
                dx,
                dy,
                point,
                ..
            } => Some((phase, dx, dy, point)),
            _ => None,
        })
        .collect();
    assert_eq!(
        drags.first().map(|d| d.0),
        Some(DragPhase::Started),
        "{drags:?}"
    );
    assert_eq!(
        drags.last().map(|d| d.0),
        Some(DragPhase::Stopped),
        "{drags:?}"
    );
    let (dx, dy) = drags
        .iter()
        .fold((0.0, 0.0), |(x, y), d| (x + d.1, y + d.2));
    assert!(
        (dx - 30.0).abs() < 0.5 && (dy - 5.0).abs() < 0.5,
        "{drags:?}"
    );
    let end = drags.last().unwrap().3;
    assert!(
        (end.x - (mid.x + 30.0 - r.x) as f64).abs() < 1.0,
        "{end:?} in {r:?}"
    );
    // Ctrl and the wheel zoom
    run.frame(vec![
        egui::Event::PointerMoved(mid),
        egui::Event::Zoom(1.25),
    ]);
    let events = run.state.take_events();
    assert!(
        events.iter().any(
            |e| matches!(e, UiEvent::Gesture { id: 2, zoom, .. } if (zoom - 1.25).abs() < 1e-4)
        ),
        "{events:?}"
    );
    // Two fingers move apart from 20 to 40 points: it zooms in (egui counts from the frame after both are down)
    let mut held = crate::input::Held::default();
    let mut wanted = crate::input::Wanted::default();
    let mut touch = |run: &mut Run, fingers: &[(u64, crate::UiTouchPhase, f32)]| {
        let events = fingers
            .iter()
            .flat_map(|&(id, phase, x)| {
                held.events(
                    UiInput::Touch {
                        id,
                        phase,
                        x: mid.x + x,
                        y: mid.y,
                        force: None,
                    },
                    1.0,
                    &mut wanted,
                )
            })
            .collect();
        run.frame(events);
    };
    use crate::UiTouchPhase::{End, Move, Start};
    touch(&mut run, &[(1, Start, -10.0), (2, Start, 10.0)]);
    for step in 1..=5 {
        let d = 10.0 + 2.0 * step as f32;
        touch(&mut run, &[(1, Move, -d), (2, Move, d)]);
    }
    touch(&mut run, &[(1, End, -20.0), (2, End, 20.0)]);
    let events = run.state.take_events();
    let zoom: f32 = events
        .iter()
        .filter_map(|e| match e {
            UiEvent::Gesture { zoom, .. } => Some(*zoom),
            _ => None,
        })
        .product();
    assert!(zoom > 1.5 && zoom < 2.05, "{zoom}");
    // Two fingers do not drag it, though egui's pointer follows the first one
    assert!(
        !events.iter().any(|e| matches!(e, UiEvent::Dragged { .. })),
        "{events:?}"
    );
}

#[test]
fn a_node_dragged_onto_a_list_that_takes_its_kind_is_dropped_before_a_place() {
    let source = |id, s| {
        reacting(
            plain_text(id, 2, s),
            UiReactions {
                drag_kind: Some("zeile".into()),
                ..Default::default()
            },
        )
    };
    let mut run = Run::new(vec![
        central(1),
        reacting(
            node(2, Some(1), column()),
            UiReactions {
                drop_kinds: Some(vec!["zeile".into()]),
                ..Default::default()
            },
        ),
        source(3, "Säule"),
        source(4, "Brunnen"),
        source(5, "Dach"),
        reacting(
            with(
                node(6, Some(1), column()),
                UiModifier {
                    height: Some(80.0),
                    width: Some(200.0),
                    ..Default::default()
                },
            ),
            UiReactions {
                drop_kinds: Some(vec!["material".into()]),
                ..Default::default()
            },
        ),
        plain_text(7, 6, "Materialien"),
    ]);
    let drag = |run: &mut Run, to: egui::Pos2| {
        let from = run.center(3);
        let steps: Vec<_> = (1..=6)
            .map(|i| from + (to - from) * (i as f32 / 6.0))
            .collect();
        run.drag(from, &steps);
        run.frame(vec![]);
    };
    // Below the last row: it goes to the end
    let last = run.state.bounds[&5];
    drag(
        &mut run,
        egui::pos2(last.x + 10.0, last.y + last.height - 2.0),
    );
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Dropped {
            id: 2,
            source: 3,
            index: Some(3)
        }]
    );
    // Between the second and the third
    let third = run.state.bounds[&5];
    drag(&mut run, egui::pos2(third.x + 10.0, third.y + 1.0));
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Dropped {
            id: 2,
            source: 3,
            index: Some(2)
        }]
    );
    // A list of another kind does not take it
    let other = run.center(6);
    drag(&mut run, other);
    assert_eq!(run.state.take_events(), vec![]);
}

#[test]
fn selectable_text_in_a_dragged_node_drags_the_node_and_its_clicks_stay_with_it() {
    // Cards of a list as in the outliner: selectable text inside, dragged onto the list, opened with a double click
    let card = |id| {
        reacting(
            node(id, Some(2), row()),
            UiReactions {
                double_click: true,
                drag_kind: Some("objekt".into()),
                ..Default::default()
            },
        )
    };
    let pad = reacting(
        with(
            node(9, Some(1), column()),
            UiModifier {
                width: Some(200.0),
                height: Some(60.0),
                ..Default::default()
            },
        ),
        UiReactions {
            drag: true,
            ..Default::default()
        },
    );
    let mut run = Run::new(vec![
        central(1),
        reacting(
            node(2, Some(1), column()),
            UiReactions {
                drop_kinds: Some(vec!["objekt".into()]),
                ..Default::default()
            },
        ),
        card(3),
        text(4, 3, "Brunnen"),
        card(5),
        text(6, 5, "Rasen"),
        pad,
        text(10, 9, "Drehung 0°"),
    ]);
    let from = run.center(4);
    let last = run.state.bounds[&5];
    let to = egui::pos2(from.x + 10.0, last.y + last.height - 2.0);
    let steps: Vec<_> = (1..=6)
        .map(|i| from + (to - from) * (i as f32 / 6.0))
        .collect();
    run.drag(from, &steps);
    run.frame(vec![]);
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::Dropped {
            id: 2,
            source: 3,
            index: Some(2)
        }]
    );
    run.time += 1.0;
    let at = run.center(6);
    run.frame(vec![egui::Event::PointerMoved(at)]);
    for _ in 0..2 {
        run.frame(vec![press(at, egui::PointerButton::Primary, true)]);
        run.frame(vec![press(at, egui::PointerButton::Primary, false)]);
    }
    assert_eq!(
        run.state.take_events(),
        vec![UiEvent::DoubleClicked { id: 5 }]
    );
    // The pad turns when its text is dragged
    let from = run.center(10);
    run.drag(
        from,
        &[from + egui::vec2(30.0, 0.0), from + egui::vec2(60.0, 0.0)],
    );
    let events = run.state.take_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            UiEvent::Dragged {
                id: 9,
                phase: DragPhase::Started,
                ..
            }
        )),
        "{events:?}"
    );
    let moved: f32 = events
        .iter()
        .filter_map(|e| match e {
            UiEvent::Dragged { id: 9, dx, .. } => Some(*dx),
            _ => None,
        })
        .sum();
    assert!((moved - 60.0).abs() < 1.0, "{events:?}");
}

#[test]
fn a_finger_held_still_is_one_long_press_or_a_right_click() {
    let mut run = Run::new(vec![
        central(1),
        reacting(
            with(
                node(2, Some(1), column()),
                UiModifier {
                    width: Some(200.0),
                    height: Some(60.0),
                    ..Default::default()
                },
            ),
            UiReactions {
                long_press: true,
                secondary_click: true,
                ..Default::default()
            },
        ),
        plain_text(3, 2, "Vorschau"),
        reacting(
            plain_text(4, 1, "Brunnen"),
            UiReactions {
                secondary_click: true,
                ..Default::default()
            },
        ),
    ]);
    let mut held = crate::input::Held::default();
    let mut wanted = crate::input::Wanted::default();
    // A second, past egui's own long touch (0.8 s) that takes the press
    let mut hold = |run: &mut Run, at: egui::Pos2| {
        let mut finger = |run: &mut Run, phase| {
            let input = UiInput::Touch {
                id: 7,
                phase,
                x: at.x,
                y: at.y,
                force: None,
            };
            let events = held.events(input, 1.0, &mut wanted);
            run.frame(events)
        };
        finger(run, crate::UiTouchPhase::Start);
        for _ in 0..60 {
            run.frame(vec![]);
        }
        finger(run, crate::UiTouchPhase::End);
        run.frame(vec![]);
        run.state.take_events()
    };
    let at = run.center(2);
    assert_eq!(hold(&mut run, at), vec![UiEvent::LongPressed { id: 2 }]);
    let at = run.center(4);
    match hold(&mut run, at).as_slice() {
        [UiEvent::SecondaryClicked { id: 4, .. }] => {}
        events => panic!("{events:?}"),
    }
}

#[test]
fn a_node_of_its_own_theme_has_its_text_sizes_and_egui_s_panels_are_nodes() {
    let themed = with(
        node(2, Some(1), column()),
        UiModifier {
            theme: Some(UiTheme {
                text_sizes: vec![UiTextSize {
                    style: TextStyle::Body,
                    size: 25.0,
                    family: None,
                }],
                // A family without fonts is the text font
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let tool = |id, tool| node(id, Some(1), Widget::EguiTool { tool });
    let mut run = Run::new(vec![
        central(1),
        plain_text(3, 1, "Säule"),
        themed,
        plain_text(4, 2, "Säule"),
        tool(5, EguiTool::Settings),
        tool(6, EguiTool::Style),
        tool(7, EguiTool::Inspection),
        tool(8, EguiTool::Memory),
        tool(9, EguiTool::Textures),
    ]);
    run.frame(vec![]);
    let (plain, large) = (run.state.bounds[&3], run.state.bounds[&4]);
    assert!(large.height > plain.height * 1.6, "{plain:?} {large:?}");
    for id in 5..=9 {
        let r = run.state.bounds[&id];
        assert!(r.height > 10.0, "{id}: {r:?}");
    }
    // Fonts egui could not read are refused before egui takes them
    let font = |name: &str, bytes: Vec<u8>| UiFont {
        name: name.into(),
        bytes,
        family: None,
        fallback: false,
        scale: None,
        y_offset: None,
    };
    let hack = epaint_default_fonts::HACK_REGULAR.to_vec();
    assert_eq!(
        crate::theme::check_fonts(&[font("Hack", hack.clone())]),
        Ok(())
    );
    for (fonts, why) in [
        (vec![font("", hack.clone())], "a font needs a name"),
        (
            vec![font("Hack", hack.clone()), font("Hack", hack.clone())],
            "two fonts are called 'Hack'",
        ),
    ] {
        assert_eq!(crate::theme::check_fonts(&fonts), Err(why.to_string()));
    }
    assert!(crate::theme::check_fonts(&[font("Kaputt", vec![0; 64])])
        .unwrap_err()
        .starts_with("'Kaputt' is no TTF or OTF file"));
}

/// The accessibility tree of the next frame, with egui's AccessKit on.
fn access_tree(run: &mut Run, events: Vec<egui::Event>) -> UiAccessTree {
    run.state.access = true;
    run.egui.enable_accesskit();
    let output = run.frame(events);
    let update = output
        .platform_output
        .accesskit_update
        .expect("AccessKit is on");
    crate::access::tree(&update, 2.0, &run.state)
}

fn access_nodes() -> Vec<UiNode> {
    let semantics = |s: UiSemantics| {
        Some(UiModifier {
            semantics: Some(s),
            ..Default::default()
        })
    };
    let heading = |id, s: &str| {
        node(
            id,
            Some(1),
            Widget::Text {
                text: s.into(),
                style: TextStyle::Heading,
                color: None,
                spans: vec![],
                wrap: None,
                selectable: true,
            },
        )
    };
    vec![
        panel(),
        heading(2, "Licht"),
        checkbox(3, true),
        node(
            4,
            Some(1),
            Widget::SliderFloat {
                label: "Stärke".into(),
                value: 0.5,
                min: 0.0,
                max: 1.0,
                logarithmic: false,
                decimals: Some(1),
                suffix: String::new(),
                prefix: String::new(),
                step: Some(0.1),
                vertical: false,
                clamping: None,
                show_value: true,
                trailing_fill: None,
                handle: None,
            },
        ),
        node(
            5,
            Some(1),
            Widget::TextField {
                label: "Name".into(),
                value: "Laterne".into(),
                hint: "Wie sie heißt".into(),
                password: false,
                lines: None,
                code: false,
                language: None,
                char_limit: None,
                align: None,
                read_only: false,
            },
        ),
        node(
            6,
            Some(1),
            Widget::Switch {
                text: "Sonne".into(),
                checked: true,
            },
        ),
        UiNode {
            modifier: semantics(UiSemantics {
                label: Some("Fenster schließen".into()),
                ..Default::default()
            }),
            ..node(
                7,
                Some(1),
                Widget::Button {
                    text: "✖".into(),
                    small: false,
                    frame: true,
                    selected: false,
                    shortcut: None,
                    fill: None,
                    image: None,
                    image_size: None,
                },
            )
        },
        UiNode {
            modifier: semantics(UiSemantics {
                merge: true,
                ..Default::default()
            }),
            ..node(8, Some(1), column())
        },
        text(9, 8, "Laternen"),
        text(10, 8, "12 Objekte"),
        UiNode {
            modifier: semantics(UiSemantics {
                hidden: true,
                ..Default::default()
            }),
            ..text(11, 1, "Deko")
        },
        UiNode {
            modifier: semantics(UiSemantics {
                role: Some(AccessRole::Tab),
                selected: Some(true),
                state: Some("offen".into()),
                live: Some(AccessLive::Polite),
                description: Some("Reiter der Ansicht".into()),
                ..Default::default()
            }),
            ..node(
                12,
                Some(1),
                Widget::SelectableLabel {
                    text: "Ansicht".into(),
                    selected: true,
                },
            )
        },
    ]
}

#[test]
fn the_accessibility_tree_has_names_roles_values_and_the_nodes_of_kotlin() {
    let mut run = Run::new(access_nodes());
    let tree = access_tree(&mut run, vec![]);
    let find = |label: &str| {
        tree.nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("no node '{label}' in {:#?}", tree.nodes))
    };
    // The layout's containers are left out: the panel holds the widgets directly
    let panel = tree
        .nodes
        .iter()
        .find(|n| n.role == AccessRole::Pane)
        .unwrap_or_else(|| panic!("no panel in {:#?}", tree.nodes));
    let heading = find("Licht");
    assert_eq!(
        (heading.role, heading.node, heading.parent),
        (AccessRole::Heading, Some(2), Some(panel.id))
    );
    // In pixels: twice the points
    let points = run.state.bounds[&2];
    assert!(
        (heading.bounds.width - 2.0 * points.width).abs() < 1.0,
        "{heading:?} {points:?}"
    );
    let shadows = find("Shadows");
    assert_eq!(
        (shadows.role, shadows.toggled, shadows.node),
        (AccessRole::CheckBox, Some(AccessToggle::On), Some(3))
    );
    assert!(shadows.actions.contains(&AccessAction::Click));
    let slider = tree.nodes.iter().find(|n| n.node == Some(4)).unwrap();
    assert_eq!(
        (slider.role, slider.numeric, slider.min, slider.max),
        (AccessRole::Slider, Some(0.5), Some(0.0), Some(1.0))
    );
    assert!(
        [
            AccessAction::Increment,
            AccessAction::Decrement,
            AccessAction::SetValue
        ]
        .iter()
        .all(|a| slider.actions.contains(a)),
        "{slider:?}"
    );
    // The label of a field is its name, not a text of its own
    let name = find("Name");
    assert_eq!(
        (
            name.role,
            name.value.as_deref(),
            name.placeholder.as_deref(),
            name.node
        ),
        (
            AccessRole::TextInput,
            Some("Laterne"),
            Some("Wie sie heißt"),
            Some(5)
        )
    );
    assert!(name.actions.contains(&AccessAction::SetValue));
    assert_eq!(
        tree.nodes
            .iter()
            .filter(|n| n.label.as_deref() == Some("Name"))
            .count(),
        1
    );
    let sun = find("Sonne");
    assert_eq!(
        (sun.role, sun.toggled),
        (AccessRole::Switch, Some(AccessToggle::On))
    );
    // Semantics from Kotlin: a name instead of the text, one node for a card, nothing for decoration
    assert_eq!(find("Fenster schließen").role, AccessRole::Button);
    let card = find("Laternen, 12 Objekte");
    assert_eq!((card.role, card.node), (AccessRole::Group, Some(8)));
    // A container of egui's layout has no place of its own: the card is where its texts are
    let (texts, at) = (run.state.bounds[&10], card.bounds);
    assert!(
        at.height > 0.0 && at.y + at.height >= 2.0 * (texts.y + texts.height) - 1.0,
        "{at:?} {texts:?}"
    );
    assert!(tree
        .nodes
        .iter()
        .all(|n| n.label.as_deref() != Some("Laternen")
            && n.label.as_deref() != Some("Deko")
            && n.parent != Some(card.id)));
    let tab = find("Ansicht");
    assert_eq!(
        (
            tab.role,
            tab.selected,
            tab.state.as_deref(),
            tab.live,
            tab.description.as_deref()
        ),
        (
            AccessRole::Tab,
            Some(true),
            Some("offen"),
            Some(AccessLive::Polite),
            Some("Reiter der Ansicht")
        )
    );
    // Each node after the one it is in
    for (i, n) in tree.nodes.iter().enumerate() {
        if let Some(parent) = n.parent {
            assert!(tree.nodes[..i].iter().any(|p| p.id == parent), "{n:?}");
        }
    }
    assert_eq!(tree.focus, None);
}

#[test]
fn a_screen_reader_clicks_sets_values_types_and_focuses_through_access_actions() {
    let mut run = Run::new(access_nodes());
    let tree = access_tree(&mut run, vec![]);
    let id = |node: u64| tree.nodes.iter().find(|n| n.node == Some(node)).unwrap().id;
    let request = |node, action, data| crate::access::request(id(node), action, data);
    access_tree(&mut run, vec![request(3, AccessAction::Click, None)]);
    access_tree(
        &mut run,
        vec![request(
            4,
            AccessAction::SetValue,
            Some(egui::accesskit::ActionData::NumericValue(0.8)),
        )],
    );
    access_tree(
        &mut run,
        vec![request(
            5,
            AccessAction::SetValue,
            Some(egui::accesskit::ActionData::Value("Fackel".into())),
        )],
    );
    access_tree(&mut run, vec![request(6, AccessAction::Click, None)]);
    let events = run.state.take_events();
    assert_eq!(
        events,
        vec![
            UiEvent::Checked {
                id: 3,
                checked: false
            },
            UiEvent::FloatChanged { id: 4, value: 0.8 },
            UiEvent::TextChanged {
                id: 5,
                value: "Fackel".into()
            },
            UiEvent::Checked {
                id: 6,
                checked: false
            },
        ]
    );
    // A step of the slider, and the keyboard to the field
    access_tree(&mut run, vec![request(4, AccessAction::Increment, None)]);
    let after = access_tree(&mut run, vec![request(5, AccessAction::Focus, None)]);
    let after = access_tree(&mut run, vec![])
        .nodes
        .into_iter()
        .chain(after.nodes)
        .collect::<Vec<_>>();
    assert!(matches!(
        run.state.take_events().as_slice(),
        [UiEvent::FloatChanged { id: 4, value }] if (*value - 0.9).abs() < 1e-9
    ));
    let field = after.iter().find(|n| n.node == Some(5)).unwrap();
    assert!(field.focused, "{field:?}");
    assert_eq!(field.value.as_deref(), Some("Fackel"));
}

#[test]
fn every_widget_is_described_with_a_role_and_egui_s_layout_is_left_out() {
    let mut run = Run::new(every_widget());
    let tree = access_tree(&mut run, vec![]);
    let of = |node: u64| {
        tree.nodes
            .iter()
            .find(|n| n.node == Some(node))
            .unwrap_or_else(|| panic!("no access node for {node} in {:#?}", tree.nodes))
    };
    let described = |node: u64| (of(node).role, of(node).label.as_deref());
    assert_eq!(described(105), (AccessRole::SpinButton, Some("Angle")));
    assert_eq!(described(106), (AccessRole::ComboBox, Some("Scene")));
    assert_eq!(of(106).value.as_deref(), Some("garden"));
    assert_eq!(
        described(109),
        (AccessRole::Status, Some("Takes effect after a restart"))
    );
    assert_eq!(of(109).live, Some(AccessLive::Polite));
    assert_eq!(described(110), (AccessRole::Alert, Some("Failed")));
    assert_eq!(of(117).role, AccessRole::Canvas);
    assert_eq!(of(107).numeric, Some(40.0));
    // Texts are read, not clicked; no node of egui's layout is left
    for n in &tree.nodes {
        assert_ne!(n.role, AccessRole::Unknown, "{n:?}");
        if n.role == AccessRole::Label {
            assert!(n.actions.is_empty(), "{n:?}");
        }
    }
    // The bars and the panel are panes at the top
    assert!([1, 10, 20]
        .iter()
        .all(|&p| of(p).role == AccessRole::Pane && of(p).parent.is_none()));
}
