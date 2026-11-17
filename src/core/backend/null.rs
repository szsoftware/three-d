//!
//! A backend without a GPU that only records the calls made to it. Used in unit tests.
//!
//! Programs are "compiled" by scanning the shader sources for `uniform` and `in` declarations,
//! so [Program] behaves like on GL with regards to [Program::requires_uniform] and the panics on unknown names.
//!

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

///
/// One recorded backend call.
///
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Call {
    CreateBuffer(BufferId),
    DeleteBuffer(BufferId),
    WriteBuffer {
        buffer: BufferId,
        kind: BufferKind,
        bytes: Vec<u8>,
        usage: BufferUsage,
    },
    WriteBufferSubset {
        buffer: BufferId,
        kind: BufferKind,
        offset: u32,
        bytes: Vec<u8>,
    },
    CreateTexture(TextureId, TextureDesc),
    DeleteTexture(TextureId),
    WriteTexture {
        texture: TextureId,
        region: TextureRegion,
        format: ColorFormat,
        bytes: Vec<u8>,
    },
    GenerateMipmaps(TextureId),
    CreateRenderbuffer(RenderbufferId),
    DeleteRenderbuffer(RenderbufferId),
    CreateFramebuffer(FramebufferId),
    DeleteFramebuffer(FramebufferId),
    BeginPass(PassTarget, ScissorBox),
    EndPass,
    Clear(PassTarget, ScissorBox, ClearState),
    CopyTarget(PassTarget, PassTarget, CopyMask),
    ReadPixels(PassTarget, ScissorBox, ReadFormat),
    CreateProgram(ProgramId),
    DeleteProgram(ProgramId),
    SetUniform(ProgramId, String, UniformValue),
    BindTexture(u32, TextureId),
    BindUniformBlock(ProgramId, u32, u32, BufferId),
    BindVertexAttribute(ProgramId, u32, BufferId, VertexFormat, u32),
    Draw {
        program: ProgramId,
        /// Debug print, since [RenderStates] does not implement `PartialEq`.
        render_states: String,
        viewport: Viewport,
        indices: Option<IndexBuffer>,
        count: u32,
        instances: Option<u32>,
    },
}

///
/// The recording backend.
///
#[derive(Default)]
pub(crate) struct NullBackend {
    next_id: AtomicU32,
    calls: Mutex<Vec<Call>>,
    uniform_blocks: Mutex<HashMap<(ProgramId, String), u32>>,
    program_sources: Mutex<Vec<(String, String)>>,
}

impl NullBackend {
    /// Returns and forgets all calls recorded so far.
    pub fn take_calls(&self) -> Vec<Call> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }

    /// Returns and forgets the vertex and fragment shader source of all programs created so far.
    #[cfg_attr(not(feature = "wgpu"), allow(dead_code))]
    pub fn take_program_sources(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.program_sources.lock().unwrap())
    }

    fn record(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
    }

    fn id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Finds the names declared with the given storage qualifier, for example `uniform` or `in`.
fn declared_names(source: &str, qualifier: &str) -> Vec<String> {
    let mut names = Vec::new();
    for statement in source.split(';') {
        let tokens: Vec<&str> = statement.split_whitespace().collect();
        if let Some(position) = tokens.iter().position(|t| *t == qualifier) {
            // qualifier [precision] type name
            if let Some(name) = tokens.get(position + 2..).and_then(|rest| rest.last()) {
                if !name.contains('(') && !name.contains('{') && !name.contains('}') {
                    names.push(name.split('[').next().unwrap().to_string());
                }
            }
        }
    }
    names
}

impl BackendApi for NullBackend {
    fn set_viewport(&self, _viewport: Viewport) {}
    fn set_scissor(&self, _scissor_box: ScissorBox) {}
    fn set_cull(&self, _cull: Cull) {}
    fn set_write_mask(&self, _write_mask: WriteMask) {}
    fn set_depth_test(&self, _depth_test: DepthTest) {}
    fn set_blend(&self, _blend: Blend) {}
    fn set_render_states(&self, _render_states: RenderStates) {}
    fn error_check(&self) -> Result<(), CoreError> {
        Ok(())
    }
    fn max_samples(&self) -> u32 {
        16
    }

