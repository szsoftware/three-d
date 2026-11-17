//!
//! Draws [egui] with the [core](crate::core) module, so with every backend.
//! The OpenGL/WebGL backend uses the painter of `egui_glow` instead, as before.
//!

use crate::core::*;
use std::collections::HashMap;

/// The same as the shaders of `egui_glow`: colors and textures are in gamma space and premultiplied with alpha.
const VERTEX: &str = "
    uniform vec2 screenSize;
    in vec2 position;
    in vec2 uv;
    in vec4 color;
    out vec2 uvs;
    out vec4 col;
    void main() {
        gl_Position = vec4(2.0 * position.x / screenSize.x - 1.0, 1.0 - 2.0 * position.y / screenSize.y, 0.0, 1.0);
        col = color;
        // The first row of the image of egui is the top row, the first row of a texture is the bottom row
        uvs = vec2(uv.x, 1.0 - uv.y);
    }";

const FRAGMENT: &str = "
    uniform sampler2D tex;
    in vec2 uvs;
    in vec4 col;
    layout (location = 0) out vec4 outColor;
    void main() {
        outColor = col * texture(tex, uvs);
    }";

struct Image {
    width: usize,
    height: usize,
    /// Kept to be able to update a part of the image.
    pixels: Vec<[u8; 4]>,
    options: egui::TextureOptions,
    texture: Texture2D,
}

pub(super) struct CorePainter {
    context: Context,
    program: Program,
    images: HashMap<egui::TextureId, Image>,
}

impl CorePainter {
    pub fn new(context: &Context) -> Result<Self, CoreError> {
        Ok(Self {
            context: context.clone(),
            program: Program::from_source(context, VERTEX, FRAGMENT)?,
            images: HashMap::new(),
        })
    }

    ///
    /// Updates the textures and draws the meshes into the given viewport of the render target that is written to.
    ///
    pub fn paint_and_update_textures(
        &mut self,
        viewport: Viewport,
        pixels_per_point: f32,
        clipped_primitives: &[egui::ClippedPrimitive],
        textures_delta: &egui::TexturesDelta,
    ) {
        for (id, delta) in &textures_delta.set {
            self.set_texture(*id, delta);
        }
        let render_states = RenderStates {
            depth_test: DepthTest::Always,
            write_mask: WriteMask::COLOR,
            cull: Cull::None,
            blend: Blend::Enabled {
                source_rgb_multiplier: BlendMultiplierType::One,
                source_alpha_multiplier: BlendMultiplierType::OneMinusDstAlpha,
                destination_rgb_multiplier: BlendMultiplierType::OneMinusSrcAlpha,
                destination_alpha_multiplier: BlendMultiplierType::One,
                rgb_equation: BlendEquationType::Add,
                alpha_equation: BlendEquationType::Add,
            },
        };
        let screen_size = vec2(
            viewport.width as f32 / pixels_per_point,
            viewport.height as f32 / pixels_per_point,
        );
        for egui::ClippedPrimitive {
            clip_rect,
            primitive,
        } in clipped_primitives
        {
            // Callbacks draw with the API of a specific backend, which is not possible here
            let egui::epaint::Primitive::Mesh(mesh) = primitive else {
                continue;
            };
            let Some(image) = self.images.get(&mesh.texture_id) else {
                continue;
            };
            // The clip rectangle in pixels, with the origin in the bottom left corner
            let min_x = ((clip_rect.min.x * pixels_per_point).round() as i32)
                .clamp(0, viewport.width as i32);
            let max_x = ((clip_rect.max.x * pixels_per_point).round() as i32)
                .clamp(min_x, viewport.width as i32);
            let min_y = ((clip_rect.min.y * pixels_per_point).round() as i32)
                .clamp(0, viewport.height as i32);
            let max_y = ((clip_rect.max.y * pixels_per_point).round() as i32)
                .clamp(min_y, viewport.height as i32);
            if max_x == min_x || max_y == min_y || mesh.indices.is_empty() {
                continue;
            }
            self.context.set_scissor(ScissorBox {
                x: viewport.x + min_x,
                y: viewport.y + viewport.height as i32 - max_y,
                width: (max_x - min_x) as u32,
                height: (max_y - min_y) as u32,
            });
            let positions: Vec<Vec2> = mesh
                .vertices
                .iter()
                .map(|v| vec2(v.pos.x, v.pos.y))
                .collect();
            let uvs: Vec<Vec2> = mesh.vertices.iter().map(|v| vec2(v.uv.x, v.uv.y)).collect();
            let colors: Vec<Vec4> = mesh
                .vertices
                .iter()
                .map(|v| {
                    let [r, g, b, a] = v.color.to_array();
                    vec4(r as f32, g as f32, b as f32, a as f32) / 255.0
                })
                .collect();
            let positions = VertexBuffer::new_with_data(&self.context, &positions);
            let uvs = VertexBuffer::new_with_data(&self.context, &uvs);
            let colors = VertexBuffer::new_with_data(&self.context, &colors);
            let elements = ElementBuffer::new_with_data(&self.context, &mesh.indices);
            self.program.use_uniform("screenSize", screen_size);
            self.program.use_texture("tex", &image.texture);
            self.program.use_vertex_attribute("position", &positions);
            self.program.use_vertex_attribute("uv", &uvs);
            self.program.use_vertex_attribute("color", &colors);
            self.program
                .draw_elements(render_states, viewport, &elements);
        }
        // What is drawn after the GUI is not clipped by the last clip rectangle
        self.context.set_scissor(viewport.into());
        for id in &textures_delta.free {
            self.images.remove(id);
        }
    }

