struct Parameters {
    canvas: vec4<f32>, // Output width/height, document width/height.
    bounds: vec4<f32>, // Origin and extent.
    rotation: vec4<f32>, // Cosine, sine, horizontal and vertical signs.
    flags: vec4<u32>, // Blend mode, adjustment kind, coverage present, curve point count.
    appearance: vec4<f32>, // Opacity, unused.
    first: vec4<f32>,
    second: vec4<f32>,
    points: array<vec4<f32>, 32>,
}

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

fn lum(c: vec3<f32>) -> f32 { return dot(c, vec3(0.3, 0.59, 0.11)); }
fn sat(c: vec3<f32>) -> f32 { return max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b); }

fn set_lum(color: vec3<f32>, value: f32) -> vec3<f32> {
    var c = color + value - lum(color);
    let low = min(min(c.r, c.g), c.b);
    let high = max(max(c.r, c.g), c.b);
    if low < 0.0 { c = value + (c - value) * value / max(value - low, 0.000001); }
    if high > 1.0 { c = value + (c - value) * (1.0 - value) / max(high - value, 0.000001); }
    return c;
}

fn set_sat(c: vec3<f32>, value: f32) -> vec3<f32> {
    let low = min(min(c.r, c.g), c.b);
    let high = max(max(c.r, c.g), c.b);
    return select(vec3(0.0), (c - low) * value / max(high - low, 0.000001), high > low);
}

fn blend(d: vec3<f32>, s: vec3<f32>, mode: u32) -> vec3<f32> {
    switch mode {
        case 1u: { return d * s; }
        case 2u: { return d + s - d * s; }
        case 3u: { return select(1.0 - 2.0 * (1.0 - d) * (1.0 - s), 2.0 * d * s, d <= vec3(0.5)); }
        case 4u: { return min(d, s); }
        case 5u: { return max(d, s); }
        case 6u: { return abs(d - s); }
        case 7u: { return select(min(vec3(1.0), d / max(1.0 - s, vec3(0.000001))), vec3(0.0), d == vec3(0.0)); }
        case 8u: { return select(1.0 - min(vec3(1.0), (1.0 - d) / max(s, vec3(0.000001))), vec3(1.0), d == vec3(1.0)); }
        case 9u: { return set_lum(set_sat(s, sat(d)), lum(d)); }
        case 10u: { return set_lum(set_sat(d, sat(s)), lum(d)); }
        case 11u: { return set_lum(s, lum(d)); }
        case 12u: { return set_lum(d, lum(s)); }
        default: { return s; }
    }
}

fn hue_to_rgb(hue: f32, saturation: f32, lightness: f32) -> vec3<f32> {
    let h = ((hue % 360.0) + 360.0) % 360.0 / 60.0;
    let c = (1.0 - abs(2.0 * lightness - 1.0)) * saturation;
    let x = c * (1.0 - abs(h % 2.0 - 1.0));
    var rgb = vec3(c, 0.0, x);
    if h < 1.0 { rgb = vec3(c, x, 0.0); }
    else if h < 2.0 { rgb = vec3(x, c, 0.0); }
    else if h < 3.0 { rgb = vec3(0.0, c, x); }
    else if h < 4.0 { rgb = vec3(0.0, x, c); }
    else if h < 5.0 { rgb = vec3(x, 0.0, c); }
    return rgb + lightness - c * 0.5;
}

fn curve_slope(index: u32) -> f32 {
    let a = params.points[index].xy;
    let b = params.points[index + 1u].xy;
    return (b.y - a.y) / max(b.x - a.x, 0.000001);
}

fn curve_tangent(index: u32) -> f32 {
    if index == 0u { return curve_slope(0u); }
    if index == params.flags.w - 1u { return curve_slope(index - 1u); }
    let a = curve_slope(index - 1u);
    let b = curve_slope(index);
    if a * b <= 0.0 { return 0.0; }
    return 2.0 / (1.0 / a + 1.0 / b);
}

