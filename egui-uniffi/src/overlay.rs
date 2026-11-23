use crate::input::Held;
use crate::state::UiState;
use crate::{UiError, UiEvent, UiInput, UiNode, UiRect};
use cargo_uniffi_support::ThreadBound;
use std::sync::{Arc, Mutex, MutexGuard};
use three_d::egui;
use three_d::{ColorTarget, Context, CoreError, Event, RenderTarget, Viewport, GUI};
use web_time::Instant;

/// Colors of the overlay: egui's dark or light look with an accent of the application.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct UiStyle {
    /// egui's dark look, else the light one.
    pub dark: bool,
    /// Selections, slider tracks and links in this color (ARGB, Compose's `Color.toArgb()`), `None` egui's blue.
    pub accent: Option<u32>,
}

/// The pointer as the overlay saw it: a button that went down on a panel belongs to the panel until it goes up, one that went
/// down beside the panels belongs to the application (the camera), also when the pointer passes over a panel.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Pointer {
    #[default]
    Free,
    Captured,
    Outside,
}

struct Shared {
    ui: UiState,
    /// What came in since the last frame, turned into three-d's events when the height of the picture is known.
    input: Vec<UiInput>,
    held: Held,
    pointer: Pointer,
    egui: Option<egui::Context>,
    pixels_per_point: f32,
    style: Option<UiStyle>,
    style_changed: bool,
}

/// egui with its painter, which belongs to the thread and the three-d context it was made on.
struct Painting {
    gui: GUI,
    /// The context it paints on; contexts differ by their caches of programs.
    context: Context,
    started: Instant,
}

///
/// egui over the picture of a three-d context, described from Kotlin as a tree of [UiNode]s.
///
/// Kotlin calls [EguiOverlay::set_tree] when its state changed, hands the pointer and key events over with
/// [EguiOverlay::input] (which says whether egui used them) and takes what the user did with [EguiOverlay::take_events]
/// after each frame. The application crate (or three-d-raytrace-uniffi's view) paints it on top of its own picture with
/// [EguiOverlay::paint], every frame or when [EguiOverlay::needs_repaint] says so.
///
/// Everything but painting may be called from any thread. Painting belongs to the thread that painted first.
///
#[derive(uniffi::Object)]
pub struct EguiOverlay {
    shared: Mutex<Shared>,
    painting: Mutex<Option<ThreadBound<Painting>>>,
}

