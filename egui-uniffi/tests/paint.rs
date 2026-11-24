//!
//! The overlay painted over a three-d picture on the graphics card, on OpenGL (through EGL, Linux) and on wgpu, used the
//! way Kotlin uses it: pixels from the top left, events through `input`, what the user did through `take_events`.
//!

use std::sync::Arc;
use three_d::*;
use three_d_egui_uniffi::*;

const W: u32 = 400;
const H: u32 = 300;

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

fn tree(checked: bool) -> Vec<UiNode> {
    vec![
        node(
            1,
            None,
            Widget::SidePanel {
                side: Side::Left,
                title: Some("Options".into()),
                width: 150.0,
                resizable: false,
                min_width: None,
                max_width: None,
                expanded: true,
            },
        ),
        node(
            2,
            Some(1),
            Widget::Checkbox {
                text: "Shadows".into(),
                checked,
                indeterminate: false,
            },
        ),
        node(
            3,
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
    ]
}

/// The contexts with a screen of `W` x `H` this machine has, by name.
fn contexts() -> Vec<(&'static str, Context)> {
    if std::env::var_os("THREE_D_SKIP_GPU_TESTS").is_some() {
        return Vec::new();
    }
    let mut all = Vec::new();
    #[cfg(target_os = "linux")]
    match Context::new_headless_gl_with_screen(W, H, false, false) {
        Ok(c) => all.push(("OpenGL", c)),
        Err(e) => eprintln!("no OpenGL through EGL: {e}"),
    }
    #[cfg(feature = "wgpu")]
    match Context::new_headless_wgpu_with_screen(W, H, false, false) {
        Ok(c) => all.push(("wgpu", c)),
        Err(e) if std::env::var_os("THREE_D_REQUIRE_WGPU").is_some() => {
            panic!("no wgpu adapter: {e}")
        }
        Err(e) => eprintln!("no wgpu adapter: {e}"),
    }
    all
}

/// The picture: red, the overlay over it. Rows from the top.
fn frame(context: &Context, overlay: &EguiOverlay) -> Vec<[u8; 4]> {
    let screen = RenderTarget::screen(context, W, H);
    screen.clear(ClearState::color(1.0, 0.0, 0.0, 1.0));
    overlay
        .paint(context, &screen, Viewport::new_at_origo(W, H))
        .unwrap();
    screen.read_color()
}

fn pixel(picture: &[[u8; 4]], x: u32, y: u32) -> [u8; 4] {
    picture[(y * W + x) as usize]
}

fn near(a: [u8; 4], b: [u8; 3], by: u8) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= by)
}

fn release(context: Context) {
    context.programs.write().unwrap().clear();
}

fn center(overlay: &EguiOverlay, id: u64) -> (f32, f32) {
    let r = overlay.bounds(id).expect("drawn");
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

/// A click as Compose reports it, with a frame after each event.
fn click(context: &Context, overlay: &EguiOverlay, (x, y): (f32, f32)) -> [bool; 3] {
    let moved = overlay.input(UiInput::Move { x, y });
    frame(context, overlay);
    let pressed = overlay.input(UiInput::Press { x, y, button: 0 });
    frame(context, overlay);
    let released = overlay.input(UiInput::Release { x, y, button: 0 });
    frame(context, overlay);
    [moved, pressed, released]
}

#[test]
fn the_panel_lies_over_the_picture_and_the_rest_stays() {
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(tree(false)).unwrap();
        frame(&context, &overlay);
        let picture = frame(&context, &overlay);
        let panel = overlay.bounds(1).unwrap();
        assert!(
            (140.0..=170.0).contains(&panel.width) && panel.height == H as f32,
            "{name}: {panel:?}"
        );
        // egui's dark panel (gray 27) on the left, the red picture on the right
        assert!(
            near(pixel(&picture, 140, 280), [27, 27, 27], 3),
            "{name}: {:?}",
            pixel(&picture, 140, 280)
        );
        assert!(
            near(pixel(&picture, 300, 150), [255, 0, 0], 0),
            "{name}: {:?}",
            pixel(&picture, 300, 150)
        );
        // Something light is written in the panel: the heading
        let heading = overlay.bounds(2).unwrap();
        let text = (0..heading.y as u32)
            .flat_map(|y| (0..panel.width as u32).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel(&picture, x, y)[0] > 120)
            .count();
        assert!(text > 30, "{name}: {text} light pixels above the checkbox");
        drop(overlay);
        release(context);
    }
}

