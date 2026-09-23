@group(0) @binding(0) var<storage, read> pixels: array<u32>;
@group(0) @binding(1) var<storage, read> unused: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read> config: array<vec4<f32>>;
var<workgroup> bins: array<atomic<u32>, 1027>;
@compute @workgroup_size(8, 8)
fn analyze_pixels(@builtin(workgroup_id) group: vec3<u32>,
                                                 @builtin(local_invocation_index) local: u32) {
    for (var i = local; i < 1027u; i += 64u) {
        atomicStore(&bins[i], 0u);
    }
    workgroupBarrier();
    let size = vec2<u32>(config[0].xy);
    let stride = max(size.x, u32(config[0].w));
    // Amortize bin initialization and global atomics over a 32 x 32 tile.
    for (var pixel = local; pixel < 1024u; pixel += 64u) {
        let point = group.xy * 32u + vec2(pixel % 32u, pixel / 32u);
        if (any(point >= size)) {
            continue;
        }
        let i = point.y * stride + point.x;
        let value = pixels[i];
        let rgba = vec4(value & 255u, (value >> 8u) & 255u, (value >> 16u) & 255u, value >> 24u);
        var warning = value;
        if (rgba.a != 0u) {
            let luma = (rgba.r * 2126u + rgba.g * 7152u + rgba.b * 722u + 5000u) / 10000u;
            atomicAdd(&bins[min(luma, 255u)], 1u);
            atomicAdd(&bins[256u + rgba.r], 1u);
            atomicAdd(&bins[512u + rgba.g], 1u);
            atomicAdd(&bins[768u + rgba.b], 1u);
            atomicAdd(&bins[1024u], 1u);
            if (any(rgba.rgb == vec3(255u))) {
                warning = 0xff4123ffu;
                atomicAdd(&bins[1026u], 1u);
            } else if (all(rgba.rgb <= vec3(1u))) {
                warning = 0xffff6428u;
                atomicAdd(&bins[1025u], 1u);
            }
        }
        if (config[0].z != 0.0) {
            atomicStore(&output[1028u + i], warning);
        }
    }
    workgroupBarrier();
    for (var i = local; i < 1027u; i += 64u) {
        let value = atomicLoad(&bins[i]);
        if (value != 0u) {
            atomicAdd(&output[i], value);
        }
    }
}
