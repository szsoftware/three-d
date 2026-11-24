use crate::input::{Held, Wanted};
use crate::state::{Focus, UiState};
use crate::{UiError, UiEvent, UiFont, UiInput, UiNode, UiOutput, UiRect, UiTheme};
use cargo_uniffi_support::ThreadBound;
use std::sync::{Arc, Mutex, MutexGuard};
use three_d::egui;
use three_d::{ColorTarget, Context, CoreError, Event, RenderTarget, Viewport, GUI};
use web_time::Instant;

/// Colors of the overlay: egui's dark or light look with an accent of the application ([UiTheme] has all of the style).
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
    /// What egui asked of the platform since Kotlin last took it.
    output: UiOutput,
    pointer: Pointer,
    /// The fingers down, whether each came down on egui.
    fingers: std::collections::HashMap<u64, bool>,
    egui: Option<egui::Context>,
    pixels_per_point: f32,
    /// The style from Kotlin; [Shared::apply_style] puts it onto egui when it changed.
    theme: Option<UiTheme>,
    style_changed: bool,
    /// Whether the system looks dark, if Kotlin said: for a theme that follows it.
    system_dark: Option<bool>,
    fonts: Vec<UiFont>,
    fonts_changed: bool,
    /// The user's zoom (Ctrl and Plus, Minus, 0), on top of [Shared::pixels_per_point].
    zoom: f32,
    zoom_keys: bool,
    /// The modifiers as [EguiOverlay::input] saw them, before the frame turns the input into egui's events.
    early: Held,
    /// egui's memory from [EguiOverlay::restore_state] before there was an egui.
    memory: Option<egui::Memory>,
}

/// egui with its painter, which belongs to the thread and the three-d context it was made on.
struct Painting {
    gui: GUI,
    /// The context it paints on; contexts differ by their caches of programs.
    context: Context,
    started: Instant,
    /// The viewport of the last frame egui ran in; `None` before the first.
    viewport: Option<Viewport>,
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
                output: UiOutput::default(),
                pointer: Pointer::Free,
                fingers: Default::default(),
                egui: None,
                pixels_per_point: 1.0,
                theme: None,
                style_changed: false,
                system_dark: None,
                fonts: Vec::new(),
                fonts_changed: false,
                zoom: 1.0,
                zoom_keys: true,
                early: Held::default(),
                memory: None,
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

    /// The text of the rows of every [crate::Widget::Table] whose `data` is `key`: `cells` row by row, `columns` per row
    /// (a table shows as many of them as it has columns). Replaces what `key` had. Kotlin sends the rows when they
    /// change, not with every tree; the table draws only the rows that are seen, so tens of thousands are fine.
    pub fn set_table_data(
        &self,
        key: String,
        columns: u32,
        cells: Vec<String>,
    ) -> Result<(), UiError> {
        let invalid = |why: String| UiError::InvalidData { why };
        if columns == 0 {
            return Err(invalid(format!(
                "the rows of '{key}' need at least one cell"
            )));
        }
        if !cells.len().is_multiple_of(columns as usize) {
            return Err(invalid(format!(
                "{} cells of '{key}' do not fill rows of {columns}",
                cells.len()
            )));
        }
        let mut shared = lock(&self.shared);
        let data = crate::state::TableData {
            columns: columns as usize,
            cells,
        };
        if shared.ui.tables.insert(key, data.clone()).as_ref() != Some(&data) {
            shared.ui.dirty = true;
        }
        Ok(())
    }

    /// More rows at the end of the rows of `key`, as many cells per row as [EguiOverlay::set_table_data] gave: a log
    /// that grows without sending what it had.
    pub fn append_table_rows(&self, key: String, cells: Vec<String>) -> Result<(), UiError> {
        let mut shared = lock(&self.shared);
        let Some(data) = shared.ui.tables.get_mut(&key) else {
            return Err(UiError::InvalidData {
                why: format!("'{key}' has no rows yet: set_table_data first"),
            });
        };
        if !cells.len().is_multiple_of(data.columns) {
            return Err(UiError::InvalidData {
                why: format!(
                    "{} cells do not fill rows of {} of '{key}'",
                    cells.len(),
                    data.columns
                ),
            });
        }
        data.cells.extend(cells);
        shared.ui.dirty = true;
        Ok(())
    }

