//!
//! Translates the GLSL this crate is written in (GLSL ES 3.00 / GLSL 3.30 without a version line, with loose uniforms and
//! combined texture samplers) into GLSL 4.50 with Vulkan semantics, which is what the GLSL frontend of naga accepts.
//!
//! The translation works on the text, statement by statement, without a full parser:
//! - conditional compilation (`#ifdef` and friends) is resolved first, since declarations are often inside such blocks,
//! - loose `uniform` variables are collected into one uniform block per shader stage (group 0, binding 0 for the vertex and 1 for the fragment stage),
//! - `uniform sampler2D name;` becomes a texture and a sampler (group 1) and a macro `name` that combines the two again, so the call sites stay as they are,
//! - `in` and `out` variables without a location get one, shared by name between the two stages,
//! - functions with a combined texture sampler as parameter take a texture and a sampler instead, and the calls are changed accordingly,
//! - `gl_VertexID` and `gl_InstanceID` are renamed,
//! - `inverse` is replaced by an own function, since naga cannot write it for Metal and DirectX,
//! - the vertex shader gets an epilogue that converts the clip space of OpenGL into the one of wgpu.
//!

use std::collections::{HashMap, HashSet};

/// Bind group of the uniform blocks generated for the loose uniforms.
pub(crate) const UNIFORM_GROUP: u32 = 0;
/// Bind group of the textures and samplers.
pub(crate) const TEXTURE_GROUP: u32 = 1;
/// Bind group of the uniform blocks written by the user.
pub(crate) const BLOCK_GROUP: u32 = 2;
/// Name of the generated uniform block instance types.
const VERTEX_BLOCK: &str = "ThreeDVertexUniforms";
const FRAGMENT_BLOCK: &str = "ThreeDFragmentUniforms";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Vertex,
    Fragment,
}

///
/// A combined texture sampler of the original source.
/// The texture has binding `2 * index` and the sampler `2 * index + 1` in [TEXTURE_GROUP], where `index` is the position in [Translation::samplers].
///
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SamplerDecl {
    pub name: String,
    /// The GLSL sampler type, for example `sampler2D`.
    pub glsl_type: String,
    pub in_vertex: bool,
    pub in_fragment: bool,
}

///
/// The result of [translate].
///
#[derive(Clone, Debug, Default)]
pub(crate) struct Translation {
    /// GLSL 4.50 source of the vertex shader.
    pub vertex: String,
    /// GLSL 4.50 source of the fragment shader.
    pub fragment: String,
    /// Vertex attribute name and location.
    pub attributes: Vec<(String, u32)>,
    /// Names of the loose uniforms of the vertex shader, now members of the block at binding 0 in [UNIFORM_GROUP].
    pub vertex_uniforms: Vec<String>,
    /// Names of the loose uniforms of the fragment shader, now members of the block at binding 1 in [UNIFORM_GROUP].
    pub fragment_uniforms: Vec<String>,
    pub samplers: Vec<SamplerDecl>,
    /// Names of the uniform blocks written by the user, the binding in [BLOCK_GROUP] is the position in this list.
    pub uniform_blocks: Vec<String>,
}

///
/// Translates a vertex and fragment shader pair. The error is a description of the construct that is not supported.
///
pub(crate) fn translate(vertex: &str, fragment: &str) -> Result<Translation, String> {
    let mut shared = Shared::default();
    let mut translation = Translation::default();
    let (vertex, vertex_uniforms) = translate_stage(vertex, Stage::Vertex, &mut shared)?;
    let (fragment, fragment_uniforms) = translate_stage(fragment, Stage::Fragment, &mut shared)?;
    translation.vertex = vertex;
    translation.fragment = fragment;
    translation.vertex_uniforms = vertex_uniforms;
    translation.fragment_uniforms = fragment_uniforms;
    translation.attributes = shared.attributes;
    translation.samplers = shared.samplers;
    translation.uniform_blocks = shared.uniform_blocks;
    Ok(translation)
}

