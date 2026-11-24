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
                image: None,
                image_size: None,
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
                image: None,
                image_size: None,
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
                image: None,
                image_size: None,
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
        x2: vec![],
        y2: vec![],
        labels: vec![],
        style: None,
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
        clickable: false,
        report_pointer: false,
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
                image: None,
                image_size: None,
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
                image: None,
                image_size: None,
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

#[test]
fn tables_with_a_sorted_column_and_a_selected_row_in_a_strip() {
    let col =
        |title: &str, width: Option<ColumnWidth>, align: Option<Align>, sortable| TableColumn {
            title: title.into(),
            width,
            min_width: None,
            max_width: None,
            resizable: None,
            clip: false,
            align,
            sortable,
            style: None,
        };
    let table = |columns, data: Option<&str>, selection, selected| Widget::Table {
        columns,
        data: data.map(Into::into),
        striped: true,
        resizable: false,
        selection,
        selected,
        sort: data.map(|_| TableSort {
            column: 1,
            ascending: true,
        }),
        row_height: None,
        max_height: None,
        stick_to_end: false,
        scroll_to: None,
    };
    let text = |id, parent, s: &str| {
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
    };
    let remainder = Some(ColumnWidth::Remainder);
    let narrow = Some(ColumnWidth::Exact { width: 80.0 });
    let mut nodes = vec![
        node(1, None, Widget::CentralPanel { frame: true }),
        node(
            2,
            Some(1),
            Widget::Strip {
                sizes: vec![
                    StripSize::Exact { points: 130.0 },
                    StripSize::Remainder {
                        min: None,
                        max: None,
                    },
                ],
                vertical: true,
                clip: true,
            },
        ),
        node(
            3,
            Some(2),
            table(
                vec![
                    col("Szene", remainder, None, false),
                    col("ms", narrow, Some(Align::End), true),
                ],
                Some("zeiten"),
                None,
                vec![],
            ),
        ),
        node(
            10,
            Some(2),
            table(
                vec![
                    col("Objekt", remainder, None, false),
                    col("Polygone", narrow, Some(Align::End), false),
                ],
                None,
                Some(TableSelection::Single),
                vec![1],
            ),
        ),
    ];
    for (i, (name, polygons)) in [("Säule", "12400"), ("Brunnen", "48900"), ("Dach", "3200")]
        .iter()
        .enumerate()
    {
        let row = 20 + 10 * i as u64;
        nodes.push(node(
            row,
            Some(10),
            Widget::TableRow {
                header: false,
                height: None,
            },
        ));
        nodes.push(text(row + 1, row, name));
        nodes.push(text(row + 2, row, polygons));
    }
    let cells: Vec<String> = (0..1000)
        .flat_map(|i| [format!("Szene {i}"), format!("{}.5", 10 + i % 7)])
        .collect();
    let save = |name: &str, picture: &[[u8; 4]]| {
        if let Some(dir) = std::env::var_os("THREE_D_EGUI_PICTURE") {
            let bytes: Vec<u8> = picture.iter().flatten().copied().collect();
            std::fs::write(std::path::Path::new(&dir).join(name), bytes).unwrap();
        }
    };
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(nodes.clone()).unwrap();
        overlay
            .set_table_data("zeiten".into(), 2, cells.clone())
            .unwrap();
        frame(&context, &overlay);
        let picture = frame(&context, &overlay);
        save(&format!("table-{name}.rgba"), &picture);
        // The data table in its cell of 130 points, the other table under it
        let top = overlay.bounds(3).expect("the table of data is drawn");
        assert!(top.height <= 131.0, "{name}: {top:?}");
        let selected = overlay.bounds(30).expect("row 1 is drawn");
        assert!(
            selected.y >= top.y + top.height,
            "{name}: {selected:?} under {top:?}"
        );
        // The selected row in the selection color beside its text, the others not
        let at = |row: UiRect| ((row.x + 150.0) as u32, (row.y + row.height / 2.0) as u32);
        let (x, y) = at(selected);
        let p = pixel(&picture, x, y);
        assert!(
            near(p, [0, 92, 128], 30),
            "{name}: the selected row at {x}, {y} is {p:?}"
        );
        let (x, y) = at(overlay.bounds(20).unwrap());
        let p = pixel(&picture, x, y);
        assert!(
            !near(p, [0, 92, 128], 30) && !near(p, [255, 0, 0], 60),
            "{name}: the row at {x}, {y} is {p:?}"
        );
        click(&context, &overlay, center(&overlay, 40));
        assert_eq!(
            overlay.take_events(),
            vec![UiEvent::RowsSelected {
                id: 10,
                rows: vec![2]
            }]
        );
        // A second click at once opens the row
        click(&context, &overlay, center(&overlay, 40));
        assert_eq!(
            overlay.take_events(),
            vec![UiEvent::RowActivated { id: 10, row: 2 }],
            "{name}"
        );
        // Also without a move between the clicks, as a hand that rests
        std::thread::sleep(std::time::Duration::from_millis(400));
        let (x, y) = center(&overlay, 20);
        overlay.input(UiInput::Move { x, y });
        frame(&context, &overlay);
        for _ in 0..2 {
            overlay.input(UiInput::Press { x, y, button: 0 });
            frame(&context, &overlay);
            overlay.input(UiInput::Release { x, y, button: 0 });
            frame(&context, &overlay);
        }
        assert_eq!(
            overlay.take_events(),
            vec![
                UiEvent::RowsSelected {
                    id: 10,
                    rows: vec![0]
                },
                UiEvent::RowActivated { id: 10, row: 0 }
            ],
            "{name}"
        );
        drop(overlay);
        release(context);
    }
}