    /// Forgets the rows of `key`; a table that still names it shows none.
    pub fn remove_table_data(&self, key: String) {
        let mut shared = lock(&self.shared);
        if shared.ui.tables.remove(&key).is_some() {
            shared.ui.dirty = true;
        }
    }

    /// An image for [crate::Widget::Image] and the other nodes that name `key`: the bytes of a PNG, JPEG, GIF, WebP or
    /// SVG file, which egui_extras decodes when it is first drawn (natively on a thread of its own, a spinner shows
    /// until then). Replaces what `key` had. Kotlin sends an image once, the tree only names it.
    pub fn set_image(&self, key: String, bytes: Vec<u8>) -> Result<(), UiError> {
        lock(&self.shared).ui.set_image(key, bytes)
    }

    /// An image from pixels: `width` × `height` colors of four bytes each (red, green, blue, alpha, not premultiplied),
    /// row by row from the top; a preview of the 3D view, for example. Replaces what `key` had.
    pub fn set_image_rgba(
        &self,
        key: String,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), UiError> {
        lock(&self.shared)
            .ui
            .set_image_rgba(key, width, height, rgba)
    }

    /// Forgets the image of `key`; nodes that still name it show nothing.
    pub fn remove_image(&self, key: String) {
        let mut shared = lock(&self.shared);
        if let Some(old) = shared.ui.images.remove(&key) {
            shared.ui.forget_image(old);
            shared.ui.dirty = true;
        }
    }

    /// Shapes for every [crate::Widget::Canvas] whose `data` is `key`, drawn under its own shapes: many shapes that
    /// seldom change (a floor plan) go once, not with every tree. Replaces what `key` had.
    pub fn set_canvas_shapes(
        &self,
        key: String,
        shapes: Vec<crate::CanvasShape>,
    ) -> Result<(), UiError> {
        if let Some((i, why)) = shapes
            .iter()
            .enumerate()
            .find_map(|(i, s)| s.check().map(|why| (i, why)))
        {
            return Err(UiError::InvalidData {
                why: format!("shape {i} of '{key}': {why}"),
            });
        }
        let mut shared = lock(&self.shared);
        if shared.ui.shapes.get(&key) != Some(&shapes) {
            shared.ui.shapes.insert(key, shapes);
            shared.ui.dirty = true;
        }
        Ok(())
    }