#[test]
fn clicks_on_the_panel_belong_to_egui_and_the_rest_to_the_application() {
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(tree(false)).unwrap();
        frame(&context, &overlay);
        frame(&context, &overlay);
        assert_eq!(
            click(&context, &overlay, center(&overlay, 2)),
            [true; 3],
            "{name}"
        );
        assert_eq!(
            overlay.take_events(),
            vec![UiEvent::Checked {
                id: 2,
                checked: true
            }],
            "{name}"
        );
        // Kotlin took it and sends its state; then the button
        overlay.set_tree(tree(true)).unwrap();
        assert_eq!(
            click(&context, &overlay, center(&overlay, 3)),
            [true; 3],
            "{name}"
        );
        assert_eq!(
            overlay.take_events(),
            vec![UiEvent::Clicked { id: 3 }],
            "{name}"
        );
        // Beside the panel: the camera's
        assert_eq!(
            click(&context, &overlay, (300.0, 150.0)),
            [false; 3],
            "{name}"
        );
        // A drag of the camera that passes over the panel stays the camera's, a drag that starts on the panel egui's
        assert!(!overlay.input(UiInput::Press {
            x: 300.0,
            y: 150.0,
            button: 0
        }));
        assert!(!overlay.input(UiInput::Move { x: 50.0, y: 150.0 }));
        assert!(!overlay.input(UiInput::Release {
            x: 50.0,
            y: 150.0,
            button: 0
        }));
        assert!(overlay.input(UiInput::Press {
            x: 50.0,
            y: 150.0,
            button: 0
        }));
        assert!(overlay.input(UiInput::Move { x: 300.0, y: 150.0 }));
        assert!(overlay.input(UiInput::Release {
            x: 300.0,
            y: 150.0,
            button: 0
        }));
        frame(&context, &overlay);
        assert!(overlay.covers(50.0, 150.0) && !overlay.covers(300.0, 150.0));
        // Keys go to the application while no text field has the focus
        assert!(!overlay.input(UiInput::Key {
            key: "W".into(),
            down: true
        }));
        drop(overlay);
        release(context);
    }
}

#[test]
fn the_scale_makes_the_panel_wider_in_pixels() {
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(tree(false)).unwrap();
        frame(&context, &overlay);
        let one = overlay.bounds(1).unwrap().width;
        overlay.set_scale(1.5);
        frame(&context, &overlay);
        frame(&context, &overlay);
        let wide = overlay.bounds(1).unwrap().width;
        assert!((wide / one - 1.5).abs() < 0.05, "{name}: {one} -> {wide}");
        // The checkbox is still hit where bounds says it is
        assert_eq!(
            click(&context, &overlay, center(&overlay, 2)),
            [true; 3],
            "{name}"
        );
        assert_eq!(overlay.take_events().len(), 1, "{name}");
        drop(overlay);
        release(context);
    }
}

#[test]
fn light_style_and_accent_color() {
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_style(UiStyle {
            dark: false,
            accent: Some(0xFF00A040),
        });
        let mut nodes = tree(true);
        nodes.push(node(
            4,
            Some(1),
            Widget::ProgressBar {
                progress: 1.0,
                text: Some(String::new()),
            },
        ));
        overlay.set_tree(nodes).unwrap();
        frame(&context, &overlay);
        let picture = frame(&context, &overlay);
        // egui's light panel is gray 248, the full progress bar has the accent color
        assert!(
            near(pixel(&picture, 140, 280), [248, 248, 248], 3),
            "{name}: {:?}",
            pixel(&picture, 140, 280)
        );
        let (x, y) = center(&overlay, 4);
        assert!(
            near(pixel(&picture, x as u32, y as u32), [0, 160, 64], 4),
            "{name}: {:?}",
            pixel(&picture, x as u32, y as u32)
        );
        drop(overlay);
        release(context);
    }
}

