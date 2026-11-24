//!
//! The look of the overlay from Kotlin, as Compose's `MaterialTheme`: egui's whole style ([UiTheme]) for all of it or for
//! a node and the nodes inside it, fonts ([UiFont]) and the dark or light look of the system.
//!

use crate::tree::{TextStyle, UiShadow};
use std::collections::HashSet;
use three_d::egui::{self, Color32, CornerRadius, FontFamily};

/// egui's style from Kotlin: what is left out stays as egui has it (or as the theme around a node has it).
#[derive(Debug, Clone, PartialEq, Default, uniffi::Record)]
pub struct UiTheme {
    /// Dark or light. For the whole overlay `None` follows the system ([crate::EguiOverlay::set_system_theme]); for a
    /// node it keeps the look around it.
    #[uniffi(default)]
    pub dark: Option<bool>,
    /// Selections, slider tracks and links in this color (ARGB, Compose's `Color.toArgb()`).
    #[uniffi(default)]
    pub accent: Option<u32>,
    /// Colors of text, backgrounds and widgets.
    #[uniffi(default)]
    pub colors: Option<UiColors>,
    /// Corners of buttons, fields and frames of widgets, in points.
    #[uniffi(default)]
    pub corner_radius: Option<f32>,
    /// Corners of windows, menus and popups, in points.
    #[uniffi(default)]
    pub window_corner_radius: Option<f32>,
    /// Width of the outlines of widgets and separators, in points (egui draws buttons without one).
    #[uniffi(default)]
    pub stroke_width: Option<f32>,
    /// Room between and inside widgets.
    #[uniffi(default)]
    pub spacing: Option<UiSpacing>,
    /// The shadow of windows.
    #[uniffi(default)]
    pub window_shadow: Option<UiShadow>,
    /// The shadow of menus, popups and tooltips.
    #[uniffi(default)]
    pub popup_shadow: Option<UiShadow>,
    /// Size and font of the kinds of text ([TextStyle]); buttons have the size of [TextStyle::Body].
    #[uniffi(default)]
    pub text_sizes: Vec<UiTextSize>,
    /// How long egui's animations take (opening sections, hover), in seconds; 0 switches them off.
    #[uniffi(default)]
    pub animation_time: Option<f32>,
}

/// Colors of a [UiTheme], each as ARGB; `None` keeps egui's.
#[derive(Debug, Clone, Copy, PartialEq, Default, uniffi::Record)]
pub struct UiColors {
    /// Text and the marks of widgets (ticks, arrows).
    #[uniffi(default)]
    pub text: Option<u32>,
    /// [TextStyle::Weak] text.
    #[uniffi(default)]
    pub weak_text: Option<u32>,
    /// Panels and the background of the central panel.
    #[uniffi(default)]
    pub background: Option<u32>,
    /// Windows, menus and popups.
    #[uniffi(default)]
    pub window: Option<u32>,
    /// The outline of windows, menus and popups.
    #[uniffi(default)]
    pub window_border: Option<u32>,
    /// Text fields and the tracks of sliders.
    #[uniffi(default)]
    pub field: Option<u32>,
    /// Every other row of striped tables and lists.
    #[uniffi(default)]
    pub stripe: Option<u32>,
    /// Buttons, boxes and handles at rest.
    #[uniffi(default)]
    pub widget: Option<u32>,
    /// Widgets under the pointer.
    #[uniffi(default)]
    pub widget_hovered: Option<u32>,
    /// Widgets pressed or open.
    #[uniffi(default)]
    pub widget_active: Option<u32>,
    /// Outlines of widgets, separators and frames.
    #[uniffi(default)]
    pub border: Option<u32>,
    /// Text on a selection (on the accent).
    #[uniffi(default)]
    pub selection_text: Option<u32>,
    /// Links (else the accent).
    #[uniffi(default)]
    pub link: Option<u32>,
    /// Warnings.
    #[uniffi(default)]
    pub warn: Option<u32>,
    /// Errors.
    #[uniffi(default)]
    pub error: Option<u32>,
}