/// The first pixel of row `y` from `x0` to `x1` that is near `color`.
fn find_in_row(picture: &[[u8; 4]], y: u32, x0: u32, x1: u32, color: [u8; 3]) -> Option<u32> {
    (x0..x1.min(W)).find(|&x| near(pixel(picture, x, y), color, 40))
}

#[test]
fn images_a_floor_plan_on_a_canvas_and_a_scene_over_the_picture() {
    let image = |id, key: &str| {
        node(
            id,
            Some(2),
            Widget::Image {
                image: key.into(),
                width: Some(40.0),
                height: Some(40.0),
                fit: None,
                tint: None,
                corner_radius: None,
                rotation: None,
                description: None,
            },
        )
    };
    // An L-shaped room of 20 by 10 m (concave), y upwards as on a plan
    let room = CanvasShape::Polygon {
        points: vec![
            0.0, 0.0, 20.0, 0.0, 20.0, 4.0, 8.0, 4.0, 8.0, 10.0, 0.0, 10.0,
        ],
        fill: Some(0xFF00C000),
        stroke: None,
    };
    let nodes = vec![
        node(1, None, Widget::CentralPanel { frame: true }),
        node(2, Some(1), row()),
        image(3, "blau"),
        image(4, "png"),
        image(5, "svg"),
        node(
            6,
            Some(2),
            Widget::Button {
                text: "Blau".into(),
                small: false,
                frame: true,
                selected: false,
                shortcut: None,
                fill: None,
                image: Some("blau".into()),
                image_size: None,
            },
        ),
        node(
            10,
            Some(1),
            Widget::Canvas {
                shapes: vec![],
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
                report_pointer: false,
            },
        ),
        node(
            20,
            Some(1),
            Widget::Scene {
                height: 80.0,
                width: Some(200.0),
                min_zoom: None,
                max_zoom: Some(4.0),
                view: Some(UiRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 40.0,
                }),
                report_view: true,
            },
        ),
        node(
            21,
            Some(20),
            Widget::Button {
                text: "Tor".into(),
                small: false,
                frame: true,
                selected: false,
                shortcut: None,
                fill: None,
                image: None,
                image_size: None,
            },
        ),
    ];
    let blue: Vec<u8> = [0, 0, 255, 255].repeat(4);
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ffd700"/></svg>"##;
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(nodes.clone()).unwrap();
        overlay
            .set_image_rgba("blau".into(), 2, 2, blue.clone())
            .unwrap();
        overlay.set_image("svg".into(), svg.to_vec()).unwrap();
        #[cfg(feature = "images")]
        {
            let mut png = Vec::new();
            ::image::RgbaImage::from_pixel(8, 8, ::image::Rgba([0, 200, 200, 255]))
                .write_to(
                    &mut std::io::Cursor::new(&mut png),
                    ::image::ImageFormat::Png,
                )
                .unwrap();
            overlay.set_image("png".into(), png).unwrap();
        }
        overlay
            .set_canvas_shapes("plan".into(), vec![room.clone()])
            .unwrap();
        // Files are decoded on threads: frames until they are there
        let middle = |id| {
            let (x, y) = center(&overlay, id);
            (x as u32, y as u32)
        };
        let mut picture = frame(&context, &overlay);
        for _ in 0..200 {
            picture = frame(&context, &overlay);
            let (x, y) = middle(5);
            let decoded = near(pixel(&picture, x, y), [255, 215, 0], 40);
            #[cfg(feature = "images")]
            let decoded = decoded && {
                let (x, y) = middle(4);
                near(pixel(&picture, x, y), [0, 200, 200], 40)
            };
            if decoded {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for (id, color) in [(3, [0, 0, 255]), (5, [255, 215, 0])] {
            let (x, y) = middle(id);
            let p = pixel(&picture, x, y);
            assert!(
                near(p, color, 40),
                "{name}: image {id} at {x}, {y} is {p:?}"
            );
        }
        #[cfg(feature = "images")]
        {
            let (x, y) = middle(4);
            let p = pixel(&picture, x, y);
            assert!(
                near(p, [0, 200, 200], 40),
                "{name}: the PNG at {x}, {y} is {p:?}"
            );
        }
        let b = overlay.bounds(6).expect("the button is drawn");
        let y = (b.y + b.height / 2.0) as u32;
        assert!(
            find_in_row(&picture, y, b.x as u32, (b.x + b.width) as u32, [0, 0, 255]).is_some(),
            "{name}: no image in the button {b:?}"
        );
        // The room filled in its L, the corner beside it not; a metre is 10 points
        let c = overlay.bounds(10).expect("the canvas is drawn");
        assert!(
            (c.width - 200.0).abs() < 1.0 && (c.height - 100.0).abs() < 1.0,
            "{name}: {c:?}"
        );
        let at = |x: f32, y: f32| (c.x + x * 10.0, c.y + (10.0 - y) * 10.0);
        for (x, y, inside) in [(4.0, 8.0, true), (15.0, 2.0, true), (15.0, 8.0, false)] {
            let (px, py) = at(x, y);
            let p = pixel(&picture, px as u32, py as u32);
            assert_eq!(
                near(p, [0, 192, 0], 30),
                inside,
                "{name}: the plan at {x}, {y} m ({px}, {py}) is {p:?}"
            );
        }
        click(&context, &overlay, at(4.0, 8.0));
        match overlay.take_events().as_slice() {
            [UiEvent::PointClicked { id: 10, point }] => assert!(
                (point.x - 4.0).abs() < 0.1 && (point.y - 8.0).abs() < 0.1,
                "{name}: {point:?}"
            ),
            events => panic!("{name}: {events:?}"),
        }
        // The scene shows its button zoomed twice to fit the view of 100 x 40 into 200 x 80
        let scene = overlay.bounds(20).expect("the scene is drawn");
        let tor = overlay
            .bounds(21)
            .expect("the button in the scene is drawn");
        let plain = overlay.bounds(6).unwrap().height;
        assert!(tor.height > 1.6 * plain, "{name}: {tor:?} beside {plain}");
        assert!(
            tor.x >= scene.x - 1.0 && tor.y >= scene.y - 1.0,
            "{name}: {tor:?} in {scene:?}"
        );
        // Dragging the empty part moves the view and the button with it
        let (x, y) = (scene.x + scene.width - 20.0, scene.y + scene.height - 10.0);
        overlay.input(UiInput::Move { x, y });
        frame(&context, &overlay);
        overlay.input(UiInput::Press { x, y, button: 0 });
        frame(&context, &overlay);
        for step in 1..=4 {
            overlay.input(UiInput::Move {
                x: x - 10.0 * step as f32,
                y,
            });
            frame(&context, &overlay);
        }
        overlay.input(UiInput::Release {
            x: x - 40.0,
            y,
            button: 0,
        });
        frame(&context, &overlay);
        let moved = overlay.bounds(21).unwrap();
        assert!(moved.x < tor.x - 20.0, "{name}: {moved:?} after {tor:?}");
        let events = overlay.take_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, UiEvent::ViewChanged { id: 20, view } if view.x > 5.0)),
            "{name}: {events:?}"
        );
        drop(overlay);
        release(context);
    }
}

