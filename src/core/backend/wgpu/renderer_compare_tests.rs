//!
//! Renders what the examples of this crate render with the [renderer](crate::renderer) module, on the OpenGL backend and on the wgpu backend,
//! and compares the images. The scenes are built in code, so the tests need no files.
//! See [compare_tests](super::compare_tests) for how the backends are compared.
//!

use super::compare_tests::{
    color_texture, compare, compare_with_outliers, depth_texture, floats, pattern, STEP,
};
use crate::renderer::*;
use std::sync::Arc;

const SIZE: u32 = 48;

fn camera() -> Camera {
    Camera::new_perspective(
        Viewport::new_at_origo(SIZE, SIZE),
        vec3(2.0, 2.5, 5.0),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.1,
        100.0,
    )
}

/// Renders into a new color and depth target and returns the colors.
fn render(context: &Context, render: impl FnOnce(&RenderTarget)) -> Vec<f32> {
    let color = color_texture(context, SIZE, SIZE);
    let depth = depth_texture::<f32>(context, SIZE, SIZE);
    let target = RenderTarget::new(color.as_color_target(None), depth.as_depth_target());
    target.clear(ClearState::color_and_depth(0.1, 0.15, 0.2, 1.0, 1.0));
    render(&target);
    floats(target.read_color())
}

fn texture(seed: u32) -> CpuTexture {
    CpuTexture {
        data: TextureData::RgbaU8(pattern(8, 8, seed)),
        width: 8,
        height: 8,
        ..Default::default()
    }
}

/// An environment with a bright sky, a sun and a dark ground.
fn environment() -> CpuTexture {
    let (width, height) = (32u32, 16u32);
    let data = (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            if y < 7 {
                if (x as i32 - 8).abs() < 2 && y > 2 && y < 6 {
                    [8.0, 7.0, 5.0, 1.0]
                } else {
                    [0.3, 0.5, 0.9, 1.0]
                }
            } else {
                [0.2 + 0.02 * (x % 4) as f32, 0.15, 0.1, 1.0]
            }
        })
        .collect();
    CpuTexture {
        data: TextureData::RgbaF32(data),
        width,
        height,
        ..Default::default()
    }
}

fn textured_material() -> CpuMaterial {
    CpuMaterial {
        albedo: Srgba::new(220, 200, 180, 255),
        albedo_texture: Some(texture(1)),
        metallic: 0.6,
        roughness: 0.5,
        metallic_roughness_texture: Some(texture(2)),
        occlusion_texture: Some(texture(3)),
        normal_texture: Some(CpuTexture {
            data: TextureData::RgbaU8(
                pattern(8, 8, 4)
                    .into_iter()
                    .map(|c| [118 + c[0] / 12, 118 + c[1] / 12, 255, 255])
                    .collect(),
            ),
            width: 8,
            height: 8,
            ..Default::default()
        }),
        emissive: Srgba::new(40, 10, 0, 255),
        emissive_texture: Some(texture(5)),
        ..Default::default()
    }
}

/// Adds texture coordinates to a mesh that has none.
fn with_uvs(mut mesh: CpuMesh) -> CpuMesh {
    if mesh.uvs.is_none() {
        mesh.uvs = Some(
            mesh.positions
                .to_f32()
                .iter()
                .map(|p| vec2(0.5 * p.x + 0.5 * p.z + 0.5, 0.5 * p.y + 0.5))
                .collect(),
        );
    }
    mesh
}

fn shapes(context: &Context) -> Vec<Mesh> {
    let mut sphere = with_uvs(CpuMesh::sphere(12));
    sphere.compute_tangents();
    let mut cube = CpuMesh::cube();
    cube.compute_tangents();
    cube.colors = Some(
        (0..cube.positions.len())
            .map(|i| Srgba::new((i * 40 % 256) as u8, (i * 90 % 256) as u8, 200, 255))
            .collect(),
    );
    let mut cylinder = with_uvs(CpuMesh::cylinder(10));
    cylinder.compute_tangents();
    let mut meshes = vec![
        Mesh::new(context, &sphere),
        Mesh::new(context, &cube),
        Mesh::new(context, &cylinder),
    ];
    meshes[0].set_transformation(Mat4::from_translation(vec3(-1.6, 0.0, 0.0)));
    meshes[1].set_transformation(
        Mat4::from_translation(vec3(0.6, 0.0, 0.0))
            * Mat4::from_angle_y(degrees(30.0))
            * Mat4::from_scale(0.7),
    );
    meshes[2].set_transformation(
        Mat4::from_translation(vec3(1.2, 1.0, -1.0))
            * Mat4::from_angle_z(degrees(60.0))
            * Mat4::from_nonuniform_scale(1.5, 0.4, 0.4),
    );
    // As in the animation example
    meshes[2].set_animation(|time| Mat4::from_angle_x(radians(time)));
    meshes[2].animate(0.7);
    meshes
}

