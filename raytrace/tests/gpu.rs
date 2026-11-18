//!
//! The ray tracing crate together with three-d's wgpu backend. Skipped without a wgpu adapter unless
//! `THREE_D_REQUIRE_WGPU` is set.
//!
#![cfg(feature = "wgpu")]

use three_d::Context;
use three_d_raytrace::*;

fn context() -> Option<Context> {
    match Context::new_headless_wgpu() {
        Ok(context) => Some(context),
        Err(error) if std::env::var("THREE_D_REQUIRE_WGPU").is_ok() => {
            panic!("no wgpu adapter: {error}")
        }
        Err(error) => {
            eprintln!("skipped, no wgpu adapter: {error}");
            None
        }
    }
}

/// A film with a different value in every pixel: its index from the top left.
fn numbered_film(width: u32, height: u32) -> Film {
    let mut film = Film::new(width, height);
    let sums: Vec<[f32; 3]> = (0..width * height).map(|i| [i as f32; 3]).collect();
    film.accumulate(&sums, 1);
    film
}

#[test]
fn compute_is_supported_on_wgpu() {
    let Some(context) = context() else { return };
    let support = RaytraceSupport::of(&context);
    // Software adapters (lavapipe, WARP) and every desktop GPU have compute shaders
    if std::env::var("WGPU_BACKEND").as_deref() != Ok("gl") {
        assert!(support >= RaytraceSupport::Compute, "{support:?}");
    }
    assert!(GpuDevice::from_context(&context).is_some());
}

#[test]
fn film_shows_upright_in_three_d() {
    let Some(context) = context() else { return };
    let film = numbered_film(3, 2);
    let texture = film.to_three_d_texture(&context);
    // Reading a texture returns the bottom row first (texture coordinate v = 0), the film starts at the top
    let pixels: Vec<[f32; 4]> = texture.as_color_target(None).read();
    let numbers: Vec<f32> = pixels.iter().map(|p| p[0]).collect();
    assert_eq!(numbers, vec![3.0, 4.0, 5.0, 0.0, 1.0, 2.0]);
}

#[test]
fn film_updates_the_texture_in_place() {
    let Some(context) = context() else { return };
    let mut film = numbered_film(4, 4);
    let texture = film.to_three_d_texture(&context);
    film.reset();
    film.write_to_three_d_texture(&texture);
    let pixels: Vec<[f32; 4]> = texture.as_color_target(None).read();
    assert!(pixels.iter().all(|p| p[0] == 0.0));
}
