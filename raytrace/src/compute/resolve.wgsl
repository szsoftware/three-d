// Average of every pixel with alpha one, in rows padded for copying into a texture.

struct Resolve {
    // x: width, y: height, z: row pitch in pixels, w: vec4 per pixel in the sums
    size: vec4<u32>,
    // x: one over the number of samples
    scale: vec4<f32>,
}

@group(0) @binding(0) var<uniform> resolve_params: Resolve;
@group(0) @binding(1) var<storage, read> sums: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> averages: array<vec4<f32>>;

@compute @workgroup_size(8, 8)
fn resolve(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = resolve_params.size;
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let sum = sums[(id.y * size.x + id.x) * size.w];
    // three-d keeps the bottom row of the image first in the memory of a texture
    averages[(size.y - 1u - id.y) * size.z + id.x] = vec4<f32>(sum.rgb * resolve_params.scale.x, 1.0);
}
