//!
//! Notices when the graphics card was reset, for example because a command took so long that the driver thought the card hangs.
//!
//! After a reset everything the card computed may be wrong. Vulkan, Metal and DirectX 12 tell wgpu, which then reports a lost device.
//! OpenGL only tells a context that was created with the reset notification strategy `LOSE_CONTEXT_ON_RESET` (`GL_KHR_robustness`,
//! `EGL_EXT_create_context_robustness`, EGL 1.5): then `glGetGraphicsResetStatus` returns something other than `NO_ERROR`
//! and the context is lost, every later command does nothing and `glGetError` returns `CONTEXT_LOST`.
//! Every other context goes on as if nothing happened, and wrong pixels come out without an error.
//! This crate creates its OpenGL contexts without a window with that strategy (see `gl_headless`). wgpu on top of OpenGL does that
//! with the patched wgpu-hal in `vendor/wgpu-hal`, and reports a lost device then (see "Lost devices" in `docs/wgpu-backend.md`).
//!

// Which parts are used depends on the features
#![allow(dead_code)]

use std::sync::OnceLock;

/// `glGetError` after the context was lost (OpenGL 4.5, OpenGL ES 3.2 and `GL_KHR_robustness`).
pub(crate) const CONTEXT_LOST: u32 = 0x0507;
const RESET_NOTIFICATION_STRATEGY: u32 = 0x8256;
const LOSE_CONTEXT_ON_RESET: u32 = 0x8252;
const GUILTY_CONTEXT_RESET: u32 = 0x8253;
const INNOCENT_CONTEXT_RESET: u32 = 0x8254;

/// `glGetGraphicsResetStatus` and its variants of the extensions.
type ResetStatus = unsafe extern "system" fn() -> u32;

///
/// Whether the graphics device of a backend was lost, and why. Once lost, it stays lost.
///
#[derive(Default)]
pub(crate) struct DeviceLost {
    /// Asks OpenGL whether the card was reset. Only set for a context that is told about resets.
    status: Option<ResetStatus>,
    reason: OnceLock<String>,
}

impl DeviceLost {
    ///
    /// For an OpenGL context that is current on this thread: asks for resets if the context was created to be told about them.
    /// The loader returns the address of an OpenGL function.
    ///
    /// # Safety
    /// The context must be current on this thread whenever [DeviceLost::check] is called.
    ///
    #[allow(unsafe_code)]
    pub unsafe fn of_gl_context(
        gl: &crate::context::Context,
        loader: impl Fn(&str) -> *const std::ffi::c_void,
    ) -> Self {
        use crate::context::HasContext;
        let version = gl.version();
        let extensions = gl.supported_extensions();
        // The core name exists from OpenGL 4.5 and OpenGL ES 3.2 on, before only with one of the extensions
        let core = if version.is_embedded {
            (version.major, version.minor) >= (3, 2)
        } else {
            (version.major, version.minor) >= (4, 5)
        };
        let name = [
            (core, "glGetGraphicsResetStatus"),
            (
                extensions.contains("GL_KHR_robustness"),
                if version.is_embedded {
                    "glGetGraphicsResetStatusKHR"
                } else {
                    "glGetGraphicsResetStatus"
                },
            ),
            (
                extensions.contains("GL_ARB_robustness"),
                "glGetGraphicsResetStatusARB",
            ),
            (
                extensions.contains("GL_EXT_robustness"),
                "glGetGraphicsResetStatusEXT",
            ),
        ]
        .into_iter()
        .find_map(|(supported, name)| supported.then_some(name));
        let Some(name) = name else {
            return Self::default();
        };
        // Without the extensions this would be an error that the next error check reports
        let strategy = unsafe { gl.get_parameter_i32(RESET_NOTIFICATION_STRATEGY) } as u32;
        let address = loader(name);
        if strategy != LOSE_CONTEXT_ON_RESET || address.is_null() {
            return Self::default();
        }
        Self {
            // Safety: the address belongs to a function without parameters that returns an enum, as the specification says
            status: Some(unsafe {
                std::mem::transmute::<*const std::ffi::c_void, ResetStatus>(address)
            }),
            reason: OnceLock::new(),
        }
    }

