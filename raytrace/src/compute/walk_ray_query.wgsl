// Hardware ray queries in place of the hierarchies in storage buffers (EXPERIMENTAL_RAY_QUERY of wgpu, see compute.rs
// RayQueryScene): the instances in a top level structure whose custom data is their index in instances, the triangles
// of each mesh in a bottom level structure in the order of triangles, so a hit is the same Hit as of the software walk.
// Instances whose surfaces need the shader on some rays (cut-outs, and materials that let light through for shadow rays
// and the CONTACT rule) are in bottom level structures without the opaque flag: their hits come as candidates, which
// get the same tests as in walk_mesh. Hits on all other instances are taken by the hardware itself.
@group(0) @binding(10) var scene_structure: acceleration_structure;

// The closest hit before t_max, or in shadow mode every hit until nothing passes (result in transmittance)
fn walk(o: vec3<f32>, d: vec3<f32>, t_max_in: f32, shadow: bool) -> Hit {
    var hit = Hit(t_max_in, 0.0, 0.0, NONE, 0u);
    // A shadow ray ends at the first opaque surface; surfaces that let light through are never committed
    var flags = RAY_FLAG_NONE;
    if shadow {
        flags = RAY_FLAG_TERMINATE_ON_FIRST_HIT;
    }
    var query: ray_query;
    rayQueryInitialize(&query, scene_structure, RayDesc(flags, 0xffu, 0.0, t_max_in, o, d));
    while rayQueryProceed(&query) {
        let c = rayQueryGetCandidateIntersection(&query);
        if c.kind != RAY_QUERY_INTERSECTION_TRIANGLE {
            continue;
        }
        let index = c.instance_custom_data;
        if CONTACT && !shadow && index == skip_instance && c.t < skip_until {
            continue;
        }
        let info = instances[index].info;
        let ti = info.y + c.primitive_index;
        let tri = triangles[ti];
        // Textures for cut-outs (alpha of the base color) and for glass on shadow rays, with one lookup
        let raw = materials[info.z];
        let glass = shadow && raw.params.w > 0.0;
        var m = raw;
        if info.w == 2u || glass {
            m = material_at(raw, uv_at(tri, c.barycentrics.x, c.barycentrics.y), select(1u, 2u, glass));
        }
        if info.w == 2u && m.base_color.a < raw.flags.z {
            continue;
        }
        if shadow {
            let p0 = vertices[tri.x].position.xyz;
            let p1 = vertices[tri.y].position.xyz;
            let p2 = vertices[tri.z].position.xyz;
            if shadow_hit(index, m, p0, p1, p2, c.t) {
                rayQueryTerminate(&query);
            }
        } else {
            rayQueryConfirmIntersection(&query);
        }
    }
    let h = rayQueryGetCommittedIntersection(&query);
    if h.kind == RAY_QUERY_INTERSECTION_TRIANGLE {
        if shadow {
            // An opaque surface
            transmittance = vec3<f32>(0.0);
        } else {
            let index = h.instance_custom_data;
            hit = Hit(h.t, h.barycentrics.x, h.barycentrics.y, index, instances[index].info.y + h.primitive_index);
        }
    }
    return hit;
}
