//!
//! Shader programs written in WGSL instead of GLSL (only with the `wgsl` feature).
//!
//! A WGSL program is one module with a vertex and a fragment entry point. What `Program` does by name with a GLSL program
//! works with a WGSL program through these conventions:
//! - **uniforms:** the members of the struct of `@group(0) @binding(0) var<uniform>` (and of a second one at `@binding(1)`) are the uniforms,
//! - **textures:** the texture number `i` is at `@group(1) @binding(2 * i)` and its sampler at `@group(1) @binding(2 * i + 1)`, the name of the texture variable is the name of the uniform,
//! - **uniform blocks:** `@group(2) @binding(i) var<uniform>`, the name of the variable is the name of the block,
//! - **attributes:** the arguments of the vertex entry point with a `@location` (or the members of an argument that is a struct), by their name.
//!
//! The shader is written with the conventions of OpenGL like every other shader of this crate: the position it returns is in the clip space of OpenGL
//! (z from -w to w), and the first row of a texture is the bottom one. The conversion of the position that the GLSL path appends to the vertex shader
//! is done here on the parsed module, see [convert_clip_space].
//!

use super::glsl::{SamplerDecl, BLOCK_GROUP, TEXTURE_GROUP, UNIFORM_GROUP};
use naga::{Expression, Handle, Statement};

pub(super) struct Shader {
    pub module: naga::Module,
    /// The names of the vertex and the fragment entry point.
    pub entry_points: [String; 2],
    pub attributes: Vec<(String, u32)>,
    pub samplers: Vec<SamplerDecl>,
    pub uniform_blocks: Vec<String>,
}

