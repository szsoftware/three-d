//!
//! An OpenGL context without a window, created through EGL.
//!

use super::gl::{GlBackend, KeepAlive};
use super::reset::DeviceLost;
use crate::core::CoreError;
use glutin::api::egl::{context::PossiblyCurrentContext, device::Device, display::Display};
use glutin::config::{ConfigSurfaceTypes, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, Robustness, Version};
use glutin::prelude::*;
use std::sync::Arc;

/// Renderers that draw on the CPU.
const SOFTWARE: [&str; 3] = ["llvmpipe", "softpipe", "swiftshader"];

///
/// Creates a context on one of the graphics devices EGL knows, makes it current on this thread and creates a backend for it.
/// A device that renders in hardware is preferred. Returns the backend and whether it renders in hardware.
///
pub(crate) fn new(hardware_only: bool) -> Result<(GlBackend, bool), CoreError> {
    let error = |e: &dyn std::fmt::Display| CoreError::ContextCreation(e.to_string());
    let devices = Device::query_devices().map_err(|e| error(&e))?;
    let mut software = None;
    let mut last_error = error(&"EGL knows no graphics device");
    for device in devices {
        match create(&device) {
            Ok((context, display)) => {
                let gl = load(&display);
                let renderer = unsafe {
                    use crate::context::HasContext;
                    gl.get_parameter_string(crate::context::RENDERER)
                }
                .to_lowercase();
                let candidate = (gl, context, display);
                if SOFTWARE.iter().any(|name| renderer.contains(name)) {
                    // Only one context can be current, so a software device is created again if it is needed
                    software.get_or_insert(device);
                    drop(candidate);
                } else {
                    return backend(candidate).map(|backend| (backend, true));
                }
            }
            Err(e) => last_error = error(&e),
        }
    }
    match software {
        Some(_) if hardware_only => Err(error(
            &"there is no hardware accelerated OpenGL device without a window",
        )),
        Some(device) => {
            let (context, display) = create(&device).map_err(|e| error(&e))?;
            let gl = load(&display);
            backend((gl, context, display)).map(|backend| (backend, false))
        }
        None => Err(last_error),
    }
}

fn proc_address(display: &Display, name: &str) -> *const std::ffi::c_void {
    match std::ffi::CString::new(name) {
        Ok(name) => display.get_proc_address(&name) as *const _,
        Err(_) => std::ptr::null(),
    }
}

fn load(display: &Display) -> crate::context::Context {
    unsafe { crate::context::Context::from_loader_function(|name| proc_address(display, name)) }
}

fn backend(
    (gl, context, display): (crate::context::Context, PossiblyCurrentContext, Display),
) -> Result<GlBackend, CoreError> {
    // Safety: the context is current on this thread, where it is used
    let lost = unsafe { DeviceLost::of_gl_context(&gl, |name| proc_address(&display, name)) };
    let mut backend = GlBackend::new(Arc::new(gl))?;
    backend.keep_alive = Some(KeepAlive(Box::new((context, display))));
    backend.lost = Arc::new(lost);
    Ok(backend)
}

/// A current context without a surface on the device: OpenGL 3.3 or newer, otherwise OpenGL ES 3.0 or newer.
/// The context is lost when the graphics card is reset, so the reset is noticed (see [super::reset]), if the driver can do that.
fn create(device: &Device) -> Result<(PossiblyCurrentContext, Display), String> {
    let error = |e: glutin::error::Error| e.to_string();
    let display = unsafe { Display::with_device(device, None) }.map_err(error)?;
    let template = ConfigTemplateBuilder::new()
        .with_surface_type(ConfigSurfaceTypes::empty())
        .build();
    let config = unsafe { display.find_configs(template) }
        .map_err(error)?
        .next()
        .ok_or("the graphics device has no configuration without a surface")?;
    let mut result = Err("no context".to_string());
    'apis: for api in [
        ContextApi::OpenGl(Some(Version::new(3, 3))),
        ContextApi::Gles(Some(Version::new(3, 0))),
    ] {
        for robustness in [Robustness::RobustLoseContextOnReset, Robustness::NotRobust] {
            let attributes = ContextAttributesBuilder::new()
                .with_context_api(api)
                .with_robustness(robustness)
                .build(None);
            result = unsafe { display.create_context(&config, &attributes) }
                .and_then(|context| context.make_current_surfaceless())
                .map_err(error);
            if result.is_ok() {
                break 'apis;
            }
        }
    }
    Ok((result?, display))
}

#[cfg(test)]
mod tests {
    use crate::renderer::*;