    fn set_texture(&mut self, id: egui::TextureId, delta: &egui::epaint::ImageDelta) {
        let egui::ImageData::Color(source) = &delta.image;
        let [width, height] = source.size;
        let source_pixels = source.pixels.iter().map(|c| c.to_array());
        match (delta.pos, self.images.get_mut(&id)) {
            (Some([x, y]), Some(image)) => {
                for (row, pixels) in source.pixels.chunks_exact(width).enumerate() {
                    let start = (y + row) * image.width + x;
                    for (target, pixel) in image.pixels[start..start + width].iter_mut().zip(pixels)
                    {
                        *target = pixel.to_array();
                    }
                }
                image.texture.fill(&image.pixels);
            }
            _ => {
                let pixels: Vec<[u8; 4]> = source_pixels.collect();
                let texture = Self::texture(&self.context, width, height, &pixels, delta.options);
                self.images.insert(
                    id,
                    Image {
                        width,
                        height,
                        pixels,
                        options: delta.options,
                        texture,
                    },
                );
            }
        }
        // The options can change together with a part of the image
        if let Some(image) = self.images.get_mut(&id) {
            if image.options != delta.options {
                image.options = delta.options;
                image.texture = Self::texture(
                    &self.context,
                    image.width,
                    image.height,
                    &image.pixels,
                    image.options,
                );
            }
        }
    }

    fn texture(
        context: &Context,
        width: usize,
        height: usize,
        pixels: &[[u8; 4]],
        options: egui::TextureOptions,
    ) -> Texture2D {
        let interpolation = |filter| match filter {
            egui::TextureFilter::Nearest => Interpolation::Nearest,
            egui::TextureFilter::Linear => Interpolation::Linear,
        };
        let wrapping = match options.wrap_mode {
            egui::TextureWrapMode::ClampToEdge => Wrapping::ClampToEdge,
            egui::TextureWrapMode::Repeat => Wrapping::Repeat,
            egui::TextureWrapMode::MirroredRepeat => Wrapping::MirroredRepeat,
        };
        let texture = Texture2D::new_empty::<[u8; 4]>(
            context,
            width as u32,
            height as u32,
            interpolation(options.minification),
            interpolation(options.magnification),
            None,
            wrapping,
            wrapping,
        );
        texture.fill(pixels);
        texture
    }
}