/// Replacement for the built-in function `inverse`.
const INVERSE: &str = "
mat2 three_d_inverse(mat2 m) {
    return mat2(m[1][1], -m[0][1], -m[1][0], m[0][0]) / (m[0][0] * m[1][1] - m[0][1] * m[1][0]);
}
mat3 three_d_inverse(mat3 m) {
    vec3 a = cross(m[1], m[2]);
    vec3 b = cross(m[2], m[0]);
    vec3 c = cross(m[0], m[1]);
    return transpose(mat3(a, b, c)) / dot(m[0], a);
}
mat4 three_d_inverse(mat4 m) {
    float a00 = m[0][0], a01 = m[0][1], a02 = m[0][2], a03 = m[0][3];
    float a10 = m[1][0], a11 = m[1][1], a12 = m[1][2], a13 = m[1][3];
    float a20 = m[2][0], a21 = m[2][1], a22 = m[2][2], a23 = m[2][3];
    float a30 = m[3][0], a31 = m[3][1], a32 = m[3][2], a33 = m[3][3];
    float b00 = a00 * a11 - a01 * a10;
    float b01 = a00 * a12 - a02 * a10;
    float b02 = a00 * a13 - a03 * a10;
    float b03 = a01 * a12 - a02 * a11;
    float b04 = a01 * a13 - a03 * a11;
    float b05 = a02 * a13 - a03 * a12;
    float b06 = a20 * a31 - a21 * a30;
    float b07 = a20 * a32 - a22 * a30;
    float b08 = a20 * a33 - a23 * a30;
    float b09 = a21 * a32 - a22 * a31;
    float b10 = a21 * a33 - a23 * a31;
    float b11 = a22 * a33 - a23 * a32;
    float det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
    return mat4(
        a11 * b11 - a12 * b10 + a13 * b09, a02 * b10 - a01 * b11 - a03 * b09,
        a31 * b05 - a32 * b04 + a33 * b03, a22 * b04 - a21 * b05 - a23 * b03,
        a12 * b08 - a10 * b11 - a13 * b07, a00 * b11 - a02 * b08 + a03 * b07,
        a32 * b02 - a30 * b05 - a33 * b01, a20 * b05 - a22 * b02 + a23 * b01,
        a10 * b10 - a11 * b08 + a13 * b06, a01 * b08 - a00 * b10 - a03 * b06,
        a30 * b04 - a31 * b02 + a33 * b00, a21 * b02 - a20 * b04 - a23 * b00,
        a11 * b07 - a10 * b09 - a12 * b06, a00 * b09 - a01 * b07 + a02 * b06,
        a31 * b01 - a30 * b03 - a32 * b00, a20 * b03 - a21 * b01 + a22 * b00) / det;
}
";

/// What the two stages of a program have in common.
#[derive(Default)]
struct Shared {
    attributes: Vec<(String, u32)>,
    /// Location of the variables passed from the vertex to the fragment shader.
    varyings: HashMap<String, u32>,
    next_varying: u32,
    samplers: Vec<SamplerDecl>,
    uniform_blocks: Vec<String>,
}

