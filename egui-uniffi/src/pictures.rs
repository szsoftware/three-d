//! Images and the canvas: what Kotlin draws instead of describing widgets.

use crate::state::{ImageBytes, UiState};
use crate::{CanvasShape, CanvasStroke, ImageFit, UiEvent, UiNode, UiPoint, UiRect, Widget};
use three_d::egui::{self, emath, epaint, Color32, Pos2, Response, Shape, Stroke, Ui, Vec2};

/// Hands the images Kotlin gave since the last frame to egui (all of them to an egui it has not seen) and lets egui
/// forget the replaced ones.
pub(crate) fn hand_images(ctx: &egui::Context, state: &mut UiState) {
    for uri in state.forget.drain(..) {
        ctx.forget_image(&uri);
    }
    if state.images.is_empty() {
        return;
    }
    if state.images_in.as_ref() != Some(ctx) {
        for image in state.images.values_mut() {
            image.handed = false;
            image.texture = None;
        }
        state.images_in = Some(ctx.clone());
    }
    #[cfg(feature = "images")]
    if state.images.values().any(|i| !i.handed) {
        // Safe to call again: only loaders egui does not have yet are added
        egui_extras::install_image_loaders(ctx);
    }
    for (key, image) in state.images.iter_mut().filter(|(_, i)| !i.handed) {
        match &image.source {
            ImageBytes::Encoded { uri, bytes } => ctx.include_bytes(uri.clone(), bytes.clone()),
            ImageBytes::Pixels { size, rgba } => {
                let pixels = egui::ColorImage::from_rgba_unmultiplied(*size, rgba);
                image.texture = Some(ctx.load_texture(
                    format!("three-d-egui/{key}"),
                    pixels,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        image.handed = true;
    }
}

/// What egui draws for the image of `key`, `None` if Kotlin gave none.
pub(crate) fn image_source(state: &UiState, key: &str) -> Option<egui::ImageSource<'static>> {
    let image = state.images.get(key)?;
    match &image.source {
        ImageBytes::Encoded { uri, .. } => Some(egui::ImageSource::Uri(uri.clone().into())),
        ImageBytes::Pixels { .. } => image
            .texture
            .as_ref()
            .map(|t| egui::ImageSource::Texture(egui::load::SizedTexture::from_handle(t))),
    }
}

/// The texture of the image of `key` for `size` points, once it is decoded.
pub(crate) fn texture(
    ui: &Ui,
    state: &UiState,
    key: &str,
    size: Vec2,
) -> Option<egui::load::SizedTexture> {
    let image = egui::Image::new(image_source(state, key)?).fit_to_exact_size(size);
    match image.load_for_size(ui.ctx(), size) {
        Ok(egui::load::TexturePoll::Ready { texture }) => Some(texture),
        _ => None,
    }
}

/// A [Widget::Image].
pub(crate) fn image(ui: &mut Ui, state: &UiState, node: &UiNode) -> Response {
    let Widget::Image {
        image,
        width,
        height,
        fit,
        tint,
        corner_radius,
        rotation,
        description,
    } = &node.widget
    else {
        unreachable!("only for images")
    };
    let Some(source) = image_source(state, image) else {
        // The room it would take, so that nothing jumps when the image comes
        let size = egui::vec2(width.unwrap_or(0.0), height.unwrap_or(0.0));
        return ui.allocate_response(size, egui::Sense::hover());
    };
    let mut picture = egui::Image::new(source).show_loading_spinner(true);
    match (width, height) {
        (Some(w), Some(h)) => {
            let room = egui::vec2(*w, *h);
            picture = picture.fit_to_exact_size(room);
            match fit.unwrap_or(ImageFit::Contain) {
                ImageFit::Contain => {}
                ImageFit::Fill => picture = picture.maintain_aspect_ratio(false),
                ImageFit::Cover => {
                    picture = picture.maintain_aspect_ratio(false);
                    // The middle of the image in the shape of the room
                    if let Ok(egui::load::TexturePoll::Ready { texture }) =
                        picture.load_for_size(ui.ctx(), room)
                    {
                        picture = picture.uv(cover(texture.size, room));
                    }
                }
            }
        }
        (Some(w), None) => picture = picture.fit_to_exact_size(egui::vec2(*w, f32::INFINITY)),
        (None, Some(h)) => picture = picture.fit_to_exact_size(egui::vec2(f32::INFINITY, *h)),
        (None, None) => {
            picture = picture
                .fit_to_original_size(1.0)
                .max_width(ui.available_width())
        }
    }
    if let Some(tint) = tint {
        picture = picture.tint(argb(*tint));
    }
    if let Some(r) = corner_radius {
        picture = picture.corner_radius(*r);
    }
    if let Some(degrees) = rotation {
        picture = picture.rotate(degrees.to_radians(), egui::Vec2::splat(0.5));
    }
    if let Some(text) = description {
        picture = picture.alt_text(text.as_str());
    }
    ui.add(picture)
}

/// The part of an image of `size` that covers a room of the shape of `room`, in the middle.
fn cover(size: Vec2, room: Vec2) -> egui::Rect {
    let (image, wanted) = (size.x / size.y, room.x / room.y);
    if !(image.is_finite() && wanted.is_finite() && image > 0.0 && wanted > 0.0) {
        return egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
    }
    if image > wanted {
        let part = wanted / image;
        egui::Rect::from_x_y_ranges((1.0 - part) / 2.0..=(1.0 + part) / 2.0, 0.0..=1.0)
    } else {
        let part = image / wanted;
        egui::Rect::from_x_y_ranges(0.0..=1.0, (1.0 - part) / 2.0..=(1.0 + part) / 2.0)
    }
}

pub(crate) fn argb(c: u32) -> Color32 {
    let [a, r, g, b] = c.to_be_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// From the coordinates of a canvas to the screen and back.
#[derive(Debug, Clone, Copy)]
struct Mapping {
    /// The screen point of the canvas' origin with `scale` applied: screen = offset + scale * point.
    offset: Vec2,
    scale: Vec2,
}

impl Mapping {
    fn new(rect: egui::Rect, view: Option<UiRect>, stretch: bool, y_up: bool) -> Self {
        let (min, size) = match view {
            Some(v) => (egui::vec2(v.x, v.y), egui::vec2(v.width, v.height)),
            None => (Vec2::ZERO, rect.size()),
        };
        let mut scale = rect.size() / size;
        if !stretch {
            scale = Vec2::splat(scale.min_elem());
        }
        // The view in the middle of the canvas, y from the bottom if it grows upwards
        let used = size * scale;
        let corner = rect.min + (rect.size() - used) / 2.0;
        if y_up {
            Self {
                offset: egui::vec2(
                    corner.x - min.x * scale.x,
                    corner.y + used.y + min.y * scale.y,
                ),
                scale: egui::vec2(scale.x, -scale.y),
            }
        } else {
            Self {
                offset: corner.to_vec2() - min * scale,
                scale,
            }
        }
    }

    fn screen(&self, x: f32, y: f32) -> Pos2 {
        (self.offset + egui::vec2(x, y) * self.scale).to_pos2()
    }

    fn point(&self, p: Pos2) -> UiPoint {
        let v = (p.to_vec2() - self.offset) / self.scale;
        UiPoint {
            x: v.x as f64,
            y: v.y as f64,
        }
    }

    /// A length in the canvas as points on the screen.
    fn length(&self, l: f32) -> f32 {
        l * self.scale.x.abs().min(self.scale.y.abs())
    }

    fn rect(&self, x: f32, y: f32, width: f32, height: f32) -> egui::Rect {
        egui::Rect::from_two_pos(self.screen(x, y), self.screen(x + width, y + height))
    }

    fn points(&self, coordinates: &[f32]) -> Vec<Pos2> {
        coordinates
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| self.screen(p[0], p[1]))
            .collect()
    }
}

fn stroke(s: &Option<CanvasStroke>) -> Stroke {
    s.map_or(Stroke::NONE, |s| Stroke::new(s.width, argb(s.color)))
}

fn fill(f: &Option<u32>) -> Color32 {
    f.map_or(Color32::TRANSPARENT, argb)
}

/// A line through `points`, dashed if its stroke says so.
fn line(shapes: &mut Vec<Shape>, points: Vec<Pos2>, s: &CanvasStroke) {
    let stroke = Stroke::new(s.width, argb(s.color));
    match s.dash {
        Some(dash) => shapes.extend(Shape::dashed_line(&points, stroke, dash, dash)),
        None => shapes.push(Shape::line(points, stroke)),
    }
}

/// The shapes of egui for `shape`.
fn paint(ui: &Ui, state: &UiState, map: &Mapping, shape: &CanvasShape, out: &mut Vec<Shape>) {
    match shape {
        CanvasShape::Line { points, stroke: s } => line(out, map.points(points), s),
        CanvasShape::Polygon {
            points,
            fill: f,
            stroke: s,
        } => {
            let points = map.points(points);
            if f.is_some() {
                out.push(filled(&points, fill(f)));
            }
            if let Some(s) = s {
                let mut closed = points;
                closed.push(closed[0]);
                line(out, closed, s);
            }
        }
        CanvasShape::Rect {
            x,
            y,
            width,
            height,
            fill: f,
            stroke: s,
            corner_radius,
        } => out.push(Shape::Rect(epaint::RectShape::new(
            map.rect(*x, *y, *width, *height),
            *corner_radius,
            fill(f),
            stroke(s),
            egui::StrokeKind::Middle,
        ))),
        CanvasShape::Circle {
            x,
            y,
            radius,
            fill: f,
            stroke: s,
        } => out.push(Shape::Circle(epaint::CircleShape {
            center: map.screen(*x, *y),
            radius: map.length(*radius),
            fill: fill(f),
            stroke: stroke(s),
        })),
        CanvasShape::Ellipse {
            x,
            y,
            radius_x,
            radius_y,
            fill: f,
            stroke: s,
        } => out.push(Shape::Ellipse(epaint::EllipseShape {
            center: map.screen(*x, *y),
            radius: egui::vec2(radius_x * map.scale.x.abs(), radius_y * map.scale.y.abs()),
            angle: 0.0,
            fill: fill(f),
            stroke: stroke(s),
        })),
        CanvasShape::Curve {
            points,
            closed,
            fill: f,
            stroke: s,
        } => {
            let p = map.points(points);
            let stroke = stroke(s);
            out.push(if let [a, b, c, d] = p[..] {
                Shape::CubicBezier(epaint::CubicBezierShape::from_points_stroke(
                    [a, b, c, d],
                    *closed,
                    fill(f),
                    stroke,
                ))
            } else {
                Shape::QuadraticBezier(epaint::QuadraticBezierShape::from_points_stroke(
                    [p[0], p[1], p[2]],
                    *closed,
                    fill(f),
                    stroke,
                ))
            });
        }
        CanvasShape::Arrow {
            x,
            y,
            to_x,
            to_y,
            stroke: s,
        } => {
            let (from, to) = (map.screen(*x, *y), map.screen(*to_x, *to_y));
            let stroke = Stroke::new(s.width, argb(s.color));
            let vector = to - from;
            out.push(Shape::line_segment([from, to], stroke));
            if vector.length() == 0.0 {
                return;
            }
            let tip = vector.length() / 4.0;
            let back = -vector.normalized() * tip;
            let rot = emath::Rot2::from_angle(std::f32::consts::TAU / 10.0);
            out.push(Shape::line_segment([to, to + rot * back], stroke));
            out.push(Shape::line_segment([to, to + rot.inverse() * back], stroke));
        }
        CanvasShape::Text {
            x,
            y,
            text,
            size,
            color,
            anchor,
            monospace,
        } => {
            let style = if *monospace {
                egui::TextStyle::Monospace
            } else {
                egui::TextStyle::Body
            };
            let mut font = style.resolve(ui.style());
            if let Some(size) = size {
                font.size = *size;
            }
            let color = color.map_or(ui.visuals().text_color(), argb);
            let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.clone(), font, color));
            let anchor = anchor.map_or(egui::Align2::LEFT_TOP, crate::draw::align2);
            let rect = anchor.anchor_size(map.screen(*x, *y), galley.size());
            out.push(Shape::galley(rect.min, galley, color));
        }
        CanvasShape::Image {
            image,
            x,
            y,
            width,
            height,
            tint,
        } => {
            let rect = map.rect(*x, *y, *width, *height);
            if let Some(texture) = texture(ui, state, image, rect.size()) {
                let uv = egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
                out.push(Shape::image(
                    texture.id,
                    rect,
                    uv,
                    tint.map_or(Color32::WHITE, argb),
                ));
            }
        }
    }
}

/// A filled polygon of any shape whose edges do not cross: egui fills convex ones itself (with smooth edges), the
/// others become triangles.
fn filled(points: &[Pos2], color: Color32) -> Shape {
    if convex(points) {
        return Shape::convex_polygon(points.to_vec(), color, Stroke::NONE);
    }
    let mut mesh = epaint::Mesh::default();
    for p in points {
        mesh.colored_vertex(*p, color);
    }
    mesh.indices = triangles(points);
    Shape::mesh(mesh)
}

fn cross(o: Pos2, a: Pos2, b: Pos2) -> f32 {
    (a - o).x * (b - o).y - (a - o).y * (b - o).x
}

/// All turns go the same way.
fn convex(points: &[Pos2]) -> bool {
    let n = points.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let c = cross(points[i], points[(i + 1) % n], points[(i + 2) % n]);
        if c.abs() > f32::EPSILON {
            if sign != 0.0 && c.signum() != sign {
                return false;
            }
            sign = c.signum();
        }
    }
    true
}

