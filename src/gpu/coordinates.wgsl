fn document_point(uv_input: vec2<f32>) -> vec2<f32> {
    let h = vec3(uv_input, 1.0);
    let uv = vec2(dot(config[3].xyz, h), dot(config[4].xyz, h)) / dot(config[5].xyz, h);
    let local = (uv - 0.5) * config[2].zw * config[1].zw;
    return config[1].xy + config[1].zw * 0.5 +
           vec2(local.x * config[2].x - local.y * config[2].y,
                local.x * config[2].y + local.y * config[2].x);
}

fn selected(point: vec2<f32>) -> f32 {
    let size = vec2<u32>(config[6].xy);
    if (size.x == 0u) {
        return 1.0;
    }
    if (any(point < vec2(0.0)) || any(point >= vec2<f32>(size))) {
        return 0.0;
    }
    let p = vec2<u32>(point);
    let i = p.y * size.x + p.x;
    return f32((auxiliary[i / 4u] >> ((i % 4u) * 8u)) & 255u) / 255.0;
}