fn translate_stage(
    source: &str,
    stage: Stage,
    shared: &mut Shared,
) -> Result<(String, Vec<String>), String> {
    let source = preprocess(&strip_comments(source))?;
    let items = separate_sampler_parameters(split_items(&source))?;

    let mut structs = String::new();
    let mut members = String::new();
    let mut uniform_names = Vec::new();
    // Uniform arrays with elements made wider (see [widened_element]) and the components that hold the value
    let mut widened: Vec<(String, &str)> = Vec::new();
    let mut body = String::new();
    let mut leading_directives = String::new();
    let mut in_leading_directives = true;

    // Locations that are given explicitly must not be handed out again
    let mut used_inputs = HashSet::new();
    let mut used_outputs = HashSet::new();
    let declarations: Vec<Option<Declaration>> = items
        .iter()
        .map(|item| match item {
            Item::Statement(text) => Declaration::parse(text),
            _ => None,
        })
        .collect();
    for declaration in declarations.iter().flatten() {
        if let Some(location) = declaration.location {
            match declaration.storage {
                Storage::In => used_inputs.insert(location),
                Storage::Out => used_outputs.insert(location),
                Storage::Uniform => false,
            };
        }
    }
    let mut next_input = 0;
    let mut next_output = 0;

    for (item, declaration) in items.iter().zip(declarations) {
        let text = match item {
            Item::Directive(text) => {
                if in_leading_directives {
                    leading_directives.push_str(text);
                    leading_directives.push('\n');
                } else {
                    body.push_str(text);
                    body.push('\n');
                }
                continue;
            }
            Item::Function(text) => {
                in_leading_directives = false;
                body.push_str(text);
                body.push('\n');
                continue;
            }
            Item::Statement(text) => text,
        };
        in_leading_directives = false;
        let first_word = text.split_whitespace().next().unwrap_or("");
        if first_word == "precision" {
            continue;
        }
        if first_word == "struct" {
            structs.push_str(text);
            structs.push('\n');
            continue;
        }
        let Some(declaration) = declaration else {
            body.push_str(text);
            body.push('\n');
            continue;
        };
        match declaration.storage {
            Storage::Uniform => {
                if let Some(block_name) = &declaration.block_name {
                    let binding = match shared.uniform_blocks.iter().position(|n| n == block_name) {
                        Some(binding) => binding,
                        None => {
                            shared.uniform_blocks.push(block_name.clone());
                            shared.uniform_blocks.len() - 1
                        }
                    };
                    body.push_str(&format!(
                        "layout(std140, set = {BLOCK_GROUP}, binding = {binding}) {}\n",
                        declaration.rest
                    ));
                } else if is_sampler_type(&declaration.ty) {
                    for declarator in &declaration.declarators {
                        if declarator.array.is_some() {
                            return Err(format!(
                                "arrays of samplers are not supported by the wgpu backend: {}",
                                text.trim()
                            ));
                        }
                        let index = match shared
                            .samplers
                            .iter()
                            .position(|s| s.name == declarator.name)
                        {
                            Some(index) => {
                                if shared.samplers[index].glsl_type != declaration.ty {
                                    return Err(format!("the sampler {} has different types in the vertex and fragment shader", declarator.name));
                                }
                                index
                            }
                            None => {
                                shared.samplers.push(SamplerDecl {
                                    name: declarator.name.clone(),
                                    glsl_type: declaration.ty.clone(),
                                    in_vertex: false,
                                    in_fragment: false,
                                });
                                shared.samplers.len() - 1
                            }
                        };
                        match stage {
                            Stage::Vertex => shared.samplers[index].in_vertex = true,
                            Stage::Fragment => shared.samplers[index].in_fragment = true,
                        }
                        let name = &declarator.name;
                        let (texture_type, sampler_type) = separate_types(&declaration.ty);
                        body.push_str(&format!(
                            "layout(set = {TEXTURE_GROUP}, binding = {}) uniform {texture_type} three_d_{name}_t;\n\
                             layout(set = {TEXTURE_GROUP}, binding = {}) uniform {sampler_type} three_d_{name}_s;\n\
                             #define {name} {}(three_d_{name}_t, three_d_{name}_s)\n",
                            2 * index,
                            2 * index + 1,
                            declaration.ty
                        ));
                    }
                } else {
                    for declarator in &declaration.declarators {
                        let mut ty = declaration.ty.as_str();
                        if declarator.array.is_some() {
                            if let Some((wide, components)) = widened_element(ty) {
                                ty = wide;
                                widened.push((declarator.name.clone(), components));
                            }
                        }
                        members.push_str(&format!(
                            "    {} {}{};\n",
                            ty,
                            declarator.name,
                            declarator
                                .array
                                .as_ref()
                                .map(|a| format!("[{a}]"))
                                .unwrap_or_default()
                        ));
                        uniform_names.push(declarator.name.clone());
                    }
                }
            }
            Storage::In | Storage::Out => {
                if let Some(explicit_location) = declaration.location {
                    if stage == Stage::Vertex && declaration.storage == Storage::In {
                        for declarator in &declaration.declarators {
                            shared
                                .attributes
                                .push((declarator.name.clone(), explicit_location));
                        }
                    }
                    body.push_str(text);
                    body.push('\n');
                    continue;
                }
                for declarator in &declaration.declarators {
                    if declarator.array.is_some() {
                        return Err(format!(
                            "arrays as shader inputs or outputs are not supported by the wgpu backend: {}",
                            text.trim()
                        ));
                    }
                    let size = location_count(&declaration.ty);
                    let location = match (stage, declaration.storage) {
                        (Stage::Vertex, Storage::In) => {
                            let location = free_location(&mut next_input, size, &used_inputs);
                            shared.attributes.push((declarator.name.clone(), location));
                            location
                        }
                        (Stage::Vertex, Storage::Out) | (Stage::Fragment, Storage::In) => {
                            match shared.varyings.get(&declarator.name) {
                                Some(location) => *location,
                                None => {
                                    let location = shared.next_varying;
                                    shared.next_varying += size;
                                    shared.varyings.insert(declarator.name.clone(), location);
                                    location
                                }
                            }
                        }
                        (Stage::Fragment, Storage::Out) => {
                            free_location(&mut next_output, size, &used_outputs)
                        }
                        (_, Storage::Uniform) => unreachable!(),
                    };
                    body.push_str(&format!(
                        "layout(location = {location}) {} {} {} {};\n",
                        declaration.qualifiers.join(" "),
                        if declaration.storage == Storage::In {
                            "in"
                        } else {
                            "out"
                        },
                        declaration.ty,
                        declarator.name
                    ));
                }
            }
        }
    }

    let mut output = String::from("#version 450\n");
    output.push_str(&leading_directives);
    output.push_str(&structs);
    if !members.is_empty() {
        let (binding, name) = match stage {
            Stage::Vertex => (0, VERTEX_BLOCK),
            Stage::Fragment => (1, FRAGMENT_BLOCK),
        };
        output.push_str(&format!(
            "layout(set = {UNIFORM_GROUP}, binding = {binding}) uniform {name} {{\n{members}}};\n"
        ));
    }
    for (name, components) in &widened {
        body = select_components(&body, name, components)?;
    }
    if contains_word(&body, "inverse") {
        output.push_str(INVERSE);
        body = replace_word(&body, "inverse", "three_d_inverse");
    }
    output.push_str(&body);
    // The indices are unsigned in naga
    let mut output = replace_word(&output, "gl_VertexID", "int(gl_VertexIndex)");
    output = replace_word(&output, "gl_InstanceID", "int(gl_InstanceIndex)");
    if stage == Stage::Vertex {
        output = add_clip_space_conversion(&output)?;
    }
    Ok((output, uniform_names))
}