#[test]
fn an_overlay_without_changes_asks_for_no_frames() {
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        assert!(overlay.needs_repaint());
        overlay.set_tree(tree(false)).unwrap();
        for _ in 0..5 {
            frame(&context, &overlay);
        }
        assert!(!overlay.needs_repaint(), "{name}");
        overlay.set_tree(tree(false)).unwrap();
        assert!(
            !overlay.needs_repaint(),
            "{name}: the same tree again changes nothing"
        );
        overlay.set_tree(tree(true)).unwrap();
        assert!(overlay.needs_repaint(), "{name}");
        frame(&context, &overlay);
        overlay.input(UiInput::Move { x: 10.0, y: 10.0 });
        assert!(overlay.needs_repaint(), "{name}");
        drop(overlay);
        release(context);
    }
}

/// A frame without changes paints egui's meshes of the frame before, without running egui: the same picture, also on a
/// new picture under it, and the next click works as before.
#[test]
fn an_unchanged_panel_is_painted_again() {
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(tree(false)).unwrap();
        for _ in 0..5 {
            frame(&context, &overlay);
        }
        assert!(!overlay.needs_repaint(), "{name}");
        let ran = frame(&context, &overlay);
        let again = frame(&context, &overlay);
        assert!(ran == again, "{name}: the painted again frame differs");
        assert!(near(pixel(&again, 300, 150), [255, 0, 0], 0), "{name}");
        assert!(
            !near(pixel(&again, 20, 150), [255, 0, 0], 40),
            "{name}: no panel"
        );

        let before = frame(&context, &overlay);
        assert_eq!(
            click(&context, &overlay, center(&overlay, 2)),
            [true, true, true],
            "{name}"
        );
        assert_eq!(
            overlay.take_events(),
            vec![UiEvent::Checked {
                id: 2,
                checked: true
            }],
            "{name}"
        );
        // The checked box shows, also in the frames after, when egui rests again
        let checked = frame(&context, &overlay);
        assert!(checked != before, "{name}: the click does not show");
        for _ in 0..5 {
            frame(&context, &overlay);
        }
        assert!(frame(&context, &overlay) == checked, "{name}");
        drop(overlay);
        release(context);
    }
}

#[test]
fn an_overlay_moves_to_a_new_context() {
    let first = contexts();
    let overlay: Arc<EguiOverlay> = EguiOverlay::new();
    overlay.set_tree(tree(false)).unwrap();
    for (_, context) in &first {
        frame(context, &overlay);
        assert!(overlay.bounds(1).is_some());
    }
    drop(overlay);
    for (_, context) in first {
        release(context);
    }
}

