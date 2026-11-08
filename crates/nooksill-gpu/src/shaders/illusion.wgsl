// 实验性「视觉欺骗」模糊：一次真高斯提供空间结构，之后只用低频色场
// 做颜色重投影 + 时间插值，跟着桌面实时变色，不再每帧跑卷积。
//
// 面板底色只透出约 22% 的模糊层，且模糊本身已经抹掉高频细节，所以
// 「结构沿用上一帧、颜色立刻跟上」在视觉上几乎等价于重新模糊一遍。

struct Params {
    width: u32,
    height: u32,
    gw: u32,
    gh: u32,
    gain: f32,
    t: f32,
    // 本帧已用真高斯重算过的列 [dirty_x0, dirty_x1)：这些列直接取基底，
    // 不再叠加色场差值（否则同一处颜色变化会被算两遍）。
    dirty_x0: u32,
    dirty_x1: u32,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var base_tex: texture_2d<f32>;
// 三段连续色场：base（生成基底时）/ prev / cur，每段 gw*gh 个 vec4。
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
    // 色场差值就是「这块区域比它上次真模糊时亮/暗了多少」。
    // 本帧刚跑过真高斯的列已经包含最新颜色，跳过补偿；其余列用
    // 低频色场把颜色补上 —— 于是结构持续更新，颜色始终实时。
    let covered = f32(x) >= f32(p.dirty_x0) && f32(x) < f32(p.dirty_x1);
    let gain = select(p.gain, 0.0, covered);
    let delta = (mix(prev, cur, p.t) - b) * gain / 255.0;
    let rgb = clamp(base.rgb + delta * base.a, vec3<f32>(0.0), vec3<f32>(1.0));
    textureStore(out_tex, vec2<i32>(x, y), vec4<f32>(rgb, base.a));
}
