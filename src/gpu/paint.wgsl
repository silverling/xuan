fn over(dst: vec4<f32>, src: vec4<f32>) -> vec4<f32> {
    let alpha = src.a + dst.a * (1.0 - src.a);
    if (alpha <= 0.0) {
        return vec4(0.0);
    }
    return vec4(((1.0 - src.a) * dst.a * dst.rgb + src.a * src.rgb) / alpha, alpha);
}

@compute @workgroup_size(8, 8)
fn paint_pixels(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    let i = id.y * size.x + id.x;
    let point = document_point((vec2<f32>(id.xy) + 0.5) / vec2<f32>(size));
    let amount = selected(point);
    let mode = config[7].x;
    let mask = config[6].w != 0.0;
    var p = rgba(i);
    if (mask) {
        p = vec4(vec3(byte_at(i)), 1.0);
    }
    var color = config[8];
    if (mode >= 2.0 && mode <= 3.0) {
        let delta = config[10].zw - config[10].xy;
        let length_sq = max(dot(delta, delta), 0.01);
        var t = dot(point - config[10].xy, delta) / length_sq;
        if (mode == 3.0) {
            t = length(point - config[10].xy) / sqrt(length_sq);
        }
        color = mix(color, config[9], clamp(t, 0.0, 1.0));
        color.a *= config[7].y;
    }
    color.a *= amount;
    if (mask) {
        var value = dot(color.rgb, vec3(0.3, 0.59, 0.11));
        var opacity = color.a;
        if (mode <= 1.0) {
            value = select(config[8].x, 0.0, mode == 1.0);
            opacity = amount;
        }
        if (mode == 4.0) {
            value = amount;
            opacity = 1.0;
        }
        result[i] = u32(floor(clamp(mix(p.r, value, opacity), 0.0, 1.0) * 255.0 + 0.5));
    } else {
        if (mode == 1.0) {
            p.a *= 1.0 - amount;
        } else {
            p = over(p, color);
        }
        result[i] = packed(p);
    }
}

@compute @workgroup_size(8, 8)
fn shape_pixels(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    var coverage = 0.0;
    for (var y = 0u; y < 2u; y++) {
        for (var x = 0u; x < 2u; x++) {
            let p = vec2<f32>(id.xy) + vec2(f32(x), f32(y)) * 0.5 + 0.25;
            var inside = true;
            if (config[1].x == 1.0) {
                let v = (p / config[0].xy - 0.5) * 2.0;
                inside = dot(v, v) <= 1.0;
            } else if (config[1].x == 2.0) {
                let center = clamp(p, vec2(config[1].y), config[0].xy - config[1].y);
                inside = length(p - center) <= config[1].y;
            }
            if (inside) {
                coverage += 0.25;
            }
        }
    }
    result[id.y * size.x + id.x] = packed(vec4(config[2].rgb, config[2].a * coverage));
}

@compute @workgroup_size(8, 8)
fn match_colors(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    let i = id.y * size.x + id.x;
    let p = rgba(i) * 255.0;
    let color = config[1];
    let matches =
        (color.a == 0.0 && p.a == 0.0) || all(abs(p - color) <= vec4(config[0].z + 0.0001));
    result[i] = select(0u, 255u, matches);
}

fn brush_source(uv: vec2<f32>) -> vec4<f32> {
    if (any(uv < vec2(0.0)) || any(uv >= vec2(1.0))) {
        return vec4(0.0);
    }
    let size = vec2<u32>(config[12].xy);
    let offset = bitcast<u32>(config[12].z);
    let p = uv * vec2<f32>(size) - 0.5;
    let low = vec2<i32>(floor(p));
    let f = fract(p);
    var sum = vec4(0.0);
    for (var k = 0u; k < 4u; k++) {
        let q = vec2<u32>(
            clamp(low + vec2<i32>(i32(k % 2u), i32(k / 2u)), vec2(0), vec2<i32>(size) - 1));
        let c = unpack4x8unorm(auxiliary[offset + q.y * size.x + q.x]);
        let weight = select(1.0 - f.x, f.x, k % 2u == 1u) * select(1.0 - f.y, f.y, k / 2u == 1u);
        sum += vec4(c.rgb * c.a, c.a) * weight;
    }
    return vec4(sum.rgb / max(sum.a, 0.000001), sum.a);
}