/// Room of a [UiTheme] in points; `None` keeps egui's.
#[derive(Debug, Clone, Copy, PartialEq, Default, uniffi::Record)]
pub struct UiSpacing {
    /// Between widgets side by side.
    #[uniffi(default)]
    pub item_x: Option<f32>,
    /// Between widgets one above the other.
    #[uniffi(default)]
    pub item_y: Option<f32>,
    /// Inside buttons, left and right.
    #[uniffi(default)]
    pub button_x: Option<f32>,
    /// Inside buttons, above and below.
    #[uniffi(default)]
    pub button_y: Option<f32>,
    /// How far sections and trees indent.
    #[uniffi(default)]
    pub indent: Option<f32>,
    /// How high widgets are at least (what a finger needs: 40 or more).
    #[uniffi(default)]
    pub min_height: Option<f32>,
    /// Inside windows.
    #[uniffi(default)]
    pub window_padding: Option<f32>,
    /// Width of sliders.
    #[uniffi(default)]
    pub slider_width: Option<f32>,
    /// Width of text fields.
    #[uniffi(default)]
    pub text_field_width: Option<f32>,
    /// Width of scroll bars.
    #[uniffi(default)]
    pub scroll_bar_width: Option<f32>,
}

/// The size and font of a kind of text.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UiTextSize {
    /// The kind; [TextStyle::Strong] and [TextStyle::Weak] are [TextStyle::Body] in another color.
    pub style: TextStyle,
    /// In points.
    pub size: f32,
    /// A family of [UiFont]s, `"monospace"`, or `None` for the text font.
    #[uniffi(default)]
    pub family: Option<String>,
}

/// A font from Kotlin: the bytes of a TTF or OTF file.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UiFont {
    /// Its name, unique among the fonts.
    pub name: String,
    /// The file.
    pub bytes: Vec<u8>,
    /// The family it belongs to: `None` the text font, `"monospace"` the one of code and numbers, any other name a
    /// family of its own for [UiTextSize::family] (egui's fonts stand behind it for what it lacks).
    #[uniffi(default)]
    pub family: Option<String>,
    /// Behind the family's fonts, for what they lack: icons, other scripts. Else it comes first and is the font of the text.
    #[uniffi(default)]
    pub fallback: bool,
    /// Draws its glyphs larger or smaller than the size (icons that look small).
    #[uniffi(default)]
    pub scale: Option<f32>,
    /// Moves its glyphs down by this part of the size (negative up), when they sit too high.
    #[uniffi(default)]
    pub y_offset: Option<f32>,
}

/// One of egui's own panels as a node ([crate::Widget::EguiTool]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EguiTool {
    /// egui's settings: theme, zoom, what is debugged.
    Settings,
    /// egui's style editor: every value of the style.
    Style,
    /// What egui knows of the frame: the widget under the pointer, the layers.
    Inspection,
    /// egui's memory: windows, focus, what is kept.
    Memory,
    /// The textures egui has: fonts, images.
    Textures,
}

/// The family of a name of [UiFont::family] and [UiTextSize::family]; one without fonts is the text font.
pub(crate) fn family(name: Option<&str>, named: &HashSet<String>) -> FontFamily {
    match name {
        None | Some("proportional") => FontFamily::Proportional,
        Some("monospace") => FontFamily::Monospace,
        Some(name) if named.contains(name) => FontFamily::Name(name.into()),
        Some(_) => FontFamily::Proportional,
    }
}

/// Checks the fonts: unique names, files that egui can read.
pub(crate) fn check_fonts(fonts: &[UiFont]) -> Result<(), String> {
    let mut names = HashSet::new();
    for font in fonts {
        if font.name.is_empty() {
            return Err("a font needs a name".into());
        }
        if !names.insert(font.name.as_str()) {
            return Err(format!("two fonts are called '{}'", font.name));
        }
        let read = skrifa::FontRef::from_index(&font.bytes, 0)
            .map_err(|e| format!("'{}' is no TTF or OTF file: {e}", font.name))?;
        use skrifa::MetadataProvider;
        if read.charmap().mappings().next().is_none() {
            return Err(format!("'{}' has no characters", font.name));
        }
    }
    Ok(())
}