/// A panel as the POC's raytrace window has it, with a frame time in the corner. With `THREE_D_EGUI_PICTURE=<dir>` it
/// writes the pictures as raw RGBA (`<dir>/<backend>.rgba`, `W` x `H`) for a look.
#[test]
fn an_options_panel_as_in_the_raytrace_window() {
    let text = |t: &str| Widget::Text {
        text: t.into(),
        style: TextStyle::Body,
        color: None,
        spans: vec![],
        wrap: None,
        selectable: true,
    };
    let mut gpu = node(
        5,
        Some(1),
        Widget::Radio {
            options: vec!["Auto".into(), "GPU".into(), "CPU".into()],
            selected: 2,
            horizontal: true,
        },
    );
    gpu.enabled = false;
    gpu.tooltip = Some("no compute shaders on this device".into());
    let nodes = vec![
        node(
            1,
            None,
            Widget::SidePanel {
                side: Side::Left,
                title: Some("Raytrace".into()),
                width: 180.0,
                resizable: true,
                min_width: None,
                max_width: None,
                expanded: true,
            },
        ),
        node(
            2,
            Some(1),
            Widget::ComboBox {
                label: "Scene".into(),
                options: vec!["Cloister".into(), "Garden".into(), "Pergola".into()],
                selected: 0,
                width: Some(100.0),
            },
        ),
        node(
            3,
            Some(1),
            Widget::Checkbox {
                text: "Denoise".into(),
                checked: true,
                indeterminate: false,
            },
        ),
        node(4, Some(1), text("Tracer")),
        gpu,
        node(
            6,
            Some(1),
            Widget::SliderInt {
                label: "Downscale".into(),
                value: 2,
                min: 1,
                max: 8,
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
        ),
        node(7, Some(1), Widget::Separator),
        node(8, Some(1), row()),
        node(
            9,
            Some(8),
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
            10,
            Some(8),
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
            11,
            Some(1),
            Widget::ProgressBar {
                progress: 0.62,
                text: Some("62 %, 14 s left".into()),
            },
        ),
        node(
            12,
            Some(1),
            Widget::Notice {
                text: "Threads: takes effect after a restart".into(),
                level: NoticeLevel::Warning,
            },
        ),
        node(
            20,
            None,
            Widget::Area {
                anchor: Anchor::TopRight,
                dx: -8.0,
                dy: 8.0,
                frame: true,
            },
        ),
        node(
            21,
            Some(20),
            Widget::Text {
                text: "11.8 ms".into(),
                style: TextStyle::Monospace,
                color: None,
                spans: vec![],
                wrap: None,
                selectable: true,
            },
        ),
    ];
    let ids: Vec<u64> = nodes.iter().map(|n| n.id).collect();
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(nodes.clone()).unwrap();
        frame(&context, &overlay);
        let picture = frame(&context, &overlay);
        for &id in &ids {
            let r = overlay
                .bounds(id)
                .unwrap_or_else(|| panic!("{name}: {id} not drawn"));
            assert!(
                r.x >= 0.0 && r.y >= 0.0 && r.x + r.width <= W as f32 + 0.5,
                "{name}: {id} at {r:?}"
            );
        }
        // The frame time sits in the top right corner, over the picture: egui uses the pointer there
        let (x, y) = center(&overlay, 21);
        assert!(x > W as f32 / 2.0 && y < 40.0, "{name}: {x} {y}");
        assert!(overlay.covers(x, y) && overlay.input(UiInput::Press { x, y, button: 0 }));
        if let Some(dir) = std::env::var_os("THREE_D_EGUI_PICTURE") {
            let bytes: Vec<u8> = picture.iter().flatten().copied().collect();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("{name}.rgba")),
                bytes,
            )
            .unwrap();
        }
        drop(overlay);
        release(context);
    }
}

