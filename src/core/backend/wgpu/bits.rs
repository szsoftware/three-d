//!
//! Rendering into textures of 32 bit floats on a device that cannot do that, which is wgpu on OpenGL with the drivers of Mesa.
//!
//! Such a texture gets a second texture of unsigned integers with the same size, the "bits". Everything that uses the texture as a
//! target (rendering, clearing, reading, copying) uses the bits instead, and fragment shaders write the bit pattern of their floats.
//! Before the texture is sampled, the slices that were rendered are copied from the bits into the texture through a buffer.
//! Blending is not possible on such a target.
//!

use super::*;

/// The format of the bits of a texture of 32 bit floats.
pub(super) fn bits_format(format: w::TextureFormat) -> Option<w::TextureFormat> {
    use w::TextureFormat as F;
    match format {
        F::R32Float => Some(F::R32Uint),
        F::Rg32Float => Some(F::Rg32Uint),
        F::Rgba32Float => Some(F::Rgba32Uint),
        _ => None,
    }
}

/// Whether the format is one of the formats of the bits.
pub(super) fn is_bits(format: w::TextureFormat) -> bool {
    use w::TextureFormat as F;
    matches!(format, F::R32Uint | F::Rg32Uint | F::Rgba32Uint)
}

///
/// Changes the fragment shader so that the outputs at the locations in the mask are the bit patterns of the floats it wrote before.
///
pub(super) fn output_bits(module: &mut naga::Module, entry_point: &str, mask: u32) {
    use naga::{Binding, Expression, ScalarKind, Type, TypeInner};
    let Some(index) = module
        .entry_points
        .iter()
        .position(|e| e.stage == naga::ShaderStage::Fragment && e.name == entry_point)
    else {
        return;
    };
    let Some(result) = module.entry_points[index].function.result.clone() else {
        return;
    };
    let in_mask = |binding: &Option<Binding>| match binding {
        Some(Binding::Location { location, .. }) => mask >> location & 1 == 1,
        _ => false,
    };
    let unsigned = |types: &mut naga::UniqueArena<Type>, ty: naga::Handle<Type>| {
        let inner = match types[ty].inner {
            TypeInner::Scalar(scalar) if scalar.kind == ScalarKind::Float => {
                TypeInner::Scalar(naga::Scalar::U32)
            }
            TypeInner::Vector { size, scalar } if scalar.kind == ScalarKind::Float => {
                TypeInner::Vector {
                    size,
                    scalar: naga::Scalar::U32,
                }
            }
            _ => return ty,
        };
        types.insert(Type { name: None, inner }, naga::Span::UNDEFINED)
    };
    // For a struct, which members change
    let (ty, members) = match module.types[result.ty].inner.clone() {
        TypeInner::Struct { mut members, span } => {
            let mut changed = Vec::new();
            for member in &mut members {
                let ty = if in_mask(&member.binding) {
                    unsigned(&mut module.types, member.ty)
                } else {
                    member.ty
                };
                changed.push(ty != member.ty);
                member.ty = ty;
            }
            let ty = module.types.insert(
                Type {
                    name: None,
                    inner: TypeInner::Struct { members, span },
                },
                naga::Span::UNDEFINED,
            );
            (ty, Some(changed))
        }
        _ if in_mask(&result.binding) => (unsigned(&mut module.types, result.ty), None),
        _ => return,
    };
    if ty == result.ty {
        return;
    }
    let function = &mut module.entry_points[index].function;
    function.result.as_mut().unwrap().ty = ty;

    fn rewrite(
        block: &mut naga::Block,
        expressions: &mut naga::Arena<Expression>,
        ty: naga::Handle<Type>,
        members: &Option<Vec<bool>>,
    ) {
        use naga::Statement as S;
        let bits = |expr| Expression::As {
            expr,
            kind: ScalarKind::Uint,
            convert: None,
        };
        for (statement, span) in std::mem::take(block).span_into_iter() {
            let statement = match statement {
                S::Return { value: Some(value) } => {
                    let start = expressions.len();
                    let value = match members {
                        None => expressions.append(bits(value), span),
                        Some(members) => {
                            let components = members
                                .iter()
                                .enumerate()
                                .map(|(index, changed)| {
                                    let member = expressions.append(
                                        Expression::AccessIndex {
                                            base: value,
                                            index: index as u32,
                                        },
                                        span,
                                    );
                                    if *changed {
                                        expressions.append(bits(member), span)
                                    } else {
                                        member
                                    }
                                })
                                .collect();
                            expressions.append(Expression::Compose { ty, components }, span)
                        }
                    };
                    block.push(S::Emit(expressions.range_from(start)), span);
                    S::Return { value: Some(value) }
                }
                S::Block(mut inner) => {
                    rewrite(&mut inner, expressions, ty, members);
                    S::Block(inner)
                }
                S::If {
                    condition,
                    mut accept,
                    mut reject,
                } => {
                    rewrite(&mut accept, expressions, ty, members);
                    rewrite(&mut reject, expressions, ty, members);
                    S::If {
                        condition,
                        accept,
                        reject,
                    }
                }
                S::Switch {
                    selector,
                    mut cases,
                } => {
                    for case in &mut cases {
                        rewrite(&mut case.body, expressions, ty, members);
                    }
                    S::Switch { selector, cases }
                }
                S::Loop {
                    mut body,
                    mut continuing,
                    break_if,
                } => {
                    rewrite(&mut body, expressions, ty, members);
                    rewrite(&mut continuing, expressions, ty, members);
                    S::Loop {
                        body,
                        continuing,
                        break_if,
                    }
                }
                other => other,
            };
            block.push(statement, span);
        }
    }
    rewrite(&mut function.body, &mut function.expressions, ty, &members);
}