/// The polygon cut into triangles, ear by ear; what is left of one whose edges cross stays out.
fn triangles(points: &[Pos2]) -> Vec<u32> {
    let n = points.len();
    let area: f32 = (0..n)
        .map(|i| cross(Pos2::ZERO, points[i], points[(i + 1) % n]))
        .sum();
    let turn = area.signum();
    let mut left: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(3 * (n - 2));
    while left.len() > 3 {
        let m = left.len();
        let ear = (0..m).find(|&i| {
            let (a, b, c) = (left[(i + m - 1) % m], left[i], left[(i + 1) % m]);
            let (pa, pb, pc) = (points[a], points[b], points[c]);
            if cross(pa, pb, pc) * turn <= 0.0 {
                return false;
            }
            // No other corner inside the triangle
            !left.iter().any(|&o| {
                o != a
                    && o != b
                    && o != c
                    && cross(pa, pb, points[o]) * turn >= 0.0
                    && cross(pb, pc, points[o]) * turn >= 0.0
                    && cross(pc, pa, points[o]) * turn >= 0.0
            })
        });
        let Some(i) = ear else {
            return out;
        };
        out.extend([left[(i + m - 1) % m], left[i], left[(i + 1) % m]].map(|v| v as u32));
        left.remove(i);
    }
    out.extend(left.iter().map(|&v| v as u32));
    out
}

