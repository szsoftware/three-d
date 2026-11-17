use crate::core::*;
use std::collections::HashMap;
use std::sync::RwLock;

///
/// A shader program consisting of a programmable vertex shader followed by a programmable fragment shader.
/// Functionality includes transferring per vertex data to the vertex shader (see the use_attribute functionality)
/// and transferring uniform data to both shader stages (see the use_uniform and use_texture functionality)
/// and execute the shader program (see the draw functionality).
///
pub struct Program {
    context: Context,
    id: backend::ProgramId,
    attributes: HashMap<String, u32>,
    attribute_locations: Vec<u32>,
    textures: RwLock<HashMap<String, u32>>,
    uniforms: HashMap<String, backend::UniformLocation>,
    uniform_blocks: RwLock<HashMap<String, (u32, u32)>>,
}

impl Program {
    ///
    /// Creates a new shader program from the given vertex and fragment glsl shader source.
    ///
    pub fn from_source(
        context: &Context,
        vertex_shader_source: &str,
        fragment_shader_source: &str,
    ) -> Result<Self, CoreError> {
        let (id, reflection) = context
            .backend()
            .create_program(vertex_shader_source, fragment_shader_source)?;
        Ok(Self::new(context, id, reflection))
    }

    ///
    /// Creates a new shader program from the given WGSL source, which contains both a vertex and a fragment entry point.
    /// This needs a context with the wgpu backend (see [Context::as_wgpu]), with another backend it returns an error.
    ///
    /// The program is used like one created from GLSL. What is addressed by name is found by these conventions:
    /// - `use_uniform`: the members of the struct of `@group(0) @binding(0) var<uniform>` (and of a second one at `@binding(1)`).
    /// - `use_texture` and its variants: the name of a texture variable. The texture number `i` is at `@group(1) @binding(2 * i)`
    ///   and its sampler at `@group(1) @binding(2 * i + 1)`. Textures have `f32` values (`texture_2d<f32>`, `texture_2d_array<f32>`, `texture_cube<f32>` or `texture_3d<f32>`),
    ///   also depth textures, which give the depth in the first channel.
    /// - `use_uniform_block`: the name of a variable `@group(2) @binding(i) var<uniform>`, with `i` counting from 0.
    /// - `use_vertex_attribute` and `use_instance_attribute`: the name of an argument of the vertex entry point with a `@location`, or of a member of an argument that is a struct.
    ///
    /// The shader uses the conventions of OpenGL like a GLSL shader of this crate, so the matrices of a [Camera](crate::renderer::Camera) work as they are:
    /// the position returned by the vertex entry point is in the clip space of OpenGL (y up, z from `-w` to `w`), and the texel (0, 0) of a texture is at the bottom left of the image.
    ///
    #[cfg(feature = "wgsl")]
    pub fn from_wgsl_source(context: &Context, source: &str) -> Result<Self, CoreError> {
        let (id, reflection) = context.backend().create_program_wgsl(source)?;
        Ok(Self::new(context, id, reflection))
    }

    fn new(
        context: &Context,
        id: backend::ProgramId,
        reflection: backend::ProgramReflection,
    ) -> Self {
        let attribute_locations = reflection.attributes.values().copied().collect();
        Program {
            context: context.clone(),
            id,
            attributes: reflection.attributes,
            attribute_locations,
            uniforms: reflection.uniforms,
            uniform_blocks: RwLock::new(HashMap::new()),
            textures: RwLock::new(HashMap::new()),
        }
    }