impl WgpuBackend {
    ///
    /// Makes sure that the color textures of the target have their bits, if they need them.
    /// If the target is written to, the slices are remembered, to be copied into the textures before those are sampled.
    ///
    pub(super) fn prepare_bits(&self, state: &mut State, target: &PassTarget, written: bool) {
        if self.float32_renderable || target.framebuffer.is_none() {
            return;
        }
        let (id, slices): (u32, Vec<(u32, u32)>) = match &target.color {
            Some(ColorAttachment::Texture2D { texture, mip_level }) => {
                (texture.wgpu(), vec![(*mip_level, 0)])
            }
            Some(ColorAttachment::Texture2DArray {
                texture,
                layers,
                mip_level,
            }) => (
                texture.wgpu(),
                layers.iter().map(|layer| (*mip_level, *layer)).collect(),
            ),
            Some(ColorAttachment::CubeMap {
                texture,
                sides,
                mip_level,
            }) => (
                texture.wgpu(),
                sides
                    .iter()
                    .map(|side| (*mip_level, cube_layer(*side)))
                    .collect(),
            ),
            _ => return,
        };
        self.bits(state, id);
        if written {
            if let Some(entry) = state.textures.get_mut(&id).filter(|e| e.bits.is_some()) {
                for slice in slices {
                    if !entry.stale.contains(&slice) {
                        entry.stale.push(slice);
                    }
                }
            }
        }
    }

    ///
    /// Creates the bits of the texture if it needs them and does not have them, with the content of the texture.
    ///
    pub(super) fn bits(&self, state: &mut State, id: u32) -> Option<w::Texture> {
        let entry = &state.textures[&id];
        if entry.bits.is_some() {
            return entry.bits.clone();
        }
        let texture = entry.texture.clone();
        let format = bits_format(texture.format())?;
        if self.float32_renderable || texture.dimension() != w::TextureDimension::D2 {
            return None;
        }
        let size = texture.size();
        let layers = size.depth_or_array_layers;
        let cube = size.width == size.height && layers == 6;
        let bits = self.device.create_texture(&w::TextureDescriptor {
            label: None,
            size: w::Extent3d {
                // Not 6 layers, of which wgpu would make a cube map on OpenGL. From a cube map it cannot copy.
                depth_or_array_layers: if cube { 7 } else { layers },
                ..size
            },
            mip_level_count: texture.mip_level_count(),
            sample_count: 1,
            dimension: w::TextureDimension::D2,
            format,
            usage: w::TextureUsages::RENDER_ATTACHMENT
                | w::TextureUsages::COPY_SRC
                | w::TextureUsages::COPY_DST
                | w::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        if entry.has_content {
            let source = cube.then(|| {
                texture.create_view(&w::TextureViewDescriptor {
                    dimension: Some(w::TextureViewDimension::Cube),
                    ..Default::default()
                })
            });
            for mip_level in 0..texture.mip_level_count() {
                for layer in 0..layers {
                    let to = Slice {
                        texture: bits.clone(),
                        mip_level,
                        layer,
                    };
                    match &source {
                        // wgpu cannot copy from a cube map on OpenGL
                        Some(source) => self.builtin_pass(
                            state,
                            builtin::Kind::Flatten {
                                cube: true,
                                depth: false,
                                bits: true,
                            },
                            source,
                            &to.view(),
                            format,
                            Some(layer | (mip_level << 8)),
                        ),
                        None => {
                            let from = Slice {
                                texture: texture.clone(),
                                mip_level,
                                layer,
                            };
                            self.copy_through_buffer(state, &from, &to);
                        }
                    }
                }
            }
        }
        state.textures.get_mut(&id).unwrap().bits = Some(bits.clone());
        Some(bits)
    }

    ///
    /// Copies what was rendered into the bits of the texture into the texture, so that it can be sampled.
    ///
    pub(super) fn synchronize_bits(&self, state: &mut State, id: u32) {
        let Some(entry) = state.textures.get_mut(&id) else {
            return;
        };
        if entry.stale.is_empty() {
            return;
        }
        entry.has_content = true;
        let stale = std::mem::take(&mut entry.stale);
        let (texture, bits) = (entry.texture.clone(), entry.bits.clone().unwrap());
        for (mip_level, layer) in stale {
            let slice = |texture: &w::Texture| Slice {
                texture: texture.clone(),
                mip_level,
                layer,
            };
            self.copy_through_buffer(state, &slice(&bits), &slice(&texture));
        }
    }

    ///
    /// Copies a slice into a slice of another format with the same size of a texel. The bits of the texels stay the same.
    ///
    fn copy_through_buffer(&self, state: &mut State, from: &Slice, to: &Slice) {
        let size = from.texture.size();
        let (width, height) = (
            (size.width >> from.mip_level).max(1),
            (size.height >> from.mip_level).max(1),
        );
        let texel_size = from.texture.format().block_copy_size(None).unwrap();
        let bytes_per_row = (width * texel_size).next_multiple_of(w::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&w::BufferDescriptor {
            label: None,
            size: (bytes_per_row * height) as u64,
            usage: w::BufferUsages::COPY_DST | w::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        fn texture(slice: &Slice) -> w::TexelCopyTextureInfo<'_> {
            w::TexelCopyTextureInfo {
                texture: &slice.texture,
                mip_level: slice.mip_level,
                origin: w::Origin3d {
                    x: 0,
                    y: 0,
                    z: slice.layer,
                },
                aspect: w::TextureAspect::All,
            }
        }
        let layout = w::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: w::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        };
        let extent = w::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let encoder = self.encoder(state);
        encoder.copy_texture_to_buffer(texture(from), layout, extent);
        encoder.copy_buffer_to_texture(layout, texture(to), extent);
    }
}
