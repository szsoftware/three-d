//! What the test files share: the GPU context, which `THREE_D_SKIP_GPU_TESTS` turns off (CI on Windows in pull
//! requests, where WARP needs about three minutes per test to build the tracer's shaders).

/// A headless wgpu context, or why there is none.
#[cfg(feature = "wgpu")]
#[allow(dead_code)]
pub fn headless_wgpu() -> Result<three_d::Context, String> {
    skipped()?;
    three_d::Context::new_headless_wgpu().map_err(|e| e.to_string())
}

/// A headless wgpu context with a screen, or why there is none.
#[cfg(feature = "wgpu")]
#[allow(dead_code)]
pub fn headless_wgpu_with_screen(
    width: u32,
    height: u32,
    depth: bool,
    msaa: bool,
) -> Result<three_d::Context, String> {
    skipped()?;
    three_d::Context::new_headless_wgpu_with_screen(width, height, depth, msaa)
        .map_err(|e| e.to_string())
}

#[allow(dead_code)]
fn skipped() -> Result<(), String> {
    match std::env::var_os("THREE_D_SKIP_GPU_TESTS") {
        Some(_) => Err("GPU tests turned off by THREE_D_SKIP_GPU_TESTS".to_owned()),
        None => Ok(()),
    }
}