    ///
    /// Send the given uniform data to this shader program and associate it with the given named variable.
    /// The glsl shader variable must be of type `uniform int` if the data is an integer, `uniform vec2` if it is of type [Vec2] etc.
    /// The uniform variable is uniformly available across all processing of vertices and fragments.
    ///
    /// # Panic
    /// Will panic if the uniform is not defined or not used in the shader code.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_uniform<T: UniformDataType>(&self, name: &str, data: T) {
        let location = self.get_uniform_location(name);
        self.context
            .backend()
            .set_uniform(self.id, location, &T::uniform_value(&[data]));
    }

    ///
    /// Calls [Self::use_uniform] if [Self::requires_uniform] returns true.
    ///
    pub fn use_uniform_if_required<T: UniformDataType>(&self, name: &str, data: T) {
        if self.requires_uniform(name) {
            self.use_uniform(name, data);
        }
    }

    ///
    /// Send the given array of uniform data to this shader program and associate it with the given named variable.
    /// The glsl shader variable must be of same type and length as the data, so if the data is an array of three [Vec2], the variable must be `uniform vec2[3]`.
    /// The uniform variable is uniformly available across all processing of vertices and fragments.
    ///
    /// # Panic
    /// Will panic if the uniform is not defined in the shader code or not used.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_uniform_array<T: UniformDataType>(&self, name: &str, data: &[T]) {
        let location = self.get_uniform_location(name);
        self.context
            .backend()
            .set_uniform(self.id, location, &T::uniform_value(data));
    }

    fn get_uniform_location(&self, name: &str) -> &backend::UniformLocation {
        self.uniforms.get(name).unwrap_or_else(|| {
            panic!(
                "the uniform {} is sent to the shader but not defined or never used",
                name
            )
        })
    }

    ///
    /// Use the given [Texture2D] in this shader program and associate it with the given named variable.
    /// The glsl shader variable must be of type `uniform sampler2D` and can only be accessed in the fragment shader.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code or not used.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_texture(&self, name: &str, texture: &Texture2D) {
        let unit = self.use_texture_internal(name);
        self.context
            .backend()
            .bind_texture(unit, texture.id(), backend::TextureKind::D2);
    }

    ///
    /// Use the given [DepthTexture2D] in this shader program and associate it with the given named variable.
    /// The glsl shader variable must be of type `uniform sampler2D` and can only be accessed in the fragment shader.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code or not used.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_depth_texture(&self, name: &str, texture: &DepthTexture2D) {
        let unit = self.use_texture_internal(name);
        self.context
            .backend()
            .bind_texture(unit, texture.id(), backend::TextureKind::D2);
    }

    ///
    /// Use the given texture array in this shader program and associate it with the given named variable.
    /// The glsl shader variable must be of type `uniform sampler2DArray` and can only be accessed in the fragment shader.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code or not used.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_texture_array(&self, name: &str, texture: &Texture2DArray) {
        let unit = self.use_texture_internal(name);
        self.context
            .backend()
            .bind_texture(unit, texture.id(), backend::TextureKind::D2Array);
    }

    ///
    /// Use the given texture array in this shader program and associate it with the given named variable.
    /// The glsl shader variable must be of type `uniform sampler2DArray` and can only be accessed in the fragment shader.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code or not used.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_depth_texture_array(&self, name: &str, texture: &DepthTexture2DArray) {
        let unit = self.use_texture_internal(name);
        self.context
            .backend()
            .bind_texture(unit, texture.id(), backend::TextureKind::D2Array);
    }

    ///
    /// Use the given texture cube map in this shader program and associate it with the given named variable.
    /// The glsl shader variable must be of type `uniform samplerCube` and can only be accessed in the fragment shader.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code or not used.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_texture_cube(&self, name: &str, texture: &TextureCubeMap) {
        let unit = self.use_texture_internal(name);
        self.context
            .backend()
            .bind_texture(unit, texture.id(), backend::TextureKind::CubeMap);
    }

    ///
    /// Use the given texture cube map in this shader program and associate it with the given named variable.
    /// The glsl shader variable must be of type `uniform samplerCube` and can only be accessed in the fragment shader.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code or not used.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_depth_texture_cube(&self, name: &str, texture: &DepthTextureCubeMap) {
        let unit = self.use_texture_internal(name);
        self.context
            .backend()
            .bind_texture(unit, texture.id(), backend::TextureKind::CubeMap);
    }

    ///
    /// Use the given 3D texture in this shader program and associate it with the given named variable.
    /// The glsl shader variable must be of type `uniform sampler3D` and can only be accessed in the fragment shader.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code or not used.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_texture_3d(&self, name: &str, texture: &Texture3D) {
        let unit = self.use_texture_internal(name);
        self.context
            .backend()
            .bind_texture(unit, texture.id(), backend::TextureKind::D3);
    }

    ///
    /// Use this function if you want to use a texture which was created using low-level context calls and not using the functionality in the [texture] module.
    /// This function is only needed in special cases for example if you have a special source of texture data.
    ///
    #[deprecated = "Instead, create normal textures, eg. Texture2D, using the new_unchecked() methods, eg. Texture2D::new_unchecked()"]
    pub fn use_raw_texture(&self, name: &str, target: u32, id: crate::context::Texture) {
        let unit = self.use_texture_internal(name);
        unsafe {
            self.context.active_texture(crate::context::TEXTURE0 + unit);
            self.context.bind_texture(target, Some(id));
        }
    }

    fn use_texture_internal(&self, name: &str) -> u32 {
        if !self.textures.read().unwrap().contains_key(name) {
            let mut map = self.textures.write().unwrap();
            let index = map.len() as u32;
            map.insert(name.to_owned(), index);
        };
        let index = *self.textures.read().unwrap().get(name).unwrap();
        self.use_uniform(name, index as i32);
        index
    }

    ///
    /// Use the given [UniformBuffer] in this shader program and associate it with the given named variable.
    ///
    pub fn use_uniform_block(&self, name: &str, buffer: &UniformBuffer) {
        if !self.uniform_blocks.read().unwrap().contains_key(name) {
            let mut map = self.uniform_blocks.write().unwrap();
            let location = self
                .context
                .backend()
                .uniform_block_index(self.id, name)
                .unwrap_or_else(|| {
                    panic!(
                        "the uniform block {} is sent to the shader but not defined or never used",
                        name
                    )
                });
            let index = map.len() as u32;
            map.insert(name.to_owned(), (location, index));
        };
        let (location, index) = *self.uniform_blocks.read().unwrap().get(name).unwrap();
        self.context
            .backend()
            .bind_uniform_block(self.id, location, index, buffer.id());
    }

    ///
    /// Uses the given [VertexBuffer] data in this shader program and associates it with the given named variable.
    /// Each value in the buffer is used when rendering one vertex using the [Program::draw_arrays] or [Program::draw_elements] methods.
    /// Therefore the buffer must contain the same number of values as the number of vertices specified in those draw calls.
    ///
    /// # Panic
    /// Will panic if the attribute is not defined in the shader code or not used.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_vertex_attribute<T: BufferDataType>(&self, name: &str, buffer: &VertexBuffer<T>) {
        if buffer.count() > 0 {
            let loc = self.location(name);
            self.context.backend().bind_vertex_attribute(
                self.id,
                loc,
                buffer.id(),
                backend::VertexFormat::of::<T>(),
                0,
            );
        }
    }

    ///
    /// Uses the given [InstanceBuffer] data in this shader program and associates it with the given named variable.
    /// Each value in the buffer is used when rendering one instance using the [Program::draw_arrays_instanced] or [Program::draw_elements_instanced] methods.
    /// Therefore the buffer must contain the same number of values as the number of instances specified in those draw calls.
    ///
    /// # Panic
    /// Will panic if the attribute is not defined in the shader code or not used.
    /// In the latter case the variable is removed by the shader compiler.
    ///
    pub fn use_instance_attribute<T: BufferDataType>(
        &self,
        name: &str,
        buffer: &InstanceBuffer<T>,
    ) {
        if buffer.count() > 0 {
            let loc = self.location(name);
            self.context.backend().bind_vertex_attribute(
                self.id,
                loc,
                buffer.id(),
                backend::VertexFormat::of::<T>(),
                1,
            );
        }
    }

    ///
    /// Draws triangles with the given render states and viewport using this shader program.
    /// The number of vertices to draw is defined by the `count` parameter.
    /// Requires that all attributes and uniforms have been defined using the use_attribute and use_uniform methods.
    /// Assumes that the data for the three vertices in a triangle is defined contiguous in each vertex buffer.
    /// If you want to use an [ElementBuffer], see [Program::draw_elements].
    ///
    pub fn draw_arrays(&self, render_states: RenderStates, viewport: Viewport, count: u32) {
        self.draw(render_states, viewport, None, count, None)
    }

    ///
    /// Same as [Program::draw_arrays] except it renders 'instance_count' instances of the same set of triangles.
    /// Use the [Program::use_instance_attribute], method to send unique data for each instance to the shader.
    ///
    pub fn draw_arrays_instanced(
        &self,
        render_states: RenderStates,
        viewport: Viewport,
        count: u32,
        instance_count: u32,
    ) {
        self.draw(render_states, viewport, None, count, Some(instance_count))
    }

    ///
    /// Draws the triangles defined by the given [ElementBuffer] with the given render states and viewport using this shader program.
    /// Requires that all attributes and uniforms have been defined using the use_attribute and use_uniform methods.
    /// If you do not want to use an [ElementBuffer], see [Program::draw_arrays]. If you only want to draw a subset of the triangles in the given [ElementBuffer], see [Program::draw_subset_of_elements].
    ///
    pub fn draw_elements<T: ElementBufferDataType>(
        &self,
        render_states: RenderStates,
        viewport: Viewport,
        element_buffer: &ElementBuffer<T>,
    ) {
        self.draw_subset_of_elements(
            render_states,
            viewport,
            element_buffer,
            0,
            element_buffer.count(),
        )
    }

    ///
    /// Draws a subset of the triangles defined by the given [ElementBuffer] with the given render states and viewport using this shader program.
    /// The number of vertices to draw is defined by the `count` parameter.
    /// Requires that all attributes and uniforms have been defined using the use_attribute and use_uniform methods.
    /// If you do not want to use an [ElementBuffer], see [Program::draw_arrays].
    ///
    pub fn draw_subset_of_elements<T: ElementBufferDataType>(
        &self,
        render_states: RenderStates,
        viewport: Viewport,
        element_buffer: &ElementBuffer<T>,
        first: u32,
        count: u32,
    ) {
        self.draw(
            render_states,
            viewport,
            Some(Self::index_buffer(element_buffer, first)),
            count,
            None,
        )
    }

    ///
    /// Same as [Program::draw_elements] except it renders 'instance_count' instances of the same set of triangles.
    /// Use the [Program::use_instance_attribute] method to send unique data for each instance to the shader.
    ///
    pub fn draw_elements_instanced<T: ElementBufferDataType>(
        &self,
        render_states: RenderStates,
        viewport: Viewport,
        element_buffer: &ElementBuffer<T>,
        instance_count: u32,
    ) {
        self.draw_subset_of_elements_instanced(
            render_states,
            viewport,
            element_buffer,
            0,
            element_buffer.count(),
            instance_count,
        )
    }

    ///
    /// Same as [Program::draw_subset_of_elements] except it renders 'instance_count' instances of the same set of triangles.
    /// Use the [Program::use_instance_attribute] method to send unique data for each instance to the shader.
    ///
    pub fn draw_subset_of_elements_instanced<T: ElementBufferDataType>(
        &self,
        render_states: RenderStates,
        viewport: Viewport,
        element_buffer: &ElementBuffer<T>,
        first: u32,
        count: u32,
        instance_count: u32,
    ) {
        self.draw(
            render_states,
            viewport,
            Some(Self::index_buffer(element_buffer, first)),
            count,
            Some(instance_count),
        )
    }

    ///
    /// Calls drawing callback `draw` after setting up rendering to use this shader program, cleaning up before return.
    /// Requires that all attributes and uniforms have been defined using the [use_attribute] and [use_uniform] methods.
    ///
    /// **Note:** Only available with the OpenGL/WebGL backend, since the callback is expected to make low-level GL calls.
    ///
    pub fn draw_with(&self, render_states: RenderStates, viewport: Viewport, draw: impl FnOnce()) {
        let mut draw = Some(draw);
        self.context.gl_backend().draw_with(
            self.id,
            &self.attribute_locations,
            render_states,
            viewport,
            &mut || (draw.take().unwrap())(),
        );
    }

    ///
    /// Calls drawing callback `draw` after setting up rendering to use this shader program and element buffer `elements`, cleaning up before return.
    /// Requires that all attributes and uniforms have been defined using the [use_attribute] and [use_uniform] methods.
    ///
    /// **Note:** Only available with the OpenGL/WebGL backend, since the callback is expected to make low-level GL calls.
    ///
    pub fn draw_elements_with<T: ElementBufferDataType>(
        &self,
        render_states: RenderStates,
        viewport: Viewport,
        element_buffer: &ElementBuffer<T>,
        draw: impl FnOnce(),
    ) {
        self.draw_with(render_states, viewport, move || {
            unsafe {
                self.context.bind_buffer(
                    crate::context::ELEMENT_ARRAY_BUFFER,
                    Some(element_buffer.id().gl()),
                );
            }
            draw();
            unsafe {
                self.context
                    .bind_buffer(crate::context::ELEMENT_ARRAY_BUFFER, None);
            }
        })
    }

    fn index_buffer<T: ElementBufferDataType>(
        element_buffer: &ElementBuffer<T>,
        first: u32,
    ) -> backend::IndexBuffer {
        backend::IndexBuffer {
            buffer: element_buffer.id(),
            index_type: T::primitive_type(),
            first,
        }
    }

    ///
    /// Returns true if this program uses the uniform with the given name.
    ///
    pub fn requires_uniform(&self, name: &str) -> bool {
        self.uniforms.contains_key(name)
    }

    ///
    /// Returns true if this program uses the attribute with the given name.
    ///
    pub fn requires_attribute(&self, name: &str) -> bool {
        self.attributes.contains_key(name)
    }

    fn location(&self, name: &str) -> u32 {
        *self.attributes.get(name).unwrap_or_else(|| {
            panic!(
                "the attribute {} is sent to the shader but not defined or never used",
                name
            )
        })
    }

    fn draw(
        &self,
        render_states: RenderStates,
        viewport: Viewport,
        indices: Option<backend::IndexBuffer>,
        count: u32,
        instances: Option<u32>,
    ) {
        self.context.backend().draw(&backend::DrawCall {
            program: self.id,
            attribute_locations: &self.attribute_locations,
            render_states,
            viewport,
            indices,
            count,
            instances,
        });
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        self.context.backend().delete_program(self.id);
    }
}