///
/// Renames `main` and adds a new `main` which calls the old one and then converts the position.
/// - Depth: the normalized device coordinates of OpenGL have z in [-1, 1], the ones of wgpu in [0, 1].
/// - Y: flipped, so that the first row of a texture in memory is the bottom row like in OpenGL.
///   The backend flips the front face accordingly.
///
fn add_clip_space_conversion(source: &str) -> Result<String, String> {
    let bytes = source.as_bytes();
    let mut search = 0;
    while let Some(found) = source[search..].find("main") {
        let start = search + found;
        let end = start + 4;
        search = end;
        let before = source[..start].trim_end();
        let after = source[end..].trim_start();
        let is_word = (start == 0 || !is_identifier(bytes[start - 1]))
            && (end == source.len() || !is_identifier(bytes[end]));
        if is_word && before.ends_with("void") && after.starts_with('(') {
            return Ok(format!(
                "{}three_d_main{}\nvoid main() {{\n    three_d_main();\n    gl_Position.z = 0.5 * (gl_Position.z + gl_Position.w);\n    gl_Position.y = -gl_Position.y;\n}}\n",
                &source[..start],
                &source[end..]
            ));
        }
    }
    Err("the vertex shader has no main function".to_string())
}

fn free_location(next: &mut u32, size: u32, used: &HashSet<u32>) -> u32 {
    while (*next..*next + size).any(|l| used.contains(&l)) {
        *next += 1;
    }
    let location = *next;
    *next += size;
    location
}

/// Number of locations a shader input or output of the given type takes up.
fn location_count(ty: &str) -> u32 {
    match ty {
        "mat2" | "mat2x2" | "mat2x3" | "mat2x4" => 2,
        "mat3" | "mat3x2" | "mat3x3" | "mat3x4" => 3,
        "mat4" | "mat4x2" | "mat4x3" | "mat4x4" => 4,
        _ => 1,
    }
}

fn is_sampler_type(ty: &str) -> bool {
    let ty = ty.trim_start_matches(['i', 'u']);
    ty.starts_with("sampler")
}

/// The separate texture and sampler type of a combined sampler type, for example `texture2D` and `sampler` for `sampler2D`.
fn separate_types(ty: &str) -> (String, &'static str) {
    let (prefix, rest) = match ty.find("sampler") {
        Some(position) => (&ty[..position], &ty[position + "sampler".len()..]),
        None => ("", ty),
    };
    match rest.strip_suffix("Shadow") {
        Some(dimension) => (format!("{prefix}texture{dimension}"), "samplerShadow"),
        None => (format!("{prefix}texture{rest}"), "sampler"),
    }
}

// FUNCTIONS WITH SAMPLER PARAMETERS

