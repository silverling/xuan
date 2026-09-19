struct Sample {
    offset: vec4<i32>,
    weights: vec4<f32>,
}

struct Parameters {
    // Output width/height, transparent padding, sample count.
    size: vec4<u32>,
    samples: array<Sample, 256>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> params: Parameters;

fn pixel_at(point: vec2<i32>) -> vec4<f32> {
    if any(point < vec2(0)) || any(point >= vec2<i32>(textureDimensions(source))) {
        return vec4(0.0);
    }
    // Accumulate byte values just like the CPU reference, in premultiplied form.
    return floor(textureLoad(source, point, 0) * 255.0 + 0.5);
}

@compute @workgroup_size(8, 8)
fn motion_blur(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= params.size.xy) { return; }
    let point = vec2<i32>(id.xy) - vec2<i32>(i32(params.size.z));
    var sum = vec4(0.0);
    for (var i = 0u; i < params.size.w; i++) {
        let sample = params.samples[i];
        for (var j = 0u; j < 4u; j++) {
            let p = pixel_at(point + sample.offset.xy + vec2<i32>(i32(j % 2u), i32(j / 2u)));
            let alpha = p.a * sample.weights[j];
            sum += vec4(p.rgb * alpha, alpha);
        }
    }
    var result = vec4(0.0);
    if sum.a > 0.0 { result = vec4(sum.rgb / sum.a, sum.a / f32(params.size.w)); }
    // Store straight RGBA for the editable layer, including RGB at low alpha.
    textureStore(output, vec2<i32>(id.xy), floor(result + 0.5) / 255.0);
}
