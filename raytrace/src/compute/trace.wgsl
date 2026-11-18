// Path tracer in a compute shader, the same algorithm as the CPU reference tracer (tracer.rs). One invocation adds the
// samples of one dispatch to one pixel of the tile.

const PI: f32 = 3.14159265358979;
const BIG: f32 = 1e30;
const NONE: u32 = 0xffffffffu;

struct Params {
    // x: 1 for a perspective camera, y: width, z: height, w: most bounces
    camera: vec4<u32>,
    camera_position: vec4<f32>,
    screen_to_direction: mat4x4<f32>,
    // Orthographic cameras: origin at uv (0, 0), its change along u and v, and the view direction
    ortho_origin: vec4<f32>,
    ortho_du: vec4<f32>,
    ortho_dv: vec4<f32>,
    ortho_direction: vec4<f32>,
    // x: seed, y: number of lights, z: environment (0 uniform, 1 sky), w: 1 if indirect light is clamped
    settings: vec4<u32>,
    // x: indirect clamp, yzw: luminance of a uniform environment
    clamp_and_uniform: vec4<f32>,
    // xyz: up, w: sky scale for the twilight
    sky_up: vec4<f32>,
    // xyz: towards the sun, w: cosine of the sun's angular radius
    sun_direction: vec4<f32>,
    // xyz: sun color times illuminance, w: illuminance
    sun_light: vec4<f32>,
    sun_radiance: vec4<f32>,
    // Perez coefficients, five per channel of xyY
    perez: array<vec4<f32>, 4>,
    zenith: vec4<f32>,
    norm: vec4<f32>,
    ground: vec4<f32>,
}

struct Tile {
    // x, y: first pixel, z, w: size
    rect: vec4<u32>,
    // x: index of the first sample, y: samples in this dispatch
    samples: vec4<u32>,
}

struct Node {
    min: vec3<f32>,
    first: u32,
    max: vec3<f32>,
    count: u32,
}

struct Vertex {
    position: vec4<f32>,
    // w: 1 if the mesh has vertex normals
    normal: vec4<f32>,
}

struct Instance {
    transform: mat4x4<f32>,
    inverse: mat4x4<f32>,
    // x: first node of the mesh hierarchy, y: first triangle of the mesh, z: material, w: 1 if not cut out
    info: vec4<u32>,
}

struct Material {
    // rgb: base color, a: alpha
    base_color: vec4<f32>,
    // metallic, roughness, specular, transmission
    params: vec4<f32>,
    // rgb: emitted luminance, a: index of refraction
    emission: vec4<f32>,
    // x: 1 if thin walled
    flags: vec4<f32>,
}

