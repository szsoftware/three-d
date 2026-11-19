//!
//! Threads that let the application go first: the ones of the preview, which trace beside the frames of the
//! application and compile shaders, on machines with few cores.
//!

/// The nice value of the threads of the preview that trace on the CPU, from 0 (normal) to 19 (lowest).
pub(crate) const TRACING: i32 = 10;
/// The nice value of the thread that compiles the shaders of the preview.
#[cfg_attr(
    not(all(feature = "wgpu", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
pub(crate) const COMPILING: i32 = 19;

///
/// Lowers the priority of the calling thread to the nice value. On Linux and Android the nice value belongs to the
/// thread; elsewhere nothing changes.
///
pub(crate) fn lower_this_thread(#[allow(unused_variables)] nice: i32) {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    // SAFETY: setpriority only reads its arguments; 0 is the calling thread
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, nice);
    }
}
