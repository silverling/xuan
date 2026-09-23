@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output: texture_storage_2d<OUTPUT_FORMAT, write>;
@group(0) @binding(2) var<storage, read> config: array<vec4<f32>>;

// Match the CPU filter boundary: a straight-alpha, eight-bit backdrop.
fn pixel(position: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(source));
    return floor(clamp(textureLoad(source, clamp(position, vec2(0), size - 1), 0),
        vec4(0.0), vec4(1.0)) * 255.0 + 0.5) / 255.0;
}

fn premultiplied(position: vec2<i32>) -> vec4<f32> {
    let p = pixel(position);
    return vec4(p.rgb * p.a, p.a);
}

fn sample_premultiplied(point: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(source));
    if any(point < vec2(0.0)) || any(point >= size) { return vec4(0.0); }
    let low = vec2<i32>(floor(point - 0.5));
    let fraction = fract(point - 0.5);
    let a = mix(premultiplied(low), premultiplied(low + vec2(1, 0)), fraction.x);
    let b = mix(premultiplied(low + vec2(0, 1)), premultiplied(low + vec2(1, 1)), fraction.x);
    return mix(a, b, fraction.y);
}

fn noise(position: vec2<u32>, seed: u32) -> f32 {
    var value = position.x * 374761393u + position.y * 668265263u + seed;
    value = (value ^ (value >> 13u)) * 1274126177u;
    value ^= value >> 16u;
    return f32(value) / 4294967295.0 * 2.0 - 1.0;
}

@compute @workgroup_size(8, 8)
fn filter_layer(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(source);
    if any(id.xy >= size) { return; }
    let position = vec2<i32>(id.xy);
    let settings = config[0];
    let mode = u32(settings.x);
    var result = vec4(0.0);
    if mode <= 1u {
        let radius = i32(settings.y);
        let direction = select(vec2(0, 1), vec2(1, 0), mode == 0u);
        for (var k = -radius; k <= radius; k++) {
            let p = clamp(position + direction * k, vec2(0), vec2<i32>(size) - 1);
            var value = textureLoad(source, p, 0);
            if mode == 0u {
                // Gaussian Blur historically truncates premultiplied bytes.
                value = floor(premultiplied(p) * 255.0 + 0.00001) / 255.0;
            }
            result += value * config[u32(k + radius) + 1u].x;
        }
        if mode == 1u {
            result = floor(clamp(result, vec4(0.0), vec4(1.0)) * 255.0 + 0.5);
            result = vec4(select(result.rgb, floor(result.rgb * 255.0 / max(result.a, 1.0)), result.a > 0.0), result.a) / 255.0;
        }
    } else if mode == 2u {
        let steps = u32(settings.y);
        for (var i = 0u; i < steps; i++) {
            let offset = (f32(i) + 0.5) / f32(steps) - 0.5;
            result += sample_premultiplied(vec2<f32>(id.xy) + 0.5 + offset * settings.zw);
        }
        result = vec4(result.rgb / max(result.a, 0.000001), result.a / f32(steps));
    } else if mode == 3u {
        result = pixel(position);
        for (var c = 0u; c < 3u; c++) {
            result[c] += noise(id.xy, 3187u + select(c * 12345u, 0u, settings.z != 0.0)) * settings.y;
        }
    } else {
        let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size) * 2.0 - 1.0;
        let radius = dot(uv, uv);
        let k = 1.0 + settings.y * radius / 100.0;
        result = sample_premultiplied((uv * k + 1.0) * 0.5 * vec2<f32>(size));
        result = vec4(result.rgb / max(result.a, 0.000001) * (1.0 - settings.z * radius * 0.005), result.a);
    }
    textureStore(output, position, result);
}