/// A benchmark window: frame times as a line with the 60 fps mark, and per scene the parts of a frame as stacked bars.
/// With `THREE_D_EGUI_PICTURE=<dir>` it writes `<dir>/plot-<backend>.rgba` (`W` x `H`).
#[test]
fn plots_of_a_benchmark() {
    const GREEN: u32 = 0xFF2ECC71;
    const BLUE: u32 = 0xFF3498DB;
    const ORANGE: u32 = 0xFFF39C12;
    let series = |name: &str, kind, y: Vec<f64>, color| PlotSeries {
        name: name.into(),
        kind,
        x: vec![],
        y,
        color: Some(color),
    };
    let plot = |series, categories: Vec<String>| Widget::Plot {
        series,
        height: 110.0,
        width: None,
        x_label: String::new(),
        y_label: "ms".into(),
        x_unit: String::new(),
        y_unit: " ms".into(),
        categories,
        legend: true,
        interactive: true,
        link: None,
        include_x: vec![],
        include_y: vec![0.0],
    };
    let bars = |stack: &str| SeriesKind::Bars {
        width: 0.6,
        stack: Some(stack.into()),
    };
    let times: Vec<f64> = (0..60)
        .map(|i| 12.0 + 3.0 * (i as f64 / 6.0).sin())
        .collect();
    let nodes = vec![
        node(
            1,
            None,
            Widget::Window {
                title: "Benchmark".into(),
                x: 10.0,
                y: 10.0,
                width: Some(370.0),
                closable: false,
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
        ),
        node(
            2,
            Some(1),
            plot(
                vec![
                    series(
                        "Frame time",
                        SeriesKind::Line {
                            width: 1.5,
                            fill: true,
                        },
                        times,
                        GREEN,
                    ),
                    series("60 fps", SeriesKind::HLine, vec![16.7], 0xFFE74C3C),
                ],
                vec![],
            ),
        ),
        node(
            3,
            Some(1),
            plot(
                vec![
                    series("Trace", bars("frame"), vec![9.0, 14.0, 6.0], BLUE),
                    series("Denoise", bars("frame"), vec![3.0, 3.0, 3.0], ORANGE),
                ],
                vec!["Cloister".into(), "Garden".into(), "Hall".into()],
            ),
        ),
    ];
    let argb = |c: u32| {
        let [_, r, g, b] = c.to_be_bytes();
        [r, g, b]
    };
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(nodes.clone()).unwrap();
        // Until the window faded in and the axes found their widths
        for _ in 0..100 {
            frame(&context, &overlay);
            if !overlay.needs_repaint() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let picture = frame(&context, &overlay);
        if let Some(dir) = std::env::var_os("THREE_D_EGUI_PICTURE") {
            let bytes: Vec<u8> = picture.iter().flatten().copied().collect();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("plot-{name}.rgba")),
                bytes,
            )
            .unwrap();
        }
        let in_plot = |id: u64, color: u32| {
            let r = overlay.bounds(id).expect("drawn");
            let mut ys = Vec::new();
            for y in r.y as u32..(r.y + r.height) as u32 {
                for x in r.x as u32..(r.x + r.width) as u32 {
                    if near(pixel(&picture, x, y), argb(color), 24) {
                        ys.push(y as f64);
                    }
                }
            }
            ys
        };
        let line = in_plot(2, GREEN);
        assert!(
            line.len() > 50,
            "{name}: the line has {} pixels",
            line.len()
        );
        let (trace, denoise) = (in_plot(3, BLUE), in_plot(3, ORANGE));
        assert!(
            trace.len() > 50 && denoise.len() > 50,
            "{name}: bars {} {}",
            trace.len(),
            denoise.len()
        );
        // The denoise part stands on the trace part
        let mean = |ys: &[f64]| ys.iter().sum::<f64>() / ys.len() as f64;
        assert!(
            mean(&denoise) < mean(&trace),
            "{name}: the denoise bars are not on top"
        );
        // The pointer on a plot belongs to egui, which shows the values there
        let (x, y) = center(&overlay, 3);
        assert!(
            overlay.input(UiInput::Move { x, y }) && overlay.needs_repaint(),
            "{name}"
        );
        drop(overlay);
        release(context);
    }
}