///
/// A compute shader program written in WGSL, which runs on the GPU without drawing anything and writes its results into textures.
/// Only available with the `wgsl` feature and with a context that uses the wgpu backend (see [Context::as_wgpu]).
///
/// The source has one compute entry point. Uniforms, textures and uniform blocks are found by name with the same conventions
/// as in [Program::from_wgsl_source] (groups 0, 1 and 2). In a compute shader a texture is read with `textureLoad` or `textureSampleLevel`.
/// The textures the program writes to are in group 3: `@group(3) @binding(i) var name: texture_storage_2d<F, write>`, with `i` counting from 0
/// and the format `F` one of `rgba8unorm`, `rgba16float`, `rgba32float` or `r32float`.
///
/// The results are used like any other texture: read them with [ColorTarget::read] or [ColorTarget::read_async], use them in a material
/// or as the input of the next run. As in OpenGL, the texel (0, 0) of a texture is at the bottom left of the image.
///
#[cfg(feature = "wgsl")]
pub struct ComputeProgram {
    // A compute program has uniforms, textures and uniform blocks like every program
    program: Program,
}

#[cfg(feature = "wgsl")]
impl ComputeProgram {
    ///
    /// Creates a new compute program from the given WGSL source.
    /// Returns an error if the source is not valid, does not follow the conventions or if the context does not use the wgpu backend.
    ///
    pub fn from_wgsl_source(context: &Context, source: &str) -> Result<Self, CoreError> {
        let (id, reflection) = context.backend().create_compute_program(source)?;
        Ok(Self {
            program: Program::new(context, id, reflection),
        })
    }

