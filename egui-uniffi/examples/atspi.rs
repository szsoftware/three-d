//!
//! An overlay on the accessibility bus of a Linux desktop, to see what a screen reader gets: run it, then look with
//! Orca or Accerciser (the application "Gartenplaner"), or with AT-SPI's Python bindings. What a reader does is printed
//! as the overlay's events.
//!
//! ```sh
//! cargo run -p three-d-egui-uniffi --example atspi --features atspi -- 60
//! ```
//!
//! It paints without a window (OpenGL through EGL) as long as the seconds given (30 without).
//!

#[cfg(all(target_os = "linux", feature = "atspi"))]
fn main() {
    use std::io::Write;
    use std::time::{Duration, Instant};
    use three_d::*;
    use three_d_egui_uniffi::*;

    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let context =
        Context::new_headless_gl_with_screen(400, 300, false, false).expect("OpenGL through EGL");
    let node = |id, parent, widget| UiNode {
        id,
        parent,
        widget,
        enabled: true,
        tooltip: None,
        modifier: None,
    };
    let overlay = EguiOverlay::new();
    let tree = |shadows: bool, notes: &str, spacing: f64| {
        vec![
            node(
                1,
                None,
                Widget::SidePanel {
                    side: Side::Left,
                    title: Some("Garten".into()),
                    width: 260.0,
                    resizable: false,
                    min_width: None,
                    max_width: None,
                    expanded: true,
                },
            ),
            node(
                2,
                Some(1),
                Widget::Switch {
                    text: "Schatten".into(),
                    checked: shadows,
                },
            ),
            node(
                3,
                Some(1),
                Widget::SliderFloat {
                    label: "Abstand".into(),
                    value: spacing,
                    min: 0.0,
                    max: 2.0,
                    logarithmic: false,
                    decimals: Some(2),
                    suffix: " m".into(),
                    prefix: String::new(),
                    step: Some(0.25),
                    vertical: false,
                    clamping: None,
                    show_value: true,
                    trailing_fill: None,
                    handle: None,
                },
            ),
            node(
                4,
                Some(1),
                Widget::TextField {
                    label: "Notizen".into(),
                    value: notes.into(),
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
            node(
                5,
                Some(1),
                Widget::Button {
                    text: "Speichern".into(),
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
    };
    let (mut shadows, mut notes, mut spacing) = (true, String::new(), 0.5);
    overlay.set_tree(tree(shadows, &notes, spacing)).unwrap();
    assert!(overlay.enable_atspi("Gartenplaner".into()));
    overlay.set_atspi_window(
        UiRect {
            x: 100.0,
            y: 100.0,
            width: 400.0,
            height: 300.0,
        },
        true,
    );
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut painted = Instant::now() - Duration::from_secs(1);
    println!("ready");
    while Instant::now() < end {
        if overlay.needs_repaint() || painted.elapsed() > Duration::from_millis(250) {
            let screen = RenderTarget::screen(&context, 400, 300);
            screen.clear(ClearState::color(0.1, 0.1, 0.1, 1.0));
            overlay
                .paint(&context, &screen, Viewport::new_at_origo(400, 300))
                .unwrap();
            painted = Instant::now();
            let events = overlay.take_events();
            for event in &events {
                println!("event {event:?}");
                match event {
                    UiEvent::Checked { id: 2, checked } => shadows = *checked,
                    UiEvent::TextChanged { id: 4, value } => notes = value.clone(),
                    UiEvent::FloatChanged { id: 3, value } => spacing = *value,
                    _ => {}
                }
            }
            if !events.is_empty() {
                overlay.set_tree(tree(shadows, &notes, spacing)).unwrap();
            }
            std::io::stdout().flush().ok();
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

#[cfg(not(all(target_os = "linux", feature = "atspi")))]
fn main() {
    eprintln!("only on Linux with the feature atspi");
}
