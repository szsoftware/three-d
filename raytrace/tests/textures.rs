//!
//! Textured materials, with the CPU reference tracer and, with the `wgpu` feature, the compute tracer. The scenes are
//! built so that every path gives the exact value, which both tracers have to hit.
//!

use std::f32::consts::PI;
use three_d_raytrace::*;

/// The tracers to check: the CPU tracer and, where there is a wgpu adapter with compute shaders, the compute tracer.
fn tracers() -> Vec<(&'static str, Box<dyn Tracer>)> {
    #[allow(unused_mut)]
    let mut tracers: Vec<(&'static str, Box<dyn Tracer>)> =
        vec![("cpu", Box::new(CpuReferenceTracer::new()))];
    #[cfg(feature = "wgpu")]
    match three_d::Context::new_headless_wgpu() {
        Ok(context) => match ComputeTracer::from_context(&context) {
            Some(tracer) => tracers.push(("compute", Box::new(tracer))),
            None if std::env::var("WGPU_BACKEND").as_deref() == Ok("gl") => {}
            None => panic!("no compute shaders: {:?}", RaytraceSupport::of(&context)),
        },
        Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
            panic!("no wgpu adapter: {error}")
        }
        Err(error) => eprintln!("compute tracer skipped, no wgpu adapter: {error}"),
    }
    tracers
}

/// The luminance seen by a one pixel camera at `from` looking straight down.
fn look_down(tracer: &mut dyn Tracer, scene: &RtScene, from: Vec3) -> Vec3 {
    let camera = Camera::new_perspective(
        Viewport::new_at_origo(1, 1),
        from,
        from - vec3(0.0, 1.0, 0.0),
        vec3(0.0, 0.0, -1.0),
        degrees(0.5),
        0.01,
        100.0,
    );
    let mut image = RenderImage::new(1, 1);
    tracer
        .trace(
            scene,
            &camera,
            &mut image,
            &TraceSettings {
                samples_per_call: 4,
                max_bounces: 1,
                ..Default::default()
            },
        )
        .unwrap();
    image.pixel(0, 0)
}

fn close(got: Vec3, expected: Vec3) -> bool {
    (0..3).all(|c| (got[c] - expected[c]).abs() <= 0.01 * expected[c].abs().max(1.0))
}

/// A square of 2 × 2 at height y facing up. Texture coordinates: u = (x + 1) / 2, v = (z + 1) / 2.
fn square_at(scene: &mut RtScene, y: f32, material: MaterialId, tangents: bool) {
    let mut mesh = CpuMesh::square();
    if !tangents {
        mesh.tangents = None;
    }
    let mesh = scene.add_mesh(&mesh).unwrap();
    scene
        .add_instance(
            mesh,
            material,
            Mat4::from_translation(vec3(0.0, y, 0.0)) * Mat4::from_angle_x(degrees(-90.0)),
        )
        .unwrap();
}

fn nearest(width: u32, height: u32, texels: Vec<[u8; 4]>) -> RtTexture {
    RtTexture {
        nearest: true,
        ..RtTexture::new(width, height, texels)
    }
}

#[test]
fn base_color_texture_colors_the_surface() {
    // A white Lambert surface under a uniform sky of 1000 cd/m² reflects 1000 cd/m² times its color
    let mut scene = RtScene::new();
    scene.set_environment(RtEnvironment::Uniform {
        color: vec3(1.0, 1.0, 1.0),
        luminance: 1000.0,
    });
    let texture = scene.add_texture(nearest(
        2,
        2,
        vec![
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 255],
        ],
    ));
    let material = scene.add_material(RtMaterial {
        base_color_texture: Some(texture),
        ..RtMaterial::diffuse(vec3(1.0, 1.0, 1.0))
    });
    square_at(&mut scene, 0.0, material, true);
    for (name, mut tracer) in tracers() {
        for (x, z, expected) in [
            (-0.5, -0.5, vec3(1000.0, 0.0, 0.0)),
            (0.5, -0.5, vec3(0.0, 1000.0, 0.0)),
            (-0.5, 0.5, vec3(0.0, 0.0, 1000.0)),
            (0.5, 0.5, vec3(1000.0, 1000.0, 1000.0)),
        ] {
            let got = look_down(tracer.as_mut(), &scene, vec3(x, 0.5, z));
            assert!(close(got, expected), "{name} at {x} {z}: {got:?}");
        }
    }
}

