//!
//! What wgpu does not do on OpenGL and what is done with OpenGL itself instead, through the context that wgpu uses.
//! wgpu executes its OpenGL commands when they are submitted, so after a submit the context is in a state that is known.
//!

use super::*;
use crate::context::HasContext;
use w::hal::api::Gles;
use w::hal::gles::TextureInner;

///
/// Lets cube maps be filtered across their sides, as on every other graphics API. wgpu only gets that for free on OpenGL ES.
///
pub(super) fn seamless_cube_maps(device: &w::Device) {
    // Safety: the context is locked while it is used and only a global switch is changed, which wgpu never touches
    unsafe {
        let Some(device) = device.as_hal::<Gles>() else {
            return;
        };
        let gl = device.context().lock();
        if !gl.version().is_embedded {
            gl.enable(crate::context::TEXTURE_CUBE_MAP_SEAMLESS);
        }
    }
}

///
/// Copies the depth of a multisample texture into a texture with one sample and the same format and size.
/// Everything that wgpu recorded for the two textures must be submitted, and the destination must have been rendered into.
/// Returns false if that is not possible.
///
pub(super) fn resolve_depth(device: &w::Device, from: &w::Texture, to: &w::Texture) -> bool {
    // Safety: the context is locked while it is used. The framebuffers are created and deleted here,
    // the bindings and the scissor test are left as wgpu expects them: it binds its framebuffers before every use.
    unsafe {
        // One guard at a time, wgpu does not allow more
        let inner = |texture: &w::Texture| texture.as_hal::<Gles>().map(|t| t.inner.clone());
        let (Some(from_inner), Some(to_inner)) = (inner(from), inner(to)) else {
            return false;
        };
        let Some(device) = device.as_hal::<Gles>() else {
            return false;
        };
        let gl = device.context().lock();
        let attach = |target: u32, inner: &TextureInner| match inner {
            TextureInner::Renderbuffer { raw } => {
                gl.framebuffer_renderbuffer(
                    target,
                    crate::context::DEPTH_ATTACHMENT,
                    crate::context::RENDERBUFFER,
                    Some(*raw),
                );
                true
            }
            TextureInner::Texture { raw, target: kind } => {
                gl.framebuffer_texture_2d(
                    target,
                    crate::context::DEPTH_ATTACHMENT,
                    *kind,
                    Some(*raw),
                    0,
                );
                true
            }
            _ => false,
        };
        let (Ok(read), Ok(draw)) = (gl.create_framebuffer(), gl.create_framebuffer()) else {
            return false;
        };
        gl.bind_framebuffer(crate::context::READ_FRAMEBUFFER, Some(read));
        gl.bind_framebuffer(crate::context::DRAW_FRAMEBUFFER, Some(draw));
        let attached = attach(crate::context::READ_FRAMEBUFFER, &from_inner)
            && attach(crate::context::DRAW_FRAMEBUFFER, &to_inner);
        if attached {
            let scissor = gl.is_enabled(crate::context::SCISSOR_TEST);
            gl.disable(crate::context::SCISSOR_TEST);
            let (width, height) = (from.width() as i32, from.height() as i32);
            gl.blit_framebuffer(
                0,
                0,
                width,
                height,
                0,
                0,
                width,
                height,
                crate::context::DEPTH_BUFFER_BIT,
                crate::context::NEAREST,
            );
            if scissor {
                gl.enable(crate::context::SCISSOR_TEST);
            }
        }
        gl.bind_framebuffer(crate::context::READ_FRAMEBUFFER, None);
        gl.bind_framebuffer(crate::context::DRAW_FRAMEBUFFER, None);
        gl.delete_framebuffer(read);
        gl.delete_framebuffer(draw);
        attached
    }
}