///
/// naga does not accept combined texture samplers as function parameters.
/// `float f(sampler2D map, vec2 uv)` becomes `float f(texture2D three_d_map_t, sampler three_d_map_s, vec2 uv)` with a macro `map`
/// that combines the two inside the function, and a call `f(shadowMap, uv)` becomes `f(three_d_shadowMap_t, three_d_shadowMap_s, uv)`.
///
fn separate_sampler_parameters(mut items: Vec<Item>) -> Result<Vec<Item>, String> {
    // The functions with sampler parameters and the positions of those parameters
    let mut functions: HashMap<String, Vec<usize>> = HashMap::new();
    for item in items.iter_mut() {
        let (text, is_definition) = match item {
            Item::Function(text) => (text, true),
            Item::Statement(text) => (text, false),
            Item::Directive(_) => continue,
        };
        let header_end = text.find('{').unwrap_or(text.len());
        let header = &text[..header_end];
        let (Some(open), Some(close)) = (header.find('('), header.rfind(')')) else {
            continue;
        };
        let mut before = header[..open].split_whitespace();
        let (Some(_return_type), Some(name), None) = (before.next(), before.next(), before.next())
        else {
            continue;
        };
        if !name.bytes().all(is_identifier) || header[..open].contains('=') {
            continue;
        }
        let mut positions = Vec::new();
        let mut parameters = Vec::new();
        let mut macros = Vec::new();
        for (position, parameter) in header[open + 1..close].split(',').enumerate() {
            let words: Vec<&str> = parameter.split_whitespace().collect();
            match words.as_slice() {
                [ty, parameter_name] | ["in", ty, parameter_name] if is_sampler_type(ty) => {
                    let (texture_type, sampler_type) = separate_types(ty);
                    positions.push(position);
                    parameters.push(format!(
                        "{texture_type} three_d_{parameter_name}_t, {sampler_type} three_d_{parameter_name}_s"
                    ));
                    macros.push((parameter_name.to_string(), ty.to_string()));
                }
                _ => parameters.push(parameter.trim().to_string()),
            }
        }
        if positions.is_empty() {
            continue;
        }
        let mut rewritten = String::new();
        if is_definition {
            for (parameter_name, ty) in &macros {
                // Replaces the macro of a uniform sampler with the same name, which has the same definition
                rewritten.push_str(&format!(
                    "#undef {parameter_name}\n#define {parameter_name} {ty}(three_d_{parameter_name}_t, three_d_{parameter_name}_s)\n"
                ));
            }
        }
        rewritten.push_str(&format!(
            "{}({}){}",
            &header[..open],
            parameters.join(", "),
            &text[close + 1..]
        ));
        functions.insert(name.to_string(), positions);
        *text = rewritten;
    }
    if functions.is_empty() {
        return Ok(items);
    }

    // The calls
    for item in items.iter_mut() {
        let Item::Function(text) = item else {
            continue;
        };
        let body_start = text.find('{').unwrap_or(0);
        let mut output = text[..body_start].to_string();
        let body = &text[body_start..];
        let bytes = body.as_bytes();
        let mut copied = 0;
        let mut i = 0;
        while i < bytes.len() {
            if !is_identifier(bytes[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && is_identifier(bytes[i]) {
                i += 1;
            }
            let Some(positions) = functions.get(&body[start..i]) else {
                continue;
            };
            let after = body[i..].trim_start();
            if !after.starts_with('(') {
                continue;
            }
            let open = body.len() - after.len();
            // Split the arguments at the commas that are not nested
            let mut depth = 0;
            let mut arguments = Vec::new();
            let mut argument_start = open + 1;
            let mut close = None;
            for (offset, c) in body[open..].char_indices() {
                let position = open + offset;
                match c {
                    '(' | '[' => depth += 1,
                    ')' | ']' => {
                        depth -= 1;
                        if depth == 0 {
                            arguments.push(&body[argument_start..position]);
                            close = Some(position);
                            break;
                        }
                    }
                    ',' if depth == 1 => {
                        arguments.push(&body[argument_start..position]);
                        argument_start = position + 1;
                    }
                    _ => {}
                }
            }
            let Some(close) = close else {
                continue;
            };
            let mut new_arguments = Vec::new();
            for (position, argument) in arguments.iter().enumerate() {
                let argument = argument.trim();
                if positions.contains(&position) {
                    if !argument.bytes().all(is_identifier) {
                        return Err(format!(
                            "only the name of a sampler can be passed to a function with a sampler parameter on the wgpu backend, not: {argument}"
                        ));
                    }
                    new_arguments.push(format!("three_d_{argument}_t, three_d_{argument}_s"));
                } else {
                    new_arguments.push(argument.to_string());
                }
            }
            output.push_str(&body[copied..open + 1]);
            output.push_str(&new_arguments.join(", "));
            copied = close;
            // Nested calls inside the arguments are not handled, continue after the call
            i = close;
        }
        output.push_str(&body[copied..]);
        *text = output;
    }
    Ok(items)
}

// DECLARATIONS

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Storage {
    Uniform,
    In,
    Out,
}

#[derive(Debug)]
struct Declarator {
    name: String,
    /// The text between the array brackets.
    array: Option<String>,
}

/// A global `uniform`, `in` or `out` declaration.
#[derive(Debug)]
struct Declaration {
    storage: Storage,
    /// Interpolation qualifiers, for example `flat`.
    qualifiers: Vec<String>,
    location: Option<u32>,
    ty: String,
    declarators: Vec<Declarator>,
    /// The name of the uniform block if this is one.
    block_name: Option<String>,
    /// The statement without the layout qualifier.
    rest: String,
}

impl Declaration {
    fn parse(statement: &str) -> Option<Self> {
        let mut rest = statement.trim();
        let mut location = None;
        if let Some(after) = rest.strip_prefix("layout") {
            let after = after.trim_start();
            if !after.starts_with('(') {
                return None;
            }
            let close = after.find(')')?;
            for part in after[1..close].split(',') {
                let mut key_value = part.split('=');
                if key_value.next().map(|k| k.trim()) == Some("location") {
                    location = key_value.next().and_then(|v| v.trim().parse().ok());
                }
            }
            rest = after[close + 1..].trim_start();
        }
        let without_layout = rest.to_string();
        let mut qualifiers = Vec::new();
        let mut storage = None;
        loop {
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            let word = &rest[..end];
            match word {
                "flat" | "smooth" | "noperspective" | "centroid" => {
                    qualifiers.push(word.to_string())
                }
                "highp" | "mediump" | "lowp" | "invariant" => {}
                "uniform" if storage.is_none() => storage = Some(Storage::Uniform),
                "in" if storage.is_none() => storage = Some(Storage::In),
                "out" if storage.is_none() => storage = Some(Storage::Out),
                _ => break,
            }
            rest = rest[end..].trim_start();
        }
        let storage = storage?;
        let rest = rest.trim_end_matches(';').trim();
        if let Some(brace) = rest.find('{') {
            // A uniform block
            return (storage == Storage::Uniform).then(|| Self {
                storage,
                qualifiers,
                location,
                ty: String::new(),
                declarators: Vec::new(),
                block_name: Some(rest[..brace].trim().to_string()),
                rest: without_layout,
            });
        }
        let type_end = rest.find(|c: char| c.is_whitespace())?;
        let ty = rest[..type_end].to_string();
        let declarators = rest[type_end..]
            .split(',')
            .map(|declarator| {
                let declarator = declarator.split('=').next().unwrap().trim();
                match declarator.find('[') {
                    Some(open) => Declarator {
                        name: declarator[..open].trim().to_string(),
                        array: Some(
                            declarator[open + 1..]
                                .trim_end_matches(']')
                                .trim()
                                .to_string(),
                        ),
                    },
                    None => Declarator {
                        name: declarator.to_string(),
                        array: None,
                    },
                }
            })
            .collect::<Vec<_>>();
        if declarators
            .iter()
            .any(|d| d.name.is_empty() || !d.name.bytes().all(is_identifier))
        {
            return None;
        }
        Some(Self {
            storage,
            qualifiers,
            location,
            ty,
            declarators,
            block_name: None,
            rest: without_layout,
        })
    }
}

///
/// The elements of an array in a uniform block are 16 bytes apart. Metal and WGSL place the elements of an array of
/// scalars or vectors with two components closer together, so such an array becomes an array of vectors with four
/// components. Returns that type and the components which hold the value.
///
fn widened_element(ty: &str) -> Option<(&'static str, &'static str)> {
    Some(match ty {
        "float" => ("vec4", ".x"),
        "int" => ("ivec4", ".x"),
        "uint" => ("uvec4", ".x"),
        "vec2" => ("vec4", ".xy"),
        "ivec2" => ("ivec4", ".xy"),
        "uvec2" => ("uvec4", ".xy"),
        _ => return None,
    })
}

/// Appends the components to each `name[index]` in the source, for an array of [widened_element].
fn select_components(source: &str, name: &str, components: &str) -> Result<String, String> {
    let bytes = source.as_bytes();
    // The positions behind the closing brackets, uses can be nested
    let mut positions = Vec::new();
    let mut search = 0;
    while let Some(found) = source[search..].find(name) {
        let start = search + found;
        let mut end = start + name.len();
        search = end;
        if (start > 0 && (is_identifier(bytes[start - 1]) || bytes[start - 1] == b'.'))
            || (end < source.len() && is_identifier(bytes[end]))
        {
            continue;
        }
        while end < source.len() && bytes[end].is_ascii_whitespace() {
            end += 1;
        }
        if bytes.get(end) != Some(&b'[') {
            return Err(format!(
                "the uniform array {name} is used without an index, which is not supported by the wgpu backend"
            ));
        }
        let mut depth = 0;
        while end < source.len() {
            match bytes[end] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                _ => {}
            }
            end += 1;
            if depth == 0 {
                break;
            }
        }
        positions.push(end);
    }
    positions.sort_unstable();
    let mut output = String::with_capacity(source.len() + positions.len() * components.len());
    let mut copied = 0;
    for position in positions {
        output.push_str(&source[copied..position]);
        output.push_str(components);
        copied = position;
    }
    output.push_str(&source[copied..]);
    Ok(output)
}

// TEXT HANDLING

fn is_identifier(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn contains_word(source: &str, word: &str) -> bool {
    replace_word(source, word, "") != source
}

fn replace_word(source: &str, word: &str, replacement: &str) -> String {
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut copied = 0;
    let mut search = 0;
    while let Some(found) = source[search..].find(word) {
        let start = search + found;
        let end = start + word.len();
        search = end;
        if (start == 0 || !is_identifier(bytes[start - 1]))
            && (end == source.len() || !is_identifier(bytes[end]))
        {
            output.push_str(&source[copied..start]);
            output.push_str(replacement);
            copied = end;
        }
    }
    output.push_str(&source[copied..]);
    output
}

/// Removes the comments but keeps the line breaks.
fn strip_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'/') {
            for c in chars.by_ref() {
                if c == '\n' {
                    output.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut previous = ' ';
            for c in chars.by_ref() {
                if c == '\n' {
                    output.push('\n');
                }
                if previous == '*' && c == '/' {
                    break;
                }
                previous = c;
            }
            output.push(' ');
        } else {
            output.push(c);
        }
    }
    output
}

///
/// Resolves the conditional compilation. `#define` and `#undef` lines are kept for the preprocessor of naga, which expands the macros.
///
fn preprocess(source: &str) -> Result<String, String> {
    struct Level {
        /// Whether the enclosing levels are all active
        parent_active: bool,
        /// Whether one of the branches of this level has been taken already
        taken: bool,
        active: bool,
    }
    let mut defines: HashMap<String, String> = HashMap::new();
    let mut levels: Vec<Level> = Vec::new();
    let mut output = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim();
        let active = levels.last().map(|l| l.active).unwrap_or(true);
        if let Some(directive) = trimmed.strip_prefix('#') {
            let directive = directive.trim_start();
            let end = directive
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(directive.len());
            let (name, argument) = (&directive[..end], directive[end..].trim());
            match name {
                "ifdef" | "ifndef" | "if" => {
                    let condition = active
                        && match name {
                            "ifdef" => defines.contains_key(argument),
                            "ifndef" => !defines.contains_key(argument),
                            _ => evaluate_condition(argument, &defines)?,
                        };
                    levels.push(Level {
                        parent_active: active,
                        taken: condition,
                        active: condition,
                    });
                }
                "elif" | "else" => {
                    let level = levels
                        .last_mut()
                        .ok_or_else(|| format!("#{name} without #if"))?;
                    let condition = level.parent_active
                        && !level.taken
                        && (name == "else" || evaluate_condition(argument, &defines)?);
                    level.active = condition;
                    level.taken |= condition;
                }
                "endif" => {
                    levels.pop().ok_or("#endif without #if")?;
                }
                "define" if active => {
                    let end = argument
                        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                        .unwrap_or(argument.len());
                    defines.insert(
                        argument[..end].to_string(),
                        argument[end..].trim().to_string(),
                    );
                    output.push_str(line);
                }
                "undef" if active => {
                    defines.remove(argument);
                    output.push_str(line);
                }
                // The version is set by the translation and extensions are not needed in GLSL 4.50
                _ => {}
            }
        } else if active {
            output.push_str(line);
        }
        output.push('\n');
    }
    if !levels.is_empty() {
        return Err("#if without #endif".to_string());
    }
    Ok(output)
}