#[test]
fn leaves_are_cut_out_by_the_texture() {
    // A leaf above the ground, its left half transparent: the ground is lit there and in shadow under the right half
    let mut scene = RtScene::new();
    let grey = scene.add_material(RtMaterial::diffuse(vec3(0.5, 0.5, 0.5)));
    let ground = scene.add_mesh(&CpuMesh::square()).unwrap();
    scene
        .add_instance(
            ground,
            grey,
            Mat4::from_angle_x(degrees(-90.0)) * Mat4::from_scale(100.0),
        )
        .unwrap();
    let texture = scene.add_texture(nearest(2, 1, vec![[40, 120, 30, 0], [40, 120, 30, 255]]));
    let leaf = scene.add_material(RtMaterial {
        base_color_texture: Some(texture),
        alpha_cutout: Some(0.5),
        ..RtMaterial::diffuse(vec3(1.0, 1.0, 1.0))
    });
    square_at(&mut scene, 1.0, leaf, true);
    scene.add_light(RtLight::Directional {
        direction: vec3(0.0, -1.0, 0.0),
        color: vec3(1.0, 1.0, 1.0),
        illuminance: 1000.0,
    });
    let lit = 1000.0 * 0.5 / PI;
    for (name, mut tracer) in tracers() {
        // From above: through the cut out half onto the lit ground
        let got = look_down(tracer.as_mut(), &scene, vec3(-0.5, 2.0, 0.0));
        assert!(close(got, vec3(lit, lit, lit)), "{name} through: {got:?}");
        // From below the leaf: shadow under the right half
        let got = look_down(tracer.as_mut(), &scene, vec3(0.5, 0.5, 0.0));
        assert_eq!(got, vec3(0.0, 0.0, 0.0), "{name} shadow");
        let got = look_down(tracer.as_mut(), &scene, vec3(-0.5, 0.5, 0.0));
        assert!(close(got, vec3(lit, lit, lit)), "{name} no shadow: {got:?}");
    }
}

#[test]
fn normal_texture_tilts_the_shading() {
    // Normals tilted by 60° towards the tangent (+x, red) or the bitangent (-z, green), lit from that side by a light
    // 60° off the vertical: the surface faces the light and reflects E·ρ/π, lit from the other side it is dark
    let tilt = 60f32.to_radians();
    let encode = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
    let towards_tangent = [encode(tilt.sin()), encode(0.0), encode(tilt.cos()), 255];
    let towards_bitangent = [encode(0.0), encode(tilt.sin()), encode(tilt.cos()), 255];
    let light_from = |d: Vec3| RtLight::Directional {
        direction: -d.normalize(),
        color: vec3(1.0, 1.0, 1.0),
        illuminance: 1000.0,
    };
    let facing = 1000.0 * 0.5 / PI;
    for (texel, side) in [
        (towards_tangent, vec3(tilt.sin(), tilt.cos(), 0.0)),
        (towards_bitangent, vec3(0.0, tilt.cos(), -tilt.sin())),
    ] {
        for tangents in [true, false] {
            let mut scene = RtScene::new();
            let texture = scene.add_texture(RtTexture {
                linear: true,
                ..nearest(1, 1, vec![texel])
            });
            let material = scene.add_material(RtMaterial {
                normal_texture: Some(texture),
                ..RtMaterial::diffuse(vec3(0.5, 0.5, 0.5))
            });
            square_at(&mut scene, 0.0, material, tangents);
            scene.add_light(light_from(side));
            let other = scene.clone();
            let mut other = other;
            other.clear_lights();
            other.add_light(light_from(vec3(-side.x, side.y, -side.z)));
            for (name, mut tracer) in tracers() {
                let got = look_down(tracer.as_mut(), &scene, vec3(0.0, 0.5, 0.0));
                // The quantization of 8 bits moves the normal a little
                assert!(
                    (got.x / facing - 1.0).abs() < 0.02,
                    "{name} tangents {tangents} {texel:?}: {got:?} vs {facing}"
                );
                let got = look_down(tracer.as_mut(), &other, vec3(0.0, 0.5, 0.0));
                assert!(
                    got.x < 0.02 * facing,
                    "{name} tangents {tangents} {texel:?} other side: {got:?}"
                );
            }
        }
    }
}

#[test]
fn emission_texture_colors_the_light() {
    let mut scene = RtScene::new();
    let texture = scene.add_texture(nearest(1, 1, vec![[255, 0, 0, 255]]));
    let material = scene.add_material(RtMaterial {
        emission: vec3(500.0, 500.0, 500.0),
        emission_texture: Some(texture),
        ..RtMaterial::diffuse(vec3(0.0, 0.0, 0.0))
    });
    square_at(&mut scene, 0.0, material, true);
    for (name, mut tracer) in tracers() {
        let got = look_down(tracer.as_mut(), &scene, vec3(0.0, 0.5, 0.0));
        assert!(close(got, vec3(500.0, 0.0, 0.0)), "{name}: {got:?}");
    }
}

#[test]
fn cpu_materials_bring_their_textures() {
    let texture = CpuTexture {
        data: TextureData::RgbaU8(vec![[255, 128, 0, 255]]),
        width: 1,
        height: 1,
        ..Default::default()
    };
    let mut scene = RtScene::new();
    let id = scene.add_cpu_material(&CpuMaterial {
        albedo_texture: Some(texture.clone()),
        normal_texture: Some(texture),
        ..Default::default()
    });
    let m = &scene.materials()[id.0 as usize];
    assert_eq!(scene.textures().len(), 2);
    assert_eq!(m.base_color_texture, Some(TextureId(0)));
    assert_eq!(m.normal_texture, Some(TextureId(1)));
    assert_eq!(m.metallic_roughness_texture, None);
    // Without the textures by the plain conversion
    let mut plain = RtScene::new();
    let id = plain.add_material(CpuMaterial::default());
    assert!(!plain.materials()[id.0 as usize].has_textures());
}
