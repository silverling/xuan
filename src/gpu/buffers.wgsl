@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read> auxiliary: array<u32>;
@group(0) @binding(2) var<storage, read_write> result: array<u32>;
@group(0) @binding(3) var<storage, read> config: array<vec4<f32>>;

fn load_float(index: u32) -> vec4<f32> {
    let i = index * 4u;
    return bitcast<vec4<f32>>(vec4(input[i], input[i+1u], input[i+2u], input[i+3u]));
}
fn store_float(index: u32, value: vec4<f32>) {
    let i = index * 4u;
    let v = bitcast<vec4<u32>>(value);
    result[i] = v.x; result[i+1u] = v.y; result[i+2u] = v.z; result[i+3u] = v.w;
}
fn rgba(index: u32) -> vec4<f32> { return unpack4x8unorm(input[index]); }
fn packed(value: vec4<f32>) -> u32 {
    let v = vec4<u32>(floor(clamp(value, vec4(0.0), vec4(1.0)) * 255.0 + 0.5));
    return v.x | (v.y << 8u) | (v.z << 16u) | (v.w << 24u);
}
fn byte_at(index: u32) -> f32 {
    return f32((input[index/4u] >> ((index%4u)*8u)) & 255u) / 255.0;
}