///
/// Evaluates the expression of an `#if` or `#elif`: integers, macros with integer values, `defined`, `!`, `&&`, `||`, comparisons and parentheses.
///
fn evaluate_condition(expression: &str, defines: &HashMap<String, String>) -> Result<bool, String> {
    struct Parser<'a> {
        tokens: Vec<&'a str>,
        position: usize,
        defines: &'a HashMap<String, String>,
    }
    impl<'a> Parser<'a> {
        fn peek(&self) -> Option<&'a str> {
            self.tokens.get(self.position).copied()
        }
        fn next(&mut self) -> Option<&'a str> {
            self.position += 1;
            self.tokens.get(self.position - 1).copied()
        }
        fn or(&mut self) -> Result<i64, String> {
            let mut value = self.and()?;
            while self.peek() == Some("||") {
                self.next();
                let right = self.and()?;
                value = (value != 0 || right != 0) as i64;
            }
            Ok(value)
        }
        fn and(&mut self) -> Result<i64, String> {
            let mut value = self.comparison()?;
            while self.peek() == Some("&&") {
                self.next();
                let right = self.comparison()?;
                value = (value != 0 && right != 0) as i64;
            }
            Ok(value)
        }
        fn comparison(&mut self) -> Result<i64, String> {
            let left = self.unary()?;
            let Some(operator) = self.peek() else {
                return Ok(left);
            };
            if !["==", "!=", "<", ">", "<=", ">="].contains(&operator) {
                return Ok(left);
            }
            self.next();
            let right = self.unary()?;
            Ok(match operator {
                "==" => left == right,
                "!=" => left != right,
                "<" => left < right,
                ">" => left > right,
                "<=" => left <= right,
                _ => left >= right,
            } as i64)
        }
        fn unary(&mut self) -> Result<i64, String> {
            match self.next() {
                Some("!") => Ok((self.unary()? == 0) as i64),
                Some("(") => {
                    let value = self.or()?;
                    self.next();
                    Ok(value)
                }
                Some("defined") => {
                    let parenthesis = self.peek() == Some("(");
                    if parenthesis {
                        self.next();
                    }
                    let name = self.next().ok_or("incomplete #if expression")?;
                    if parenthesis {
                        self.next();
                    }
                    Ok(self.defines.contains_key(name) as i64)
                }
                Some(token) => {
                    if let Ok(value) = token.parse::<i64>() {
                        Ok(value)
                    } else if token.bytes().all(is_identifier) {
                        // An undefined macro counts as 0, like in the C preprocessor
                        Ok(self
                            .defines
                            .get(token)
                            .and_then(|value| value.parse::<i64>().ok())
                            .unwrap_or(0))
                    } else {
                        Err(format!("unsupported token {token} in #if expression"))
                    }
                }
                None => Err("incomplete #if expression".to_string()),
            }
        }
    }

    let mut tokens = Vec::new();
    let bytes = expression.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        } else if is_identifier(bytes[i]) {
            while i < bytes.len() && is_identifier(bytes[i]) {
                i += 1;
            }
        } else if i + 1 < bytes.len()
            && ["&&", "||", "==", "!=", "<=", ">="].contains(&&expression[i..i + 2])
        {
            i += 2;
        } else {
            i += 1;
        }
        tokens.push(&expression[start..i]);
    }
    let mut parser = Parser {
        tokens,
        position: 0,
        defines,
    };
    Ok(parser.or()? != 0)
}

