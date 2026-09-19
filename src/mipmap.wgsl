@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(output);
    if any(id.xy >= size) { return; }

    // Average premultiplied colors, including fractional edge coverage for odd
    // dimensions. Each destination texel covers at most 3 x 3 source texels.
    let scale = vec2<f32>(textureDimensions(source)) / vec2<f32>(size);
    let start = vec2<f32>(id.xy) * scale;
    let end = vec2<f32>(id.xy + vec2(1u)) * scale;
    var color = vec4(0.0);
    for (var y = i32(floor(start.y)); y < i32(ceil(end.y)); y++) {
        let wy = min(end.y, f32(y + 1)) - max(start.y, f32(y));
        for (var x = i32(floor(start.x)); x < i32(ceil(end.x)); x++) {
            let wx = min(end.x, f32(x + 1)) - max(start.x, f32(x));
            color += textureLoad(source, vec2(x, y), 0) * wx * wy;
        }
    }
    textureStore(output, vec2<i32>(id.xy), color / (scale.x * scale.y));
}