    /// Forgets the shapes of `key`.
    pub fn remove_canvas_shapes(&self, key: String) {
        let mut shared = lock(&self.shared);
        if shared.ui.shapes.remove(&key).is_some() {
            shared.ui.dirty = true;
        }
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

    /// What egui wants of the platform since the last call: text for the clipboard, a wish to paste, links to open, the
    /// look of the pointer, where text is typed. Take it after each frame, as the events.
    pub fn take_output(&self) -> UiOutput {
        lock(&self.shared).output.take()
    }

    /// Gives node `id` the keyboard with the next frame (a text field, a button for Enter and Space), as Compose's
    /// `FocusRequester.requestFocus()`; `None` takes it from whoever has it.
    pub fn request_focus(&self, id: Option<u64>) {
        let mut shared = lock(&self.shared);
        shared.ui.focus = Some(id.map_or(Focus::Clear, Focus::Node));
        shared.ui.dirty = true;
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
        self.set_theme(UiTheme {
            dark: Some(style.dark),
            accent: style.accent,
            ..Default::default()
        });
    }

    /// egui's whole style from Kotlin (Compose's `MaterialTheme`): colors, corners, room, outlines, shadows, the sizes
    /// and fonts of text. What it leaves out is egui's; a new theme replaces the old one completely.
    pub fn set_theme(&self, theme: UiTheme) {
        let mut shared = lock(&self.shared);
        shared.theme = Some(theme);
        shared.style_changed = true;
        shared.ui.dirty = true;
    }

    /// Whether the system looks dark (`None` if it does not say): a [UiTheme] without `dark` follows it. Kotlin tells
    /// it at the start and when it changes (Compose's `isSystemInDarkTheme()`, `prefers-color-scheme` in the browser).
    pub fn set_system_theme(&self, dark: Option<bool>) {
        let mut shared = lock(&self.shared);
        if shared.system_dark != dark {
            shared.system_dark = dark;
            shared.style_changed = true;
            shared.ui.dirty = true;
        }
    }

    /// Fonts from Kotlin besides egui's: a text font, one for code, families of their own for [crate::UiTextSize]
    /// (headings), icon fonts as fallbacks. Replaces the fonts given before; an empty list leaves egui's.
    pub fn set_fonts(&self, fonts: Vec<UiFont>) -> Result<(), UiError> {
        crate::theme::check_fonts(&fonts).map_err(|why| UiError::InvalidData { why })?;
        let mut shared = lock(&self.shared);
        shared.ui.font_families = crate::theme::named_families(&fonts);
        shared.fonts = fonts;
        shared.fonts_changed = true;
        shared.style_changed = shared.theme.is_some();
        shared.ui.dirty = true;
        Ok(())
    }

    /// The user's zoom, 1 at egui's size, on top of [EguiOverlay::set_scale] (as Ctrl and Plus has it in a browser).
    pub fn set_zoom(&self, factor: f32) {
        let mut shared = lock(&self.shared);
        let zoom = zoom_within(factor);
        if zoom != shared.zoom {
            shared.zoom = zoom;
            shared.ui.dirty = true;
        }
    }

    /// The user's zoom.
    pub fn zoom(&self) -> f32 {
        lock(&self.shared).zoom
    }

    /// Whether Ctrl and Plus, Minus or 0 (Command on macOS) zoom the overlay: [UiEvent::ZoomChanged]. On by default.
    pub fn set_zoom_keys(&self, enabled: bool) {
        lock(&self.shared).zoom_keys = enabled;
    }

    /// What egui keeps of the user's doing, as bytes for the application to store: where windows were moved and how
    /// large they were made, sections opened, widths of columns, views of diagrams, scroll positions, the zoom.
    /// [EguiOverlay::restore_state] brings it back, also in the next run (node ids that stay give the same places).
    pub fn save_state(&self) -> Vec<u8> {
        let shared = lock(&self.shared);
        let zoom = shared.zoom;
        let text = match (&shared.egui, &shared.memory) {
            (Some(egui), _) => {
                egui.data_mut(|d| d.insert_persisted(zoom_id(), zoom));
                egui.memory(ron::to_string)
            }
            (None, Some(memory)) => {
                let mut memory = memory.clone();
                memory.data.insert_persisted(zoom_id(), zoom);
                ron::to_string(&memory)
            }
            (None, None) => {
                let mut memory = egui::Memory::default();
                memory.data.insert_persisted(zoom_id(), zoom);
                ron::to_string(&memory)
            }
        };
        text.map(String::into_bytes).unwrap_or_default()
    }

    /// Brings back what [EguiOverlay::save_state] gave (an empty list does nothing). Kotlin's own state (values, open
    /// panels it owns) comes from Kotlin's tree as always.
    pub fn restore_state(&self, bytes: Vec<u8>) -> Result<(), UiError> {
        if bytes.is_empty() {
            return Ok(());
        }
        let invalid = |e: &dyn std::fmt::Display| UiError::InvalidData {
            why: format!("not a state of save_state: {e}"),
        };
        let text = std::str::from_utf8(&bytes).map_err(|e| invalid(&e))?;
        let mut memory: egui::Memory = ron::from_str(text).map_err(|e| invalid(&e))?;
        let mut shared = lock(&self.shared);
        if let Some(zoom) = memory.data.get_persisted::<f32>(zoom_id()) {
            shared.zoom = zoom_within(zoom);
        }
        match shared.egui.clone() {
            Some(egui) => {
                egui.memory_mut(|m| *m = memory);
                own_zoom(&egui);
            }
            None => shared.memory = Some(memory),
        }
        // The memory has egui's options with its styles: the theme from Kotlin counts
        shared.style_changed = shared.theme.is_some();
        shared.ui.dirty = true;
        Ok(())
    }

    /// Whether the pixel (from the top left) lies on a panel, window or area of the last frame.
    pub fn covers(&self, x: f32, y: f32) -> bool {
        let shared = lock(&self.shared);
        let s = shared.scale();
        shared.over(x / s, y / s)
    }

    /// Where node `id` was drawn in the last frame, in pixels from the top left; `None` if it was not drawn (a closed
    /// section, a hidden window). For tests that click on a node, and to place things next to it.
    pub fn bounds(&self, id: u64) -> Option<UiRect> {
        let shared = lock(&self.shared);
        let s = shared.scale();
        let ui = &shared.ui;
        ui.screen_bounds
            .get(&id)
            .or_else(|| ui.bounds.get(&id))
            .map(|r| UiRect {
                x: r.x * s,
                y: r.y * s,
                width: r.width * s,
                height: r.height * s,
            })
    }
}

/// Where the overlay keeps its zoom in egui's memory.
fn zoom_id() -> egui::Id {
    egui::Id::new("three-d-egui-zoom")
}

/// A zoom as egui has it: from 0.2 to 5.
fn zoom_within(factor: f32) -> f32 {
    if factor.is_finite() {
        factor.clamp(0.2, 5.0)
    } else {
        1.0
    }
}

/// The overlay zooms itself: three-d sets egui's points per pixel every frame, which would undo egui's own zoom.
fn own_zoom(egui: &egui::Context) {
    egui.options_mut(|o| o.zoom_with_keyboard = false);
}

impl Shared {
    /// Pixels per point of egui: the density and the user's zoom.
    fn scale(&self) -> f32 {
        self.pixels_per_point * self.zoom
    }