///
/// Parses and validates a WGSL program and finds what [Program](crate::core::Program) addresses by name.
/// The error is a description of what is wrong.
///
pub(super) fn parse(source: &str) -> Result<Shader, String> {
    let mut module = naga::front::wgsl::parse_str(source).map_err(|e| e.emit_to_string(source))?;
    let validate = |module: &naga::Module| {
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(module)
        .map(|_| ())
        .map_err(|e| e.emit_to_string(source))
    };
    validate(&module)?;

    let entry_point = |stage: naga::ShaderStage, name: &str| {
        let mut entries = module.entry_points.iter().filter(|e| e.stage == stage);
        match (entries.next(), entries.next()) {
            (Some(entry), None) => Ok(entry.name.clone()),
            (None, _) => Err(format!("the program has no {name} entry point")),
            _ => Err(format!("the program has more than one {name} entry point")),
        }
    };
    let entry_points = [
        entry_point(naga::ShaderStage::Vertex, "vertex")?,
        entry_point(naga::ShaderStage::Fragment, "fragment")?,
    ];

    let mut attributes = Vec::new();
    let vertex = module
        .entry_points
        .iter()
        .find(|e| e.stage == naga::ShaderStage::Vertex)
        .unwrap();
    for argument in &vertex.function.arguments {
        let mut add = |name: &Option<String>, binding: &Option<naga::Binding>| {
            if let (Some(name), Some(naga::Binding::Location { location, .. })) = (name, binding) {
                attributes.push((name.clone(), *location));
            }
        };
        add(&argument.name, &argument.binding);
        if let naga::TypeInner::Struct { members, .. } = &module.types[argument.ty].inner {
            for member in members {
                add(&member.name, &member.binding);
            }
        }
    }

    let mut textures = Vec::new();
    let mut blocks = Vec::new();
    for (_, variable) in module.global_variables.iter() {
        let Some(binding) = &variable.binding else {
            continue;
        };
        let name = variable.name.clone().unwrap_or_default();
        let inner = &module.types[variable.ty].inner;
        let is_uniform = variable.space == naga::AddressSpace::Uniform;
        match binding.group {
            UNIFORM_GROUP => {
                if binding.binding > 1
                    || !is_uniform
                    || !matches!(inner, naga::TypeInner::Struct { .. })
                {
                    return Err(format!(
                        "{name}: group {UNIFORM_GROUP} is for one or two uniform structs at binding 0 and 1, their members are the uniforms"
                    ));
                }
            }
            TEXTURE_GROUP => match inner {
                naga::TypeInner::Image {
                    dim,
                    arrayed,
                    class:
                        naga::ImageClass::Sampled {
                            kind: naga::ScalarKind::Float,
                            multi: false,
                        },
                } if binding.binding % 2 == 0 => {
                    let glsl_type = match (dim, arrayed) {
                        (naga::ImageDimension::D2, false) => "sampler2D",
                        (naga::ImageDimension::D2, true) => "sampler2DArray",
                        (naga::ImageDimension::Cube, false) => "samplerCube",
                        (naga::ImageDimension::D3, false) => "sampler3D",
                        _ => {
                            return Err(format!(
                                "{name}: this kind of texture is not supported, use texture_2d, texture_2d_array, texture_cube or texture_3d"
                            ))
                        }
                    };
                    textures.push((binding.binding / 2, name, glsl_type));
                }
                naga::TypeInner::Sampler { comparison: false } if binding.binding % 2 == 1 => {}
                naga::TypeInner::Image { .. } | naga::TypeInner::Sampler { .. } => {
                    return Err(format!(
                        "{name}: group {TEXTURE_GROUP} is for textures with f32 values at the bindings 0, 2, 4, .. and the sampler of each texture at the binding after it (no depth, multisampled or integer textures and no comparison samplers)"
                    ))
                }
                _ => {
                    return Err(format!(
                        "{name}: group {TEXTURE_GROUP} is for textures and samplers"
                    ))
                }
            },
            BLOCK_GROUP => {
                if !is_uniform {
                    return Err(format!(
                        "{name}: group {BLOCK_GROUP} is for uniform buffers"
                    ));
                }
                blocks.push((binding.binding, name));
            }
            group => {
                return Err(format!(
                    "{name}: group {group} is not supported, use group {UNIFORM_GROUP} for uniforms, {TEXTURE_GROUP} for textures and {BLOCK_GROUP} for uniform buffers"
                ))
            }
        }
    }
    textures.sort();
    blocks.sort();
    if let Some((index, (_, name, _))) = textures
        .iter()
        .enumerate()
        .find(|(index, (number, ..))| *index as u32 != *number)
    {
        return Err(format!(
            "{name}: the textures must be at the bindings 0, 2, 4, .. without a gap, binding {} is free",
            2 * index
        ));
    }
    if let Some((index, (_, name))) = blocks
        .iter()
        .enumerate()
        .find(|(index, (binding, _))| *index as u32 != *binding)
    {
        return Err(format!(
            "{name}: the uniform buffers must be at the bindings 0, 1, 2, .. without a gap, binding {index} is free"
        ));
    }
    let samplers = textures
        .into_iter()
        .map(|(_, name, glsl_type)| SamplerDecl {
            name,
            glsl_type: glsl_type.to_string(),
            in_vertex: true,
            in_fragment: true,
        })
        .collect();
    let uniform_blocks = blocks.into_iter().map(|(_, name)| name).collect();

    convert_clip_space(&mut module)?;
    validate(&module).map_err(|e| format!("the converted vertex shader is not valid: {e}"))?;
    Ok(Shader {
        module,
        entry_points,
        attributes,
        samplers,
        uniform_blocks,
    })
}

///
/// Changes the vertex entry point so that the position it returns is converted from the clip space of OpenGL to the one of wgpu,
/// with the y axis flipped, since render targets keep their rows in the order of OpenGL (see the documentation of the backend).
/// Every `return value;` becomes `result = value; result.position = vec4(p.x, -p.y, 0.5 * (p.z + p.w), p.w); return result;`.
///
fn convert_clip_space(module: &mut naga::Module) -> Result<(), String> {
    let entry = module
        .entry_points
        .iter_mut()
        .find(|e| e.stage == naga::ShaderStage::Vertex)
        .unwrap();
    let result = entry
        .function
        .result
        .clone()
        .ok_or("the vertex entry point returns nothing")?;
    let is_position = |binding: &Option<naga::Binding>| {
        matches!(
            binding,
            Some(naga::Binding::BuiltIn(naga::BuiltIn::Position { .. }))
        )
    };
    // The position is the result itself or a member of it
    let (member, vector) = if is_position(&result.binding) {
        (None, result.ty)
    } else if let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner {
        let (index, member) = members
            .iter()
            .enumerate()
            .find(|(_, m)| is_position(&m.binding))
            .ok_or("the vertex entry point does not return a position")?;
        (Some(index as u32), member.ty)
    } else {
        return Err("the vertex entry point does not return a position".to_string());
    };

    let function = &mut entry.function;
    let span = naga::Span::UNDEFINED;
    let variable = function.local_variables.append(
        naga::LocalVariable {
            name: Some("three_d_result".to_string()),
            ty: result.ty,
            init: None,
        },
        span,
    );
    let mut conversion = Conversion {
        pointer: function
            .expressions
            .append(Expression::LocalVariable(variable), span),
        half: function
            .expressions
            .append(Expression::Literal(naga::Literal::F32(0.5)), span),
        member,
        vector,
        expressions: &mut function.expressions,
    };
    let body = std::mem::take(&mut function.body);
    function.body = conversion.block(body);
    Ok(())
}

