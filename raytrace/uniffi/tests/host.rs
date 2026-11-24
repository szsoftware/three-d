//!
//! A scene view drawn by a host into its own target with its own context (as an application that renders with
//! three-d itself): the same image as the view's own frame, only inside the viewport, nothing read back.
//!

use std::sync::Arc;
use three_d::*;
use three_d_raytrace_uniffi::*;

const HOST: (u32, u32) = (96, 64);
const VIEWPORT: Viewport = Viewport {
    x: 16,
    y: 4,
    width: 64,
    height: 48,
};
const HOST_COLOR: [u8; 4] = [255, 0, 255, 255];

fn view(renderer: &Arc<Renderer>, when_still: Option<u32>) -> Arc<SceneView> {
    let demo = demo_scene("pergola".to_owned()).unwrap();
    let realtime = RealtimeSpec {
        sun_shadow_map: 512,
        raytrace_when_still_ms: when_still,
        look: demo.look,
    };
    renderer
        .clone()
        .scene_view(demo.scene.clone(), demo.camera, 64, 48, realtime)
        .unwrap()
}

/// The pixels of `viewport` (from the bottom left) out of pixels in rows from the top, as three-d reads them.
fn inside(pixels: &[[u8; 4]], width: u32, height: u32, viewport: Viewport) -> Vec<[u8; 4]> {
    let top = height as i32 - viewport.y - viewport.height as i32;
    let mut out = Vec::new();
    for y in top..top + viewport.height as i32 {
        for x in viewport.x..viewport.x + viewport.width as i32 {
            out.push(pixels[(y as u32 * width + x as u32) as usize]);
        }
    }
    out
}

/// The view's own frame, rows from the top.
fn own_frame(view: &SceneView) -> Vec<[u8; 4]> {
    let frame = view.next_frame().unwrap();
    let bytes = unsafe {
        std::slice::from_raw_parts(frame.rgba.address as *const u8, frame.rgba.length as usize)
    };
    bytes.chunks(4).map(|p| [p[0], p[1], p[2], p[3]]).collect()
}

fn check(host: &[[u8; 4]], own: &[[u8; 4]]) {
    if let Some(dir) = std::env::var_os("THREE_D_HOST_DUMP") {
        let save = |name: &str, px: &[[u8; 4]], w: u32, h: u32| {
            let bytes: Vec<u8> = px.iter().flatten().copied().collect();
            image::RgbaImage::from_raw(w, h, bytes)
                .unwrap()
                .save(std::path::Path::new(&dir).join(name))
                .unwrap();
        };
        save("host.png", host, HOST.0, HOST.1);
        save("own.png", own, 64, 48);
    }
    // Outside the viewport the host's color stays
    assert_eq!(host[0], HOST_COLOR);
    assert_eq!(host[host.len() - 1], HOST_COLOR);
    let drawn = inside(host, HOST.0, HOST.1, VIEWPORT);
    assert!(
        drawn.iter().all(|p| *p != HOST_COLOR),
        "the whole viewport drawn"
    );
    let diff = drawn
        .iter()
        .zip(own)
        .flat_map(|(a, b)| (0..4).map(move |i| (a[i] as i32 - b[i] as i32).abs()))
        .max()
        .unwrap();
    assert!(
        diff <= 2,
        "the host's image is the view's frame, at most {diff} apart"
    );
}

