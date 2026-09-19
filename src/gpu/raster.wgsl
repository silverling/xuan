// Format: 0 straight RGBA8 -> premultiplied float; 1 mask; 2 camera RGB32F.
// config[1].y selects the historical integer premultiplication of Gaussian Blur.
@compute @workgroup_size(8, 8)
fn decode_pixels(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    let i = id.y * size.x + id.x;
    var p = vec4(0.0);
    if (config[1].x == 0.0) {
        p = rgba(i);
        p = vec4(p.rgb * p.a, p.a);
        if (config[1].y != 0.0) {
            p = floor(p * 255.0 + 0.00001) / 255.0;
        }
    } else if (config[1].x == 1.0) {
        p = vec4(byte_at(i));
    } else {
        p = vec4(bitcast<f32>(input[i * 3u]), bitcast<f32>(input[i * 3u + 1u]),
                 bitcast<f32>(input[i * 3u + 2u]), 1.0);
    }
    store_float(i, p);
}

@compute @workgroup_size(8, 8)
fn encode_pixels(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    let i = id.y * size.x + id.x;
    var p = load_float(i);
    if (config[1].x == 0.0) {
        if (config[1].y != 0.0) {
            p = floor(clamp(p, vec4(0.0), vec4(1.0)) * 255.0 + 0.5);
            p = vec4(select(p.rgb, floor(p.rgb * 255.0 / max(p.a, 1.0)), p.a > 0.0), p.a) / 255.0;
        } else {
            let alpha = clamp(p.a, 0.0, 1.0);
            p = vec4(p.rgb / max(alpha, 0.00001), alpha);
        }
        result[i] = packed(p);
    } else if (config[1].x == 1.0) {
        result[i] = u32(floor(clamp(p.x, 0.0, 1.0) * 255.0 + 0.5));
    } else {
        result[i * 3u] = bitcast<u32>(p.r);
        result[i * 3u + 1u] = bitcast<u32>(p.g);
        result[i * 3u + 2u] = bitcast<u32>(p.b);
    }
}

@compute @workgroup_size(8, 8)
fn gaussian(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    let radius = i32(config[1].x);
    let direction = vec2<i32>(config[1].yz);
    var sum = vec4(0.0);
    for (var k = -radius; k <= radius; k++) {
        let p = vec2<u32>(clamp(vec2<i32>(id.xy) + direction * k, vec2(0), vec2<i32>(size) - 1));
        sum += load_float(p.y * size.x + p.x) * config[u32(k + radius) + 2u].x;
    }
    store_float(id.y * size.x + id.x, sum);
}

@compute @workgroup_size(8, 8)
fn resample(@builtin(global_invocation_id) id: vec3<u32>) {
    let source = vec2<u32>(config[0].xy);
    let destination = vec2<u32>(config[0].zw);
    if (any(id.xy >= destination)) {
        return;
    }
    let horizontal = config[1].x != 0.0;
    let axis = select(id.y, id.x, horizontal);
    let header = config[axis + 2u];
    var sum = vec4(0.0);
    for (var k = 0u; k < u32(header.y); k++) {
        let p = select(vec2(id.x, u32(header.x) + k), vec2(u32(header.x) + k, id.y), horizontal);
        sum += load_float(p.y * source.x + p.x) * config[u32(header.z) + k].x;
    }
    // image::resize clamps only the final horizontal pass, including float images.
    if (horizontal) {
        sum = clamp(sum, vec4(0.0), vec4(1.0));
    }
    store_float(id.y * destination.x + id.x, sum);
}

fn sampled(uv: vec2<f32>, size: vec2<u32>) -> vec4<f32> {
    if (any(uv < vec2(0.0)) || any(uv >= vec2(1.0))) {
        return vec4(0.0);
    }
    let pos = uv * vec2<f32>(size) - 0.5;
    let low = vec2<i32>(floor(pos));
    let f = fract(pos);
    var sum = vec4(0.0);
    for (var k = 0u; k < 4u; k++) {
        let p = vec2<u32>(
            clamp(low + vec2<i32>(i32(k % 2u), i32(k / 2u)), vec2(0), vec2<i32>(size) - 1));
        let c = rgba(p.y * size.x + p.x);
        let weight = select(1.0 - f.x, f.x, k % 2u == 1u) * select(1.0 - f.y, f.y, k / 2u == 1u);
        sum += vec4(c.rgb * c.a, c.a) * weight;
    }
    return vec4(sum.rgb / max(sum.a, 0.000001), sum.a);
}

@compute @workgroup_size(8, 8)
fn lens(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size) * 2.0 - 1.0;
    let radius = dot(uv, uv);
    let k = 1.0 + config[1].x * radius / 100.0;
    let p = sampled((uv * k + 1.0) * 0.5, size);
    result[id.y * size.x + id.x] = packed(vec4(p.rgb * (1.0 - config[1].y * radius * 0.005), p.a));
}
