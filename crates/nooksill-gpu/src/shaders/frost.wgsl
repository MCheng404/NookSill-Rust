struct Params {
    width: u32,
    height: u32,
    radius: u32,
    taps: u32,
    corner: f32,
    // First dirty column; 0 requests a full frame.
    x0: u32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> src: array<u32>;
@group(0) @binding(2) var<storage, read_write> mid: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> weights: array<f32>;
@group(0) @binding(4) var out_tex: texture_storage_2d<rgba8unorm, write>;

fn unpack(c: u32) -> vec3<f32> {
    return vec3<f32>(
        f32(c & 255u),
        f32((c >> 8u) & 255u),
        f32((c >> 16u) & 255u)
    );
}

// Integer hash dither, +/-0.5 of one 8-bit step. The texture is 8-bit and is
// upscaled four times, so a smooth blur would otherwise show contour bands.
// Integer math keeps the CPU reference bit-identical (same wrapping ops).
fn dither_offset(x: u32, y: u32) -> f32 {
    var h = x * 1664525u + y * 1013904223u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    h = h ^ (h >> 16u);
    return f32((h >> 24u) & 255u) / 255.0 - 0.5;
}

fn coverage(x: f32, y: f32) -> f32 {
    if (p.corner <= 0.0) {
        return 1.0;
    }
    let hw = f32(p.width) * 0.5;
    let hh = f32(p.height) * 0.5;
    let r = min(p.corner, min(hw, hh));
    let px = x + 0.5 - hw;
    let py = y + 0.5 - hh;
    let qx = max(abs(px) - hw + r, 0.0);
    let qy = max(abs(py) - hh + r, 0.0);
    let sdf = sqrt(qx * qx + qy * qy) - r;
    return clamp(0.5 - sdf, 0.0, 1.0);
}

@compute @workgroup_size(64)
fn h_blur(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    // Column-major: x = x0 + i/height, y = i%height; mid[] is transposed.
    let x = i32(p.x0) + i32(i / p.height);
    if (x >= i32(p.width)) {
        return;
    }
    let y = i32(i % p.height);
    let r = i32(p.radius);
    // Drop out-of-bounds taps and renormalise; clamping repeats the border
    // pixel and turns both corner passes into one flat colour patch.
    var acc = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var k = 0u; k < p.taps; k = k + 1u) {
        let sx = x + i32(k) - r;
        if (sx >= 0 && sx < i32(p.width)) {
            acc += unpack(src[u32(y * i32(p.width) + sx)]) * weights[k];
            wsum += weights[k];
        }
    }
    mid[u32(x * i32(p.height) + y)] = vec4<f32>(acc / max(wsum, 0.00001), 0.0);
}

@compute @workgroup_size(64)
fn v_blur(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let x = i32(p.x0) + i32(i / p.height);
    if (x >= i32(p.width)) {
        return;
    }
    let y = i32(i % p.height);
    let r = i32(p.radius);
    var acc = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var k = 0u; k < p.taps; k = k + 1u) {
        let sy = y + i32(k) - r;
        if (sy >= 0 && sy < i32(p.height)) {
            acc += mid[u32(x * i32(p.height) + sy)].xyz * weights[k];
            wsum += weights[k];
        }
    }
    acc = acc / max(wsum, 0.00001);

    let d = dither_offset(u32(x), u32(y));
    let rgb = floor(clamp(acc + d, vec3<f32>(0.0), vec3<f32>(255.0)) + vec3<f32>(0.5));
    let a = floor(coverage(f32(x), f32(y)) * 255.0 + 0.5);
    var out_rgb = rgb;
    if (a < 255.0) {
        out_rgb = floor(rgb * (a / 255.0) + vec3<f32>(0.5));
    }
    textureStore(out_tex, vec2<i32>(x, y), vec4<f32>(out_rgb / 255.0, a / 255.0));
}
