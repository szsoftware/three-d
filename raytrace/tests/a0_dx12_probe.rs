//! TEMPORARY: what the media kernel computes on DirectX 12 with parts switched off.
#![cfg(feature = "wgpu")]
use three_d_raytrace::*;

#[test]
fn probe() {
    let Ok(context) = three_d::Context::new_headless_wgpu() else {
        return;
    };
    let mut lines = Vec::new();
    for probe in [0u32, 2] {
        // One process, one test: nothing else reads the variable
        unsafe { std::env::set_var("RAYTRACE_PROBE", probe.to_string()) };
        for scene_kind in ["fog", "box"] {
            let mut scene = RtScene::new();
            scene.set_environment(RtEnvironment::Uniform {
                color: vec3(1.0, 1.0, 1.0),
                luminance: 1000.0,
            });
            if scene_kind == "fog" {
                scene.set_fog(Some(RtFog {
                    medium: RtMedium {
                        absorption: vec3(0.05, 0.05, 0.05),
                        scattering: vec3(0.0, 0.0, 0.0),
                        anisotropy: 0.0,
                    },
                    height: 0.0,
                    scale_height: 10.0,
                }));
            } else {
                let material =
                    scene.add_material(RtMaterial::medium_boundary(RtMedium::fog(0.7, 1.0, 0.3)));
                let cube = scene.add_mesh(&CpuMesh::cube()).unwrap();
                scene
                    .add_instance(cube, material, Mat4::identity())
                    .unwrap();
            }
            let (at, target, up) = if scene_kind == "fog" {
                (
                    vec3(0.0, 5.0, 0.0),
                    vec3(0.0, 6.0, 0.0),
                    vec3(0.0, 0.0, -1.0),
                )
            } else {
                (
                    vec3(0.0, 0.0, 3.0),
                    vec3(0.0, 0.0, 2.0),
                    vec3(0.0, 1.0, 0.0),
                )
            };
            let camera = Camera::new_orthographic(
                Viewport::new_at_origo(4, 4),
                at,
                target,
                up,
                1.5,
                0.01,
                100.0,
            );
            let mut tracer = ComputeTracer::from_context(&context).unwrap();
            let mut image = RenderImage::new(4, 4);
            let settings = TraceSettings {
                samples_per_call: 16,
                max_bounces: 8,
                ..Default::default()
            };
            let r = tracer.trace(&scene, &camera, &mut image, &settings);
            let p = image.pixel(1, 1);
            lines.push(format!("PROBE {probe} {scene_kind}: {:?} {:?}", r.err(), p));
        }
    }
    // Fails on purpose, so the test harness shows the lines in the log
    panic!("{}", lines.join("\n"));
}