    /// The zoom after `input` if it is Ctrl and Plus, Minus or 0 (with the modifiers of the frame).
    fn zoom_key(&self, input: &UiInput, modifiers: egui::Modifiers) -> Option<f32> {
        let UiInput::Key { key, down: true } = input else {
            return None;
        };
        if !self.zoom_keys || !modifiers.command {
            return None;
        }
        // In steps of a tenth, as egui's own
        let step = |d: f32| ((self.zoom + d).clamp(0.2, 5.0) * 10.0).round() / 10.0;
        match crate::input::key_named(key)? {
            egui::Key::Plus | egui::Key::Equals => Some(step(0.1)),
            egui::Key::Minus => Some(step(-0.1)),
            egui::Key::Num0 => Some(1.0),
            _ => None,
        }
    }

    /// Whether the point (in points) is on egui: on a window, an area or a popup (their own layers), or on a panel.
    fn over(&self, x: f32, y: f32) -> bool {
        let on_layer = self.egui.as_ref().is_some_and(|egui| {
            egui.layer_id_at(egui::pos2(x, y))
                .is_some_and(|layer| layer.order != egui::Order::Background)
        });
        on_layer || self.ui.panels.iter().any(|p| p.contains(x, y))
    }

    fn uses(&mut self, event: &UiInput) -> bool {
        let s = self.scale();
        if let UiInput::Key { key, down } = event {
            self.early.key(key, *down);
            if self.zoom_key(event, self.early.modifiers()).is_some() {
                return true;
            }
        }
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
            UiInput::Key { .. }
            | UiInput::Text { .. }
            | UiInput::Copy
            | UiInput::Cut
            | UiInput::Paste { .. }
            | UiInput::ImeStart
            | UiInput::ImeCompose { .. }
            | UiInput::ImeCommit { .. }
            | UiInput::ImeEnd => self
                .egui
                .as_ref()
                .is_some_and(|egui| egui.egui_wants_keyboard_input()),
            UiInput::Leave => false,
            // A finger belongs to egui if it came down on it, until it comes up
            UiInput::Touch {
                id, phase, x, y, ..
            } => match phase {
                crate::UiTouchPhase::Start => {
                    let over = self.over(x / s, y / s);
                    self.fingers.insert(id, over);
                    over
                }
                crate::UiTouchPhase::Move => self.fingers.get(&id).copied().unwrap_or(false),
                crate::UiTouchPhase::End | crate::UiTouchPhase::Cancel => {
                    self.fingers.remove(&id).unwrap_or(false)
                }
            },
        }
    }