fn lights(context: &Context) -> (AmbientLight, DirectionalLight, PointLight, SpotLight) {
    (
        AmbientLight::new(context, 0.3, Srgba::WHITE),
        DirectionalLight::new(context, 2.0, Srgba::WHITE, vec3(-1.0, -2.0, -0.5)),
        PointLight::new(
            context,
            1.5,
            Srgba::new(150, 255, 150, 255),
            vec3(0.0, 1.5, 2.5),
            Attenuation::default(),
        ),
        SpotLight::new(
            context,
            3.0,
            Srgba::new(255, 200, 120, 255),
            vec3(2.0, 3.0, 1.0),
            vec3(-2.0, -3.0, -1.0),
            degrees(35.0),
            Attenuation::default(),
        ),
    )
}

/// Examples shapes, normals, texture, animation: the materials without lighting.
#[test]
fn materials_without_lighting() {
    // The software renderers choose the mip level of a few texels differently
    compare_with_outliers(2.0 * STEP, 0.001, |context| {
        let shapes = shapes(context);
        let camera = camera();
        let cpu_material = textured_material();
        let materials: Vec<Box<dyn Material>> = vec![
            Box::new(ColorMaterial::default()),
            Box::new(ColorMaterial::new_opaque(context, &cpu_material)),
            Box::new(ColorMaterial::new_transparent(
                context,
                &CpuMaterial {
                    albedo: Srgba::new(255, 255, 255, 120),
                    ..cpu_material.clone()
                },
            )),
            Box::new(DepthMaterial::default()),
            Box::new(NormalMaterial::default()),
            Box::new(NormalMaterial::new(context, &cpu_material)),
            Box::new(ORMMaterial::default()),
            Box::new(ORMMaterial::new(context, &cpu_material)),
            Box::new(PositionMaterial::default()),
            Box::new(UVMaterial::default()),
        ];
        let mut values = Vec::new();
        for material in &materials {
            values.extend(render(context, |target| {
                target.render_with_material(&**material, &camera, &shapes, &[]);
            }));
        }
        values
    });
}

/// Examples pbr, lighting, lights, statues: the physical material with every lighting model and kind of light.
#[test]
fn physical_material_and_lights() {
    compare(4.0 * STEP, |context| {
        let shapes = shapes(context);
        let camera = camera();
        let (ambient, directional, point, spot) = lights(context);
        let mut values = Vec::new();
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
            let mut material = PhysicalMaterial::new_opaque(context, &textured_material());
            material.lighting_model = lighting_model;
            values.extend(render(context, |target| {
                target.render_with_material(
                    &material,
                    &camera,
                    &shapes,
                    &[&ambient, &directional, &point, &spot],
                );
            }));
        }
        // Each light alone, with a material without textures, and a transparent material
        let plain = PhysicalMaterial::new_opaque(
            context,
            &CpuMaterial {
                albedo: Srgba::new(200, 80, 60, 255),
                roughness: 0.4,
                metallic: 0.2,
                ..Default::default()
            },
        );
        let all: [&dyn Light; 4] = [&ambient, &directional, &point, &spot];
        for light in all {
            values.extend(render(context, |target| {
                target.render_with_material(&plain, &camera, &shapes, &[light]);
            }));
        }
        let transparent = PhysicalMaterial::new_transparent(
            context,
            &CpuMaterial {
                albedo: Srgba::new(60, 80, 220, 130),
                ..Default::default()
            },
        );
        values.extend(render(context, |target| {
            target.render_with_material(&plain, &camera, &shapes[1..], &all);
            target.render_with_material(&transparent, &camera, &shapes[..1], &all);
        }));
        values
    });
}