fn curve(value: f32) -> f32 {
    var index = 0u;
    for (var i = 0u; i + 1u < params.flags.w; i++) { if params.points[i].x <= value { index = i; } }
    index = min(index, params.flags.w - 2u);
    let a = params.points[index].xy;
    let b = params.points[index + 1u].xy;
    let h = max(b.x - a.x, 0.000001);
    let t = clamp((value - a.x) / h, 0.0, 1.0);
    let t2 = t * t;
    let t3 = t2 * t;
    return clamp((2.0*t3 - 3.0*t2 + 1.0)*a.y + (t3 - 2.0*t2 + t)*h*curve_tangent(index)
        + (-2.0*t3 + 3.0*t2)*b.y + (t3 - t2)*h*curve_tangent(index + 1u), 0.0, 1.0);
}

fn noise(point: vec2<u32>, seed: u32) -> f32 {
    var value = point.x * 374761393u + point.y * 668265263u + seed;
    value = (value ^ (value >> 13u)) * 1274126177u;
    value = value ^ (value >> 16u);
    return f32(value) / 4294967295.0 * 2.0 - 1.0;
}

fn adjust(rgb: vec3<f32>, point: vec2<f32>) -> vec3<f32> {
    let a = params.first;
    let b = params.second;
    switch params.flags.y {
        case 1u: {
            let high = max(max(rgb.r, rgb.g), rgb.b);
            let low = min(min(rgb.r, rgb.g), rgb.b);
            let lightness = (high + low) * 0.5;
            let delta = high - low;
            var hue = 0.0;
            var saturation = 0.0;
            if delta > 0.000001 {
                saturation = delta / max(1.0 - abs(2.0 * lightness - 1.0), 0.000001);
                if high == rgb.r { hue = (rgb.g - rgb.b) / delta; }
                else if high == rgb.g { hue = (rgb.b - rgb.r) / delta + 2.0; }
                else { hue = (rgb.r - rgb.g) / delta + 4.0; }
                hue *= 60.0;
            }
            hue += a.x;
            saturation += a.y / 100.0;
            if a.w > 0.0 { hue = a.x; saturation = a.y / 100.0; }
            let result = hue_to_rgb(hue, clamp(saturation, 0.0, 1.0), lightness);
            let light = a.z / 100.0;
            return select(result * (1.0 + light), result + (1.0 - result) * light, light >= 0.0);
        }
        case 2u: { let value = pow(clamp((rgb * 255.0 - a.x) / max(a.z - a.x, 1.0), vec3(0.0), vec3(1.0)), vec3(1.0 / max(a.y, 0.01))); return (a.w + value * (b.x - a.w)) / 255.0; }
        case 3u: { return vec3(curve(rgb.r), curve(rgb.g), curve(rgb.b)); }
        case 4u: { return pow(max(rgb * exp2(a.x) + a.y, vec3(0.0)), vec3(1.0 / max(a.z, 0.01))); }
        case 5u: { return mix(a.rgb, b.rgb, dot(rgb, vec3(0.2126, 0.7152, 0.0722))); }
        case 6u: {
            let seed = bitcast<u32>(a.z);
            let p = vec2<u32>(point);
            let step = select(12345u, 0u, a.y > 0.0);
            return rgb + vec3(noise(p, seed), noise(p, seed + step), noise(p, seed + step * 2u)) * a.x / 100.0;
        }
        case 7u: { return 1.0 - rgb; }
        default: { return rgb; }
    }
}

@compute @workgroup_size(8, 8)
fn composite(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2<u32>(params.canvas.xy)) { return; }
    let position = vec2<i32>(id.xy);
    let dst = textureLoad(previous, position, 0);
    if params.flags.y == 100u {
        textureStore(display, position, vec4(dst.rgb * dst.a, dst.a));
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
    var src = sample_source(uv);
    src.a *= amount;
    let alpha = src.a + dst.a * (1.0 - src.a);
    let color = ((1.0 - src.a) * dst.a * dst.rgb + (1.0 - dst.a) * src.a * src.rgb
        + dst.a * src.a * blend(dst.rgb, src.rgb, params.flags.x)) / max(alpha, 0.000001);
    textureStore(output, position, vec4(color, alpha));
}