/// The named families of the fonts (those that are neither the text font nor monospace).
pub(crate) fn named_families(fonts: &[UiFont]) -> HashSet<String> {
    fonts
        .iter()
        .filter_map(|f| f.family.clone())
        .filter(|f| f != "proportional" && f != "monospace")
        .collect()
}

/// egui's fonts with those of Kotlin.
pub(crate) fn font_definitions(fonts: &[UiFont]) -> egui::FontDefinitions {
    let mut definitions = egui::FontDefinitions::default();
    let named = named_families(fonts);
    for font in fonts {
        let tweak = egui::FontTweak {
            scale: font
                .scale
                .filter(|s| s.is_finite() && *s > 0.0)
                .unwrap_or(1.0),
            y_offset_factor: font.y_offset.filter(|o| o.is_finite()).unwrap_or(0.0),
            ..Default::default()
        };
        definitions.font_data.insert(
            font.name.clone(),
            std::sync::Arc::new(egui::FontData::from_owned(font.bytes.clone()).tweak(tweak)),
        );
        let list = definitions
            .families
            .entry(family(font.family.as_deref(), &named))
            .or_default();
        if font.fallback {
            list.push(font.name.clone());
        } else {
            list.insert(0, font.name.clone());
        }
    }
    // A family of its own falls back on the text font
    let text = definitions.families[&FontFamily::Proportional].clone();
    for name in &named {
        definitions
            .families
            .entry(FontFamily::Name(name.as_str().into()))
            .or_default()
            .extend(text.iter().cloned());
    }
    definitions
}