/// Examples environment, pbr: a sky box and lighting from an environment map.
#[test]
fn environment_lighting_and_skybox() {
    compare(6.0 * STEP, |context| {
        let shapes = shapes(context);
        let camera = camera();
        let skybox = Skybox::new_from_equirectangular(context, &environment());
        let ambient =
            AmbientLight::new_with_environment(context, 1.0, Srgba::WHITE, skybox.texture());
        let material = PhysicalMaterial::new_opaque(
            context,
            &CpuMaterial {
                albedo: Srgba::new(230, 230, 230, 255),
                metallic: 0.9,
                roughness: 0.3,
                ..Default::default()
            },
        );
        let mut values = render(context, |target| {
            target.render(&camera, &skybox, &[]);
            target.render_with_material(&material, &camera, &shapes, &[&ambient]);
        });
        // A sky box from six images
        let sides: Vec<CpuTexture> = (10..16).map(texture).collect();
        let skybox = Skybox::new(
            context, &sides[0], &sides[1], &sides[2], &sides[3], &sides[4], &sides[5],
        );
        values.extend(render(context, |target| {
            target.render(&camera, &skybox, &[]);
        }));
        values
    });
}

/// Example lighting: the deferred pipeline.
#[test]
fn deferred_rendering() {
    compare(4.0 * STEP, |context| {
        let camera = camera();
        let (ambient, mut directional, point, mut spot) = lights(context);
        let objects: Vec<_> = shapes(context)
            .into_iter()
            .map(|mesh| {
                Gm::new(
                    mesh,
                    DeferredPhysicalMaterial::new(context, &textured_material()),
                )
            })
            .collect();
        let mut ground = Gm::new(
            Mesh::new(context, &CpuMesh::square()),
            DeferredPhysicalMaterial::new(
                context,
                &CpuMaterial {
                    albedo: Srgba::new(160, 160, 160, 255),
                    ..Default::default()
                },
            ),
        );
        ground.set_transformation(
            Mat4::from_translation(vec3(0.0, -1.0, 0.0))
                * Mat4::from_angle_x(degrees(-90.0))
                * Mat4::from_scale(4.0),
        );
        directional
            .generate_shadow_map(128, objects.iter().map(|o| &o.geometry))
            .unwrap();
        spot.generate_shadow_map(128, objects.iter().map(|o| &o.geometry))
            .unwrap();
        render(context, |target| {
            target.render(
                &camera,
                objects.iter().chain(std::iter::once(&ground)),
                &[&ambient, &directional, &point, &spot],
            );
        })
    });
}