/// A part of the source at global scope.
#[derive(Debug)]
enum Item {
    /// A preprocessor line.
    Directive(String),
    /// A declaration ending with a semicolon, including struct definitions and uniform blocks.
    Statement(String),
    /// A function definition.
    Function(String),
}

fn split_items(source: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut braces = 0;
    let mut parentheses = 0;
    for line in source.lines() {
        if line.trim_start().starts_with('#') {
            if braces == 0 && current.trim().is_empty() {
                items.push(Item::Directive(line.to_string()));
            } else {
                // A directive inside a function or declaration stays where it is
                current.push_str(line);
                current.push('\n');
            }
            continue;
        }
        for c in line.chars() {
            current.push(c);
            match c {
                '{' => braces += 1,
                '(' => parentheses += 1,
                ')' => parentheses -= 1,
                '}' => {
                    braces -= 1;
                    if braces == 0 && parentheses == 0 {
                        let first_word = current.split_whitespace().next().unwrap_or("");
                        let is_function = !matches!(first_word, "struct" | "uniform" | "layout")
                            && !current[..current.find('{').unwrap_or(0)].contains('=');
                        if is_function {
                            items.push(Item::Function(std::mem::take(&mut current)));
                        }
                    }
                }
                ';' if braces == 0 && parentheses == 0 => {
                    items.push(Item::Statement(std::mem::take(&mut current)));
                }
                _ => {}
            }
        }
        current.push('\n');
    }
    if !current.trim().is_empty() {
        items.push(Item::Statement(current));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_parameters_are_separated() {
        let translation = translate(
            "void main() { gl_Position = vec4(0.0); }",
            "uniform sampler2D map0;\nfloat inner(sampler2D map, vec2 uv) { return texture(map, uv).r; }\nfloat outer(vec3 n, sampler2D map, vec2 uv) { return inner(map, uv * n.xy); }\nout vec4 color;\nvoid main() { color = vec4(outer(vec3(max(1.0, 2.0)), map0, vec2(0.5))); }",
        )
        .unwrap();
        let fragment = &translation.fragment;
        assert!(fragment
            .contains("float inner(texture2D three_d_map_t, sampler three_d_map_s, vec2 uv)"));
        assert!(fragment.contains("return inner(three_d_map_t, three_d_map_s, uv * n.xy);"));
        assert!(fragment
            .contains("outer(vec3(max(1.0, 2.0)), three_d_map0_t, three_d_map0_s, vec2(0.5))"));
    }

    #[test]
    fn conditions_are_resolved() {
        let source = "#define A\n#define N 3\n#ifdef A\nx\n#else\ny\n#endif\n#ifndef A\nz\n#endif\n#if defined(B) || N >= 3\nw\n#elif 1\nv\n#endif\n";
        let output = preprocess(source).unwrap();
        let words: Vec<&str> = output
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        assert_eq!(words, ["x", "w"]);
        assert_eq!(output.lines().count(), source.lines().count());
    }

    #[test]
    fn uniform_arrays_of_small_elements_are_widened() {
        // Metal would place the layers 4 bytes apart and all following uniforms at other offsets than the data has
        let translation = translate(
            "uniform vec2 directions[4];\nin vec3 position;\nvoid main() { gl_Position = vec4(position.xy + directions [gl_VertexID % 4], 0.0, 1.0); }",
            "uniform int layers[4];\nuniform vec3 colors[2];\nlayout (location = 0) out vec4 outColor;\nvoid main() { outColor = vec4(colors[layers[layers[1]]], 1.0); }",
        )
        .unwrap();
        assert!(translation.vertex.contains("vec4 directions[4];"));
        assert!(translation
            .vertex
            .contains("directions [int(gl_VertexIndex) % 4].xy"));
        assert!(translation.fragment.contains("ivec4 layers[4];"));
        assert!(translation.fragment.contains("vec3 colors[2];"));
        assert!(translation
            .fragment
            .contains("colors[layers[layers[1].x].x]"));
        assert!(translate(
            "in vec3 position;\nvoid main() { gl_Position = vec4(position, 1.0); }",
            "uniform float weights[4];\nfloat sum(float w[4]) { return w[0] + w[1]; }\nlayout (location = 0) out vec4 outColor;\nvoid main() { outColor = vec4(sum(weights)); }",
        )
        .is_err());
    }

    #[test]
    fn uniforms_samplers_and_locations() {
        let translation = translate(
            "uniform mat4 viewProjection; // comment\n#ifdef NOPE\nuniform float unused;\n#endif\nin vec3 position;\nin vec4 color;\nout vec4 col;\nflat out int id;\nvoid main() { id = gl_InstanceID; col = color; gl_Position = viewProjection * vec4(position, 1.0); }",
            "uniform vec3 tint;\nuniform sampler2D tex;\nin vec4 col;\nflat in int id;\nlayout (location = 0) out vec4 outColor;\nvoid main() { outColor = col * vec4(tint, 1.0) * texture(tex, vec2(0.5)); }",
        )
        .unwrap();
        assert_eq!(translation.vertex_uniforms, ["viewProjection"]);
        assert_eq!(translation.fragment_uniforms, ["tint"]);
        assert_eq!(
            translation.attributes,
            [("position".to_string(), 0), ("color".to_string(), 1)]
        );
        assert_eq!(translation.samplers.len(), 1);
        assert!(translation.samplers[0].in_fragment && !translation.samplers[0].in_vertex);
        let vertex = &translation.vertex;
        assert!(vertex.starts_with("#version 450\n"));
        assert!(vertex.contains("layout(location = 1)  in vec4 color;"));
        assert!(vertex.contains("layout(location = 1) flat out int id;"));
        assert!(vertex.contains("id = int(gl_InstanceIndex);"));
        assert!(vertex.contains("void three_d_main()"));
        assert!(!vertex.contains("unused"));
        let fragment = &translation.fragment;
        assert!(fragment.contains("layout(location = 1) flat in int id;"));
        assert!(fragment.contains("uniform texture2D three_d_tex_t;"));
        assert!(fragment.contains("#define tex sampler2D(three_d_tex_t, three_d_tex_s)"));
        assert!(fragment.contains("layout (location = 0) out vec4 outColor;"));
    }
}
