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

    fn frame(&mut self, events: Vec<egui::Event>) {
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
        let _ = self.egui.run_ui(input, |ui| crate::draw::draw(ui, state));
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