/// Examples effect, fog, image, screen: effects on a rendered image, tone mapping and color mapping.
#[test]
fn effects_and_mappings() {
    // The anti-aliasing effect decides per pixel whether it is on an edge, which a difference of one step can change
    compare_with_outliers(3.0 * STEP, 0.001, |context| {
        let shapes = shapes(context);
        let mut camera = camera();
        let (ambient, directional, _, _) = lights(context);
        let material = PhysicalMaterial::new_opaque(context, &textured_material());
        // The scene is rendered into a texture first
        let color = Texture2D::new_empty::<[f16; 4]>(
            context,
            SIZE,
            SIZE,
            Interpolation::Linear,
            Interpolation::Linear,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        let depth = depth_texture::<f32>(context, SIZE, SIZE);
        RenderTarget::new(color.as_color_target(None), depth.as_depth_target())
            .clear(ClearState::color_and_depth(0.5, 0.6, 2.0, 1.0, 1.0))
            .render_with_material(&material, &camera, &shapes, &[&ambient, &directional]);
        let color_texture = ColorTexture::Single(&color);
        let depth_texture = DepthTexture::Single(&depth);

        let mut values = Vec::new();
        let fog = FogEffect {
            color: Srgba::new(200, 200, 220, 255),
            density: 0.25,
            animation: 0.1,
            time: 3.0,
        };
        values.extend(render(context, |target| {
            target.apply_screen_effect(
                &fog,
                &camera,
                &[],
                Some(color_texture),
                Some(depth_texture),
            );
        }));
        values.extend(render(context, |target| {
            target.apply_screen_effect(&FxaaEffect {}, &camera, &[], Some(color_texture), None);
        }));
        values.extend(render(context, |target| {
            target.apply_screen_effect(
                &CopyEffect::default(),
                &camera,
                &[],
                Some(color_texture),
                Some(depth_texture),
            );
        }));
        for tone_mapping in [
            ToneMapping::None,
            ToneMapping::Reinhard,
            ToneMapping::Aces,
            ToneMapping::Filmic,
        ] {
            for color_mapping in [ColorMapping::None, ColorMapping::ComputeToSrgb] {
                camera.tone_mapping = tone_mapping;
                camera.color_mapping = color_mapping;
                values.extend(render(context, |target| {
                    target.apply_screen_effect(
                        &ScreenEffect::default(),
                        &camera,
                        &[],
                        Some(color_texture),
                        None,
                    );
                }));
            }
        }
        values
    });
}

/// Examples instanced_shapes, instanced_draw_order, point_cloud, sprites, particles, wireframe: the other geometries and objects.
#[test]
fn instances_sprites_particles_and_helpers() {
    compare(3.0 * STEP, |context| {
        let camera = camera();
        let (ambient, directional, _, _) = lights(context);
        let mut cube = CpuMesh::cube();
        cube.transform(Mat4::from_scale(0.3)).unwrap();
        let transformations: Vec<Mat4> = (0..9)
            .map(|i| {
                Mat4::from_translation(vec3(
                    (i % 3) as f32 - 1.0,
                    (i / 3) as f32 - 1.0,
                    -0.3 * i as f32,
                )) * Mat4::from_angle_y(degrees(20.0 * i as f32))
            })
            .collect();
        let instances = Instances {
            transformations: transformations.clone(),
            texture_transformations: Some(
                (0..9)
                    .map(|i| Mat3::from_scale(0.5 + 0.1 * i as f32))
                    .collect(),
            ),
            colors: Some(
                (0..9)
                    .map(|i| Srgba::new(255, (i * 28) as u8, 255 - (i * 28) as u8, 150))
                    .collect(),
            ),
        };
        let opaque = Gm::new(
            InstancedMesh::new(
                context,
                &Instances {
                    transformations,
                    ..Default::default()
                },
                &cube,
            ),
            PhysicalMaterial::new_opaque(context, &textured_material()),
        );
        // Transparent instances are drawn back to front
        let transparent = Gm::new(
            InstancedMesh::new(context, &instances, &with_uvs(CpuMesh::sphere(8))),
            PhysicalMaterial::new_transparent(
                context,
                &CpuMaterial {
                    albedo: Srgba::new(255, 255, 255, 200),
                    albedo_texture: Some(texture(20)),
                    ..Default::default()
                },
            ),
        );
        let mut values = render(context, |target| {
            target.render(&camera, &opaque, &[&ambient, &directional]);
        });
        values.extend(render(context, |target| {
            target.render(
                &camera,
                opaque.into_iter().chain(&transparent),
                &[&ambient, &directional],
            );
        }));

        let sprite_material = ColorMaterial {
            color: Srgba::WHITE,
            texture: Some(Texture2DRef::from_texture(Texture2D::new(
                context,
                &texture(21),
            ))),
            ..Default::default()
        };
        let centers = [
            vec3(-1.0, 0.0, 0.0),
            vec3(1.0, 0.5, -1.0),
            vec3(0.0, -1.0, 1.0),
        ];
        let mut billboards = Sprites::new(context, &centers, None);
        billboards.set_transformation(Mat4::from_scale(0.6));
        let mut directed = Sprites::new(context, &centers, Some(vec3(0.0, 1.0, 0.3)));
        directed.set_transformation(Mat4::from_translation(vec3(0.0, 0.5, 0.0)));
        values.extend(render(context, |target| {
            target.render_with_material(&sprite_material, &camera, [&billboards, &directed], &[]);
        }));

        let mut particles = ParticleSystem::new(
            context,
            &Particles {
                start_positions: (0..12)
                    .map(|i| vec3(0.2 * i as f32 - 1.2, 0.0, 0.0))
                    .collect(),
                // Different depths, so that no faces of the cubes lie in the same plane
                start_velocities: (0..12)
                    .map(|i| vec3(0.0, 0.3 * i as f32, 0.5 - 0.37 * i as f32))
                    .collect(),
                texture_transforms: None,
                colors: Some(
                    (0..12)
                        .map(|i| Srgba::new(255, (i * 20) as u8, 50, 255))
                        .collect(),
                ),
            },
            vec3(0.0, -1.5, 0.0),
            &cube,
        );
        particles.animate(0.8);
        let bounding_box = BoundingBox::new_with_thickness(
            context,
            AxisAlignedBoundingBox::new_with_positions(&[
                vec3(-1.5, -1.0, -1.0),
                vec3(1.5, 1.5, 1.0),
            ]),
            0.05,
        );
        let axes = Axes::new(context, 0.08, 2.0);
        let wireframe =
            Wireframe::new_from_cpu_mesh(context, &CpuMesh::sphere(4), 0.03, Srgba::BLACK);
        values.extend(render(context, |target| {
            target.render_with_material(
                &ColorMaterial::default(),
                &camera,
                [&particles as &dyn Geometry, &bounding_box],
                &[],
            );
            target.render(
                &camera,
                axes.into_iter().chain(&wireframe),
                &[&ambient, &directional],
            );
        }));
        values
    });
}

/// Example shapes2d: rectangles, circles and lines in pixel coordinates.
#[test]
fn shapes_2d() {
    compare(STEP, |context| {
        let camera = Camera::new_2d(Viewport::new_at_origo(SIZE, SIZE));
        let material = |color: Srgba| ColorMaterial {
            color,
            ..Default::default()
        };
        let rectangle = Gm::new(
            Rectangle::new(context, vec2(14.0, 30.0), degrees(25.0), 18.0, 9.0),
            material(Srgba::RED),
        );
        let circle = Gm::new(
            Circle::new(context, vec2(32.0, 16.0), 9.0),
            material(Srgba::BLUE),
        );
        let line = Gm::new(
            Line::new(context, vec2(3.0, 5.0), vec2(44.0, 40.0), 2.5),
            material(Srgba::GREEN),
        );
        render(context, |target| {
            target.render(
                &camera,
                rectangle.into_iter().chain(&circle).chain(&line),
                &[],
            );
        })
    });
}

/// Example terrain: terrain, water and the water effect which uses the rendered scene.
#[test]
fn terrain_and_water() {
    compare(6.0 * STEP, |context| {
        let camera = Camera::new_perspective(
            Viewport::new_at_origo(SIZE, SIZE),
            vec3(0.0, 6.0, 12.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
            degrees(50.0),
            0.1,
            200.0,
        );
        let (ambient, directional, _, _) = lights(context);
        let skybox = Skybox::new_from_equirectangular(context, &environment());
        let terrain = Terrain::new(
            context,
            PhysicalMaterial::new_opaque(
                context,
                &CpuMaterial {
                    albedo: Srgba::new(120, 160, 90, 255),
                    albedo_texture: Some(texture(30)),
                    ..Default::default()
                },
            ),
            Arc::new(|x, y| 1.5 * (0.4 * x).sin() * (0.3 * y).cos() - 0.3),
            32.0,
            1.0,
            vec2(0.0, 0.0),
        );
        let mut water = Water::new(
            context,
            ColorMaterial::default(),
            0.0,
            vec2(0.0, 0.0),
            32.0,
            1.0,
            [WaveParameters {
                wavelength: 4.0,
                amplitude: 0.1,
                steepness: 0.5,
                speed: 1.0,
                direction: vec2(1.0, 0.3),
            }],
        );
        water.animate(1.3);
        // The scene without water, which the water reflects and refracts
        let color = color_texture(context, SIZE, SIZE);
        let depth = depth_texture::<f32>(context, SIZE, SIZE);
        RenderTarget::new(color.as_color_target(None), depth.as_depth_target())
            .clear(ClearState::color_and_depth(0.0, 0.0, 0.0, 1.0, 1.0))
            .render(
                &camera,
                skybox.into_iter().chain(&terrain),
                &[&ambient, &directional],
            );
        let mut values = floats(color.as_color_target(None).read());
        values.extend(render(context, |target| {
            target
                .render(
                    &camera,
                    skybox.into_iter().chain(&terrain),
                    &[&ambient, &directional],
                )
                .render_with_effect(
                    &WaterEffect {
                        background: Background::Texture(skybox.texture().clone()),
                        metallic: 0.0,
                        roughness: 1.0,
                        lighting_model: LightingModel::Blinn,
                    },
                    &camera,
                    &water,
                    &[&ambient, &directional],
                    Some(ColorTexture::Single(&color)),
                    Some(DepthTexture::Single(&depth)),
                );
        }));
        values
    });
}

/// Example imposters: objects rendered into a texture array from several angles and drawn as sprites.
#[test]
fn imposters() {
    compare(6.0 * STEP, |context| {
        let camera = camera();
        let (ambient, directional, _, _) = lights(context);
        let model = Gm::new(
            Mesh::new(context, &CpuMesh::cone(8)),
            PhysicalMaterial::new_opaque(
                context,
                &CpuMaterial {
                    albedo: Srgba::new(60, 160, 60, 255),
                    ..Default::default()
                },
            ),
        );
        let imposters = Imposters::new(
            context,
            &[
                vec3(-1.5, 0.0, 0.0),
                vec3(1.0, 0.0, -1.0),
                vec3(0.0, 0.0, 1.5),
            ],
            [&model],
            &[&ambient, &directional],
            64,
        );
        render(context, |target| {
            target.render(&camera, &imposters, &[&ambient, &directional]);
        })
    });
}

/// Example volume: an isosurface in a 3D texture.
#[test]
fn voxel_grid() {
    compare(6.0 * STEP, |context| {
        let camera = camera();
        let (ambient, directional, _, _) = lights(context);
        let n = 16u32;
        let data: Vec<u8> = (0..n * n * n)
            .map(|i| {
                let p = vec3((i % n) as f32, ((i / n) % n) as f32, (i / (n * n)) as f32)
                    / (n - 1) as f32
                    - vec3(0.5, 0.5, 0.5);
                (255.0 * (1.0 - 2.0 * p.magnitude()).clamp(0.0, 1.0)) as u8
            })
            .collect();
        let mut voxel_grid = VoxelGrid::<IsosurfaceMaterial>::new(
            context,
            &CpuVoxelGrid {
                voxels: CpuTexture3D {
                    data: TextureData::RU8(data),
                    width: n,
                    height: n,
                    depth: n,
                    ..Default::default()
                },
                size: vec3(3.0, 3.0, 3.0),
                ..Default::default()
            },
        );
        voxel_grid.material.threshold = 0.4;
        voxel_grid.material.color = Srgba::new(220, 120, 60, 255);
        render(context, |target| {
            target.render(&camera, &voxel_grid, &[&ambient, &directional]);
        })
    });
}

/// Example picking: the position under a pixel.
#[test]
fn picking() {
    compare(1.0e-3, |context| {
        let shapes = shapes(context);
        let camera = camera();
        let mut values = Vec::new();
        for pixel in [
            (24.0, 24.0),
            (10.0, 22.0),
            (30.0, 26.0),
            (36.0, 34.0),
            (2.0, 2.0),
        ] {
            match pick(context, &camera, pixel, &shapes, Cull::Back).unwrap() {
                Some(result) => values.extend([
                    1.0,
                    result.position.x,
                    result.position.y,
                    result.position.z,
                    result.geometry_id as f32,
                    result.instance_id as f32,
                ]),
                None => values.extend([0.0; 6]),
            }
        }
        values
    });
}

/// Example multisample: the renderer on a multisample target.
#[test]
fn multisample_rendering() {
    compare(4.0 * STEP, |context| {
        let shapes = shapes(context);
        let camera = camera();
        let (ambient, directional, _, _) = lights(context);
        let material = PhysicalMaterial::new_opaque(context, &textured_material());
        let target = RenderTargetMultisample::<[u8; 4], f32>::new(context, SIZE, SIZE, 4);
        target
            .clear(ClearState::color_and_depth(0.1, 0.15, 0.2, 1.0, 1.0))
            .write::<RendererError>(|| {
                for shape in &shapes {
                    shape.render_with_material(&material, &camera, &[&ambient, &directional]);
                }
                Ok(())
            })
            .unwrap();
        floats(target.resolve_color().as_color_target(None).read())
    });
}

struct MandelbrotMaterial;

impl Material for MandelbrotMaterial {
    fn fragment_shader_source(&self, _lights: &[&dyn Light]) -> String {
        include_str!("../../../../examples/mandelbrot/src/mandelbrot.frag").to_string()
    }

    fn use_uniforms(&self, _program: &Program, _viewer: &dyn Viewer, _lights: &[&dyn Light]) {}

    fn render_states(&self) -> RenderStates {
        RenderStates {
            depth_test: DepthTest::Always,
            write_mask: WriteMask::COLOR,
            cull: Cull::Back,
            ..Default::default()
        }
    }

    fn material_type(&self) -> MaterialType {
        MaterialType::Opaque
    }

    fn id(&self) -> EffectMaterialId {
        EffectMaterialId(0b11u16)
    }
}

struct LogoMaterial<'a> {
    image: &'a Texture2D,
}

impl Material for LogoMaterial<'_> {
    fn fragment_shader_source(&self, _lights: &[&dyn Light]) -> String {
        include_str!("../../../../examples/logo/src/shader.frag").to_string()
    }

    fn use_uniforms(&self, program: &Program, _viewer: &dyn Viewer, _lights: &[&dyn Light]) {
        program.use_texture("image", self.image);
    }

    fn render_states(&self) -> RenderStates {
        RenderStates {
            write_mask: WriteMask::COLOR,
            blend: Blend::TRANSPARENCY,
            ..Default::default()
        }
    }

    fn material_type(&self) -> MaterialType {
        MaterialType::Transparent
    }

    fn id(&self) -> EffectMaterialId {
        EffectMaterialId(0b1u16)
    }
}

