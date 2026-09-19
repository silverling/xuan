
@compute @workgroup_size(8, 8)
fn adjust_pixels(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    var point = document_point((vec2<f32>(id.xy) + 0.5) / vec2<f32>(size));
    if (config[6].z != 0.0) {
        point = vec2<f32>(id.xy);
    }
    let i = id.y * size.x + id.x;
    var p = rgba(i);
    if (config[6].w != 0.0) {
        p = vec4(vec3(byte_at(i)), 1.0);
    }
    let value = clamp(adjust(p.rgb, point), vec3(0.0), vec3(1.0));
    let adjusted = vec4(mix(p.rgb, value, selected(point)), p.a);
    if (config[6].w != 0.0) {
        result[i] = u32(floor(adjusted.r * 255.0 + 0.5));
    } else {
        result[i] = packed(adjusted);
    }
}