    /// Whether resets are noticed at all, otherwise only [DeviceLost::lose] makes the device lost.
    pub fn notices_resets(&self) -> bool {
        self.status.is_some()
    }

    /// Makes the device lost, if it is not already.
    pub fn lose(&self, reason: impl Into<String>) {
        let _ = self.reason.set(reason.into());
    }

    ///
    /// Why the device was lost, after asking OpenGL whether the card was reset (which is cheap).
    /// Must be called on the thread where the context is current, the answer is `None` on every other thread.
    ///
    #[allow(unsafe_code)]
    pub fn check(&self) -> Option<&str> {
        if self.reason.get().is_none() {
            if let Some(status) = self.status {
                // Safety: see [DeviceLost::of_gl_context]
                let status = unsafe { status() };
                if status != 0 {
                    self.lose(reset_reason(status));
                }
            }
        }
        self.reason.get().map(|reason| reason.as_str())
    }

    /// Why the device was lost, without asking.
    pub fn reason(&self) -> Option<&str> {
        self.reason.get().map(|reason| reason.as_str())
    }

    /// Panics if the device is lost, for functions that hand out pixels and cannot return an error.
    pub fn panic_if_lost(&self) {
        if let Some(reason) = self.check() {
            panic!("{}", message(reason));
        }
    }

    /// The error for [Context::error_check](crate::core::Context::error_check) if the device is lost.
    pub fn error(&self) -> Result<(), crate::core::CoreError> {
        match self.check() {
            Some(reason) => Err(crate::core::CoreError::ContextError(message(reason))),
            None => Ok(()),
        }
    }

    #[cfg(test)]
    pub fn with_status(status: ResetStatus) -> Self {
        Self {
            status: Some(status),
            reason: OnceLock::new(),
        }
    }
}

/// What a reset status of OpenGL means.
pub(crate) fn reset_reason(status: u32) -> String {
    match status {
        GUILTY_CONTEXT_RESET => {
            "the graphics card was reset because a command of this context made it hang (for example a draw or compute call that took too long)"
        }
        INNOCENT_CONTEXT_RESET => "the graphics card was reset because of another program",
        _ => "the graphics card was reset",
    }
    .to_string()
}

/// The text of the error or panic.
pub(crate) fn message(reason: &str) -> String {
    format!(
        "the graphics device is lost: {reason}. What was drawn or computed since may be wrong and the context cannot be used anymore, create a new context"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static STATUS: AtomicU32 = AtomicU32::new(0);

    extern "system" fn status() -> u32 {
        STATUS.load(Ordering::SeqCst)
    }

    #[test]
    fn a_reset_is_noticed_and_stays() {
        let lost = DeviceLost::with_status(status);
        assert!(lost.notices_resets());
        assert_eq!(lost.check(), None);
        assert!(lost.error().is_ok());
        STATUS.store(GUILTY_CONTEXT_RESET, Ordering::SeqCst);
        assert!(lost.check().unwrap().contains("hang"));
        STATUS.store(0, Ordering::SeqCst);
        // OpenGL tells only once, the device stays lost
        assert!(lost.check().is_some());
        let error = lost.error().unwrap_err().to_string();
        assert!(error.contains("graphics device is lost"), "{error}");
        assert!(std::panic::catch_unwind(|| lost.panic_if_lost()).is_err());
    }

    #[test]
    fn without_notification_only_losing_counts() {
        let lost = DeviceLost::default();
        assert!(!lost.notices_resets());
        assert_eq!(lost.check(), None);
        lost.lose("gone");
        lost.lose("the first reason stays");
        assert_eq!(lost.reason(), Some("gone"));
    }
}