/// Examples mandelbrot, logo: materials with their own fragment shader, on a mesh and on the whole screen.
#[test]
fn custom_materials() {
    // The colors of the fractal change quickly from pixel to pixel, so a few pixels on its border differ
    compare_with_outliers(2.0 * STEP, 0.002, |context| {
        let camera = Camera::new_orthographic(
            Viewport::new_at_origo(SIZE, SIZE),
            vec3(-0.6, 0.1, 1.0),
            vec3(-0.6, 0.1, 0.0),
            vec3(0.0, 1.0, 0.0),
            2.5,
            0.0,
            10.0,
        );
        let mut mesh = Gm::new(Mesh::new(context, &CpuMesh::square()), MandelbrotMaterial);
        mesh.set_transformation(Mat4::from_scale(10.0));
        let mut values = render(context, |target| {
            target.render(&camera, &mesh, &[]);
        });
        let image = Texture2D::new(context, &texture(40));
        values.extend(render(context, |target| {
            target.apply_screen_material(&LogoMaterial { image: &image }, &camera, &[]);
        }));
        values
    });
}

/// Examples screen, statues, lights: several views in parts of the target, with viewports that do not start at the origin.
#[test]
fn viewports_and_partial_rendering() {
    compare(4.0 * STEP, |context| {
        let shapes = shapes(context);
        let mut camera = camera();
        let (ambient, directional, _, _) = lights(context);
        let material = PhysicalMaterial::new_opaque(context, &textured_material());
        render(context, |target| {
            // The whole scene on the left half
            let left = Viewport {
                x: 0,
                y: 0,
                width: SIZE / 2,
                height: SIZE,
            };
            camera.set_viewport(left);
            target.render_partially(left.into(), &camera, &[] as &[Gm<Mesh, ColorMaterial>], &[]);
            target
                .write_partially::<RendererError>(left.into(), || {
                    for shape in &shapes {
                        shape.render_with_material(&material, &camera, &[&ambient, &directional]);
                    }
                    Ok(())
                })
                .unwrap();
            // Zoomed in on the top right: the viewport is larger than the target and cut by the scissor box
            let zoomed = Viewport {
                x: SIZE as i32 / 2 - 20,
                y: SIZE as i32 / 2 - 10,
                width: SIZE * 2,
                height: SIZE * 2,
            };
            camera.set_viewport(zoomed);
            let top_right = ScissorBox {
                x: SIZE as i32 / 2,
                y: SIZE as i32 / 2,
                width: SIZE / 2,
                height: SIZE / 2,
            };
            target
                .clear_partially(
                    top_right,
                    ClearState::color_and_depth(0.5, 0.5, 0.5, 1.0, 1.0),
                )
                .render_partially(
                    top_right,
                    &camera,
                    shapes.iter().map(|shape| Gm::new(shape, &material)),
                    &[&ambient, &directional],
                );
            // Small on the bottom right
            let small = Viewport {
                x: SIZE as i32 / 2 + 4,
                y: 2,
                width: SIZE / 2 - 8,
                height: SIZE / 2 - 4,
            };
            camera.set_viewport(small);
            target.render_partially(
                small.into(),
                &camera,
                shapes
                    .iter()
                    .map(|shape| Gm::new(shape, NormalMaterial::default())),
                &[],
            );
        })
    });
}