    fn create_buffer(&self) -> BufferId {
        let id = BufferId::Null(self.id());
        self.record(Call::CreateBuffer(id));
        id
    }
    fn delete_buffer(&self, buffer: BufferId) {
        self.record(Call::DeleteBuffer(buffer));
    }
    fn write_buffer(&self, buffer: BufferId, kind: BufferKind, bytes: &[u8], usage: BufferUsage) {
        self.record(Call::WriteBuffer {
            buffer,
            kind,
            bytes: bytes.to_vec(),
            usage,
        });
    }
    fn write_buffer_subset(&self, buffer: BufferId, kind: BufferKind, offset: u32, bytes: &[u8]) {
        self.record(Call::WriteBufferSubset {
            buffer,
            kind,
            offset,
            bytes: bytes.to_vec(),
        });
    }

    fn create_texture(&self, desc: &TextureDesc) -> TextureId {
        let id = TextureId::Null(self.id());
        self.record(Call::CreateTexture(id, *desc));
        id
    }
    fn delete_texture(&self, texture: TextureId) {
        self.record(Call::DeleteTexture(texture));
    }
    fn write_texture(
        &self,
        texture: TextureId,
        _kind: TextureKind,
        region: TextureRegion,
        _width: u32,
        _height: u32,
        _depth: u32,
        format: ColorFormat,
        bytes: &[u8],
    ) {
        self.record(Call::WriteTexture {
            texture,
            region,
            format,
            bytes: bytes.to_vec(),
        });
    }
    fn generate_mipmaps(&self, texture: TextureId, _kind: TextureKind) {
        self.record(Call::GenerateMipmaps(texture));
    }
    fn create_renderbuffer(
        &self,
        _format: TextureFormat,
        _width: u32,
        _height: u32,
        _number_of_samples: u32,
    ) -> RenderbufferId {
        let id = RenderbufferId::Null(self.id());
        self.record(Call::CreateRenderbuffer(id));
        id
    }
    fn delete_renderbuffer(&self, renderbuffer: RenderbufferId) {
        self.record(Call::DeleteRenderbuffer(renderbuffer));
    }

    fn create_framebuffer(&self) -> FramebufferId {
        let id = FramebufferId::Null(self.id());
        self.record(Call::CreateFramebuffer(id));
        id
    }
    fn delete_framebuffer(&self, framebuffer: FramebufferId) {
        self.record(Call::DeleteFramebuffer(framebuffer));
    }
    fn begin_pass(&self, target: &PassTarget, scissor_box: ScissorBox, _load: LoadOp) {
        self.record(Call::BeginPass(target.clone(), scissor_box));
    }
    fn end_pass(&self) {
        self.record(Call::EndPass);
    }
    fn clear(&self, target: &PassTarget, scissor_box: ScissorBox, clear_state: ClearState) {
        self.record(Call::Clear(target.clone(), scissor_box, clear_state));
    }
    fn copy_target(&self, source: &PassTarget, destination: &PassTarget, mask: CopyMask) {
        self.record(Call::CopyTarget(source.clone(), destination.clone(), mask));
    }
    fn read_pixels(&self, source: &PassTarget, rect: ScissorBox, format: ReadFormat) -> Vec<u8> {
        self.record(Call::ReadPixels(source.clone(), rect, format));
        let pixel_size = match format {
            ReadFormat::Color(format) => {
                format.channels as usize
                    * match format.primitive {
                        PrimitiveType::U8 | PrimitiveType::I8 => 1,
                        PrimitiveType::U16 | PrimitiveType::I16 | PrimitiveType::F16 => 2,
                        _ => 4,
                    }
            }
            ReadFormat::Depth => 4,
        };
        vec![0; rect.width as usize * rect.height as usize * pixel_size]
    }