#[test]
fn a_settings_window_with_frames_rows_and_text_in_parts() {
    const CARD: u32 = 0xFF1F4E79;
    const EDGE: u32 = 0xFFF39C12;
    const GOOD: u32 = 0xFF2ECC71;
    let text = |id, parent, s: &str, spans: Vec<TextSpan>| {
        node(
            id,
            Some(parent),
            Widget::Text {
                text: s.into(),
                style: TextStyle::Body,
                color: None,
                spans,
                wrap: None,
                selectable: true,
            },
        )
    };
    let button = |id, parent, s: &str| {
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
    };
    let mut card = node(5, Some(1), column());
    card.modifier = Some(UiModifier {
        fill_width: true,
        background: Some(CARD),
        border: Some(UiStroke {
            width: 2.0,
            color: EDGE,
        }),
        corner_radius: 6.0,
        padding: Some(UiInsets {
            left: 10.0,
            top: 8.0,
            right: 10.0,
            bottom: 8.0,
        }),
        ..Default::default()
    });
    let nodes = vec![
        node(
            1,
            None,
            Widget::Window {
                title: "Settings".into(),
                x: 10.0,
                y: 10.0,
                width: Some(360.0),
                closable: false,
                collapsible: false,
                resizable: false,
                height: None,
                anchor: Some(Anchor::TopLeft),
                movable: true,
                title_bar: true,
                fixed_size: true,
                scroll: false,
                open: true,
            },
        ),
        text(
            2,
            1,
            "Frame time ",
            vec![
                TextSpan {
                    text: "12.9 ms".into(),
                    style: Some(TextStyle::Strong),
                    ..Default::default()
                },
                TextSpan {
                    text: "  smooth".into(),
                    color: Some(GOOD),
                    size: Some(18.0),
                    ..Default::default()
                },
            ],
        ),
        card,
        text(6, 5, "Shadows and reflections", vec![]),
        node(7, Some(5), Widget::Sides),
        text(8, 7, "Quality", vec![]),
        button(9, 7, "High"),
        node(10, Some(1), Widget::Columns),
        text(11, 10, "Trace", vec![]),
        text(12, 10, "Denoise", vec![]),
        text(13, 10, "Present", vec![]),
        node(
            14,
            Some(1),
            Widget::Row {
                wrap: false,
                align: None,
                arrangement: Some(Arrangement::End),
                spacing: None,
            },
        ),
        button(15, 14, "Cancel"),
        button(16, 14, "Apply"),
    ];
    let argb = |c: u32| {
        let [_, r, g, b] = c.to_be_bytes();
        [r, g, b]
    };
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(nodes.clone()).unwrap();
        // Until the window faded in
        for _ in 0..100 {
            frame(&context, &overlay);
            if !overlay.needs_repaint() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let picture = frame(&context, &overlay);
        if let Some(dir) = std::env::var_os("THREE_D_EGUI_PICTURE") {
            let bytes: Vec<u8> = picture.iter().flatten().copied().collect();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("layout-{name}.rgba")),
                bytes,
            )
            .unwrap();
        }
        let count = |id: u64, color: u32| {
            let r = overlay.bounds(id).expect("drawn");
            let mut n = 0;
            for y in r.y as u32..(r.y + r.height) as u32 {
                for x in r.x as u32..(r.x + r.width) as u32 {
                    if near(pixel(&picture, x, y), argb(color), 12) {
                        n += 1;
                    }
                }
            }
            (n, r)
        };
        // The card fills the width of the window with its background, inside its border
        let (fill, card) = count(5, CARD);
        assert!(
            fill as f32 > 0.4 * card.width * card.height,
            "{name}: {fill} pixels of the card's background in {card:?}"
        );
        let (edge, _) = count(5, EDGE);
        assert!(
            edge as f32 > card.width + card.height,
            "{name}: {edge} pixels of the border"
        );
        let window = overlay.bounds(1).unwrap();
        assert!(
            card.width > 340.0 && card.width < window.width,
            "{name}: {card:?}"
        );
        // The colored part of the text, and the buttons at the right
        let (good, _) = count(2, GOOD);
        assert!(good > 20, "{name}: {good} pixels of the colored part");
        let apply = overlay.bounds(16).unwrap();
        assert!(
            apply.x + apply.width > window.x + window.width - 20.0,
            "{name}: {apply:?} {window:?}"
        );
        drop(overlay);
        release(context);
    }
}

