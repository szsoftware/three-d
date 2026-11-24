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
}

impl Run {
    fn new(nodes: Vec<UiNode>) -> Self {
        let mut state = UiState::new();
        state.set_tree(nodes).unwrap();
        let mut run = Self {
            egui: egui::Context::default(),
            state,
            time: 0.0,
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
                movable: true,
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

#[test]
fn every_widget_draws() {
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
                SeriesKind::Points { radius: 2.0 },
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
                movable: true,
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
    // The keys of the overlay: Control with C and X copy and cut, with V the overlay asks for the clipboard
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
    press(&mut held, "Control", true);
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
