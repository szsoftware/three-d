// The à-trous denoiser of denoise.rs on the GPU, in four kinds of passes: prepare, guide, one atrous pass per
// iteration, and finish, which writes the denoised averages in rows padded for copying into a texture.

struct Params {
    // x: width, y: height, z: row pitch of the output in pixels
    size: vec4<u32>,
    // x: step of this atrous pass, y: number of samples, z: 1 to estimate the variance from the neighbours
    info: vec4<u32>,
    // x: sigma of the luminance, y: of the normal, z: of the distance
    sigmas: vec4<f32>,
}

const MIN_ALBEDO: f32 = 0.02;

@group(0) @binding(0) var<uniform> params: Params;

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn inside(id: vec3<u32>) -> bool {
    return id.x < params.size.x && id.y < params.size.y;
}

fn index(x: i32, y: i32) -> u32 {
    return u32(y) * params.size.x + u32(x);
}

fn in_image(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && x < i32(params.size.x) && y < i32(params.size.y);
}

// ---------------------------------------------------------------------------------------------------------------------
// prepare: averages from the sums (three vec4 per pixel), light per surface and its variance

@group(0) @binding(1) var<storage, read> sums: array<vec4<f32>>;
// rgb: color divided by albedo, a: variance of its luminance
@group(0) @binding(2) var<storage, read_write> light_out: array<vec4<f32>>;
// xyz: normal, w: distance
@group(0) @binding(3) var<storage, read_write> normal_distance_out: array<vec4<f32>>;
// xyz: albedo, w: change of distance to the neighbours
@group(0) @binding(4) var<storage, read_write> albedo_gradient_out: array<vec4<f32>>;

@compute @workgroup_size(8, 8)
fn prepare(@builtin(global_invocation_id) id: vec3<u32>) {
    if !inside(id) {
        return;
    }
    let i = id.y * params.size.x + id.x;
    let n = f32(max(params.info.y, 1u));
    let s0 = sums[3u * i];
    let s1 = sums[3u * i + 1u];
    let s2 = sums[3u * i + 2u];
    let color = s0.rgb / n;
    let albedo = max(s1.rgb / n, vec3<f32>(MIN_ALBEDO));
    var normal = s2.xyz / n;
    if dot(normal, normal) > 0.0 {
        normal = normalize(normal);
    }
    let lum = luminance(color);
    var variance = 0.0;
    if n > 1.0 {
        variance = max(s0.a / n - lum * lum, 0.0) / (n - 1.0);
    }
    variance = variance / pow(max(luminance(albedo), MIN_ALBEDO), 2.0);
    light_out[i] = vec4<f32>(color / albedo, variance);
    normal_distance_out[i] = vec4<f32>(normal, s1.a / n);
    albedo_gradient_out[i] = vec4<f32>(albedo, 0.0);
}

// ---------------------------------------------------------------------------------------------------------------------
// guide: the change of distance and, with few samples, the variance from the neighbours on the same surface

@group(0) @binding(1) var<storage, read> light_in: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> normal_distance: array<vec4<f32>>;
// Only this pixel's own entry is written
@group(0) @binding(3) var<storage, read_write> albedo_gradient: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> light_guided: array<vec4<f32>>;

fn one_axis(p: f32, x: i32, y: i32, dx: i32, dy: i32) -> f32 {
    let a = in_image(x - dx, y - dy);
    let b = in_image(x + dx, y + dy);
    var da = 0.0;
    var db = 0.0;
    if a {
        da = abs(p - normal_distance[index(x - dx, y - dy)].w);
    }
    if b {
        db = abs(normal_distance[index(x + dx, y + dy)].w - p);
    }
    if a && b {
        return min(da, db);
    }
    return max(da, db);
}