struct Conversion<'a> {
    /// The local variable that holds the result.
    pointer: Handle<Expression>,
    half: Handle<Expression>,
    /// The index of the position in the result if the result is a struct.
    member: Option<u32>,
    /// The type of the position.
    vector: Handle<naga::Type>,
    expressions: &'a mut naga::Arena<Expression>,
}

impl Conversion<'_> {
    fn block(&mut self, block: naga::Block) -> naga::Block {
        let mut converted = naga::Block::new();
        for (statement, span) in block.span_into_iter() {
            match statement {
                Statement::Return { value: Some(value) } => {
                    self.return_value(value, &mut converted)
                }
                Statement::Block(block) => {
                    converted.push(Statement::Block(self.block(block)), span)
                }
                Statement::If {
                    condition,
                    accept,
                    reject,
                } => converted.push(
                    Statement::If {
                        condition,
                        accept: self.block(accept),
                        reject: self.block(reject),
                    },
                    span,
                ),
                Statement::Switch { selector, cases } => converted.push(
                    Statement::Switch {
                        selector,
                        cases: cases
                            .into_iter()
                            .map(|case| naga::SwitchCase {
                                value: case.value,
                                body: self.block(case.body),
                                fall_through: case.fall_through,
                            })
                            .collect(),
                    },
                    span,
                ),
                Statement::Loop {
                    body,
                    continuing,
                    break_if,
                } => converted.push(
                    Statement::Loop {
                        body: self.block(body),
                        continuing,
                        break_if,
                    },
                    span,
                ),
                statement => converted.push(statement, span),
            }
        }
        converted
    }

    fn return_value(&mut self, value: Handle<Expression>, block: &mut naga::Block) {
        let span = naga::Span::UNDEFINED;
        block.push(
            Statement::Store {
                pointer: self.pointer,
                value,
            },
            span,
        );
        let mut first = None;
        let mut add = |expressions: &mut naga::Arena<Expression>, expression| {
            let handle = expressions.append(expression, span);
            first.get_or_insert(handle);
            handle
        };
        let e = &mut *self.expressions;
        let position_pointer = match self.member {
            Some(index) => add(
                e,
                Expression::AccessIndex {
                    base: self.pointer,
                    index,
                },
            ),
            None => self.pointer,
        };
        let position = add(
            e,
            Expression::Load {
                pointer: position_pointer,
            },
        );
        let [x, y, z, w] = [0, 1, 2, 3].map(|index| {
            add(
                e,
                Expression::AccessIndex {
                    base: position,
                    index,
                },
            )
        });
        let y = add(
            e,
            Expression::Unary {
                op: naga::UnaryOperator::Negate,
                expr: y,
            },
        );
        let z = add(
            e,
            Expression::Binary {
                op: naga::BinaryOperator::Add,
                left: z,
                right: w,
            },
        );
        let z = add(
            e,
            Expression::Binary {
                op: naga::BinaryOperator::Multiply,
                left: z,
                right: self.half,
            },
        );
        let converted = add(
            e,
            Expression::Compose {
                ty: self.vector,
                components: vec![x, y, z, w],
            },
        );
        block.push(
            Statement::Emit(naga::Range::new_from_bounds(first.unwrap(), converted)),
            span,
        );
        block.push(
            Statement::Store {
                pointer: position_pointer,
                value: converted,
            },
            span,
        );
        let result = e.append(
            Expression::Load {
                pointer: self.pointer,
            },
            span,
        );
        block.push(
            Statement::Emit(naga::Range::new_from_bounds(result, result)),
            span,
        );
        block.push(
            Statement::Return {
                value: Some(result),
            },
            span,
        );
    }
}
