// Temporal colour reprojection over a cached Gaussian base.

struct Params {
    width: u32,
    height: u32,
    gw: u32,
    gh: u32,
    gain: f32,
    t: f32,
    // Columns rebuilt by the real Gaussian this frame; skip the field delta there.
    dirty_x0: u32,
    dirty_x1: u32,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var base_tex: texture_2d<f32>;
// Three consecutive colour fields: base, prev, cur; each gw*gh vec4s.
@group(0) @binding(2) var<storage, read> fields: array<vec4<f32>>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba8unorm, write>;

fn sample_field(base: u32, u: f32, v: f32) -> vec3<f32> {
    let gx = clamp(u * f32(p.gw) - 0.5, 0.0, f32(p.gw - 1u));
    let gy = clamp(v * f32(p.gh) - 0.5, 0.0, f32(p.gh - 1u));
    let x0 = u32(gx);
    let y0 = u32(gy);
    let x1 = min(x0 + 1u, p.gw - 1u);
    let y1 = min(y0 + 1u, p.gh - 1u);
    let fx = gx - f32(x0);
    let fy = gy - f32(y0);
    let a = mix(fields[base + y0 * p.gw + x0].xyz,
                fields[base + y0 * p.gw + x1].xyz, fx);
    let b = mix(fields[base + y1 * p.gw + x0].xyz,
                fields[base + y1 * p.gw + x1].xyz, fx);
    return mix(a, b, fy);
}

@compute @workgroup_size(64)
fn reproject(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= p.width * p.height) {
        return;
    }
    let x = i32(i % p.width);
    let y = i32(i / p.width);
    let base = textureLoad(base_tex, vec2<i32>(x, y), 0);
    let u = (f32(x) + 0.5) / f32(p.width);
    let v = (f32(y) + 0.5) / f32(p.height);

    let n = p.gw * p.gh;
    let b = sample_field(0u, u, v);
    let prev = sample_field(n, u, v);
    let cur = sample_field(2u * n, u, v);
    let covered = f32(x) >= f32(p.dirty_x0) && f32(x) < f32(p.dirty_x1);
    let gain = select(p.gain, 0.0, covered);
    let delta = (mix(prev, cur, p.t) - b) * gain / 255.0;
    let rgb = clamp(base.rgb + delta * base.a, vec3<f32>(0.0), vec3<f32>(1.0));
    textureStore(out_tex, vec2<i32>(x, y), vec4<f32>(rgb, base.a));
}