@compute @workgroup_size(8, 8)
fn guide(@builtin(global_invocation_id) id: vec3<u32>) {
    if !inside(id) {
        return;
    }
    let x = i32(id.x);
    let y = i32(id.y);
    let i = index(x, y);
    let p = normal_distance[i];
    let gradient = max(one_axis(p.w, x, y, 1, 0), one_axis(p.w, x, y, 0, 1));
    albedo_gradient[i] = vec4<f32>(albedo_gradient[i].rgb, gradient);
    var light = light_in[i];
    if params.info.z == 1u {
        var sum = 0.0;
        var sum2 = 0.0;
        var count = 0.0;
        for (var qy = y - 2; qy <= y + 2; qy++) {
            for (var qx = x - 2; qx <= x + 2; qx++) {
                if !in_image(qx, qy) {
                    continue;
                }
                let q = index(qx, qy);
                let nq = normal_distance[q];
                if dot(p.xyz, nq.xyz) < 0.9 || abs(p.w - nq.w) > 0.05 * max(p.w, 1e-6) {
                    continue;
                }
                let l = luminance(light_in[q].rgb);
                sum += l;
                sum2 += l * l;
                count += 1.0;
            }
        }
        let mean = sum / count;
        light.a = max(sum2 / count - mean * mean, 0.0) / f32(max(params.info.y, 1u));
    }
    light_guided[i] = light;
}

// ---------------------------------------------------------------------------------------------------------------------
// atrous: one pass of the edge avoiding filter

@group(0) @binding(1) var<storage, read> atrous_in: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> atrous_normal_distance: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> atrous_albedo_gradient: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> atrous_out: array<vec4<f32>>;

const KERNEL = array<f32, 5>(1.0 / 16.0, 1.0 / 4.0, 3.0 / 8.0, 1.0 / 4.0, 1.0 / 16.0);
const BLUR = array<f32, 3>(0.25, 0.5, 0.25);

@compute @workgroup_size(8, 8)
fn atrous(@builtin(global_invocation_id) id: vec3<u32>) {
    if !inside(id) {
        return;
    }
    let x = i32(id.x);
    let y = i32(id.y);
    let i = index(x, y);
    let step = i32(params.info.x);
    // The variance blurred over 3 × 3
    var blurred = 0.0;
    var blur_weight = 0.0;
    for (var j = 0; j < 3; j++) {
        for (var k = 0; k < 3; k++) {
            if in_image(x + k - 1, y + j - 1) {
                let w = BLUR[j] * BLUR[k];
                blurred += atrous_in[index(x + k - 1, y + j - 1)].a * w;
                blur_weight += w;
            }
        }
    }
    blurred = blurred / blur_weight;
    let center = atrous_in[i];
    let lp = luminance(center.rgb);
    let p = atrous_normal_distance[i];
    let gp = atrous_albedo_gradient[i].w;
    let sigma_l = params.sigmas.x * sqrt(max(blurred, 0.0)) + 1e-10;
    var sum_w = 0.0;
    var sum_l = vec3<f32>(0.0);
    var sum_v = 0.0;
    for (var j = 0; j < 5; j++) {
        let qy = y + (j - 2) * step;
        for (var k = 0; k < 5; k++) {
            let qx = x + (k - 2) * step;
            if !in_image(qx, qy) {
                continue;
            }
            let q = index(qx, qy);
            let value = atrous_in[q];
            var w = KERNEL[j] * KERNEL[k];
            if q != i {
                let nq = atrous_normal_distance[q];
                let wn = pow(max(dot(p.xyz, nq.xyz), 0.0), params.sigmas.y);
                let offset = sqrt(f32((k - 2) * (k - 2) + (j - 2) * (j - 2))) * f32(step);
                let wz = exp(-abs(p.w - nq.w) / (params.sigmas.z * gp * offset + 1e-4 * p.w + 1e-6));
                let wl = exp(-abs(lp - luminance(value.rgb)) / sigma_l);
                w = w * wn * wz * wl;
            }
            sum_w += w;
            sum_l += value.rgb * w;
            sum_v += value.a * w * w;
        }
    }
    atrous_out[i] = vec4<f32>(sum_l / sum_w, sum_v / (sum_w * sum_w));
}

// ---------------------------------------------------------------------------------------------------------------------
// finish: the albedo multiplied back, in rows padded for a texture copy

@group(0) @binding(1) var<storage, read> finish_light: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> finish_albedo_gradient: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> averages: array<vec4<f32>>;

@compute @workgroup_size(8, 8)
fn finish(@builtin(global_invocation_id) id: vec3<u32>) {
    if !inside(id) {
        return;
    }
    let i = id.y * params.size.x + id.x;
    var color = vec3<f32>(0.0);
    if params.info.y > 0u {
        color = finish_light[i].rgb * finish_albedo_gradient[i].rgb;
    }
    // three-d keeps the bottom row of the image first in the memory of a texture
    averages[(params.size.y - 1u - id.y) * params.size.z + id.x] = vec4<f32>(color, 1.0);
}
