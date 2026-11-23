//!
//! The overlay painted over a three-d picture on the graphics card, on OpenGL (through EGL, Linux) and on wgpu, used the
//! way Kotlin uses it: pixels from the top left, events through `input`, what the user did through `take_events`.
//!

use std::sync::Arc;
use three_d::*;
use three_d_egui_uniffi::*;

const W: u32 = 400;
const H: u32 = 300;

fn node(id: u64, parent: Option<u64>, widget: Widget) -> UiNode {
    UiNode {
        id,
        parent,
        widget,
        enabled: true,
        tooltip: None,
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
            },
        ),
        node(
            2,
            Some(1),
            Widget::Checkbox {
                text: "Shadows".into(),
                checked,
            },
        ),
        node(
            3,
            Some(1),
            Widget::Button {
                text: "Photo".into(),
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
            },
        ),
        node(7, Some(1), Widget::Separator),
        node(8, Some(1), Widget::Row),
        node(
            9,
            Some(8),
            Widget::Button {
                text: "Photo".into(),
            },
        ),
        node(
            10,
            Some(8),
            Widget::Button {
                text: "Cancel".into(),
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