/// Example text: meshes generated from a font.
#[cfg(feature = "text")]
#[test]
fn text() {
    compare(STEP, |context| {
        let camera = Camera::new_2d(Viewport::new_at_origo(SIZE, SIZE));
        let generator = TextGenerator::new(
            include_bytes!("../../../../examples/text/src/font0.ttf"),
            0,
            16.0,
        )
        .unwrap();
        let mut text = Gm::new(
            Mesh::new(
                context,
                &generator.generate("Hi!\nwgpu", TextLayoutOptions::default()),
            ),
            ColorMaterial {
                color: Srgba::RED,
                ..Default::default()
            },
        );
        text.set_transformation(Mat4::from_translation(vec3(4.0, 30.0, 0.0)));
        render(context, |target| {
            target.render(&camera, &text, &[]);
        })
    });
}

/// Draws a small GUI into a texture.
#[cfg(feature = "egui-gui")]
fn gui_scene(context: &Context, mut gui: crate::gui::GUI) -> Vec<f32> {
    let viewport = Viewport::new_at_origo(160, 120);
    let color = color_texture(context, viewport.width, viewport.height);
    let target = color.as_color_target(None);
    let mut checked = true;
    let mut value = 0.3;
    // The second frame knows the sizes of the widgets
    for _ in 0..2 {
        gui.update(&mut [], 0.0, viewport, 1.0, |ui| {
            use crate::gui::egui;
            egui::Panel::left("panel").show_inside(ui, |ui| {
                ui.heading("three-d");
                ui.label("Rendered with a backend");
                ui.checkbox(&mut checked, "Checked");
                ui.add(egui::Slider::new(&mut value, 0.0..=1.0));
                let _ = ui.button("Button");
            });
        });
        target
            .clear(ClearState::color(0.2, 0.3, 0.4, 1.0))
            .write::<CoreError>(|| gui.render())
            .unwrap();
    }
    floats(target.read())
}