    fn create_program(
        &self,
        vertex_shader_source: &str,
        fragment_shader_source: &str,
    ) -> Result<(ProgramId, ProgramReflection), CoreError> {
        let id = ProgramId::Null(self.id());
        let mut reflection = ProgramReflection::default();
        for (location, name) in declared_names(vertex_shader_source, "in")
            .into_iter()
            .enumerate()
        {
            reflection.attributes.insert(name, location as u32);
        }
        for name in declared_names(vertex_shader_source, "uniform")
            .into_iter()
            .chain(declared_names(fragment_shader_source, "uniform"))
        {
            reflection
                .uniforms
                .insert(name.clone(), UniformLocation::Null(name));
        }
        self.program_sources.lock().unwrap().push((
            vertex_shader_source.to_string(),
            fragment_shader_source.to_string(),
        ));
        self.record(Call::CreateProgram(id));
        Ok((id, reflection))
    }
    fn delete_program(&self, program: ProgramId) {
        self.record(Call::DeleteProgram(program));
    }
    fn set_uniform(&self, program: ProgramId, location: &UniformLocation, value: &UniformValue) {
        let UniformLocation::Null(name) = location else {
            unreachable!()
        };
        self.record(Call::SetUniform(program, name.clone(), value.clone()));
    }
    fn bind_texture(&self, unit: u32, texture: TextureId, _kind: TextureKind) {
        self.record(Call::BindTexture(unit, texture));
    }
    fn uniform_block_index(&self, program: ProgramId, name: &str) -> Option<u32> {
        let mut blocks = self.uniform_blocks.lock().unwrap();
        let next = blocks.len() as u32;
        Some(*blocks.entry((program, name.to_owned())).or_insert(next))
    }
    fn bind_uniform_block(
        &self,
        program: ProgramId,
        block_index: u32,
        binding: u32,
        buffer: BufferId,
    ) {
        self.record(Call::BindUniformBlock(
            program,
            block_index,
            binding,
            buffer,
        ));
    }
    fn bind_vertex_attribute(
        &self,
        program: ProgramId,
        location: u32,
        buffer: BufferId,
        format: VertexFormat,
        divisor: u32,
    ) {
        self.record(Call::BindVertexAttribute(
            program, location, buffer, format, divisor,
        ));
    }
    fn prepare_attributeless_draw(&self) {}
    fn draw(&self, call: &DrawCall) {
        self.record(Call::Draw {
            program: call.program,
            render_states: format!("{:?}", call.render_states),
            viewport: call.viewport,
            indices: call.indices,
            count: call.count,
            instances: call.instances,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context::from_backend(Backend::Null(NullBackend::default()))
    }

    fn calls(context: &Context) -> Vec<Call> {
        context.null_backend().take_calls()
    }

    const VERTEX: &str = "
        uniform mat4 viewProjection;
        in vec3 position;
        in vec4 color;
        out vec4 col;
        void main() { col = color; gl_Position = viewProjection * vec4(position, 1.0); }
    ";
    const FRAGMENT: &str = "
        uniform vec3 tint;
        uniform sampler2D tex;
        in vec4 col;
        layout (location = 0) out vec4 outColor;
        void main() { outColor = col * vec4(tint, 1.0) * texture(tex, vec2(0.5)); }
    ";

    #[test]
    fn context_without_gl() {
        let context = context();
        assert!(context.as_gl().is_none());
        let result = std::panic::catch_unwind(|| {
            let _ = context.version();
        });
        assert!(result.is_err());
    }

    #[test]
    fn vertex_buffer_writes_bytes() {
        let context = context();
        let mut buffer = VertexBuffer::new_with_data(&context, &[vec2(1.0f32, 2.0)]);
        buffer.fill(&[vec2(3.0f32, 4.0), vec2(5.0, 6.0)]);
        assert_eq!(buffer.vertex_count(), 2);
        assert_eq!(buffer.count(), 4);
        let id = buffer.id();
        drop(buffer);
        let bytes = |values: &[f32]| values.iter().flat_map(|v| v.to_ne_bytes()).collect();
        assert_eq!(
            calls(&context),
            vec![
                Call::CreateBuffer(id),
                Call::WriteBuffer {
                    buffer: id,
                    kind: BufferKind::Vertex,
                    bytes: bytes(&[1.0, 2.0]),
                    usage: BufferUsage::Static,
                },
                Call::WriteBuffer {
                    buffer: id,
                    kind: BufferKind::Vertex,
                    bytes: bytes(&[3.0, 4.0, 5.0, 6.0]),
                    usage: BufferUsage::Dynamic,
                },
                Call::DeleteBuffer(id),
            ]
        );
    }

    #[test]
    fn program_uniforms_and_draw() {
        let context = context();
        let program = Program::from_source(&context, VERTEX, FRAGMENT).unwrap();
        assert!(program.requires_uniform("viewProjection"));
        assert!(program.requires_uniform("tint"));
        assert!(program.requires_attribute("position"));
        assert!(!program.requires_uniform("unknown"));

        let positions = VertexBuffer::new_with_data(&context, &[vec3(0.0f32, 0.0, 0.0); 3]);
        let texture = Texture2D::new_empty::<[u8; 4]>(
            &context,
            2,
            2,
            Interpolation::Nearest,
            Interpolation::Nearest,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        calls(&context);

        program.use_uniform("tint", vec3(1.0f32, 0.5, 0.25));
        program.use_uniform("viewProjection", Mat4::identity());
        program.use_texture("tex", &texture);
        program.use_vertex_attribute("position", &positions);
        program.draw_arrays(RenderStates::default(), Viewport::new_at_origo(4, 4), 3);

        let calls = calls(&context);
        let p = match calls[0] {
            Call::SetUniform(p, _, _) => p,
            _ => panic!(),
        };
        let mut identity = vec![0.0; 16];
        for i in 0..4 {
            identity[i * 5] = 1.0;
        }
        assert_eq!(
            calls,
            vec![
                Call::SetUniform(
                    p,
                    "tint".to_owned(),
                    UniformValue::F32(UniformType::Vec3, vec![1.0, 0.5, 0.25])
                ),
                Call::SetUniform(
                    p,
                    "viewProjection".to_owned(),
                    UniformValue::F32(UniformType::Mat4, identity)
                ),
                Call::SetUniform(
                    p,
                    "tex".to_owned(),
                    UniformValue::I32(UniformType::Value, vec![0])
                ),
                Call::BindTexture(0, texture.id()),
                Call::BindVertexAttribute(
                    p,
                    0, // the first input of the vertex shader
                    positions.id(),
                    VertexFormat {
                        primitive: PrimitiveType::F32,
                        components: 3,
                        normalized: false
                    },
                    0
                ),
                Call::Draw {
                    program: p,
                    render_states: format!("{:?}", RenderStates::default()),
                    viewport: Viewport::new_at_origo(4, 4),
                    indices: None,
                    count: 3,
                    instances: None,
                },
            ]
        );
    }

    #[test]
    #[should_panic(expected = "the uniform unknown is sent to the shader but not defined")]
    fn unknown_uniform_panics() {
        let context = context();
        let program = Program::from_source(&context, VERTEX, FRAGMENT).unwrap();
        program.use_uniform("unknown", 1.0f32);
    }

    #[test]
    fn texture_fill_flips_rows() {
        let context = context();
        let texture = Texture2D::new_empty::<u8>(
            &context,
            2,
            2,
            Interpolation::Nearest,
            Interpolation::Nearest,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        texture.fill(&[1u8, 2, 3, 4]);
        let calls = calls(&context);
        assert!(matches!(
            calls[0],
            Call::CreateTexture(
                _,
                TextureDesc {
                    kind: TextureKind::D2,
                    storage: Some(TextureStorage {
                        format: TextureFormat::Color(ColorFormat {
                            primitive: PrimitiveType::U8,
                            channels: 1
                        }),
                        mip_levels: 1,
                        width: 2,
                        height: 2,
                        ..
                    }),
                    ..
                }
            )
        ));
        assert_eq!(
            calls[1],
            Call::WriteTexture {
                texture: texture.id(),
                region: TextureRegion::All,
                format: ColorFormat {
                    primitive: PrimitiveType::U8,
                    channels: 1
                },
                bytes: vec![3, 4, 1, 2],
            }
        );
    }

    #[test]
    fn mipmaps_are_generated_after_write() {
        let context = context();
        let texture = Texture2D::new_empty::<[u8; 4]>(
            &context,
            8,
            8,
            Interpolation::Linear,
            Interpolation::Linear,
            Some(Mipmap::default()),
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        assert!(texture.number_of_mip_maps() > 1);
        calls(&context);
        texture
            .as_color_target(None)
            .clear(ClearState::color(0.0, 0.0, 0.0, 1.0))
            .write::<CoreError>(|| Ok(()))
            .unwrap();
        let calls = calls(&context);
        let framebuffers: Vec<_> = calls
            .iter()
            .filter_map(|c| match c {
                Call::CreateFramebuffer(f) => Some(*f),
                _ => None,
            })
            .collect();
        let target = |framebuffer| PassTarget {
            framebuffer: Some(framebuffer),
            color: Some(ColorAttachment::Texture2D {
                texture: texture.id(),
                mip_level: 0,
            }),
            depth: None,
            width: 8,
            height: 8,
        };
        assert_eq!(
            calls,
            [
                Call::CreateFramebuffer(framebuffers[0]),
                Call::Clear(
                    target(framebuffers[0]),
                    ScissorBox::new_at_origo(8, 8),
                    ClearState {
                        depth: None,
                        ..ClearState::color(0.0, 0.0, 0.0, 1.0)
                    }
                ),
                Call::DeleteFramebuffer(framebuffers[0]),
                Call::CreateFramebuffer(framebuffers[1]),
                Call::BeginPass(target(framebuffers[1]), ScissorBox::new_at_origo(8, 8)),
                Call::EndPass,
                Call::GenerateMipmaps(texture.id()),
                Call::DeleteFramebuffer(framebuffers[1]),
            ]
        );
    }

    #[test]
    fn nested_writes_are_separate_passes() {
        let context = context();
        let screen = RenderTarget::screen(&context, 4, 4);
        screen
            .write::<CoreError>(|| {
                screen.write::<CoreError>(|| Ok(()))?;
                Ok(())
            })
            .unwrap();
        let calls = calls(&context);
        assert!(matches!(
            calls[..],
            [
                Call::BeginPass(..),
                Call::BeginPass(..),
                Call::EndPass,
                Call::EndPass
            ]
        ));
    }

    #[test]
    fn resolve_multisample() {
        let context = context();
        let target = RenderTargetMultisample::<[u8; 4], f32>::new(&context, 4, 4, 4);
        calls(&context);
        let _color = target.resolve_color();
        let calls = calls(&context);
        assert!(calls.iter().any(|c| matches!(
            c,
            Call::CopyTarget(
                _,
                _,
                CopyMask {
                    color: true,
                    depth: false
                }
            )
        )));
    }

    #[test]
    fn renderer_draws_without_gpu() {
        use crate::renderer::*;
        let context = context();
        let camera = Camera::new_perspective(
            Viewport::new_at_origo(16, 16),
            vec3(0.0, 0.0, 5.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
            degrees(45.0),
            0.1,
            10.0,
        );
        let cube = Gm::new(
            Mesh::new(&context, &CpuMesh::cube()),
            PhysicalMaterial::new_opaque(&context, &CpuMaterial::default()),
        );
        let light = DirectionalLight::new(&context, 1.0, Srgba::WHITE, vec3(0.0, -1.0, -1.0));
        calls(&context);
        RenderTarget::screen(&context, 16, 16)
            .clear(ClearState::default())
            .render(&camera, &cube, &[&light]);
        let calls = calls(&context);
        assert!(matches!(calls.first(), Some(Call::Clear(..))));
        let draws = calls
            .iter()
            .filter(|c| matches!(c, Call::Draw { count: 36, .. }))
            .count();
        assert_eq!(draws, 1, "{:#?}", calls);
    }
}