@compute @workgroup_size(8, 8)
fn stroke_pixels(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    let i = id.y * size.x + id.x;
    let mask = config[6].w != 0.0;
    var p = rgba(i);
    if (mask) {
        p = vec4(vec3(byte_at(i)), 1.0);
    }
    let point = document_point((vec2<f32>(id.xy) + config[0].zw + 0.5) / config[11].xy);
    let delta = config[10].zw - config[10].xy;
    let length_sq = dot(delta, delta);
    var t = 0.0;
    if (length_sq >= 0.0001) {
        t = clamp(dot(point - config[10].xy, delta) / length_sq, 0.0, 1.0);
    }
    let distance = length(point - config[10].xy - t * delta) / config[7].x;
    var amount = 0.0;
    if (distance <= 1.0) {
        var softness = 1.0;
        if (distance > config[7].y) {
            softness = (1.0 - distance) / max(1.0 - config[7].y, 0.001);
        }
        amount = softness * config[7].z * selected(point);
    }
    let mode = config[7].w;
    var color = config[8];
    if (amount > 0.0) {
        if (mask) {
            let value = select(dot(color.rgb, vec3(0.3, 0.59, 0.11)), 0.0, mode == 1.0);
            p.r = mix(p.r, value, amount);
        } else if (mode == 1.0) {
            p.a *= 1.0 - amount;
        } else if (mode == 0.0) {
            color.a *= amount;
            p = over(p, color);
        } else if (config[12].x != 0.0) {
            if (mode == 2.0 || mode == 5.0) {
                color = brush_source((point + config[11].zw) / config[12].xy);
                color.a *= amount;
                p = over(p, color);
            } else {
                let step = select(2.0, max(config[7].x, 2.0), mode == 4.0);
                color = vec4(0.0);
                var weight = 0.0;
                for (var y = -1; y <= 1; y++) {
                    for (var x = -1; x <= 1; x++) {
                        if (mode == 4.0 && x == 0 && y == 0) {
                            continue;
                        }
                        let sample =
                            brush_source((point + vec2(f32(x), f32(y)) * step) / config[12].xy);
                        color += vec4(sample.rgb * sample.a, sample.a);
                        weight += sample.a;
                    }
                }
                let average =
                    select(vec3(0.0), color.rgb / max(weight, 0.000001), weight >= 0.5 / 255.0);
                p = vec4(mix(p.rgb, average, amount), p.a);
            }
        }
    }
    if (mask) {
        result[i] = u32(floor(clamp(p.r, 0.0, 1.0) * 255.0 + 0.5));
    } else {
        result[i] = packed(p);
    }
}

@compute @workgroup_size(8, 8)
fn filter_selection(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(config[0].xy);
    if (any(id.xy >= size)) {
        return;
    }
    let index = id.y * size.x + id.x;
    let point = document_point((vec2<f32>(id.xy) + 0.5) / vec2<f32>(size));
    let amount = selected(point);
    let mask = config[6].w != 0.0;
    let old_size = vec2<u32>(config[7].xy);
    let padding = u32(config[7].z);
    let offset = bitcast<u32>(config[7].w);
    var old = vec4(0.0);
    if (all(id.xy >= vec2(padding)) && all(id.xy < vec2(padding) + old_size)) {
        let p = id.xy - vec2(padding);
        let i = p.y * old_size.x + p.x;
        if (mask) {
            old = vec4(f32((auxiliary[offset + i / 4u] >> ((i % 4u) * 8u)) & 255u) / 255.0);
        } else {
            old = unpack4x8unorm(auxiliary[offset + i]);
        }
    }
    if (mask) {
        result[index] = u32(floor(mix(old.r, byte_at(index), amount) * 255.0 + 0.5));
    } else {
        result[index] = packed(mix(old, rgba(index), amount));
    }
}