/// A [Widget::Canvas]: its shapes, then what the user did.
pub(crate) fn canvas(ui: &mut Ui, state: &mut UiState, node: &UiNode) -> Response {
    let Widget::Canvas {
        shapes,
        height,
        width,
        view,
        stretch,
        y_up,
        data,
        clickable,
        report_pointer,
    } = &node.widget
    else {
        unreachable!("only for canvases")
    };
    let width = width.unwrap_or_else(|| ui.available_width());
    let sense = if *clickable {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (response, painter) = ui.allocate_painter(egui::vec2(width, *height), sense);
    let map = Mapping::new(response.rect, *view, *stretch, *y_up);
    let mut out = Vec::new();
    let under = data.as_ref().and_then(|key| state.shapes.get(key));
    for shape in under.into_iter().flatten().chain(shapes) {
        paint(ui, state, &map, shape, &mut out);
    }
    painter.extend(out);
    pointer(
        ui,
        state,
        node,
        &response,
        |p| map.point(p),
        *clickable,
        *report_pointer,
    );
    response
}

/// [UiEvent::PointClicked] and [UiEvent::PointerAt] of a canvas or plot, with `point` turning a position on the
/// screen into its coordinates.
pub(crate) fn pointer(
    ui: &Ui,
    state: &mut UiState,
    node: &UiNode,
    response: &Response,
    point: impl Fn(Pos2) -> UiPoint,
    clickable: bool,
    report_pointer: bool,
) {
    let id = node.id;
    if clickable && response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            state.push(UiEvent::PointClicked {
                id,
                point: point(pos),
            });
        }
    }
    if report_pointer {
        let at = response.hover_pos().map(&point);
        let key = egui::Id::new(("three-d-egui-pointer", id));
        let last = ui.data(|d| d.get_temp::<Option<(f64, f64)>>(key)).flatten();
        let now = at.map(|p| (p.x, p.y));
        if now != last {
            ui.data_mut(|d| d.insert_temp(key, now));
            state.push(UiEvent::PointerAt { id, point: at });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(points: &[Pos2], triangles: &[u32]) -> f32 {
        triangles
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|i| points[t[i] as usize]);
                cross(a, b, c).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn concave_polygons_are_cut_into_triangles_that_cover_them() {
        // An L of area 20 x 4 + 8 x 6 = 128, both ways round
        let l = [
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 4.0),
            (8.0, 4.0),
            (8.0, 10.0),
            (0.0, 10.0),
        ]
        .map(|(x, y)| egui::pos2(x, y));
        assert!(!convex(&l));
        let cut = triangles(&l);
        assert_eq!(cut.len(), 12);
        assert!((area(&l, &cut) - 128.0).abs() < 1e-3);
        let mut reversed = l;
        reversed.reverse();
        assert!((area(&reversed, &triangles(&reversed)) - 128.0).abs() < 1e-3);
        // A square is convex
        let square =
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(x, y)| egui::pos2(x, y));
        assert!(convex(&square));
    }

    #[test]
    fn a_view_keeps_its_shape_in_the_middle_or_is_stretched() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 100.0));
        let view = Some(UiRect {
            x: -5.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        });
        // 10 x 10 in 200 x 100: 10 points per unit, 50 points free on each side
        let kept = Mapping::new(rect, view, false, false);
        assert_eq!(kept.screen(-5.0, 0.0), egui::pos2(60.0, 20.0));
        assert_eq!(kept.screen(5.0, 10.0), egui::pos2(160.0, 120.0));
        let up = Mapping::new(rect, view, false, true);
        assert_eq!(up.screen(-5.0, 0.0), egui::pos2(60.0, 120.0));
        assert_eq!(
            up.point(egui::pos2(110.0, 70.0)),
            UiPoint { x: 0.0, y: 5.0 }
        );
        let stretched = Mapping::new(rect, view, true, false);
        assert_eq!(stretched.screen(5.0, 10.0), egui::pos2(210.0, 120.0));
        // Without a view, points from the top left corner
        let plain = Mapping::new(rect, None, false, false);
        assert_eq!(plain.screen(3.0, 4.0), egui::pos2(13.0, 24.0));
    }
}