#[test]
fn switches_pickers_and_code_of_the_second_phase() {
    const ACCENT: u32 = 0xFF00A040;
    const PURPLE: u32 = 0xFF9B59B6;
    let row = |id| node(id, Some(1), row());
    let nodes = vec![
        node(
            1,
            None,
            Widget::Window {
                title: "Inputs".into(),
                x: 10.0,
                y: 10.0,
                width: Some(370.0),
                closable: false,
                collapsible: false,
                resizable: false,
                height: None,
                anchor: Some(Anchor::TopLeft),
                movable: true,
                title_bar: true,
                fixed_size: true,
                scroll: false,
                open: true,
            },
        ),
        row(2),
        node(
            3,
            Some(2),
            Widget::Switch {
                text: "Shadows".into(),
                checked: true,
            },
        ),
        node(
            4,
            Some(2),
            Widget::ToggleButton {
                text: "Grid".into(),
                checked: true,
            },
        ),
        node(
            5,
            Some(2),
            Widget::SelectableLabel {
                text: "Garden".into(),
                selected: true,
            },
        ),
        row(6),
        node(
            7,
            Some(6),
            Widget::ColorPicker {
                label: "Sky".into(),
                color: PURPLE,
                alpha: false,
                inline: false,
            },
        ),
        node(
            8,
            Some(6),
            Widget::DatePicker {
                label: "Day".into(),
                date: UiDate {
                    year: 2026,
                    month: 11,
                    day: 24,
                },
                format: Some("%d.%m.%Y".into()),
                calendar_week: true,
                highlight_weekends: true,
                first_year: None,
                last_year: None,
            },
        ),
        row(9),
        node(
            10,
            Some(9),
            Widget::SliderFloat {
                label: String::new(),
                value: 0.7,
                min: 0.0,
                max: 1.0,
                logarithmic: false,
                decimals: Some(1),
                suffix: String::new(),
                prefix: String::new(),
                step: None,
                vertical: true,
                clamping: None,
                show_value: false,
                trailing_fill: Some(true),
                handle: None,
            },
        ),
        node(
            11,
            Some(9),
            Widget::TextField {
                label: String::new(),
                value: "fn main() {\n    let light = \"sun\";\n    trace(light, 64);\n}".into(),
                hint: String::new(),
                password: false,
                lines: Some(4),
                code: true,
                language: Some("rs".into()),
                char_limit: None,
                align: None,
                read_only: false,
            },
        ),
        node(
            12,
            Some(1),
            Widget::Link {
                text: "three-d".into(),
                url: Some("https://github.com/asny/three-d".into()),
                new_tab: true,
            },
        ),
    ];
    let argb = |c: u32| {
        let [_, r, g, b] = c.to_be_bytes();
        [r, g, b]
    };
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_style(UiStyle {
            dark: true,
            accent: Some(ACCENT),
        });
        overlay.set_tree(nodes.clone()).unwrap();
        for _ in 0..100 {
            frame(&context, &overlay);
            if !overlay.needs_repaint() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let picture = frame(&context, &overlay);
        if let Some(dir) = std::env::var_os("THREE_D_EGUI_PICTURE") {
            let bytes: Vec<u8> = picture.iter().flatten().copied().collect();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("inputs-{name}.rgba")),
                bytes,
            )
            .unwrap();
        }
        let count = |id: u64, test: &dyn Fn([u8; 4]) -> bool| {
            let r = overlay.bounds(id).expect("drawn");
            let mut n = 0;
            for y in r.y as u32..(r.y + r.height) as u32 {
                for x in r.x as u32..(r.x + r.width) as u32 {
                    if test(pixel(&picture, x, y)) {
                        n += 1;
                    }
                }
            }
            n
        };
        let accent = |p| near(p, argb(ACCENT), 12);
        // What is on or chosen shows the accent color
        for id in [3, 4, 5] {
            let n = count(id, &accent);
            assert!(n > 40, "{name}: {n} pixels of the accent in node {id}");
        }
        // The color picker's button has its color, the slider stands upright with its track filled
        let n = count(7, &|p| near(p, argb(PURPLE), 6));
        assert!(n > 40, "{name}: {n} pixels of the chosen color");
        let slider = overlay.bounds(10).unwrap();
        assert!(slider.height > 2.0 * slider.width, "{name}: {slider:?}");
        let n = count(10, &accent);
        assert!(n > 20, "{name}: {n} pixels of the filled track");
        // The code is colored: keywords, strings and numbers in their own colors
        let colored = |p: [u8; 4]| {
            let (max, min) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
            max - min > 60 && !accent(p)
        };
        let n = count(11, &colored);
        assert!(n > 60, "{name}: {n} colored pixels in the code");
        let field = overlay.bounds(11).unwrap();
        assert!(field.height > 50.0, "{name}: four lines in {field:?}");
        // egui draws the field in a frame that would hang down from the middle of the row: it is in the middle
        let middle = |r: UiRect| r.y + r.height / 2.0;
        assert!(
            (middle(field) - middle(slider)).abs() < 2.0,
            "{name}: {field:?} beside {slider:?}"
        );
        drop(overlay);
        release(context);
    }
}

