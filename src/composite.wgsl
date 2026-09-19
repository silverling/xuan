@group(0) @binding(0) var previous: texture_2d<f32>;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var coverage: texture_2d<f32>;
@group(0) @binding(3) var output: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var<uniform> params: Parameters;
@group(0) @binding(5) var display: texture_storage_2d<rgba8unorm, write>;

fn source_pixel(pixel: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(source));
    let p = textureLoad(source, clamp(pixel, vec2(0), size - 1), 0);
    return vec4(p.rgb * p.a, p.a);
}

fn sample_source(uv: vec2<f32>) -> vec4<f32> {
    if any(uv < vec2(0.0)) || any(uv >= vec2(1.0)) { return vec4(0.0); }
    let point = uv * vec2<f32>(textureDimensions(source)) - 0.5;
    let low = vec2<i32>(floor(point));
    let fraction = fract(point);
    let a = mix(source_pixel(low), source_pixel(low + vec2(1, 0)), fraction.x);
    let b = mix(source_pixel(low + vec2(0, 1)), source_pixel(low + vec2(1, 1)), fraction.x);
    let p = mix(a, b, fraction.y);
    return vec4(select(vec3(0.0), p.rgb / max(p.a, 0.000001), p.a > 0.0), p.a);
}

fn motion_pixel(pixel: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(source));
    // Filtering a padded layer uses transparent texels outside the original image.
    if any(pixel < vec2(0)) || any(pixel >= size) { return vec4(0.0); }
    let p = textureLoad(source, pixel, 0);
    return vec4(p.rgb * p.a, p.a);
}

fn sample_motion_blur(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(source));
    let extent = abs(params.appearance.zw) * 0.5 + 0.5 / size;
    if any(uv < -extent) || any(uv > 1.0 + extent) { return vec4(0.0); }
    let steps = u32(params.appearance.y);
    var sum = vec4(0.0);
    for (var i = 0u; i < steps; i++) {
        let offset = (f32(i) + 0.5) / f32(steps) - 0.5;
        let point = (uv + offset * params.appearance.zw) * size - 0.5;
        let low = vec2<i32>(floor(point));
        let fraction = fract(point);
        let a = mix(motion_pixel(low), motion_pixel(low + vec2(1, 0)), fraction.x);
        let b = mix(motion_pixel(low + vec2(0, 1)), motion_pixel(low + vec2(1, 1)), fraction.x);
        sum += mix(a, b, fraction.y);
    }
    return vec4(sum.rgb / max(sum.a, 0.000001), sum.a / f32(steps));
}

@compute @workgroup_size(8, 8)
fn composite(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2<u32>(params.canvas.xy)) { return; }
    let position = vec2<i32>(id.xy);
    let dst = textureLoad(previous, position, 0);
    if params.flags.y >= 100u {
        textureStore(display, position, select(vec4(dst.rgb * dst.a, dst.a), dst, params.flags.y == 101u));
        return;
    }
    let point = (vec2<f32>(id.xy) + 0.5) / params.canvas.xy * params.canvas.zw;
    var amount = params.appearance.x;
    if params.flags.z != 0u { amount *= textureLoad(coverage, position, 0).r; }
    if params.flags.y != 0u {
        textureStore(output, position, vec4(mix(dst.rgb, clamp(adjust(dst.rgb, point), vec3(0.0), vec3(1.0)), amount), dst.a));
        return;
    }
    let local = point - params.bounds.xy - params.bounds.zw * 0.5;
    var uv = vec2(local.x * params.rotation.x + local.y * params.rotation.y,
        -local.x * params.rotation.y + local.y * params.rotation.x) / params.bounds.zw;
    uv = uv * params.rotation.zw + 0.5;
    if params.flags.w != 0u {
        let homogeneous = vec3(uv, 1.0);
        let divisor = dot(params.points[2].xyz, homogeneous);
        uv = vec2(dot(params.points[0].xyz, homogeneous), dot(params.points[1].xyz, homogeneous)) / divisor;
    }
    var src = vec4(0.0);
    if params.appearance.y > 0.0 {
        src = sample_motion_blur(uv);
    } else {
        src = sample_source(uv);
    }
    src.a *= amount;
    let alpha = src.a + dst.a * (1.0 - src.a);
    let color = ((1.0 - src.a) * dst.a * dst.rgb + (1.0 - dst.a) * src.a * src.rgb
        + dst.a * src.a * blend(dst.rgb, src.rgb, params.flags.x)) / max(alpha, 0.000001);
    textureStore(output, position, vec4(color, alpha));
}