    ///
    /// Send the given uniform data to this program and associate it with the given named variable, see [Program::use_uniform].
    ///
    /// # Panic
    /// Will panic if the uniform is not defined in the shader code.
    ///
    pub fn use_uniform<T: UniformDataType>(&self, name: &str, data: T) {
        self.program.use_uniform(name, data);
    }

    ///
    /// Send the given array of uniform data to this program and associate it with the given named variable, see [Program::use_uniform_array].
    ///
    /// # Panic
    /// Will panic if the uniform is not defined in the shader code.
    ///
    pub fn use_uniform_array<T: UniformDataType>(&self, name: &str, data: &[T]) {
        self.program.use_uniform_array(name, data);
    }

    ///
    /// Returns true if this program has a uniform with the given name.
    ///
    pub fn requires_uniform(&self, name: &str) -> bool {
        self.program.requires_uniform(name)
    }

    ///
    /// Use the given [Texture2D] as input of this program and associate it with the given named variable.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code.
    ///
    pub fn use_texture(&self, name: &str, texture: &Texture2D) {
        self.program.use_texture(name, texture);
    }

    ///
    /// Use the given [DepthTexture2D] as input of this program and associate it with the given named variable.
    /// The depth is in the first channel.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code.
    ///
    pub fn use_depth_texture(&self, name: &str, texture: &DepthTexture2D) {
        self.program.use_depth_texture(name, texture);
    }