#[test]
fn an_open_menu_and_a_modal_lie_over_the_picture() {
    let item = |id, parent, text: &str, shortcut: Option<&str>| {
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
            },
        )
    };
    let menus = vec![
        node(1, None, Widget::TopBar { expanded: true }),
        node(2, Some(1), Widget::MenuBar),
        node(
            3,
            Some(2),
            Widget::Menu {
                text: "Datei".into(),
            },
        ),
        item(4, 3, "Speichern", Some("Ctrl+S")),
        item(5, 3, "Schließen", None),
        node(
            6,
            Some(2),
            Widget::Menu {
                text: "Ansicht".into(),
            },
        ),
        item(7, 6, "Oben", None),
    ];
    let save = |name: &str, picture: &[[u8; 4]]| {
        if let Some(dir) = std::env::var_os("THREE_D_EGUI_PICTURE") {
            let bytes: Vec<u8> = picture.iter().flatten().copied().collect();
            std::fs::write(std::path::Path::new(&dir).join(name), bytes).unwrap();
        }
    };
    let settle = |context: &Context, overlay: &EguiOverlay| {
        for _ in 0..100 {
            frame(context, overlay);
            if !overlay.needs_repaint() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        frame(context, overlay)
    };
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(menus.clone()).unwrap();
        settle(&context, &overlay);
        assert!(overlay.bounds(4).is_none(), "{name}: closed at the start");
        click(&context, &overlay, center(&overlay, 3));
        let picture = settle(&context, &overlay);
        save(&format!("menu-{name}.rgba"), &picture);
        // The open menu lies over the red picture, under the bar
        let entry = overlay.bounds(4).expect("the menu is open");
        let bar = overlay.bounds(1).unwrap();
        assert!(
            entry.y >= bar.y + bar.height - 2.0,
            "{name}: {entry:?} {bar:?}"
        );
        let (x, y) = center(&overlay, 5);
        let p = pixel(&picture, x as u32, y as u32);
        assert!(
            !near(p, [255, 0, 0], 60),
            "{name}: the menu is not drawn at {x}, {y}: {p:?}"
        );
        assert!(overlay.covers(x, y), "{name}: the menu takes the pointer");
        click(&context, &overlay, center(&overlay, 4));
        assert_eq!(overlay.take_events(), vec![UiEvent::Clicked { id: 4 }]);

        // A modal dims the whole picture and takes every click
        let mut dialog = menus.clone();
        dialog.push(node(10, None, Widget::Modal { dismissible: true }));
        dialog.push(node(
            11,
            Some(10),
            Widget::Text {
                text: "Szene verwerfen?".into(),
                style: TextStyle::Heading,
                color: None,
                spans: vec![],
                wrap: None,
                selectable: true,
            },
        ));
        overlay.set_tree(dialog).unwrap();
        let picture = settle(&context, &overlay);
        save(&format!("modal-{name}.rgba"), &picture);
        let corner = pixel(&picture, W - 5, H - 5);
        assert!(
            (100..220).contains(&corner[0]) && corner[1] < 30 && corner[2] < 30,
            "{name}: the picture behind is not dimmed: {corner:?}"
        );
        let text = overlay.bounds(11).expect("the dialog is shown");
        assert!(
            (text.x + text.width / 2.0 - W as f32 / 2.0).abs() < 20.0,
            "{name}: the dialog is not in the middle: {text:?}"
        );
        assert!(
            overlay.covers(5.0, (H - 5) as f32),
            "{name}: the modal takes the pointer everywhere"
        );
        drop(overlay);
        release(context);
    }
}