    fn apply_style(&mut self, egui: &egui::Context) {
        if std::mem::take(&mut self.fonts_changed) {
            egui.set_fonts(crate::theme::font_definitions(&self.fonts));
        }
        if !std::mem::take(&mut self.style_changed) {
            return;
        }
        let Some(theme) = &self.theme else {
            return;
        };
        let dark = theme.dark.or(self.system_dark).unwrap_or(true);
        let (egui_theme, visuals) = if dark {
            (egui::Theme::Dark, egui::Visuals::dark())
        } else {
            (egui::Theme::Light, egui::Visuals::light())
        };
        // From egui's style on, so that what a new theme leaves out is egui's again
        let mut style = egui::Style {
            visuals,
            ..Default::default()
        };
        crate::theme::apply(&mut style, theme, &self.ui.font_families);
        egui.set_theme(egui_theme);
        egui.set_style_of(egui_theme, style);
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
            own_zoom(gui.context());
            {
                let mut shared = lock(&self.shared);
                if let Some(memory) = shared.memory.take() {
                    gui.context().memory_mut(|m| *m = memory);
                    own_zoom(gui.context());
                }
                shared.egui = Some(gui.context().clone());
                shared.fonts_changed = !shared.fonts.is_empty();
            }
            *painting = Some(ThreadBound::new(Painting {
                gui,
                context: context.clone(),
                started: Instant::now(),
                viewport: None,
            }));
            let mut shared = lock(&self.shared);
            shared.style_changed = shared.theme.is_some();
        }
        let painting = painting.as_ref().expect("made above");
        painting.with_mut(|p| -> Result<(), UiError> {
            let mut shared = lock(&self.shared);
            let shared = &mut *shared;
            // Nothing changed since the last frame (no tree, no input, no animation of egui, the same size): its
            // meshes are painted again without running egui
            let unchanged = !shared.ui.dirty
                && shared.input.is_empty()
                && events.is_empty()
                && p.viewport == Some(viewport)
                && !p.gui.context().has_requested_repaint();
            if unchanged {
                // p.viewport is only set after a frame egui ran in, so there are meshes
                write(&|| p.gui.render_previous().map(|_| ()))?;
                return Ok(());
            }
            shared.apply_style(p.gui.context());
            let mut wanted = Wanted::default();
            let mut own = Vec::new();
            for input in std::mem::take(&mut shared.input) {
                if let Some(zoom) = shared.zoom_key(&input, shared.held.modifiers()) {
                    if zoom != shared.zoom {
                        shared.zoom = zoom;
                        shared.ui.push(UiEvent::ZoomChanged { factor: zoom });
                    }
                }
                let scale = shared.scale();
                own.extend(shared.held.events(input, scale, &mut wanted));
            }
            let scale = shared.scale();
            shared.output.paste |= wanted.paste;
            shared.ui.dirty = false;
            let ms = p.started.elapsed().as_secs_f64() * 1000.0;
            let ui = &mut shared.ui;
            p.gui.update_with_egui_events(
                own,
                shared.held.modifiers(),
                events,
                ms,
                viewport,
                scale,
                |root| crate::draw::draw(root, ui),
            );
            shared.output.add(p.gui.take_platform_output(), scale);
            write(&|| p.gui.render())?;
            p.viewport = Some(viewport);
            Ok(())
        })?
    }
}