/// The GUI drawn with the core module, which is what the wgpu backend uses.
#[cfg(feature = "egui-gui")]
#[test]
fn gui() {
    compare(STEP, |context| {
        gui_scene(context, crate::gui::GUI::new_with_core_painter(context))
    });
}

/// On OpenGL, the GUI drawn with the core module looks the same as the GUI drawn by `egui_glow`.
#[cfg(feature = "egui-gui")]
#[test]
fn gui_core_painter_matches_egui_glow() {
    super::one_gpu_test_at_a_time();
    let Ok((context, _keep_alive)) = super::compare_tests::gl_context() else {
        if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() {
            panic!("no OpenGL context");
        }
        return;
    };
    let expected = gui_scene(&context, crate::gui::GUI::new(&context));
    let actual = gui_scene(&context, crate::gui::GUI::new_with_core_painter(&context));
    assert!(expected.iter().any(|v| *v != expected[0]));
    let different = expected
        .iter()
        .zip(&actual)
        .filter(|(e, a)| (*e - *a).abs() > 2.0 * STEP)
        .count();
    assert_eq!(different, 0, "of {} values", expected.len());
}

/// Reading and picking with the asynchronous functions gives the same as with the blocking ones.
#[test]
fn asynchronous_reading_and_picking() {
    compare(1.0e-3, |context| {
        let shapes = shapes(context);
        let camera = camera();
        let color = color_texture(context, SIZE, SIZE);
        let depth = depth_texture::<f32>(context, SIZE, SIZE);
        let target = RenderTarget::new(color.as_color_target(None), depth.as_depth_target());
        target
            .clear(ClearState::color_and_depth(0.1, 0.15, 0.2, 1.0, 1.0))
            .render_with_material(&NormalMaterial::default(), &camera, &shapes, &[]);
        let expected = target.read_color::<[u8; 4]>();
        let pixels = target.read_color_async::<[u8; 4]>();
        let part = ScissorBox {
            x: 10,
            y: 20,
            width: 7,
            height: 5,
        };
        let expected_part = target.read_color_partially::<[u8; 4]>(part);
        let pixels_part = color
            .as_color_target(None)
            .read_partially_async::<[u8; 4]>(part);
        let picks: Vec<_> = [(24.0, 24.0), (10.0, 22.0), (2.0, 2.0)]
            .into_iter()
            .map(|pixel| {
                (
                    pick(context, &camera, pixel, &shapes, Cull::Back).unwrap(),
                    pick_async(context, &camera, pixel, &shapes, Cull::Back),
                )
            })
            .collect();
        // The futures hold what the target contained when they were created
        target.clear(ClearState::color_and_depth(1.0, 0.0, 1.0, 1.0, 1.0));
        drop(shapes);
        let pixels = pollster::block_on(pixels);
        assert_eq!(pixels, expected);
        assert_eq!(pollster::block_on(pixels_part), expected_part);
        let mut values = floats(pixels);
        for (expected, result) in picks {
            let result = pollster::block_on(result).unwrap();
            assert_eq!(result.is_some(), expected.is_some());
            if let (Some(result), Some(expected)) = (result, expected) {
                assert_eq!(result.position, expected.position);
                assert_eq!(result.geometry_id, expected.geometry_id);
                assert_eq!(result.instance_id, expected.instance_id);
                values.extend([result.position.x, result.position.y, result.position.z]);
            }
        }
        values
    });
}