struct Light {
    // xyz: direction the light shines (w = 0) or position (w = 1)
    position: vec4<f32>,
    // rgb: color times illuminance or intensity
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<uniform> tile: Tile;
@group(0) @binding(2) var<storage, read> nodes: array<Node>;
@group(0) @binding(3) var<storage, read> triangles: array<vec4<u32>>;
@group(0) @binding(4) var<storage, read> vertices: array<Vertex>;
@group(0) @binding(5) var<storage, read> instances: array<Instance>;
@group(0) @binding(6) var<storage, read> materials: array<Material>;
@group(0) @binding(7) var<storage, read> lights: array<Light>;
@group(0) @binding(8) var<storage, read_write> sums: array<vec4<f32>>;

// ---------------------------------------------------------------------------------------------------------------------
// Random numbers: PCG with 32 bit state (no 64 bit integers in WGSL), one stream per pixel sample

var<private> rng: u32;

fn pcg_hash(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn next_f32() -> f32 {
    rng = rng * 747796405u + 2891336453u;
    let s = rng;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return f32(((w >> 22u) ^ w) >> 8u) * (1.0 / 16777216.0);
}

// ---------------------------------------------------------------------------------------------------------------------
// Ray traversal of the two level hierarchy

struct Hit {
    t: f32,
    u: f32,
    v: f32,
    // Index into instances, NONE without a hit
    instance: u32,
    // Index into triangles
    triangle: u32,
}

// What a shadow ray lets through, set by walk in shadow mode
var<private> transmittance: vec3<f32>;
// Direction of the shadow ray in world space, the hierarchies of the meshes see it in mesh coordinates
var<private> shadow_direction: vec3<f32>;

fn safe_inverse(d: vec3<f32>) -> vec3<f32> {
    let tiny = 1e-30;
    let x = select(d.x, tiny, abs(d.x) < tiny);
    let y = select(d.y, tiny, abs(d.y) < tiny);
    let z = select(d.z, tiny, abs(d.z) < tiny);
    return vec3<f32>(1.0 / x, 1.0 / y, 1.0 / z);
}

// Distance at which the ray enters the box before t_max, negative if it does not
fn slab(node: Node, o: vec3<f32>, inv: vec3<f32>, t_max: f32) -> f32 {
    let a = (node.min - o) * inv;
    let b = (node.max - o) * inv;
    let lo = min(a, b);
    let hi = max(a, b);
    let t0 = max(max(max(lo.x, lo.y), lo.z), 0.0);
    let t1 = min(min(min(hi.x, hi.y), hi.z), t_max);
    return select(-1.0, t0, t0 <= t1);
}

// Möller-Trumbore, (t, u, v) with t <= 0 if the triangle is not hit in front of the ray
fn intersect_triangle(o: vec3<f32>, d: vec3<f32>, p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>) -> vec3<f32> {
    let e1 = p1 - p0;
    let e2 = p2 - p0;
    let p = cross(d, e2);
    let det = dot(e1, p);
    if abs(det) < 1e-20 {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    let inv = 1.0 / det;
    let s = o - p0;
    let u = dot(s, p) * inv;
    if u < 0.0 || u > 1.0 {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    return vec3<f32>(dot(e2, q) * inv, u, v);
}

fn face_normal_in_world(instance: Instance, p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>) -> vec3<f32> {
    // Normals transform with the inverse transpose
    return normalize((transpose(instance.inverse) * vec4<f32>(cross(p1 - p0, p2 - p0), 0.0)).xyz);
}

// A triangle hit on a shadow ray: true if nothing passes any more
fn shadow_hit(instance: Instance, p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>) -> bool {
    let m = materials[instance.info.z];
    let transmission = m.params.w;
    if transmission <= 0.0 {
        transmittance = vec3<f32>(0.0);
        return true;
    }
    let cos_i = clamp(abs(dot(face_normal_in_world(instance, p0, p1, p2), shadow_direction)), 0.0, 1.0);
    let f = fresnel_dielectric(cos_i, m.emission.w);
    transmittance = transmittance * m.base_color.rgb * (transmission * (1.0 - f));
    return max(max(transmittance.x, transmittance.y), transmittance.z) <= 0.0;
}

// Traverses the hierarchy of one instance's mesh, the ray in mesh coordinates. Returns true to stop.
fn walk_mesh(index: u32, o: vec3<f32>, d: vec3<f32>, t_max: ptr<function, f32>, hit: ptr<function, Hit>, shadow: bool) -> bool {
    let instance = instances[index];
    let base = instance.info.x;
    let inv = safe_inverse(d);
    let root = nodes[base];
    if root.min.x > root.max.x || slab(root, o, inv, *t_max) < 0.0 {
        return false;
    }
    var stack: array<u32, 64>;
    var size = 0u;
    var current = base;
    loop {
        let node = nodes[current];
        var descend = false;
        if node.count > 0u {
            for (var i = node.first; i < node.first + node.count; i++) {
                let ti = instance.info.y + i;
                let tri = triangles[ti];
                let p0 = vertices[tri.x].position.xyz;
                let p1 = vertices[tri.y].position.xyz;
                let p2 = vertices[tri.z].position.xyz;
                let r = intersect_triangle(o, d, p0, p1, p2);
                if r.x > 0.0 && r.x < *t_max {
                    if shadow {
                        if shadow_hit(instance, p0, p1, p2) {
                            return true;
                        }
                    } else {
                        *t_max = r.x;
                        *hit = Hit(r.x, r.y, r.z, index, ti);
                    }
                }
            }
        } else {
            let a = base + node.first;
            let b = a + 1u;
            let ta = slab(nodes[a], o, inv, *t_max);
            let tb = slab(nodes[b], o, inv, *t_max);
            if ta >= 0.0 && tb >= 0.0 {
                if size < 64u {
                    stack[size] = select(a, b, ta <= tb);
                    size += 1u;
                }
                current = select(b, a, ta <= tb);
                descend = true;
            } else if ta >= 0.0 {
                current = a;
                descend = true;
            } else if tb >= 0.0 {
                current = b;
                descend = true;
            }
        }
        if !descend {
            // Next node from the stack that is still in reach
            var found = false;
            loop {
                if size == 0u {
                    break;
                }
                size -= 1u;
                let candidate = stack[size];
                if slab(nodes[candidate], o, inv, *t_max) >= 0.0 {
                    current = candidate;
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }
    }
    return false;
}

// The closest hit before t_max, or in shadow mode every hit until nothing passes (result in transmittance)
fn walk(o: vec3<f32>, d: vec3<f32>, t_max_in: f32, shadow: bool) -> Hit {
    var hit = Hit(t_max_in, 0.0, 0.0, NONE, 0u);
    var t_max = t_max_in;
    let inv = safe_inverse(d);
    // A scene without instances has an empty box, which the slab test can not tell from an infinite one
    let root = nodes[0];
    if root.min.x > root.max.x || slab(root, o, inv, t_max) < 0.0 {
        return hit;
    }
    var stack: array<u32, 32>;
    var size = 0u;
    var current = 0u;
    loop {
        let node = nodes[current];
        var descend = false;
        if node.count > 0u {
            for (var i = node.first; i < node.first + node.count; i++) {
                let instance = instances[i];
                if instance.info.w == 0u {
                    continue;
                }
                // The same t on the ray in mesh coordinates, the direction is not normalised again
                let lo = (instance.inverse * vec4<f32>(o, 1.0)).xyz;
                let ld = (instance.inverse * vec4<f32>(d, 0.0)).xyz;
                if walk_mesh(i, lo, ld, &t_max, &hit, shadow) {
                    return hit;
                }
            }
        } else {
            let a = node.first;
            let b = a + 1u;
            let ta = slab(nodes[a], o, inv, t_max);
            let tb = slab(nodes[b], o, inv, t_max);
            if ta >= 0.0 && tb >= 0.0 {
                if size < 32u {
                    stack[size] = select(a, b, ta <= tb);
                    size += 1u;
                }
                current = select(b, a, ta <= tb);
                descend = true;
            } else if ta >= 0.0 {
                current = a;
                descend = true;
            } else if tb >= 0.0 {
                current = b;
                descend = true;
            }
        }
        if !descend {
            var found = false;
            loop {
                if size == 0u {
                    break;
                }
                size -= 1u;
                let candidate = stack[size];
                if slab(nodes[candidate], o, inv, t_max) >= 0.0 {
                    current = candidate;
                    found = true;
                    break;
                }
            }
            if !found {
                return hit;
            }
        }
    }
    return hit;
}

// ---------------------------------------------------------------------------------------------------------------------
// Surfaces

struct Surface {
    position: vec3<f32>,
    // Face normal on the side the ray came from
    geometric_normal: vec3<f32>,
    // Interpolated normal on the same side
    shading_normal: vec3<f32>,
    front_face: bool,
    material: u32,
}

fn surface_at(hit: Hit, d: vec3<f32>) -> Surface {
    let instance = instances[hit.instance];
    let tri = triangles[hit.triangle];
    let v0 = vertices[tri.x];
    let v1 = vertices[tri.y];
    let v2 = vertices[tri.z];
    let w = 1.0 - hit.u - hit.v;
    let local = v0.position.xyz * w + v1.position.xyz * hit.u + v2.position.xyz * hit.v;
    let position = (instance.transform * vec4<f32>(local, 1.0)).xyz;
    let normal_matrix = transpose(instance.inverse);
    let face = normalize((normal_matrix * vec4<f32>(cross(v1.position.xyz - v0.position.xyz, v2.position.xyz - v0.position.xyz), 0.0)).xyz);
    var shading = face;
    if v0.normal.w > 0.0 {
        let n = v0.normal.xyz * w + v1.normal.xyz * hit.u + v2.normal.xyz * hit.v;
        if dot(n, n) > 0.0 {
            shading = normalize((normal_matrix * vec4<f32>(n, 0.0)).xyz);
            // Vertex normals of the other side keep the face orientation
            if dot(shading, face) < 0.0 {
                shading = -shading;
            }
        }
    }
    let front_face = dot(face, d) < 0.0;
    let side = select(-1.0, 1.0, front_face);
    return Surface(position, face * side, shading * side, front_face, instance.info.z);
}

fn offset_point(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let scale = max(max(max(1.0, abs(p.x)), abs(p.y)), abs(p.z));
    return p + n * (1e-4 * scale);
}

// ---------------------------------------------------------------------------------------------------------------------
// Sampling helpers (sampling.rs)

fn orthonormal_t(n: vec3<f32>) -> vec3<f32> {
    let sign = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (sign + n.z);
    let b = n.x * n.y * a;
    return vec3<f32>(1.0 + sign * n.x * n.x * a, sign * b, -sign * n.x);
}

fn orthonormal_b(n: vec3<f32>) -> vec3<f32> {
    let sign = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (sign + n.z);
    let b = n.x * n.y * a;
    return vec3<f32>(b, sign + n.y * n.y * a, -n.y);
}

fn cosine_hemisphere(u1: f32, u2: f32) -> vec3<f32> {
    let r = sqrt(u1);
    let phi = 2.0 * PI * u2;
    return vec3<f32>(r * cos(phi), r * sin(phi), sqrt(max(1.0 - u1, 0.0)));
}

fn uniform_cone(u1: f32, u2: f32, cos_max: f32) -> vec3<f32> {
    let cos_theta = 1.0 - u1 * (1.0 - cos_max);
    let sin_theta = sqrt(max(1.0 - cos_theta * cos_theta, 0.0));
    let phi = 2.0 * PI * u2;
    return vec3<f32>(sin_theta * cos(phi), sin_theta * sin(phi), cos_theta);
}

fn ggx_d(n_dot_h: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn ggx_g1(n_dot_x: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    return 2.0 * n_dot_x / (n_dot_x + sqrt(a2 + (1.0 - a2) * n_dot_x * n_dot_x));
}

fn ggx_visibility(n_dot_l: f32, n_dot_v: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let gv = n_dot_l * sqrt(n_dot_v * n_dot_v * (1.0 - a2) + a2);
    let gl = n_dot_v * sqrt(n_dot_l * n_dot_l * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-12);
}

fn ggx_sample_visible_normal(v: vec3<f32>, alpha: f32, u1: f32, u2: f32) -> vec3<f32> {
    let vh = normalize(vec3<f32>(alpha * v.x, alpha * v.y, v.z));
    let len_sq = vh.x * vh.x + vh.y * vh.y;
    var t1 = vec3<f32>(1.0, 0.0, 0.0);
    if len_sq > 0.0 {
        t1 = vec3<f32>(-vh.y, vh.x, 0.0) / sqrt(len_sq);
    }
    let t2 = cross(vh, t1);
    let r = sqrt(u1);
    let phi = 2.0 * PI * u2;
    let p1 = r * cos(phi);
    let s = 0.5 * (1.0 + vh.z);
    let p2 = (1.0 - s) * sqrt(max(1.0 - p1 * p1, 0.0)) + s * r * sin(phi);
    let nh = p1 * t1 + p2 * t2 + sqrt(max(1.0 - p1 * p1 - p2 * p2, 0.0)) * vh;
    return normalize(vec3<f32>(alpha * nh.x, alpha * nh.y, max(nh.z, 1e-6)));
}

fn fresnel_schlick(f0: vec3<f32>, f90: f32, cos_theta: f32) -> vec3<f32> {
    let m = clamp(1.0 - cos_theta, 0.0, 1.0);
    let m5 = m * m * m * m * m;
    return f0 + (vec3<f32>(f90) - f0) * m5;
}

fn fresnel_dielectric(cos_in: f32, eta: f32) -> f32 {
    let cos_i = clamp(cos_in, 0.0, 1.0);
    let sin_t2 = (1.0 - cos_i * cos_i) / (eta * eta);
    if sin_t2 >= 1.0 {
        return 1.0;
    }
    let cos_t = sqrt(1.0 - sin_t2);
    let rs = (cos_i - eta * cos_t) / (cos_i + eta * cos_t);
    let rp = (eta * cos_i - cos_t) / (eta * cos_i + cos_t);
    return 0.5 * (rs * rs + rp * rp);
}

fn luminance(c: vec3<f32>) -> f32 {
    return 0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z;
}

fn reflect_about(v: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    return 2.0 * dot(v, n) * n - v;
}

// Direction after refraction for eta = n_from / n_to, w = 0 on total internal reflection
fn refract_through(v: vec3<f32>, n: vec3<f32>, eta: f32) -> vec4<f32> {
    let cos_i = dot(v, n);
    let sin_t2 = eta * eta * (1.0 - cos_i * cos_i);
    if sin_t2 >= 1.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(normalize(-v * eta + n * (eta * cos_i - sqrt(1.0 - sin_t2))), 1.0);
}

// ---------------------------------------------------------------------------------------------------------------------
// Material model (tracer.rs)

fn material_alpha(m: Material) -> f32 {
    let r = max(m.params.y, 0.03);
    return r * r;
}

fn specular_f0(m: Material) -> vec3<f32> {
    let dielectric = 0.04 * m.params.z;
    return vec3<f32>(dielectric) * (1.0 - m.params.x) + m.base_color.rgb * m.params.x;
}

fn specular_f90(m: Material) -> f32 {
    return clamp(m.params.z, 0.0, 1.0) * (1.0 - m.params.x) + m.params.x;
}

fn specular_probability(m: Material, n_dot_v: f32) -> f32 {
    let f = luminance(fresnel_schlick(specular_f0(m), specular_f90(m), n_dot_v));
    let d = luminance(m.base_color.rgb) * (1.0 - m.params.x);
    if f <= 0.0 {
        return 0.0;
    }
    if d <= 0.0 {
        return 1.0;
    }
    return clamp(f / (f + d), 0.1, 0.9);
}

fn evaluate_material(m: Material, v: vec3<f32>, l: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let n_dot_l = dot(n, l);
    let n_dot_v = dot(n, v);
    if n_dot_l <= 0.0 || n_dot_v <= 0.0 {
        return vec3<f32>(0.0);
    }
    let h = normalize(v + l);
    let n_dot_h = max(dot(n, h), 0.0);
    let v_dot_h = max(dot(v, h), 0.0);
    let a = material_alpha(m);
    let f0 = specular_f0(m);
    let f90 = specular_f90(m);
    let specular = fresnel_schlick(f0, f90, v_dot_h) * (ggx_d(n_dot_h, a) * ggx_visibility(n_dot_l, n_dot_v, a));
    let kd = (vec3<f32>(1.0) - fresnel_schlick(f0, f90, n_dot_v)) * (1.0 - m.params.x);
    return kd * m.base_color.rgb / PI + specular;
}

fn material_pdf(m: Material, v: vec3<f32>, l: vec3<f32>, n: vec3<f32>) -> f32 {
    let n_dot_l = dot(n, l);
    let n_dot_v = dot(n, v);
    if n_dot_l <= 0.0 || n_dot_v <= 0.0 {
        return 0.0;
    }
    let p_spec = specular_probability(m, n_dot_v);
    let a = material_alpha(m);
    let h = normalize(v + l);
    let spec_pdf = ggx_g1(n_dot_v, a) * ggx_d(max(dot(n, h), 0.0), a) / (4.0 * n_dot_v);
    return p_spec * spec_pdf + (1.0 - p_spec) * n_dot_l / PI;
}

struct MaterialSample {
    direction: vec3<f32>,
    // f cos / pdf
    weight: vec3<f32>,
    valid: bool,
}

fn sample_material(m: Material, v: vec3<f32>, n: vec3<f32>) -> MaterialSample {
    var result = MaterialSample(vec3<f32>(0.0), vec3<f32>(0.0), false);
    let n_dot_v = dot(n, v);
    if n_dot_v <= 0.0 {
        return result;
    }
    let t = orthonormal_t(n);
    let b = orthonormal_b(n);
    let p_spec = specular_probability(m, n_dot_v);
    let u0 = next_f32();
    let u1 = next_f32();
    let u2 = next_f32();
    var l: vec3<f32>;
    if u0 < p_spec {
        let v_local = vec3<f32>(dot(v, t), dot(v, b), n_dot_v);
        let hl = ggx_sample_visible_normal(v_local, material_alpha(m), u1, u2);
        let h = t * hl.x + b * hl.y + n * hl.z;
        l = reflect_about(v, h);
    } else {
        let c = cosine_hemisphere(u1, u2);
        l = t * c.x + b * c.y + n * c.z;
    }
    let n_dot_l = dot(n, l);
    if n_dot_l <= 0.0 {
        return result;
    }
    let pdf = material_pdf(m, v, l, n);
    if pdf <= 0.0 {
        return result;
    }
    result.direction = l;
    result.weight = evaluate_material(m, v, l, n) * (n_dot_l / pdf);
    result.valid = true;
    return result;
}

// ---------------------------------------------------------------------------------------------------------------------
// Sky and lights (sky.rs)

fn perez(c: u32, i: u32) -> f32 {
    let k = c * 5u + i;
    return params.perez[k / 4u][k % 4u];
}

fn xyy_to_linear_srgb(lum: f32, x: f32, y: f32) -> vec3<f32> {
    if y <= 0.0 || lum <= 0.0 {
        return vec3<f32>(0.0);
    }
    let cx = x / y * lum;
    let cz = (1.0 - x - y) / y * lum;
    let cy = lum;
    return max(vec3<f32>(
        3.2406 * cx - 1.5372 * cy - 0.4986 * cz,
        -0.9689 * cx + 1.8758 * cy + 0.0415 * cz,
        0.0557 * cx - 0.2040 * cy + 1.0570 * cz,
    ), vec3<f32>(0.0));
}

fn sky_radiance(direction: vec3<f32>) -> vec3<f32> {
    let d = normalize(direction);
    let cos_theta = dot(d, params.sky_up.xyz);
    if cos_theta <= 0.0 {
        return params.ground.xyz;
    }
    let ct = max(cos_theta, 0.01);
    let cos_gamma = clamp(dot(d, params.sun_direction.xyz), -1.0, 1.0);
    let gamma = acos(cos_gamma);
    var xyy = vec3<f32>(0.0);
    for (var c = 0u; c < 3u; c++) {
        let f = (1.0 + perez(c, 0u) * exp(perez(c, 1u) / ct))
            * (1.0 + perez(c, 2u) * exp(perez(c, 3u) * gamma) + perez(c, 4u) * cos_gamma * cos_gamma);
        xyy[c] = params.zenith[c] * f / params.norm[c];
    }
    return xyy_to_linear_srgb(max(xyy.x, 0.0) * params.sky_up.w, xyy.y, xyy.z);
}

fn environment(d: vec3<f32>, include_sun: bool) -> vec3<f32> {
    if params.settings.z == 0u {
        return params.clamp_and_uniform.yzw;
    }
    var l = sky_radiance(d);
    if include_sun && dot(d, params.sun_direction.xyz) >= params.sun_direction.w {
        l += params.sun_radiance.xyz;
    }
    return l;
}

// Reflected luminance for illuminance e from direction l
fn light_from(p: vec3<f32>, v: vec3<f32>, n: vec3<f32>, ng: vec3<f32>, m: Material, l: vec3<f32>, distance: f32, e: vec3<f32>) -> vec3<f32> {
    let c = dot(n, l);
    if c <= 0.0 || dot(ng, l) <= 0.0 {
        return vec3<f32>(0.0);
    }
    let f = evaluate_material(m, v, l, n);
    if all(f == vec3<f32>(0.0)) {
        return f;
    }
    transmittance = vec3<f32>(1.0);
    shadow_direction = l;
    _ = walk(p, l, distance, true);
    return f * e * transmittance * c;
}

fn direct_light(p: vec3<f32>, v: vec3<f32>, n: vec3<f32>, ng: vec3<f32>, m: Material) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    if params.settings.z == 1u && params.sun_light.w > 0.0 {
        let s = params.sun_direction.xyz;
        let t = orthonormal_t(s);
        let b = orthonormal_b(s);
        let u1 = next_f32();
        let u2 = next_f32();
        let c = uniform_cone(u1, u2, params.sun_direction.w);
        let l = normalize(t * c.x + b * c.y + s * c.z);
        result += light_from(p, v, n, ng, m, l, BIG, params.sun_light.xyz);
    }
    for (var i = 0u; i < params.settings.y; i++) {
        let light = lights[i];
        if light.position.w == 0.0 {
            result += light_from(p, v, n, ng, m, -normalize(light.position.xyz), BIG, light.color.rgb);
        } else {
            let to_light = light.position.xyz - p;
            let d2 = dot(to_light, to_light);
            if d2 > 0.0 {
                let d = sqrt(d2);
                result += light_from(p, v, n, ng, m, to_light / d, d * (1.0 - 1e-4), light.color.rgb / d2);
            }
        }
    }
    return result;
}

// ---------------------------------------------------------------------------------------------------------------------
// Paths

fn clamped(bounce: u32, contribution: vec3<f32>) -> vec3<f32> {
    if params.settings.w == 1u && bounce > 0u {
        let m = max(max(contribution.x, contribution.y), contribution.z);
        let limit = params.clamp_and_uniform.x;
        if m > limit {
            return contribution * (limit / m);
        }
    }
    return contribution;
}

fn radiance(origin_in: vec3<f32>, direction_in: vec3<f32>) -> vec3<f32> {
    var origin = origin_in;
    var direction = direction_in;
    var result = vec3<f32>(0.0);
    var throughput = vec3<f32>(1.0);
    // Lights that can not be hit by chance (the sun disc) are seen directly only after camera rays and mirrors
    var last_was_delta = true;
    var bounce = 0u;
    loop {
        let hit = walk(origin, direction, BIG, false);
        if hit.instance == NONE {
            result += clamped(bounce, throughput * environment(direction, last_was_delta));
            break;
        }
        let s = surface_at(hit, direction);
        let m = materials[s.material];
        result += clamped(bounce, throughput * m.emission.rgb);
        if bounce >= params.camera.w {
            break;
        }
        let v = -direction;
        let ng = s.geometric_normal;
        var n = s.shading_normal;
        if dot(n, v) <= 0.0 {
            n = ng;
        }

        let transmission = m.params.w;
        let thin_walled = m.flags.x > 0.0;
        if transmission > 0.0 && next_f32() < transmission {
            // Smooth glass: reflect or pass, chosen by the Fresnel term
            let ior = m.emission.w;
            let eta = select(1.0 / ior, ior, thin_walled || s.front_face);
            let cos_i = clamp(dot(n, v), 0.0, 1.0);
            if next_f32() < fresnel_dielectric(cos_i, eta) {
                direction = reflect_about(v, n);
                origin = offset_point(s.position, ng);
            } else {
                throughput = throughput * m.base_color.rgb;
                if !thin_walled {
                    let r = refract_through(v, n, 1.0 / eta);
                    direction = select(reflect_about(v, n), r.xyz, r.w > 0.0);
                }
                origin = offset_point(s.position, -ng);
            }
            last_was_delta = true;
        } else {
            // Diffuse and glossy reflection: light sources directly, then a new direction from the material
            let p = offset_point(s.position, ng);
            result += clamped(bounce, throughput * direct_light(p, v, n, ng, m));
            let next = sample_material(m, v, n);
            if !next.valid || dot(next.direction, ng) <= 0.0 {
                break;
            }
            throughput = throughput * next.weight;
            direction = next.direction;
            origin = p;
            last_was_delta = false;
        }

        bounce += 1u;
        if bounce >= 3u {
            // Russian roulette ends dark paths early without changing the expected value
            let q = clamp(max(max(throughput.x, throughput.y), throughput.z), 0.05, 0.95);
            if next_f32() > q {
                break;
            }
            throughput = throughput / q;
        }
    }
    return result;
}

fn camera_ray_origin(u: f32, v: f32) -> vec3<f32> {
    if params.camera.x == 1u {
        return params.camera_position.xyz;
    }
    return params.ortho_origin.xyz + u * params.ortho_du.xyz + v * params.ortho_dv.xyz;
}

fn camera_ray_direction(u: f32, v: f32) -> vec3<f32> {
    if params.camera.x == 1u {
        return normalize((params.screen_to_direction * vec4<f32>(2.0 * u - 1.0, 2.0 * v - 1.0, 0.0, 1.0)).xyz);
    }
    return params.ortho_direction.xyz;
}

@compute @workgroup_size(8, 8)
fn trace(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= tile.rect.z || id.y >= tile.rect.w {
        return;
    }
    let x = tile.rect.x + id.x;
    let y = tile.rect.y + id.y;
    let width = params.camera.y;
    let height = params.camera.z;
    var sum = vec3<f32>(0.0);
    for (var s = 0u; s < tile.samples.y; s++) {
        let sample = tile.samples.x + s;
        rng = pcg_hash(params.settings.x ^ pcg_hash(x ^ pcg_hash(y ^ pcg_hash(sample))));
        let u = (f32(x) + next_f32()) / f32(width);
        let v = 1.0 - (f32(y) + next_f32()) / f32(height);
        let l = radiance(camera_ray_origin(u, v), camera_ray_direction(u, v));
        // A broken sample must not spoil the pixel for good (comparisons with NaN are false)
        if all(abs(l) < vec3<f32>(BIG)) {
            sum += l;
        }
    }
    let i = y * width + x;
    sums[i] = sums[i] + vec4<f32>(sum, 0.0);
}