#[test]
fn a_card_dragged_over_the_picture_follows_the_pointer_and_is_dropped_in_the_list() {
    let card = |id: u64, color: u32| UiNode {
        modifier: Some(UiModifier {
            width: Some(120.0),
            height: Some(30.0),
            background: Some(color),
            reactions: Some(UiReactions {
                drag_kind: Some("karte".into()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..node(id, Some(2), column())
    };
    let nodes = vec![
        node(1, None, Widget::CentralPanel { frame: true }),
        UiNode {
            modifier: Some(UiModifier {
                reactions: Some(UiReactions {
                    drop_kinds: Some(vec![]),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..node(2, Some(1), column())
        },
        card(3, 0xFF2060E0),
        card(4, 0xFF20A040),
        card(5, 0xFFE0C020),
    ];
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(nodes.clone()).unwrap();
        frame(&context, &overlay);
        frame(&context, &overlay);
        let blue = overlay.bounds(3).expect("drawn");
        let yellow = overlay.bounds(5).unwrap();
        let (x, y) = center(&overlay, 3);
        // Taken in its middle and moved 150 points to the right and down under the yellow card
        let to = (x + 150.0, yellow.y + yellow.height + 20.0);
        overlay.input(UiInput::Move { x, y });
        frame(&context, &overlay);
        overlay.input(UiInput::Press { x, y, button: 0 });
        frame(&context, &overlay);
        let mut picture = Vec::new();
        for step in 1..=6 {
            let t = step as f32 / 6.0;
            let (px, py) = (x + (to.0 - x) * t, y + (to.1 - y) * t);
            overlay.input(UiInput::Move { x: px, y: py });
            picture = frame(&context, &overlay);
        }
        // The blue card under the pointer, its place in the list empty
        let p = pixel(&picture, to.0 as u32, to.1 as u32);
        assert!(near(p, [32, 96, 224], 30), "{name}: at the pointer {p:?}");
        let p = pixel(&picture, x as u32, y as u32);
        assert!(!near(p, [32, 96, 224], 30), "{name}: its place {p:?}");
        assert!(blue.y < yellow.y);
        overlay.input(UiInput::Release {
            x: to.0,
            y: to.1,
            button: 0,
        });
        frame(&context, &overlay);
        // Dropped beside the list: the pointer was not on it
        assert_eq!(overlay.take_events(), vec![], "{name}");
        // Once more, onto the list under the yellow card
        let list = overlay.bounds(2).unwrap();
        let to = (x, list.y + list.height - 3.0);
        overlay.input(UiInput::Move { x, y });
        frame(&context, &overlay);
        overlay.input(UiInput::Press { x, y, button: 0 });
        frame(&context, &overlay);
        for step in 1..=6 {
            let t = step as f32 / 6.0;
            overlay.input(UiInput::Move {
                x,
                y: y + (to.1 - y) * t,
            });
            frame(&context, &overlay);
        }
        overlay.input(UiInput::Release {
            x: to.0,
            y: to.1,
            button: 0,
        });
        frame(&context, &overlay);
        assert_eq!(
            overlay.take_events(),
            vec![UiEvent::Dropped {
                id: 2,
                source: 3,
                index: Some(3)
            }],
            "{name}"
        );
        drop(overlay);
        release(context);
    }
}

/// Phase 7: a panel with a text, a node of its own look and a window to move.
fn look_tree() -> Vec<UiNode> {
    let text = |id, parent, s: &str| {
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
                image: None,
                image_size: None,
            },
        )
    };
    let mut nodes = tree(false);
    nodes.push(text(4, 1, "iiii mmm"));
    let mut light = node(5, Some(1), column());
    light.modifier = Some(UiModifier {
        theme: Some(UiTheme {
            dark: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    });
    nodes.push(light);
    nodes.push(button(6, 5, "Hell"));
    nodes.push(node(
        7,
        None,
        Widget::Window {
            title: "Werkzeuge".into(),
            x: 200.0,
            y: 40.0,
            width: Some(150.0),
            closable: false,
            collapsible: false,
            resizable: false,
            // egui sizes a new window first at most as high as the screen (400 points without a height), which
            // pushes it to the top of this small one
            height: Some(80.0),
            anchor: None,
            movable: true,
            title_bar: true,
            fixed_size: false,
            scroll: false,
            open: true,
        },
    ));
    nodes.push(text(8, 7, "Gitter"));
    nodes
}

fn frames(context: &Context, overlay: &EguiOverlay) -> Vec<[u8; 4]> {
    frame(context, overlay);
    frame(context, overlay)
}

/// Drags with the left button from `from` by `by`, a frame after each event.
fn drag(context: &Context, overlay: &EguiOverlay, (x, y): (f32, f32), (dx, dy): (f32, f32)) {
    overlay.input(UiInput::Move { x, y });
    frame(context, overlay);
    overlay.input(UiInput::Press { x, y, button: 0 });
    frame(context, overlay);
    for i in 1..=4 {
        let t = i as f32 / 4.0;
        overlay.input(UiInput::Move {
            x: x + dx * t,
            y: y + dy * t,
        });
        frame(context, overlay);
    }
    overlay.input(UiInput::Release {
        x: x + dx,
        y: y + dy,
        button: 0,
    });
    frames(context, overlay);
}

#[test]
fn the_theme_fonts_zoom_and_saved_state_from_kotlin_change_the_picture() {
    for (name, context) in contexts() {
        let overlay = EguiOverlay::new();
        overlay.set_tree(look_tree()).unwrap();
        // Following the system, which said nothing yet: egui's dark look
        overlay.set_theme(UiTheme::default());
        let picture = frames(&context, &overlay);
        let bottom = pixel(&picture, 140, 280);
        assert!(near(bottom, [27, 27, 27], 3), "{name}: {bottom:?}");
        overlay.set_system_theme(Some(false));
        let picture = frames(&context, &overlay);
        let bottom = pixel(&picture, 140, 280);
        assert!(
            bottom[0] > 200,
            "{name}: the system turned light, {bottom:?}"
        );
        // A color of the panels; the node with a light theme has a light button in the dark panel
        overlay.set_system_theme(Some(true));
        overlay.set_theme(UiTheme {
            colors: Some(UiColors {
                background: Some(0xFF2040A0),
                ..Default::default()
            }),
            ..Default::default()
        });
        let picture = frames(&context, &overlay);
        let bottom = pixel(&picture, 140, 280);
        assert!(near(bottom, [0x20, 0x40, 0xA0], 2), "{name}: {bottom:?}");
        let fill = |picture: &[[u8; 4]], id| {
            let r = overlay.bounds(id).unwrap();
            pixel(picture, r.x as u32 + 3, (r.y + r.height / 2.0) as u32)
        };
        let (light, dark) = (fill(&picture, 6), fill(&picture, 3));
        assert!(
            light[0] > 200 && dark[0] < 120,
            "{name}: button of the light node {light:?}, of the panel {dark:?}"
        );
        // Larger text
        let before = overlay.bounds(4).unwrap();
        overlay.set_theme(UiTheme {
            text_sizes: vec![UiTextSize {
                style: TextStyle::Body,
                size: 25.0,
                family: None,
            }],
            ..Default::default()
        });
        frames(&context, &overlay);
        let after = overlay.bounds(4).unwrap();
        assert!(
            after.height > before.height * 1.6,
            "{name}: {before:?} -> {after:?}"
        );
        // A font from Kotlin as the text font: egui's Hack, where i is as wide as m
        overlay.set_theme(UiTheme::default());
        frames(&context, &overlay);
        let plain = overlay.bounds(4).unwrap().width;
        let hack = |family: Option<&str>| UiFont {
            name: "Hack".into(),
            bytes: epaint_default_fonts::HACK_REGULAR.to_vec(),
            family: family.map(Into::into),
            fallback: false,
            scale: None,
            y_offset: None,
        };
        overlay.set_fonts(vec![hack(None)]).unwrap();
        frames(&context, &overlay);
        let mono = overlay.bounds(4).unwrap().width;
        assert!(mono > plain * 1.15, "{name}: {plain} -> {mono}");
        // A family of its own for the body text only
        overlay.set_fonts(vec![hack(Some("Code"))]).unwrap();
        overlay.set_theme(UiTheme::default());
        frames(&context, &overlay);
        assert!(
            (overlay.bounds(4).unwrap().width - plain).abs() < 1.0,
            "{name}: a family nobody uses changes nothing"
        );
        overlay.set_theme(UiTheme {
            text_sizes: vec![UiTextSize {
                style: TextStyle::Body,
                size: 12.5,
                family: Some("Code".into()),
            }],
            ..Default::default()
        });
        frames(&context, &overlay);
        let coded = overlay.bounds(4).unwrap().width;
        assert!(coded > plain * 1.15, "{name}: {plain} -> {coded}");
        let broken = UiFont {
            bytes: vec![1, 2, 3],
            ..hack(None)
        };
        assert!(
            matches!(
                overlay.set_fonts(vec![broken]),
                Err(UiError::InvalidData { .. })
            ),
            "{name}"
        );
        overlay.set_fonts(vec![]).unwrap();
        overlay.set_theme(UiTheme::default());
        frames(&context, &overlay);
        // Ctrl and = zoom by a tenth, egui gets nothing of it
        let small = overlay.bounds(4).unwrap();
        let key = |key: &str, down| {
            overlay.input(UiInput::Key {
                key: key.into(),
                down,
            })
        };
        key("Control", true);
        assert!(key("=", true), "{name}: the zoom keys belong to egui");
        key("=", false);
        key("Control", false);
        frames(&context, &overlay);
        assert_eq!(
            overlay.take_events(),
            vec![UiEvent::ZoomChanged { factor: 1.1 }],
            "{name}"
        );
        assert_eq!(overlay.zoom(), 1.1, "{name}");
        let big = overlay.bounds(4).unwrap();
        assert!(
            (big.height / small.height - 1.1).abs() < 0.04,
            "{name}: {small:?} -> {big:?}"
        );
        // The window moved by the user, the state saved, moved again and brought back
        let w = overlay.bounds(7).unwrap();
        drag(&context, &overlay, (w.x + 30.0, w.y + 8.0), (-40.0, 40.0));
        let moved = overlay.bounds(7).unwrap();
        assert!(
            (moved.x - w.x + 40.0).abs() < 2.0 && (moved.y - w.y - 40.0).abs() < 2.0,
            "{name}: {w:?} -> {moved:?}"
        );
        let saved = overlay.save_state();
        assert!(!saved.is_empty(), "{name}");
        drag(
            &context,
            &overlay,
            (moved.x + 30.0, moved.y + 8.0),
            (-50.0, 20.0),
        );
        assert!(
            (overlay.bounds(7).unwrap().x - moved.x).abs() > 40.0,
            "{name}"
        );
        overlay.set_zoom(1.0);
        overlay.restore_state(saved.clone()).unwrap();
        frames(&context, &overlay);
        let back = overlay.bounds(7).unwrap();
        assert!(
            (back.x - moved.x).abs() < 1.0 && (back.y - moved.y).abs() < 1.0,
            "{name}: {moved:?} -> {back:?}"
        );
        assert_eq!(overlay.zoom(), 1.1, "{name}");
        // In the next run: before its first frame
        let next = EguiOverlay::new();
        next.set_tree(look_tree()).unwrap();
        next.restore_state(saved).unwrap();
        frames(&context, &next);
        let there = next.bounds(7).unwrap();
        assert!(
            (there.x - moved.x).abs() < 1.0 && (there.y - moved.y).abs() < 1.0,
            "{name}: {moved:?} -> {there:?}"
        );
        assert!(
            matches!(
                next.restore_state(b"kein Zustand".to_vec()),
                Err(UiError::InvalidData { .. })
            ),
            "{name}"
        );
        drop(overlay);
        drop(next);
        release(context);
    }
}