    ///
    /// Use the given [Texture2DArray] as input of this program and associate it with the given named variable.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code.
    ///
    pub fn use_texture_array(&self, name: &str, texture: &Texture2DArray) {
        self.program.use_texture_array(name, texture);
    }

    ///
    /// Use the given [TextureCubeMap] as input of this program and associate it with the given named variable.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code.
    ///
    pub fn use_texture_cube(&self, name: &str, texture: &TextureCubeMap) {
        self.program.use_texture_cube(name, texture);
    }

    ///
    /// Use the given [Texture3D] as input of this program and associate it with the given named variable.
    ///
    /// # Panic
    /// Will panic if the texture is not defined in the shader code.
    ///
    pub fn use_texture_3d(&self, name: &str, texture: &Texture3D) {
        self.program.use_texture_3d(name, texture);
    }

    ///
    /// Use the given [UniformBuffer] in this program and associate it with the given named variable.
    ///
    pub fn use_uniform_block(&self, name: &str, buffer: &UniformBuffer) {
        self.program.use_uniform_block(name, buffer);
    }

    ///
    /// Lets this program write to the given [Texture2D] and associate it with the given named variable of type `texture_storage_2d`.
    /// The program writes to the largest mip level, the other mip levels are not updated.
    ///
    /// The format of the texture must be the one in the shader code: `rgba8unorm` for a texture of `[u8; 4]` or `[u8; 3]`,
    /// `rgba16float` and `rgba32float` for three or four channels of `f16` and `f32` and `r32float` for one channel of `f32`.
    /// The texture must not be an input of the same run.
    ///
    /// # Panic
    /// Will panic if the storage texture is not defined in the shader code. [ComputeProgram::dispatch] panics if the format does not fit.
    ///
    pub fn use_storage_texture(&self, name: &str, texture: &Texture2D) {
        if !self
            .program
            .context
            .backend()
            .bind_storage_texture(self.program.id, name, texture.id())
        {
            panic!("the storage texture {name} is sent to the shader but not defined");
        }
    }

    ///
    /// Runs this program with the given number of workgroups in each of the three dimensions.
    /// The size of a workgroup is defined in the shader code with `@workgroup_size`, so to cover a texture of 100 x 60 texels
    /// with `@workgroup_size(8, 8)`, dispatch 13 x 8 x 1 workgroups and ignore the texels outside the texture in the shader.
    ///
    /// Everything drawn before is finished before the program runs, and its results are there for everything that comes after.
    ///
    /// # Panic
    /// Will panic if a texture, storage texture or uniform block of the shader code has not been set.
    ///
    pub fn dispatch(&self, x: u32, y: u32, z: u32) {
        self.program
            .context
            .backend()
            .dispatch(self.program.id, [x, y, z]);
    }
}