/// The rasterizer and then the raytraced preview into a texture of a host on wgpu.
#[test]
fn a_host_on_wgpu_draws_the_view_into_its_texture() {
    if std::env::var_os("THREE_D_SKIP_GPU_TESTS").is_some() {
        return;
    }
    let context = match Context::new_headless_wgpu() {
        Ok(context) => context,
        Err(e) if std::env::var("THREE_D_REQUIRE_WGPU").is_err() => {
            eprintln!("no wgpu: {e}");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let renderer = Renderer::with_context(context.clone());
    let mut color = Texture2D::new_empty::<[u8; 4]>(
        &context,
        HOST.0,
        HOST.1,
        Interpolation::Nearest,
        Interpolation::Nearest,
        None,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    );
    let mut depth = DepthTexture2D::new::<f32>(
        &context,
        HOST.0,
        HOST.1,
        Wrapping::ClampToEdge,
        Wrapping::ClampToEdge,
    );
    let target = RenderTarget::new(color.as_color_target(None), depth.as_depth_target());
    let host_clear = ClearState::color_and_depth(1.0, 0.0, 1.0, 1.0, 1.0);

    let hosted = view(&renderer, None);
    let own = view(&renderer, None);
    target.clear(host_clear);
    assert_eq!(hosted.render_into(&target, VIEWPORT).unwrap(), 0);
    let host: Vec<[u8; 4]> = target.read_color();
    check(&host, &own_frame(&own));

    // The preview takes over when still: drawn scaled into the viewport, with its samples
    hosted.set_raytrace_when_still(Some(0)).unwrap();
    let mut samples = 0;
    for _ in 0..400 {
        target.clear(host_clear);
        samples = hosted.render_into(&target, VIEWPORT).unwrap();
        if samples > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(samples > 0, "the preview took over");
    let host: Vec<[u8; 4]> = target.read_color();
    assert_eq!(host[0], HOST_COLOR);
    assert!(inside(&host, HOST.0, HOST.1, VIEWPORT)
        .iter()
        .all(|p| *p != HOST_COLOR));
    drop(target);
    let _ = (&mut color, &mut depth);
}

/// The rasterizer onto the screen of a host on OpenGL (the offscreen screen stands in for a window or JOGL's).
#[cfg(target_os = "linux")]
#[test]
fn a_host_on_opengl_draws_the_view_onto_its_screen() {
    use three_d_egui_uniffi::{EguiOverlay, Side, UiNode, Widget};
    let context = match Context::new_headless_gl_with_screen(HOST.0, HOST.1, true, false) {
        Ok(context) => context,
        Err(e) => {
            eprintln!("no OpenGL: {e}");
            return;
        }
    };
    let renderer = Renderer::with_context(context.clone());
    let hosted = view(&renderer, None);
    let own = view(&renderer, None);
    let screen = RenderTarget::screen(&context, HOST.0, HOST.1);
    screen.clear(ClearState::color_and_depth(1.0, 0.0, 1.0, 1.0, 1.0));
    let samples = hosted
        .render_to_screen(
            HOST.0,
            HOST.1,
            VIEWPORT.x,
            VIEWPORT.y,
            VIEWPORT.width,
            VIEWPORT.height,
        )
        .unwrap();
    assert_eq!(samples, 0);
    let host: Vec<[u8; 4]> = screen.read_color();
    check(&host, &own_frame(&own));

    // egui over the view, inside its viewport
    let overlay = EguiOverlay::new();
    overlay
        .set_tree(vec![UiNode {
            id: 1,
            parent: None,
            widget: Widget::SidePanel {
                side: Side::Left,
                title: None,
                width: 24.0,
                resizable: false,
            },
            enabled: true,
            tooltip: None,
        }])
        .unwrap();
    hosted.set_overlay(Some(overlay)).unwrap();
    for _ in 0..2 {
        screen.clear(ClearState::color_and_depth(1.0, 0.0, 1.0, 1.0, 1.0));
        hosted
            .render_to_screen(
                HOST.0,
                HOST.1,
                VIEWPORT.x,
                VIEWPORT.y,
                VIEWPORT.width,
                VIEWPORT.height,
            )
            .unwrap();
    }
    let host: Vec<[u8; 4]> = screen.read_color();
    let drawn = inside(&host, HOST.0, HOST.1, VIEWPORT);
    let gray = |p: [u8; 4]| p[..3].iter().all(|&c| c.abs_diff(27) <= 3);
    let at = |x: u32, y: u32| drawn[(y * VIEWPORT.width + x) as usize];
    assert!(
        gray(at(4, 24)),
        "egui's panel at the left of the viewport: {:?}",
        at(4, 24)
    );
    assert!(!gray(at(50, 24)), "the scene beside the panel");
    assert_eq!(host[0], HOST_COLOR, "nothing outside the viewport");
}