fn argb(c: u32) -> Color32 {
    let [a, r, g, b] = c.to_be_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// A length of points from Kotlin: finite and not negative.
fn points(v: f32) -> Option<f32> {
    (v.is_finite()).then_some(v.max(0.0))
}

fn radius(v: f32) -> Option<CornerRadius> {
    points(v).map(|v| CornerRadius::same(v.round().min(u8::MAX as f32) as u8))
}

pub(crate) fn shadow(s: &UiShadow) -> egui::Shadow {
    let i = |v: f32| v.round().clamp(i8::MIN as f32, i8::MAX as f32) as i8;
    let u = |v: f32| v.round().clamp(0.0, u8::MAX as f32) as u8;
    egui::Shadow {
        offset: [i(s.dx), i(s.dy)],
        blur: u(s.blur),
        spread: u(s.spread),
        color: argb(s.color),
    }
}

/// What a [TextStyle] is among egui's kinds of text.
fn text_style(style: TextStyle) -> egui::TextStyle {
    match style {
        TextStyle::Body | TextStyle::Strong | TextStyle::Weak => egui::TextStyle::Body,
        TextStyle::Heading => egui::TextStyle::Heading,
        TextStyle::Small => egui::TextStyle::Small,
        TextStyle::Monospace => egui::TextStyle::Monospace,
    }
}

/// Puts `theme` onto `style`: what the theme leaves out stays. `named` are the families of the fonts from Kotlin.
pub(crate) fn apply(style: &mut egui::Style, theme: &UiTheme, named: &HashSet<String>) {
    if let Some(dark) = theme.dark {
        if style.visuals.dark_mode != dark {
            style.visuals = if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };
        }
    }
    let visuals = &mut style.visuals;
    if let Some(accent) = theme.accent {
        let color = argb(accent);
        visuals.selection.bg_fill = color;
        visuals.hyperlink_color = color;
        // The text on a selection stays readable on light and dark accents
        let [_, r, g, b] = accent.to_be_bytes();
        let light = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32 > 150.0;
        visuals.selection.stroke.color = if light {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
    }
    if let Some(c) = &theme.colors {
        colors(visuals, c);
    }
    if let Some(r) = theme.corner_radius.and_then(radius) {
        for w in widgets(visuals) {
            w.corner_radius = r;
        }
    }
    if let Some(r) = theme.window_corner_radius.and_then(radius) {
        visuals.window_corner_radius = r;
        visuals.menu_corner_radius = r;
    }
    if let Some(width) = theme.stroke_width.and_then(points) {
        for w in widgets(visuals) {
            w.bg_stroke.width = width;
        }
        visuals.window_stroke.width = width;
    }
    if let Some(s) = &theme.window_shadow {
        visuals.window_shadow = shadow(s);
    }
    if let Some(s) = &theme.popup_shadow {
        visuals.popup_shadow = shadow(s);
    }
    if let Some(s) = &theme.spacing {
        spacing(&mut style.spacing, s);
    }
    for size in &theme.text_sizes {
        let Some(points) = points(size.size).filter(|p| *p >= 1.0) else {
            continue;
        };
        let font = egui::FontId::new(points, family(size.family.as_deref(), named));
        let kind = text_style(size.style);
        if kind == egui::TextStyle::Body {
            style
                .text_styles
                .insert(egui::TextStyle::Button, font.clone());
        }
        style.text_styles.insert(kind, font);
    }
    if let Some(t) = theme.animation_time.and_then(points) {
        style.animation_time = t;
    }
}

/// The looks of widgets at rest, under the pointer, pressed and open, and of what does not react.
fn widgets(v: &mut egui::Visuals) -> [&mut egui::style::WidgetVisuals; 5] {
    let w = &mut v.widgets;
    [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ]
}

fn colors(v: &mut egui::Visuals, c: &UiColors) {
    if let Some(text) = c.text.map(argb) {
        v.widgets.noninteractive.fg_stroke.color = text;
        v.widgets.inactive.fg_stroke.color = text;
    }
    if let Some(weak) = c.weak_text {
        v.weak_text_color = Some(argb(weak));
    }
    if let Some(bg) = c.background {
        v.panel_fill = argb(bg);
    }
    if let Some(window) = c.window {
        v.window_fill = argb(window);
    }
    if let Some(border) = c.window_border {
        v.window_stroke.color = argb(border);
    }
    if let Some(field) = c.field {
        v.extreme_bg_color = argb(field);
        v.text_edit_bg_color = Some(argb(field));
    }
    if let Some(stripe) = c.stripe {
        v.faint_bg_color = argb(stripe);
    }
    let fill = |w: &mut egui::style::WidgetVisuals, c: Option<u32>| {
        if let Some(c) = c.map(argb) {
            w.bg_fill = c;
            w.weak_bg_fill = c;
        }
    };
    fill(&mut v.widgets.inactive, c.widget);
    fill(&mut v.widgets.hovered, c.widget_hovered);
    fill(&mut v.widgets.active, c.widget_active);
    fill(&mut v.widgets.open, c.widget_active);
    if let Some(border) = c.border.map(argb) {
        v.widgets.noninteractive.bg_stroke.color = border;
        v.widgets.inactive.bg_stroke.color = border;
    }
    if let Some(text) = c.selection_text {
        v.selection.stroke.color = argb(text);
    }
    if let Some(link) = c.link {
        v.hyperlink_color = argb(link);
    }
    if let Some(warn) = c.warn {
        v.warn_fg_color = argb(warn);
    }
    if let Some(error) = c.error {
        v.error_fg_color = argb(error);
    }
}

fn spacing(sp: &mut egui::style::Spacing, s: &UiSpacing) {
    let set = |to: &mut f32, v: Option<f32>| {
        if let Some(v) = v.and_then(points) {
            *to = v;
        }
    };
    set(&mut sp.item_spacing.x, s.item_x);
    set(&mut sp.item_spacing.y, s.item_y);
    set(&mut sp.button_padding.x, s.button_x);
    set(&mut sp.button_padding.y, s.button_y);
    set(&mut sp.indent, s.indent);
    set(&mut sp.interact_size.y, s.min_height);
    set(&mut sp.slider_width, s.slider_width);
    set(&mut sp.text_edit_width, s.text_field_width);
    set(&mut sp.scroll.bar_width, s.scroll_bar_width);
    if let Some(p) = s.window_padding.and_then(points) {
        sp.window_margin = egui::Margin::same(p.round().min(i8::MAX as f32) as i8);
    }
}