impl Default for EguiOverlay {
    fn default() -> Self {
        Self {
            shared: Mutex::new(Shared {
                ui: UiState::new(),
                input: Vec::new(),
                held: Held::default(),
                pointer: Pointer::Free,
                egui: None,
                pixels_per_point: 1.0,
                style: None,
                style_changed: false,
            }),
            painting: Mutex::new(None),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic in a frame leaves the state as consistent as egui left it: go on with it
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[uniffi::export]
impl EguiOverlay {
    /// An overlay without nodes; it makes its egui when it is painted the first time.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The tree to draw from the next frame on. Values the user gave since are kept until Kotlin took their events.
    pub fn set_tree(&self, nodes: Vec<UiNode>) -> Result<(), UiError> {
        lock(&self.shared).ui.set_tree(nodes)
    }

    /// Hands an event to egui with the next frame. `true` if egui uses it: then the application leaves it alone (does
    /// not turn the camera, for example).
    pub fn input(&self, event: UiInput) -> bool {
        let mut shared = lock(&self.shared);
        let used = shared.uses(&event);
        shared.input.push(event);
        shared.ui.dirty = true;
        used
    }

    /// What the user did since the last call, oldest first.
    pub fn take_events(&self) -> Vec<UiEvent> {
        lock(&self.shared).ui.take_events()
    }

    /// Whether the next frame would look different: a new tree, input, an animation of egui (hover, a spinner, an
    /// opening section). An application that only draws when something changes asks this.
    pub fn needs_repaint(&self) -> bool {
        let shared = lock(&self.shared);
        shared.ui.dirty
            || shared
                .egui
                .as_ref()
                .is_some_and(|egui| egui.has_requested_repaint())
    }

    /// Pixels per point of egui: the density of the screen times a zoom of the user (Compose's `LocalDensity`).
    /// Coordinates of [UiInput] stay in pixels.
    pub fn set_scale(&self, pixels_per_point: f32) {
        let mut shared = lock(&self.shared);
        let scale = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
            pixels_per_point
        } else {
            1.0
        };
        if scale != shared.pixels_per_point {
            shared.pixels_per_point = scale;
            shared.ui.dirty = true;
        }
    }

    /// egui's dark or light look with an accent color.
    pub fn set_style(&self, style: UiStyle) {
        let mut shared = lock(&self.shared);
        shared.style = Some(style);
        shared.style_changed = true;
        shared.ui.dirty = true;
    }

    /// Whether the pixel (from the top left) lies on a panel, window or area of the last frame.
    pub fn covers(&self, x: f32, y: f32) -> bool {
        let shared = lock(&self.shared);
        let s = shared.pixels_per_point;
        shared.over(x / s, y / s)
    }

    /// Where node `id` was drawn in the last frame, in pixels from the top left; `None` if it was not drawn (a closed
    /// section, a hidden window). For tests that click on a node, and to place things next to it.
    pub fn bounds(&self, id: u64) -> Option<UiRect> {
        let shared = lock(&self.shared);
        let s = shared.pixels_per_point;
        shared.ui.bounds.get(&id).map(|r| UiRect {
            x: r.x * s,
            y: r.y * s,
            width: r.width * s,
            height: r.height * s,
        })
    }
}

impl Shared {
    /// Whether the point (in points) is on egui: on a window, an area or a popup (their own layers), or on a panel.
    fn over(&self, x: f32, y: f32) -> bool {
        let on_layer = self.egui.as_ref().is_some_and(|egui| {
            egui.layer_id_at(egui::pos2(x, y))
                .is_some_and(|layer| layer.order != egui::Order::Background)
        });
        on_layer || self.ui.panels.iter().any(|p| p.contains(x, y))
    }

    fn uses(&mut self, event: &UiInput) -> bool {
        let s = self.pixels_per_point;
        match *event {
            UiInput::Press { x, y, .. } => {
                if self.pointer == Pointer::Free {
                    self.pointer = if self.over(x / s, y / s) {
                        Pointer::Captured
                    } else {
                        Pointer::Outside
                    };
                }
                self.pointer == Pointer::Captured
            }
            UiInput::Release { .. } => std::mem::take(&mut self.pointer) == Pointer::Captured,
            UiInput::Move { x, y } => match self.pointer {
                Pointer::Captured => true,
                Pointer::Outside => false,
                Pointer::Free => self.over(x / s, y / s),
            },
            UiInput::Wheel { x, y, .. } => {
                self.pointer == Pointer::Captured
                    || (self.pointer == Pointer::Free && self.over(x / s, y / s))
            }
            UiInput::Key { .. } | UiInput::Text { .. } => self
                .egui
                .as_ref()
                .is_some_and(|egui| egui.egui_wants_keyboard_input()),
            UiInput::Leave => false,
        }
    }

    fn apply_style(&mut self, egui: &egui::Context) {
        if !std::mem::take(&mut self.style_changed) {
            return;
        }
        let Some(style) = self.style else {
            return;
        };
        let theme = if style.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        let mut visuals = theme.default_visuals();
        if let Some(accent) = style.accent {
            let [a, r, g, b] = accent.to_be_bytes();
            let color = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
            visuals.selection.bg_fill = color;
            visuals.hyperlink_color = color;
            // The text on a selection stays readable on light and dark accents
            let light = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32 > 150.0;
            visuals.selection.stroke.color = if light {
                egui::Color32::BLACK
            } else {
                egui::Color32::WHITE
            };
        }
        egui.set_theme(theme);
        egui.set_visuals_of(theme, visuals);
    }
}

impl EguiOverlay {
    ///
    /// Runs egui for a frame and draws it onto `target` over what is there, within `viewport` (from the bottom left, as
    /// everywhere in three-d; usually the whole target). Call it after the scene is drawn and before the pixels are read
    /// or shown, on the thread of `context`.
    ///
    pub fn paint(
        &self,
        context: &Context,
        target: &RenderTarget<'_>,
        viewport: Viewport,
    ) -> Result<(), UiError> {
        self.paint_with_events(context, target, viewport, &mut [])
    }

    ///
    /// [EguiOverlay::paint] for a Rust application with three-d's events (a window's `FrameInput::events`): egui gets them
    /// after the ones from [EguiOverlay::input] and marks those it uses as handled, as three-d's [GUI] does.
    ///
    pub fn paint_with_events(
        &self,
        context: &Context,
        target: &RenderTarget<'_>,
        viewport: Viewport,
        events: &mut [Event],
    ) -> Result<(), UiError> {
        self.paint_into(context, viewport, events, |render| {
            target.write(render).map(|_| ())
        })
    }

    ///
    /// [EguiOverlay::paint] onto a color target, for example the texture of a view that is read back afterwards.
    ///
    pub fn paint_color(
        &self,
        context: &Context,
        target: &ColorTarget<'_>,
        viewport: Viewport,
    ) -> Result<(), UiError> {
        self.paint_into(context, viewport, &mut [], |render| {
            target.write(render).map(|_| ())
        })
    }

    /// Runs egui for a frame; `write` draws egui's meshes with the given closure into its target.
    fn paint_into(
        &self,
        context: &Context,
        viewport: Viewport,
        events: &mut [Event],
        write: impl FnOnce(&dyn Fn() -> Result<(), CoreError>) -> Result<(), CoreError>,
    ) -> Result<(), UiError> {
        let mut painting = lock(&self.painting);
        let fresh = match painting.as_ref() {
            Some(p) => !p.with(|p| Arc::ptr_eq(&p.context.programs, &context.programs))?,
            None => true,
        };
        if fresh {
            let gui = GUI::new(context);
            lock(&self.shared).egui = Some(gui.context().clone());
            *painting = Some(ThreadBound::new(Painting {
                gui,
                context: context.clone(),
                started: Instant::now(),
            }));
            let mut shared = lock(&self.shared);
            shared.style_changed = shared.style.is_some();
        }
        let painting = painting.as_ref().expect("made above");
        painting.with_mut(|p| -> Result<(), UiError> {
            let mut shared = lock(&self.shared);
            let shared = &mut *shared;
            shared.apply_style(p.gui.context());
            let height = viewport.height as f32;
            let mut all: Vec<Event> = Vec::new();
            for input in shared.input.drain(..) {
                all.extend(shared.held.events(input, height));
            }
            let own = all.len();
            all.extend(events.iter().cloned());
            shared.ui.dirty = false;
            let ms = p.started.elapsed().as_secs_f64() * 1000.0;
            let ui = &mut shared.ui;
            p.gui
                .update(&mut all, ms, viewport, shared.pixels_per_point, |root| {
                    crate::draw::draw(root, ui)
                });
            for (given, handled) in events.iter_mut().zip(all.drain(own..)) {
                *given = handled;
            }
            drop(all);
            write(&|| p.gui.render())?;
            Ok(())
        })?
    }
}
