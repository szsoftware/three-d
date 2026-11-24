//! The tree, the values and the events with egui alone, without a graphics card: egui is run with made-up pointer events.

use crate::state::UiState;
use crate::*;
use three_d::egui;

fn node(id: u64, parent: Option<u64>, widget: Widget) -> UiNode {
    UiNode {
        id,
        parent,
        widget,
        enabled: true,
        tooltip: None,
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
        node(2, Some(3), Widget::Row),
        node(3, Some(2), Widget::Row)
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
                suffix: String::new()
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
                striped: false
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
                striped: true
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
            },
        ),
        node(5, Some(1), Widget::Row),
        node(
            6,
            Some(5),
            Widget::Button {
                text: "Cancel".into(),
            },
        ),
        node(
            7,
            Some(5),
            Widget::Button {
                text: "Cancel".into(),
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
    let mut button = node(2, Some(1), Widget::Button { text: "GPU".into() });
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
        node(10, None, Widget::TopBar),
        node(
            11,
            Some(10),
            Widget::Button {
                text: "File".into(),
            },
        ),
        node(20, None, Widget::BottomBar),
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
            },
        ),
    ];
    let widgets = vec![
        Widget::Text {
            text: "Heading".into(),
            style: TextStyle::Heading,
            color: None,
        },
        Widget::Separator,
        Widget::Spacer { size: 4.0 },
        Widget::SliderInt {
            label: "Samples".into(),
            value: 4,
            min: 1,
            max: 64,
        },
        Widget::SliderFloat {
            label: "Haze".into(),
            value: 0.01,
            min: 0.001,
            max: 1.0,
            logarithmic: true,
            decimals: Some(3),
            suffix: " /m".into(),
        },
        Widget::DragValue {
            label: "Angle".into(),
            value: 45.0,
            speed: 0.5,
            min: None,
            max: Some(90.0),
            decimals: Some(1),
            suffix: " °".into(),
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
        },
        Widget::Group,
        Widget::Collapsible {
            title: "More".into(),
            open: true,
        },
        Widget::ScrollArea {
            max_height: Some(50.0),
        },
        Widget::Grid {
            columns: 2,
            striped: true,
        },
        Widget::Column,
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