    const WIDTH: u32 = 40;
    const HEIGHT: u32 = 24;

    /// All tests share the software drivers of Mesa, see the tests of the wgpu backend.
    fn one_at_a_time() {
        #[cfg(feature = "wgpu")]
        crate::core::backend::wgpu::one_gpu_test_at_a_time();
    }

    fn context(
        create: impl Fn(u32, u32, bool, bool) -> Result<Context, CoreError>,
        multisample: bool,
    ) -> Option<Context> {
        one_at_a_time();
        match create(WIDTH, HEIGHT, true, multisample) {
            Ok(context) => Some(context),
            Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
                panic!("no context without a window: {error}")
            }
            Err(error) => {
                eprintln!("skipped, no context without a window: {error}");
                None
            }
        }
    }

    /// Two spheres, the one in front is not symmetric in the image, so a flipped image is noticed.
    fn render(context: &Context, target: &RenderTarget, width: u32, height: u32) {
        let camera = Camera::new_perspective(
            Viewport::new_at_origo(width, height),
            vec3(0.0, 0.0, 5.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
            degrees(45.0),
            0.1,
            10.0,
        );
        let sphere = |color, position| {
            let mut sphere = Gm::new(
                Mesh::new(context, &CpuMesh::sphere(16)),
                ColorMaterial {
                    color,
                    ..Default::default()
                },
            );
            sphere.set_transformation(Mat4::from_translation(position));
            sphere
        };
        target
            .clear(ClearState::color_and_depth(0.2, 0.4, 0.6, 1.0, 1.0))
            .render(
                &camera,
                [
                    &sphere(Srgba::RED, vec3(0.5, 0.8, 1.0)),
                    &sphere(Srgba::GREEN, vec3(0.0, 0.0, 0.0)),
                ],
                &[],
            );
    }

    fn check(context: Context, multisample: bool) {
        // The same into a texture is what the screen has to show
        let texture = Texture2D::new_empty::<[u8; 4]>(
            &context,
            WIDTH,
            HEIGHT,
            Interpolation::Nearest,
            Interpolation::Nearest,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        let depth_texture = DepthTexture2D::new::<f32>(
            &context,
            WIDTH,
            HEIGHT,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        let target = RenderTarget::new(
            texture.as_color_target(None),
            depth_texture.as_depth_target(),
        );
        render(&context, &target, WIDTH, HEIGHT);
        let expected = target.read_color::<[u8; 4]>();
        let expected_depth = target.read_depth();

        let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
        render(&context, &screen, WIDTH, HEIGHT);
        let actual = screen.read_color::<[u8; 4]>();
        assert_eq!(actual.len() as u32, WIDTH * HEIGHT);
        let depth = screen.read_depth();
        if multisample {
            let different = actual.iter().zip(&expected).filter(|(a, e)| a != e).count();
            assert!(different > 0, "no edge is smoothed");
            assert!(different < actual.len() / 4, "{different} pixels differ");
            assert_eq!(depth[0], 1.0);
            assert!(depth[(HEIGHT / 2 * WIDTH + WIDTH / 2) as usize] < 1.0);
        } else {
            assert_eq!(actual, expected);
            assert_eq!(depth, expected_depth);
        }

        // The first row is the top row: the red sphere is up and to the right of the center
        let red: Vec<u32> = (0..WIDTH * HEIGHT)
            .filter(|i| actual[*i as usize] == [255, 0, 0, 255])
            .collect();
        assert!(!red.is_empty());
        let row = red.iter().map(|i| i / WIDTH).sum::<u32>() / red.len() as u32;
        let column = red.iter().map(|i| i % WIDTH).sum::<u32>() / red.len() as u32;
        assert!(
            row < HEIGHT / 2 && column > WIDTH / 2,
            "row {row}, column {column}"
        );
        // A part of the screen is addressed from the bottom, as everywhere
        let corner = screen.read_color_partially::<[u8; 4]>(ScissorBox {
            x: 0,
            y: HEIGHT as i32 - 2,
            width: 3,
            height: 2,
        });
        assert_eq!(corner[..3], actual[..3]);
        assert_eq!(corner[3..], actual[WIDTH as usize..WIDTH as usize + 3]);

        // A frame that is not read, then another size
        render(&context, &screen, WIDTH, HEIGHT);
        context.submit_offscreen_screen().unwrap();
        context
            .resize_offscreen_screen(WIDTH * 2, HEIGHT + 3)
            .unwrap();
        let screen = RenderTarget::screen(&context, WIDTH * 2, HEIGHT + 3);
        render(&context, &screen, WIDTH * 2, HEIGHT + 3);
        let color = screen.read_color::<[u8; 4]>();
        assert_eq!(color.len() as u32, WIDTH * 2 * (HEIGHT + 3));
        assert!(color.contains(&[255, 0, 0, 255]));
        assert!(screen.read_depth().iter().any(|d| *d < 1.0));
        context.error_check().unwrap();
        // The programs in the cache hold the context
        context.programs.write().unwrap().clear();
    }

    #[test]
    fn offscreen_screen_with_opengl() {
        for multisample in [false, true] {
            let Some(context) = context(Context::new_headless_gl_with_screen, multisample) else {
                return;
            };
            assert!(context.as_gl().is_some());
            check(context, multisample);
        }
    }

    #[test]
    fn offscreen_screen_with_the_backend_the_system_has() {
        for multisample in [false, true] {
            let Some(context) = context(Context::new_headless_with_screen, multisample) else {
                return;
            };
            check(context, multisample);
        }
    }

    #[test]
    fn a_reset_is_noticed_where_the_driver_can() {
        let Some(context) = context(Context::new_headless_gl_with_screen, false) else {
            return;
        };
        let gl = context.gl_backend();
        // Mesa can, so the context is lost on a reset
        let renderer = unsafe {
            use crate::context::HasContext;
            gl.context.get_parameter_string(crate::context::RENDERER)
        };
        if renderer.contains("llvmpipe") {
            assert!(gl.lost.notices_resets(), "{renderer}");
        }
        let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
        render(&context, &screen, WIDTH, HEIGHT);
        assert!(!context.is_lost());
        context.error_check().unwrap();
        // What a reset makes the backend notice
        gl.lost
            .lose(crate::core::backend::reset::reset_reason(0x8253));
        assert!(context.is_lost());
        assert!(context
            .error_check()
            .unwrap_err()
            .to_string()
            .contains("hang"));
        assert!(context.submit_offscreen_screen().is_err());
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            screen.read_color::<[u8; 4]>()
        }));
        assert!(read.is_err());
        context.programs.write().unwrap().clear();
    }

    #[test]
    fn context_of_a_window_has_no_offscreen_screen() {
        one_at_a_time();
        // A context that was not created with a screen, as the one of a window
        let Ok((backend, _)) = super::new(false) else {
            return;
        };
        let context = Context::from_backend(crate::core::backend::Backend::Gl(backend));
        assert!(context.resize_offscreen_screen(WIDTH, HEIGHT).is_err());
        assert!(context.submit_offscreen_screen().is_err());
    }

    /// The framebuffer a user interface toolkit hands over: an RGBA8 texture as color buffer and no depth buffer.
    struct HostFramebuffer {
        gl: std::sync::Arc<crate::context::Context>,
        framebuffer: crate::context::Framebuffer,
        texture: crate::context::Texture,
    }

    impl HostFramebuffer {
        fn new(context: &Context, width: u32, height: u32) -> Self {
            use crate::context::{self as gl, HasContext};
            let gl = context.as_gl().unwrap().clone();
            unsafe {
                let texture = gl.create_texture().unwrap();
                gl.bind_texture(gl::TEXTURE_2D, Some(texture));
                gl.tex_image_2d(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGBA8 as i32,
                    width as i32,
                    height as i32,
                    0,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    crate::context::PixelUnpackData::Slice(None),
                );
                gl.tex_parameter_i32(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
                gl.bind_texture(gl::TEXTURE_2D, None);
                let framebuffer = gl.create_framebuffer().unwrap();
                gl.bind_framebuffer(gl::FRAMEBUFFER, Some(framebuffer));
                gl.framebuffer_texture_2d(
                    gl::FRAMEBUFFER,
                    gl::COLOR_ATTACHMENT0,
                    gl::TEXTURE_2D,
                    Some(texture),
                    0,
                );
                assert_eq!(
                    gl.check_framebuffer_status(gl::FRAMEBUFFER),
                    gl::FRAMEBUFFER_COMPLETE
                );
                gl.bind_framebuffer(gl::FRAMEBUFFER, None);
                Self {
                    gl,
                    framebuffer,
                    texture,
                }
            }
        }

        /// What the toolkit would show, with the first row at the top.
        fn pixels(&self, width: u32, height: u32) -> Vec<[u8; 4]> {
            use crate::context::{self as gl, HasContext};
            let mut bytes = vec![0u8; (width * height * 4) as usize];
            unsafe {
                self.gl
                    .bind_framebuffer(gl::READ_FRAMEBUFFER, Some(self.framebuffer));
                self.gl.read_pixels(
                    0,
                    0,
                    width as i32,
                    height as i32,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    crate::context::PixelPackData::Slice(Some(&mut bytes)),
                );
            }
            bytes
                .chunks(4)
                .map(|p| [p[0], p[1], p[2], p[3]])
                .collect::<Vec<_>>()
                .chunks(width as usize)
                .rev()
                .flatten()
                .copied()
                .collect()
        }
    }

    impl Drop for HostFramebuffer {
        fn drop(&mut self) {
            use crate::context::HasContext;
            unsafe {
                self.gl.delete_framebuffer(self.framebuffer);
                self.gl.delete_texture(self.texture);
            }
        }
    }

    /// The image of [render] in a texture with a depth texture, as the screen has to show it.
    fn expected(context: &Context, width: u32, height: u32) -> Vec<[u8; 4]> {
        let texture = Texture2D::new_empty::<[u8; 4]>(
            context,
            width,
            height,
            Interpolation::Nearest,
            Interpolation::Nearest,
            None,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        let depth_texture = DepthTexture2D::new::<f32>(
            context,
            width,
            height,
            Wrapping::ClampToEdge,
            Wrapping::ClampToEdge,
        );
        let target = RenderTarget::new(
            texture.as_color_target(None),
            depth_texture.as_depth_target(),
        );
        render(context, &target, width, height);
        target.read_color::<[u8; 4]>()
    }

    #[test]
    fn screen_framebuffer_of_the_application() {
        let Some(context) = context(Context::new_headless_gl_with_screen, false) else {
            return;
        };
        let expected = expected(&context, WIDTH, HEIGHT);
        let host = HostFramebuffer::new(&context, WIDTH, HEIGHT);

        // With an added depth buffer, the image is the same as with a depth texture
        for _ in 0..2 {
            context
                .set_screen_framebuffer(host.framebuffer, WIDTH, HEIGHT, true)
                .unwrap();
            let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
            render(&context, &screen, WIDTH, HEIGHT);
            assert_eq!(host.pixels(WIDTH, HEIGHT), expected);
            assert_eq!(screen.read_color::<[u8; 4]>(), expected);
            assert!(screen.read_depth().iter().any(|d| *d < 1.0));
        }
        context.error_check().unwrap();

        // The application's texture gets another size
        unsafe {
            use crate::context::{self as gl, HasContext};
            host.gl.bind_texture(gl::TEXTURE_2D, Some(host.texture));
            host.gl.tex_image_2d(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                (WIDTH * 2) as i32,
                (HEIGHT + 3) as i32,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                crate::context::PixelUnpackData::Slice(None),
            );
            host.gl.bind_texture(gl::TEXTURE_2D, None);
        }
        context
            .set_screen_framebuffer(host.framebuffer, WIDTH * 2, HEIGHT + 3, true)
            .unwrap();
        let screen = RenderTarget::screen(&context, WIDTH * 2, HEIGHT + 3);
        render(&context, &screen, WIDTH * 2, HEIGHT + 3);
        assert_eq!(
            host.pixels(WIDTH * 2, HEIGHT + 3),
            self::expected(&context, WIDTH * 2, HEIGHT + 3)
        );

        // Without a depth buffer, this crate draws into the application's framebuffer itself
        context
            .set_screen_framebuffer(host.framebuffer, WIDTH * 2, HEIGHT + 3, false)
            .unwrap();
        RenderTarget::screen(&context, WIDTH * 2, HEIGHT + 3)
            .clear(ClearState::color(1.0, 0.0, 1.0, 1.0));
        assert!(host
            .pixels(WIDTH * 2, HEIGHT + 3)
            .iter()
            .all(|p| *p == [255, 0, 255, 255]));

        // Back to the offscreen screen, which still has what was drawn first
        context.unset_screen_framebuffer().unwrap();
        let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
        render(&context, &screen, WIDTH, HEIGHT);
        assert_eq!(screen.read_color::<[u8; 4]>(), expected);
        context.error_check().unwrap();
        context.programs.write().unwrap().clear();
    }

    #[test]
    fn state_another_renderer_left_is_reset() {
        let Some(context) = context(Context::new_headless_gl_with_screen, false) else {
            return;
        };
        let checker = CpuTexture {
            data: TextureData::RgbaU8(
                (0..16)
                    .map(|i| {
                        if (i + i / 4) % 2 == 0 {
                            [255, 255, 0, 255]
                        } else {
                            [0, 0, 255, 255]
                        }
                    })
                    .collect(),
            ),
            width: 4,
            height: 4,
            min_filter: Interpolation::Nearest,
            mag_filter: Interpolation::Nearest,
            mipmap: None,
            ..Default::default()
        };
        let draw = |target: &RenderTarget| {
            // The texture is uploaded after the other renderer, which pixel store and pixel buffers affect
            let texture = std::sync::Arc::new(Texture2D::new(&context, &checker));
            let mut square = Gm::new(
                Mesh::new(&context, &CpuMesh::square()),
                ColorMaterial {
                    texture: Some(texture.into()),
                    ..Default::default()
                },
            );
            square.set_transformation(Mat4::from_scale(0.9));
            let camera = Camera::new_orthographic(
                Viewport::new_at_origo(WIDTH, HEIGHT),
                vec3(0.0, 0.0, 2.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.0, 1.0, 0.0),
                2.0,
                0.1,
                10.0,
            );
            target
                .clear(ClearState::color_and_depth(0.0, 0.0, 0.0, 1.0, 1.0))
                .render(&camera, &square, &[]);
        };
        let host = HostFramebuffer::new(&context, WIDTH, HEIGHT);
        context
            .set_screen_framebuffer(host.framebuffer, WIDTH, HEIGHT, true)
            .unwrap();
        draw(&RenderTarget::screen(&context, WIDTH, HEIGHT));
        let expected = host.pixels(WIDTH, HEIGHT);
        assert!(expected.contains(&[255, 255, 0, 255]) && expected.contains(&[0, 0, 255, 255]));

        // What another renderer, such as Skia, may leave behind
        use crate::context::{self as gl, HasContext};
        let gl = context.as_gl().unwrap().clone();
        let (buffer, sampler, vao) = unsafe {
            let buffer = gl.create_buffer().unwrap();
            gl.bind_buffer(gl::PIXEL_UNPACK_BUFFER, Some(buffer));
            gl.buffer_data_size(gl::PIXEL_UNPACK_BUFFER, 4, gl::STREAM_DRAW);
            gl.pixel_store_i32(gl::UNPACK_ALIGNMENT, 8);
            gl.pixel_store_i32(gl::UNPACK_ROW_LENGTH, 7);
            let sampler = gl.create_sampler().unwrap();
            gl.sampler_parameter_i32(
                sampler,
                gl::TEXTURE_MIN_FILTER,
                gl::LINEAR_MIPMAP_LINEAR as i32,
            );
            for unit in 0..4 {
                gl.bind_sampler(unit, Some(sampler));
            }
            gl.active_texture(gl::TEXTURE3);
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            gl.polygon_mode(gl::FRONT_AND_BACK, gl::LINE);
            gl.enable(gl::RASTERIZER_DISCARD);
            gl.front_face(gl::CW);
            gl.enable(gl::FRAMEBUFFER_SRGB);
            gl.bind_framebuffer(gl::FRAMEBUFFER, None);
            (buffer, sampler, vao)
        };
        context
            .set_screen_framebuffer(host.framebuffer, WIDTH, HEIGHT, true)
            .unwrap();
        context.reset_gl_state().unwrap();
        draw(&RenderTarget::screen(&context, WIDTH, HEIGHT));
        assert_eq!(host.pixels(WIDTH, HEIGHT), expected);
        context.error_check().unwrap();
        unsafe {
            gl.delete_buffer(buffer);
            gl.delete_sampler(sampler);
            gl.delete_vertex_array(vao);
        }
        context.programs.write().unwrap().clear();
    }

    #[test]
    fn screen_framebuffer_needs_a_color_buffer_for_depth() {
        let Some(context) = context(Context::new_headless_gl_with_screen, false) else {
            return;
        };
        let expected = expected(&context, WIDTH, HEIGHT);
        use crate::context::HasContext;
        let gl = context.as_gl().unwrap().clone();
        let empty = unsafe { gl.create_framebuffer().unwrap() };
        assert!(context
            .set_screen_framebuffer(empty, WIDTH, HEIGHT, true)
            .is_err());
        unsafe { gl.delete_framebuffer(empty) };
        // The screen is the one from before
        let screen = RenderTarget::screen(&context, WIDTH, HEIGHT);
        render(&context, &screen, WIDTH, HEIGHT);
        assert_eq!(screen.read_color::<[u8; 4]>(), expected);
        context.error_check().unwrap();
        context.programs.write().unwrap().clear();
    }
}
