//!
//! Sends the shaders of this crate through the GLSL translation and naga.
//!
//! The programs are collected by rendering with the recording test backend, so they are exactly the
//! vertex and fragment shader pairs the renderer module builds, including the shader variants that depend
//! on the geometry, the material, the lights and the textures.
//! Each program is translated ([glsl::translate]), parsed and validated by naga and then written in the shader
//! languages wgpu uses on the different platforms (SPIR-V for Vulkan, WGSL for the browser, MSL for Metal, HLSL for DirectX 12).
//!

use super::{glsl, parse_shader};
use crate::core::backend::{null::NullBackend, Backend};
use crate::renderer::*;
use std::sync::Arc;

fn texture(data: TextureData) -> CpuTexture {
    CpuTexture {
        data,
        width: 2,
        height: 2,
        ..Default::default()
    }
}

fn rgba() -> CpuTexture {
    texture(TextureData::RgbaU8(vec![[255, 255, 255, 255]; 4]))
}

///
/// Renders about everything the renderer module has and returns the sources of all programs that were created for it.
///
fn programs() -> Vec<(String, String)> {
    let context = Context::from_backend(Backend::Null(NullBackend::default()));
    let viewport = Viewport::new_at_origo(16, 16);
    let camera = Camera::new_perspective(
        viewport,
        vec3(0.0, 0.0, 5.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.1,
        10.0,
    );
    let color_texture = Texture2D::new_empty::<[u8; 4]>(
        &context,
        16,
        16,
        Interpolation::Nearest,
        Interpolation::Nearest,
        None,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    );
    let depth_texture = DepthTexture2D::new::<f32>(
        &context,
        16,
        16,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    );
    let mut color_array = Texture2DArray::new_empty::<[u8; 4]>(
        &context,
        16,
        16,
        2,
        Interpolation::Nearest,
        Interpolation::Nearest,
        None,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    );
    let mut depth_array = DepthTexture2DArray::new::<f32>(
        &context,
        16,
        16,
        2,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    );
    let screen = || RenderTarget::screen(&context, 16, 16);

    // LIGHTS
    let environment_map = TextureCubeMap::new_from_equirectangular::<f16>(
        &context,
        &texture(TextureData::RgbaF32(vec![[1.0; 4]; 4])),
    );
    let skybox = Skybox::new(
        &context,
        &rgba(),
        &rgba(),
        &rgba(),
        &rgba(),
        &rgba(),
        &rgba(),
    );
    let skybox_equirectangular = Skybox::new_from_equirectangular(
        &context,
        &texture(TextureData::RgbaF32(vec![[1.0; 4]; 4])),
    );
    let ambient = AmbientLight::new(&context, 0.5, Srgba::WHITE);
    let mut directional = DirectionalLight::new(&context, 1.0, Srgba::WHITE, vec3(0.0, -1.0, -1.0));
    let directional_without_shadow =
        DirectionalLight::new(&context, 1.0, Srgba::WHITE, vec3(0.0, -1.0, -1.0));
    let point = PointLight::new(
        &context,
        1.0,
        Srgba::WHITE,
        vec3(0.0, 2.0, 0.0),
        Attenuation::default(),
    );
    let mut spot = SpotLight::new(
        &context,
        1.0,
        Srgba::WHITE,
        vec3(0.0, 2.0, 0.0),
        vec3(0.0, -1.0, 0.0),
        degrees(25.0),
        Attenuation::default(),
    );
    let mut ambient_environments = Vec::new();
    for lighting_model in [
        LightingModel::Phong,
        LightingModel::Blinn,
        LightingModel::Cook(
            NormalDistributionFunction::Blinn,
            GeometryFunction::SmithSchlickGGX,
        ),
        LightingModel::Cook(
            NormalDistributionFunction::Beckmann,
            GeometryFunction::SmithSchlickGGX,
        ),
        LightingModel::Cook(
            NormalDistributionFunction::TrowbridgeReitzGGX,
            GeometryFunction::SmithSchlickGGX,
        ),
    ] {
        ambient_environments.push(AmbientLight {
            environment: Some(Environment::new_with_lighting_model(
                &context,
                &environment_map,
                lighting_model,
            )),
            ..Default::default()
        });
    }

    // GEOMETRIES
    let mut cpu_mesh = CpuMesh::cube();
    cpu_mesh.compute_tangents();
    let mut cpu_mesh_with_colors = cpu_mesh.clone();
    cpu_mesh_with_colors.colors = Some(vec![Srgba::WHITE; cpu_mesh.positions.len()]);
    let plain_mesh = CpuMesh {
        positions: cpu_mesh.positions.clone(),
        ..Default::default()
    };
    let instances = Instances {
        transformations: vec![Mat4::identity(); 2],
        ..Default::default()
    };
    let instances_with_all = Instances {
        transformations: vec![Mat4::identity(); 2],
        texture_transformations: Some(vec![Mat3::identity(); 2]),
        colors: Some(vec![Srgba::WHITE; 2]),
    };
    let particles = Particles {
        start_positions: vec![vec3(0.0, 0.0, 0.0); 2],
        start_velocities: vec![vec3(0.0, 1.0, 0.0); 2],
        texture_transforms: Some(vec![Mat3::identity(); 2]),
        colors: Some(vec![Srgba::WHITE; 2]),
    };
    let geometries: Vec<Box<dyn Geometry>> = vec![
        Box::new(Mesh::new(&context, &cpu_mesh)),
        Box::new(Mesh::new(&context, &cpu_mesh_with_colors)),
        Box::new(Mesh::new(&context, &plain_mesh)),
        Box::new(InstancedMesh::new(&context, &instances, &cpu_mesh)),
        Box::new(InstancedMesh::new(
            &context,
            &instances_with_all,
            &cpu_mesh_with_colors,
        )),
        Box::new(InstancedMesh::new(&context, &instances, &plain_mesh)),
        Box::new(ParticleSystem::new(
            &context,
            &particles,
            vec3(0.0, -9.8, 0.0),
            &cpu_mesh_with_colors,
        )),
        Box::new(Sprites::new(&context, &[vec3(0.0, 0.0, 0.0)], None)),
        Box::new(Sprites::new(
            &context,
            &[vec3(0.0, 0.0, 0.0)],
            Some(vec3(0.0, 1.0, 0.0)),
        )),
        Box::new(BoundingBox::new(
            &context,
            AxisAlignedBoundingBox::new_with_positions(&cpu_mesh.positions.to_f32()),
        )),
    ];

    // MATERIALS
    let cpu_material = CpuMaterial {
        albedo_texture: Some(rgba()),
        metallic_roughness_texture: Some(rgba()),
        occlusion_texture: Some(rgba()),
        normal_texture: Some(rgba()),
        emissive_texture: Some(rgba()),
        ..Default::default()
    };
    let cpu_material_combined = CpuMaterial {
        occlusion_metallic_roughness_texture: Some(rgba()),
        ..Default::default()
    };
    let mut materials: Vec<Box<dyn Material>> = vec![
        Box::new(ColorMaterial::default()),
        Box::new(ColorMaterial::new_opaque(&context, &cpu_material)),
        Box::new(ColorMaterial::new_transparent(&context, &cpu_material)),
        Box::new(DepthMaterial::default()),
        Box::new(NormalMaterial::default()),
        Box::new(NormalMaterial::new(&context, &cpu_material)),
        Box::new(ORMMaterial::default()),
        Box::new(ORMMaterial::new(&context, &cpu_material)),
        Box::new(ORMMaterial::new(&context, &cpu_material_combined)),
        Box::new(PositionMaterial::default()),
        Box::new(UVMaterial::default()),
        Box::new(IntersectionMaterial::default()),
        Box::new(DeferredPhysicalMaterial::new(
            &context,
            &CpuMaterial::default(),
        )),
        Box::new(DeferredPhysicalMaterial::new(&context, &cpu_material)),
        Box::new(DeferredPhysicalMaterial::new(
            &context,
            &cpu_material_combined,
        )),
    ];
    for lighting_model in [
        LightingModel::Phong,
        LightingModel::Blinn,
        LightingModel::Cook(
            NormalDistributionFunction::Blinn,
            GeometryFunction::SmithSchlickGGX,
        ),
        LightingModel::Cook(
            NormalDistributionFunction::Beckmann,
            GeometryFunction::SmithSchlickGGX,
        ),
        LightingModel::Cook(
            NormalDistributionFunction::TrowbridgeReitzGGX,
            GeometryFunction::SmithSchlickGGX,
        ),
    ] {
        for cpu_material in [
            &CpuMaterial::default(),
            &cpu_material,
            &cpu_material_combined,
        ] {
            let mut material = PhysicalMaterial::new_opaque(&context, cpu_material);
            material.lighting_model = lighting_model;
            materials.push(Box::new(material));
        }
    }
    materials.push(Box::new(PhysicalMaterial::new_transparent(
        &context,
        &cpu_material,
    )));

    // Shadow maps
    let shadow_casters = [Mesh::new(&context, &cpu_mesh)];
    let _ = directional.generate_shadow_map(64, &shadow_casters);
    let _ = spot.generate_shadow_map(64, &shadow_casters);

    let mut light_sets: Vec<Vec<&dyn Light>> = vec![
        vec![],
        vec![&ambient],
        vec![&directional],
        vec![&directional_without_shadow],
        vec![&point],
        vec![&spot],
        vec![
            &ambient,
            &directional,
            &directional_without_shadow,
            &point,
            &spot,
        ],
    ];
    for ambient in &ambient_environments {
        light_sets.push(vec![ambient, &directional]);
    }

    // Every geometry with every material and every set of lights
    for geometry in &geometries {
        for material in &materials {
            for lights in &light_sets {
                // Materials without lighting do not depend on the lights
                if material.material_type() == MaterialType::Deferred && !lights.is_empty() {
                    continue;
                }
                screen().render_with_material(&**material, &camera, [&**geometry], lights);
            }
        }
    }

    // OBJECTS
    let wireframe = Wireframe::new_from_cpu_mesh(&context, &cpu_mesh, 0.01, Srgba::WHITE);
    let axes = Axes::new(&context, 0.1, 1.0);
    let voxel_grid = VoxelGrid::<IsosurfaceMaterial>::new(&context, &CpuVoxelGrid::default());
    let terrain = Terrain::new(
        &context,
        PhysicalMaterial::new_opaque(&context, &cpu_material),
        Arc::new(|_, _| 0.0),
        16.0,
        1.0,
        vec2(0.0, 0.0),
    );
    let water = Water::new(
        &context,
        ColorMaterial::default(),
        0.0,
        vec2(0.0, 0.0),
        16.0,
        1.0,
        [WaveParameters::default()],
    );
    let water_effects = [
        WaterEffect {
            background: Background::Color(Srgba::WHITE),
            metallic: 0.0,
            roughness: 1.0,
            lighting_model: LightingModel::Blinn,
        },
        WaterEffect {
            background: Background::Texture(Arc::new(environment_map)),
            metallic: 0.0,
            roughness: 1.0,
            lighting_model: LightingModel::Blinn,
        },
    ];
    let model = Gm::new(
        Mesh::new(&context, &cpu_mesh),
        PhysicalMaterial::new_opaque(&context, &cpu_material),
    );
    let imposters = Imposters::new(&context, &[vec3(0.0, 0.0, 0.0)], [&model], &[&ambient], 64);
    for lights in &light_sets {
        screen().render(
            &camera,
            wireframe
                .into_iter()
                .chain(&axes)
                .chain(&voxel_grid)
                .chain(&terrain)
                .chain(&skybox)
                .chain(&skybox_equirectangular)
                .chain(&imposters),
            lights,
        );
    }

    // EFFECTS
    let color_textures = [
        ColorTexture::Single(&color_texture),
        ColorTexture::Array {
            texture: &color_array,
            layers: &[0, 1],
        },
    ];
    let depth_textures = [
        DepthTexture::Single(&depth_texture),
        DepthTexture::Array {
            texture: &depth_array,
            layer: 0,
        },
    ];
    for color in color_textures {
        for depth in depth_textures {
            let target = screen();
            target.apply_screen_effect(
                &FogEffect::default(),
                &camera,
                &[],
                Some(color),
                Some(depth),
            );
            target.apply_screen_effect(&FxaaEffect {}, &camera, &[], Some(color), Some(depth));
            target.apply_screen_effect(
                &CopyEffect::default(),
                &camera,
                &[],
                Some(color),
                Some(depth),
            );
            target.apply_screen_effect(&CopyEffect::default(), &camera, &[], Some(color), None);
            target.apply_screen_effect(&CopyEffect::default(), &camera, &[], None, Some(depth));
            target.apply_screen_effect(&ScreenEffect::default(), &camera, &[], Some(color), None);
            for lights in &light_sets {
                for effect in &water_effects {
                    target.render_with_effect(
                        effect,
                        &camera,
                        &water,
                        lights,
                        Some(color),
                        Some(depth),
                    );
                }
            }
        }
    }

    // Deferred rendering, every debug type and tone and color mapping
    let deferred = Gm::new(
        Mesh::new(&context, &cpu_mesh),
        DeferredPhysicalMaterial::new(&context, &cpu_material),
    );
    for lights in &light_sets {
        screen().render(&camera, &deferred, lights);
    }
    for tone_mapping in [
        ToneMapping::None,
        ToneMapping::Reinhard,
        ToneMapping::Aces,
        ToneMapping::Filmic,
    ] {
        for color_mapping in [ColorMapping::None, ColorMapping::ComputeToSrgb] {
            let mut camera = camera.clone();
            camera.tone_mapping = tone_mapping;
            camera.color_mapping = color_mapping;
            screen().render(&camera, &model, &[&ambient]);
            screen().apply_screen_effect(
                &CopyEffect::default(),
                &camera,
                &[],
                Some(color_textures[0]),
                None,
            );
        }
    }

    // Picking
    let _ = pick(
        &context,
        &camera,
        (8.0, 8.0),
        geometries.iter().map(|g| &**g),
        Cull::Back,
    );
    // Offscreen targets
    RenderTarget::new(
        color_texture.as_color_target(None),
        depth_texture.as_depth_target(),
    )
    .render(&camera, &model, &[&ambient]);
    let _ = (&mut color_array, &mut depth_array);

    let mut sources = context.null_backend().take_program_sources();
    sources.sort();
    sources.dedup();
    sources
}

struct Report {
    programs: usize,
    failures: Vec<(&'static str, String, String)>,
}

impl Report {
    /// Number of programs that failed in the given step.
    fn count(&self, step: &str) -> usize {
        self.failures.iter().filter(|(s, _, _)| *s == step).count()
    }

    /// The distinct error messages of the given step with the number of programs they occur in.
    fn summary(&self, step: &str) -> String {
        let mut messages: Vec<(String, usize)> = Vec::new();
        for (_, message, _) in self.failures.iter().filter(|(s, _, _)| *s == step) {
            let first_line = message.lines().next().unwrap_or("").to_string();
            match messages.iter_mut().find(|(m, _)| *m == first_line) {
                Some((_, count)) => *count += 1,
                None => messages.push((first_line, 1)),
            }
        }
        messages
            .iter()
            .map(|(message, count)| format!("    {count:>4} x {message}\n"))
            .collect()
    }
}

fn check(programs: &[(String, String)]) -> Report {
    let mut report = Report {
        programs: programs.len(),
        failures: Vec::new(),
    };
    for (vertex, fragment) in programs {
        let translation = match glsl::translate(vertex, fragment) {
            Ok(translation) => translation,
            Err(error) => {
                report
                    .failures
                    .push(("translate", error, format!("{vertex}\n{fragment}")));
                continue;
            }
        };
        let mut failed = false;
        for (stage, source) in [
            (glsl::Stage::Vertex, &translation.vertex),
            (glsl::Stage::Fragment, &translation.fragment),
        ] {
            let (module, info) = match parse_shader(stage, source) {
                Ok(result) => result,
                Err(error) => {
                    if !failed {
                        report.failures.push(("naga", error, source.clone()));
                    }
                    failed = true;
                    continue;
                }
            };
            let naga_stage = match stage {
                glsl::Stage::Vertex => naga::ShaderStage::Vertex,
                glsl::Stage::Fragment => naga::ShaderStage::Fragment,
            };

            // WGSL, for WebGPU in the browser
            if let Err(error) = naga::back::wgsl::write_string(
                &module,
                &info,
                naga::back::wgsl::WriterFlags::empty(),
            ) {
                report
                    .failures
                    .push(("wgsl", error.to_string(), source.clone()));
            }
            // SPIR-V, for Vulkan
            if let Err(error) = naga::back::spv::write_vec(
                &module,
                &info,
                &naga::back::spv::Options::default(),
                Some(&naga::back::spv::PipelineOptions {
                    shader_stage: naga_stage,
                    entry_point: "main".to_string(),
                }),
            ) {
                report
                    .failures
                    .push(("spirv", error.to_string(), source.clone()));
            }
            // MSL, for Metal
            if let Err(error) = naga::back::msl::write_string(
                &module,
                &info,
                &naga::back::msl::Options {
                    // wgpu chooses the version by the operating system, this one is from macOS 11
                    lang_version: (2, 3),
                    ..Default::default()
                },
                &naga::back::msl::PipelineOptions::default(),
            ) {
                report
                    .failures
                    .push(("msl", error.to_string(), source.clone()));
            }
            // HLSL, for DirectX 12
            let mut hlsl = String::new();
            let pipeline_options = Default::default();
            if let Err(error) = naga::back::hlsl::Writer::new(
                &mut hlsl,
                &naga::back::hlsl::Options::default(),
                &pipeline_options,
            )
            .write(&module, &info, None)
            {
                report
                    .failures
                    .push(("hlsl", error.to_string(), source.clone()));
            }
        }
    }
    report
}

///
/// All programs of the renderer module must translate for all platforms.
///
#[test]
fn renderer_shaders_translate() {
    let programs = programs();
    let report = check(&programs);
    let mut text = format!("{} distinct programs\n", report.programs);
    for step in ["translate", "naga", "wgsl", "spirv", "msl", "hlsl"] {
        text.push_str(&format!(
            "  {step:<9} {} failed\n{}",
            report.count(step),
            report.summary(step)
        ));
    }
    println!("{text}");
    if let Ok(path) = std::env::var("THREE_D_SHADER_REPORT") {
        let mut details = text.clone();
        for (step, message, source) in &report.failures {
            details.push_str(&format!(
                "\n===== {step} =====\n{message}\n----- source -----\n{source}\n"
            ));
        }
        std::fs::write(path, details).unwrap();
    }
    // Every shader file of the crate is part of at least one of the programs
    let files = [
        (
            "core/shared.frag",
            include_str!("../../../core/shared.frag"),
        ),
        (
            "renderer/effect/shaders/deferred_lighting.frag",
            include_str!("../../../renderer/effect/shaders/deferred_lighting.frag"),
        ),
        (
            "renderer/effect/shaders/fog_effect.frag",
            include_str!("../../../renderer/effect/shaders/fog_effect.frag"),
        ),
        (
            "renderer/effect/shaders/fxaa_effect.frag",
            include_str!("../../../renderer/effect/shaders/fxaa_effect.frag"),
        ),
        (
            "renderer/effect/shaders/water_effect.frag",
            include_str!("../../../renderer/effect/shaders/water_effect.frag"),
        ),
        (
            "renderer/geometry/shaders/mesh.vert",
            include_str!("../../../renderer/geometry/shaders/mesh.vert"),
        ),
        (
            "renderer/geometry/shaders/sprites.vert",
            include_str!("../../../renderer/geometry/shaders/sprites.vert"),
        ),
        (
            "renderer/light/shaders/brdf.frag",
            include_str!("../../../renderer/light/shaders/brdf.frag"),
        ),
        (
            "renderer/light/shaders/irradiance.frag",
            include_str!("../../../renderer/light/shaders/irradiance.frag"),
        ),
        (
            "renderer/light/shaders/light_shared.frag",
            include_str!("../../../renderer/light/shaders/light_shared.frag"),
        ),
        (
            "renderer/light/shaders/prefilter.frag",
            include_str!("../../../renderer/light/shaders/prefilter.frag"),
        ),
        (
            "renderer/material/shaders/color_material.frag",
            include_str!("../../../renderer/material/shaders/color_material.frag"),
        ),
        (
            "renderer/material/shaders/deferred_physical_material.frag",
            include_str!("../../../renderer/material/shaders/deferred_physical_material.frag"),
        ),
        (
            "renderer/material/shaders/depth_material.frag",
            include_str!("../../../renderer/material/shaders/depth_material.frag"),
        ),
        (
            "renderer/material/shaders/intersection_material.frag",
            include_str!("../../../renderer/material/shaders/intersection_material.frag"),
        ),
        (
            "renderer/material/shaders/isosurface_material.frag",
            include_str!("../../../renderer/material/shaders/isosurface_material.frag"),
        ),
        (
            "renderer/material/shaders/normal_material.frag",
            include_str!("../../../renderer/material/shaders/normal_material.frag"),
        ),
        (
            "renderer/material/shaders/orm_material.frag",
            include_str!("../../../renderer/material/shaders/orm_material.frag"),
        ),
        (
            "renderer/material/shaders/physical_material.frag",
            include_str!("../../../renderer/material/shaders/physical_material.frag"),
        ),
        (
            "renderer/material/shaders/position_material.frag",
            include_str!("../../../renderer/material/shaders/position_material.frag"),
        ),
        (
            "renderer/material/shaders/skybox_material.frag",
            include_str!("../../../renderer/material/shaders/skybox_material.frag"),
        ),
        (
            "renderer/material/shaders/uv_material.frag",
            include_str!("../../../renderer/material/shaders/uv_material.frag"),
        ),
        (
            "renderer/material/shaders/wireframe_material.frag",
            include_str!("../../../renderer/material/shaders/wireframe_material.frag"),
        ),
        (
            "renderer/object/shaders/imposter.frag",
            include_str!("../../../renderer/object/shaders/imposter.frag"),
        ),
        (
            "renderer/object/shaders/skybox.vert",
            include_str!("../../../renderer/object/shaders/skybox.vert"),
        ),
        (
            "renderer/object/shaders/terrain.vert",
            include_str!("../../../renderer/object/shaders/terrain.vert"),
        ),
        (
            "renderer/object/shaders/water.vert",
            include_str!("../../../renderer/object/shaders/water.vert"),
        ),
        (
            "renderer/object/shaders/wireframe.vert",
            include_str!("../../../renderer/object/shaders/wireframe.vert"),
        ),
    ];
    for (file, content) in files {
        // The files are split at the lines that are replaced when the shader is assembled
        let part = content
            .split("\n")
            .map(|line| line.trim())
            .filter(|line| line.len() > 12 && !line.starts_with("//"))
            .max_by_key(|line| line.len())
            .unwrap();
        assert!(
            programs
                .iter()
                .any(|(vertex, fragment)| vertex.contains(part) || fragment.contains(part)),
            "the shader file {file} is not covered by the test"
        );
    }

    assert!(report.failures.is_empty(), "{text}");
}
