//! Frost background: GDI capture -> GPU separable gaussian -> egui texture.
//!
//! `glass_backdrop` supplies the capture (BitBlt), the gaussian weights and
//! the rounded-rect SDF. The blur runs in `blur_gpu` (WGSL compute) and the
//! result is registered with egui directly, so there is no GPU->CPU readback.
//! `blur_rgba` and `compose` here are the CPU fallback and the test reference.
//!
//! Capture resolution is 25% of the client area. A visible panel is excluded
//! from screen capture (`WDA_EXCLUDEFROMCAPTURE`), otherwise GDI captures the
//! panel itself. This also hides the panel from screenshots.
//!
//! `Live` polls at 50–100 ms while the desktop moves and backs off to 800 ms
//! on a static desktop. A 25 ms pixel probe detects change during the backoff,
//! so the worst-case reaction time is ~25 ms instead of a full idle interval.
//! Captured bytes are shared with the UI thread via `Arc`, avoiding a second
//! full copy. Each frame also carries the changed column range, so the GPU
//! convolution only recomputes the columns that actually changed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use egui::{Color32, ColorImage};
use glass_backdrop::blur::{clamp_radius, normalized_gaussian_weights, sigma_for_radius};
use glass_backdrop::capture::DesktopCapture;
use glass_backdrop::glass::glass_sdf;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{ClientToScreen, GetDC, GetPixel, ReleaseDC, HDC};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

use crate::frame::FrameRenderer;
use crate::worker::Waker;

/// 捕获分辨率占客户区的百分比。
///
/// Acrylic 只让用户看到卷积结果，25% 与 100% 肉眼几乎无差，像素数却少 16 倍：
/// 全屏 2560×1600 的起动台只需模糊 640×400 = 256k 像素。
pub const CAPTURE_PCT: u8 = 25;

/// 桌面正在变化时的抓取间隔（20 Hz）。
///
/// 以前是固定 100 ms（10 Hz），背景一动就能看出「跟手慢半拍」。探针负责
/// 抓住第一帧变化，这个间隔负责之后的连续跟随；再快收益不大，抓屏本身
/// 才是开销大头。
const LIVE_ACTIVE: Duration = Duration::from_millis(50);

/// 连续抓不到变化时的起始间隔（10 Hz），之后逐次翻倍退避。
const LIVE_INTERVAL: Duration = Duration::from_millis(100);

/// 连续抓不到变化时退避到的上限。
///
/// 桌面静止时没必要每 100 ms 抓一遍；退避省 CPU，而 [`PROBE_INTERVAL`]
/// 的像素探针保证这 800 ms **不再是感知延迟** —— 桌面一变，探针下一拍
/// 就唤醒全量抓屏。
const LIVE_IDLE_MAX: Duration = Duration::from_millis(800);

/// 像素探针节拍。只读 20 个像素点，一次不到 0.1 ms，可以常年开着。
const PROBE_INTERVAL: Duration = Duration::from_millis(25);

/// 两次全量抓屏的最小间隔，防止动态桌面把探针每拍都变成一次抓屏。
const MIN_CAPTURE_GAP: Duration = Duration::from_millis(40);

/// 探针可能失效（例如某些远程桌面会话里屏幕 DC 读不到别的窗口）时的兜底：
/// 静止时最多隔这么久也要完整抓一次。
const PROBE_FORCE_GAP: Duration = Duration::from_millis(300);

/// 实验性「视觉欺骗」模糊开关。进程级：所有面板共用一个开关，
/// 由主循环每帧从配置同步（设置界面改配置后下一帧即生效）。
static ILLUSION: AtomicBool = AtomicBool::new(false);

/// 切换实验性模糊路径。原高斯算法始终保留，关掉即回到它。
pub fn set_illusion(on: bool) {
    ILLUSION.store(on, Ordering::Relaxed);
}

/// 当前是否走实验性模糊路径。
#[must_use]
pub fn illusion() -> bool {
    ILLUSION.load(Ordering::Relaxed)
}

/// 一次开窗的毛玻璃参数。`None` = 这个功能没开。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    /// 高斯半径（捕获图 texel）。来自配置键 `Blur`。
    pub radius: u32,
    /// 面板圆角（逻辑点）。起动台是方角，传 0。
    pub corner_pt: f32,
}

impl Spec {
    /// Capture once before the panel is shown. Falls back to CPU blur when the
    /// GPU pipeline is unavailable.
    pub fn apply(self, renderer: &mut FrameRenderer, hwnd: HWND, ppp: f32) {
        let mut cap = match DesktopCapture::new(true) {
            Ok(c) => c,
            Err(_) => {
                renderer.clear_frost();
                return;
            }
        };
        let captured = cap
            .capture_behind_pct(hwnd.0 as isize, CAPTURE_PCT)
            .ok()
            .flatten();
        let Some((bits, w, h)) = captured else {
            renderer.clear_frost();
            return;
        };
        if illusion() && renderer.set_frost_illusion(bits, w, h, self, ppp) {
            return;
        }
        if renderer.set_frost_gpu(bits, w, h, self, ppp) {
            return;
        }
        match compose(bits, w, h, self, ppp) {
            Some(image) => renderer.set_frost(image),
            None => renderer.clear_frost(),
        }
    }
}

/// 把窗口排除在屏幕捕获之外。
///
/// 必须在 `ShowWindow` **之前**调用：之后才设的话，中间那一帧抓到的是
/// "桌面 + 自己"，模糊层里会出现一层自己的镜像。
pub fn exclude_from_capture(hwnd: HWND) {
    // 失败只可能是系统太老（Win10 2004 之前没有这个值）。那时实时背景会
    // 抓到自己的镜像 —— 由调用方决定是否还要开，这里不额外处理。
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    }
}

/// 恢复窗口参与屏幕捕获（面板隐藏时调用）。
pub fn allow_capture(hwnd: HWND) {
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, WDA_NONE);
    }
}

/// 低频色场网格。实验性模糊只用它「跟着桌面变色」，不做卷积。
pub const GRID_W: usize = 24;
pub const GRID_H: usize = 16;

/// 每个 tile 的平均 RGB（0..255）。
pub type ColorField = Vec<[f32; 3]>;

/// 把捕获图降成 [`GRID_W`]×[`GRID_H`] 个平均色。
///
/// 色场只承载 **低频** 颜色，高频细节由一次真高斯模糊的基底纹理保留。
/// 逐像素累加（640×400 约 0.3 ms）而不是稀疏采样：跳着采会让部分 tile
/// 一个采样点都落不到，在重投影里变成黑色方块。
#[must_use]
pub fn color_field(bits: &[u8], w: usize, h: usize) -> ColorField {
    let mut sums = vec![[0f64; 3]; GRID_W * GRID_H];
    let mut counts = vec![0u32; GRID_W * GRID_H];
    if w == 0 || h == 0 || bits.len() < w.saturating_mul(h).saturating_mul(4) {
        return vec![[0.0; 3]; GRID_W * GRID_H];
    }
    for y in 0..h {
        let gy = (y * GRID_H / h).min(GRID_H - 1);
        for x in 0..w {
            let gx = (x * GRID_W / w).min(GRID_W - 1);
            let off = (y * w + x) * 4;
            let k = gy * GRID_W + gx;
            sums[k][0] += f64::from(bits[off]);
            sums[k][1] += f64::from(bits[off + 1]);
            sums[k][2] += f64::from(bits[off + 2]);
            counts[k] += 1;
        }
    }
    // 图比网格还窄时会有空 tile：用全局平均兜底，绝不留黑洞。
    let total: u32 = counts.iter().sum();
    let global = [
        sums.iter().map(|s| s[0]).sum::<f64>(),
        sums.iter().map(|s| s[1]).sum::<f64>(),
        sums.iter().map(|s| s[2]).sum::<f64>(),
    ]
    .map(|v| (v / f64::from(total.max(1))) as f32);
    sums.iter()
        .zip(&counts)
        .map(|(s, &n)| {
            if n == 0 {
                global
            } else {
                let k = 1.0 / f64::from(n);
                [(s[0] * k) as f32, (s[1] * k) as f32, (s[2] * k) as f32]
            }
        })
        .collect()
}

/// 两张色场的平均绝对差（0..255）。用来判断「桌面大变」要不要重跑真模糊。
#[must_use]
pub fn field_delta(a: &ColorField, b: &ColorField) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut sum = 0f32;
    for (p, q) in a[..n].iter().zip(&b[..n]) {
        for c in 0..3 {
            sum += (p[c] - q[c]).abs();
        }
    }
    sum / (n * 3) as f32
}

/// One captured frame. Bytes are shared with the UI thread, not copied again.
#[derive(Clone, Debug)]
pub struct FrostFrame {
    pub bits: Arc<Vec<u8>>,
    pub w: u32,
    pub h: u32,
    /// 源图里发生变化的列 `[x0, x1)`；全量帧为 `(0, w)`。
    /// GPU 卷积只重算这些列，静止区域直接沿用上一帧纹理。
    pub x0: u32,
    pub x1: u32,
    /// 该帧的低频色场（实验性模糊用；经典路径忽略）。
    pub field: Arc<ColorField>,
}

/// Background capture thread for a visible panel.
#[derive(Debug)]
pub struct Live {
    /// Dropping the sender unblocks `recv_timeout` when the panel closes.
    tx: Option<Sender<()>>,
    rx: Receiver<FrostFrame>,
    /// Never joined: a GDI capture may still be in flight at shutdown.
    _thread: JoinHandle<()>,
}

impl Live {
    #[must_use]
    pub fn start(hwnd: HWND) -> Option<Self> {
        let (tx, cmd_rx) = mpsc::channel::<()>();
        let (img_tx, rx) = mpsc::channel::<FrostFrame>();
        let waker = Waker::new(hwnd);
        // `HWND` is a raw pointer; only its numeric value crosses threads.
        let raw = hwnd.0 as isize;
        let thread = std::thread::Builder::new()
            .name("nooksill-frost".to_owned())
            .spawn(move || run(raw, &img_tx, waker, &cmd_rx))
            .ok()?;
        Some(Self {
            tx: Some(tx),
            rx,
            _thread: thread,
        })
    }

    /// Keep only the newest frame; stale backgrounds are worse than a drop.
    ///
    /// 丢帧时**必须把脏列并起来**：否则中间那帧改过的列在新帧里没变化，
    /// GPU 就不会重算，那一竖条会永远停在上上帧的背景上。
    pub(crate) fn latest(&self) -> Option<FrostFrame> {
        let mut newest: Option<FrostFrame> = None;
        while let Ok(frame) = self.rx.try_recv() {
            newest = Some(match newest {
                Some(prev) => merge_frames(prev, frame),
                None => frame,
            });
        }
        newest
    }
}

/// 合并被丢弃的中间帧：内容取最新的，脏列取并集。
fn merge_frames(prev: FrostFrame, frame: FrostFrame) -> FrostFrame {
    let (x0, x1) = if (prev.x0 == 0 && prev.x1 == prev.w) || (frame.x0 == 0 && frame.x1 == frame.w)
    {
        (0, frame.w)
    } else {
        (prev.x0.min(frame.x0), prev.x1.max(frame.x1))
    };
    FrostFrame { x0, x1, ..frame }
}

impl Drop for Live {
    fn drop(&mut self) {
        // 丢掉发送端 → 线程的 `recv_timeout` 立刻返回 `Disconnected` → 退出。
        self.tx = None;
    }
}

/// 极廉价的「桌面变了吗」探针。
///
/// 全量抓屏要读整块窗口矩形（最高 2560×1600），静止时退避到 800 ms，
/// 用户看到的模糊就会慢半拍。这里只从屏幕 DC 上读 20 个像素点，
/// 25 ms 一拍，成本可以忽略；一旦发现变化立刻唤醒全量抓屏。
/// 窗口已被 `WDA_EXCLUDEFROMCAPTURE` 排除，所以读到的是它背后的桌面。
struct Probe {
    hdc: HDC,
    last: u32,
}

impl Probe {
    fn new() -> Option<Self> {
        let hdc = unsafe { GetDC(None) };
        if hdc.0.is_null() {
            return None;
        }
        Some(Self { hdc, last: 0 })
    }

    /// 采样 5×4 个网格点，把颜色卷成一个校验和。
    /// 拿不到窗口几何时返回 `true`（宁可多抓一次，也不要漏变化）。
    fn changed(&mut self, hwnd: isize) -> bool {
        let handle = HWND(hwnd as *mut std::ffi::c_void);
        let mut rc = RECT::default();
        if unsafe { GetClientRect(handle, &mut rc) }.is_err() {
            return true;
        }
        let mut origin = POINT {
            x: rc.left,
            y: rc.top,
        };
        if !unsafe { ClientToScreen(handle, &mut origin) }.as_bool() {
            return true;
        }
        let w = (rc.right - rc.left).max(1);
        let h = (rc.bottom - rc.top).max(1);
        let mut acc = 0u32;
        for gy in 0..4 {
            for gx in 0..5 {
                let x = origin.x + w * (gx * 2 + 1) / 10;
                let y = origin.y + h * (gy * 2 + 1) / 8;
                let c = unsafe { GetPixel(self.hdc, x, y) }.0;
                acc = acc.wrapping_mul(31).wrapping_add(c & 0x00ff_ffff);
            }
        }
        let changed = acc != self.last;
        self.last = acc;
        changed
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseDC(None, self.hdc);
        }
    }
}

/// 两帧之间发生变化的列范围 `[x0, x1)`。
///
/// 高斯是局部算子：源图第 x 列变了，只可能影响输出 `[x-r, x+r]` 列。
/// 调用方按半径外扩后只 dispatch 这一段，其余像素留在纹理里不动。
fn dirty_columns(prev: &[u8], cur: &[u8], w: usize, h: usize) -> (u32, u32) {
    let need = w.saturating_mul(h).saturating_mul(4);
    if w == 0 || h == 0 || prev.len() < need || cur.len() < need {
        return (0, w as u32);
    }
    let (mut x0, mut x1) = (w, 0usize);
    for y in 0..h {
        let base = y * w * 4;
        let a = &prev[base..base + w * 4];
        let b = &cur[base..base + w * 4];
        for (x, (p, q)) in a
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .enumerate()
        {
            if p != q {
                x0 = x0.min(x);
                x1 = x1.max(x);
            }
        }
    }
    if x0 > x1 {
        return (0, w as u32);
    }
    (x0 as u32, (x1 as u32 + 1).min(w as u32))
}

/// Capture loop. Blur and upload happen on the UI thread in `FrameRenderer`.
fn run(hwnd: isize, out: &Sender<FrostFrame>, waker: Waker, stop: &Receiver<()>) {
    let Ok(mut cap) = DesktopCapture::new(true) else {
        return;
    };
    let mut probe = Probe::new();
    let mut prev: Option<Arc<Vec<u8>>> = None;
    let mut dims = (0u32, 0u32);
    let mut interval = LIVE_INTERVAL;
    let mut next_full = Instant::now();
    let mut last_full = Instant::now() - PROBE_FORCE_GAP;
    let mut seq = 0u64;

    loop {
        let now = Instant::now();
        // 探针每拍都跑：它是「桌面突然变了」能被立刻发现的唯一渠道。
        let hit = probe.as_mut().is_some_and(|p| p.changed(hwnd));
        let force = now.duration_since(last_full) >= PROBE_FORCE_GAP;
        let can_capture = now.duration_since(last_full) >= MIN_CAPTURE_GAP;
        if now >= next_full || ((hit || force) && can_capture) {
            let started = Instant::now();
            last_full = now;
            if let Ok(Some((bits, w, h))) = cap.capture_behind_pct(hwnd, CAPTURE_PCT) {
                let same = dims == (w, h) && prev.as_deref().is_some_and(|p| p.as_slice() == bits);
                if same {
                    interval = (interval * 2).min(LIVE_IDLE_MAX);
                } else {
                    let (x0, x1) = match prev.as_deref() {
                        Some(p) if dims == (w, h) => dirty_columns(p, bits, w as usize, h as usize),
                        _ => (0, w),
                    };
                    let diff = if trace() {
                        count_diff(prev.as_deref().map_or(&[][..], Vec::as_slice), bits)
                    } else {
                        0
                    };
                    let field = Arc::new(color_field(bits, w as usize, h as usize));
                    dims = (w, h);
                    let snapshot = Arc::new(bits.to_vec());
                    prev = Some(snapshot.clone());
                    interval = LIVE_ACTIVE;
                    if trace() {
                        report(seq, bits, w, h, started.elapsed(), diff);
                        seq += 1;
                    }
                    if out
                        .send(FrostFrame {
                            bits: snapshot,
                            w,
                            h,
                            x0,
                            x1,
                            field,
                        })
                        .is_err()
                    {
                        return;
                    }
                    waker.wake();
                }
            }
            next_full = last_full + interval;
        }

        // 固定 25 ms 节拍：探针驱动，全量抓屏由 `next_full` 与节流控制。
        // `recv_timeout` 同时承担停止信号，面板一关立刻退出。
        match stop.recv_timeout(PROBE_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// `NOOKSILL_FROST_TRACE=1`：每产出一帧打一行尺寸 / 平均色 / 耗时。
///
/// 验证"模糊是实时的"就靠它：背后放一个不断变色的窗口，这里的平均色应当
/// 跟着变；一直不变说明抓到的还是开窗前那张静态图（或者抓到了自己 ——
/// 那时平均色会被面板自己的浅色底拉平）。
fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NOOKSILL_FROST_TRACE").is_ok_and(|v| v.trim() != "0"))
}

/// 两帧之间变了多少个像素（诊断用，只在 [`trace`] 开着时算）。
fn count_diff(a: &[u8], b: &[u8]) -> usize {
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .filter(|(p, q)| p != q)
        .count()
}

/// 打一帧的摘要（只在 [`trace`] 开着时调用）。
fn report(seq: u64, bits: &[u8], w: u32, h: u32, spent: Duration, diff: usize) {
    let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
    let px = bits.as_chunks::<4>().0;
    for p in px {
        r += u64::from(p[0]);
        g += u64::from(p[1]);
        b += u64::from(p[2]);
    }
    let n = px.len().max(1) as u64;
    eprintln!(
        "[frost] #{seq} {w}x{h} 平均色=({},{},{}) 变化像素={diff}/{} 抓+模糊={} ms",
        r / n,
        g / n,
        b / n,
        px.len(),
        spent.as_millis()
    );
}

/// 面板底色。
///
/// 毛玻璃开着时降到约 55% 不透明：下面的模糊桌面要透上来，但文字仍需足够
/// 对比度。不开时维持原来的近实心底色。
#[must_use]
pub fn panel_bg(dark: bool, frost: bool) -> Color32 {
    let (r, g, b) = if dark { (24, 25, 30) } else { (246, 247, 250) };
    // 毛玻璃下不再是 140（55%）：桌面亮度不受控，深色主题压在浅色壁纸上时
    // 整块面板会被抬到中灰，白字只剩 3:1 出头。200/190 让壁纸最多透 22%，
    // 模糊照样看得见，但文字底色基本确定 —— 与 Win11 亚克力的 tint 同路数。
    let a = if frost {
        if dark {
            200
        } else {
            190
        }
    } else {
        250
    };
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// 圆角矩形的像素覆盖率（0..=255）。
///
/// `corner` 是**图像像素**为单位的圆角半径，不是逻辑点。SDF 在形状内部恒为
/// 负，所以覆盖用 `0.5 - sdf` 做一像素羽化 —— 不能用 `sdf < 0` 的二值判断，
/// 否则圆角会是锯齿。
#[must_use]
pub fn rounded_coverage(x: f32, y: f32, w: f32, h: f32, corner: f32) -> u8 {
    // `glass_sdf` 在形状内部恒为 `-radius`，半径 0 时会退化成处处 `sdf=0`
    // → 覆盖率恒为 50%（上游 README 里明确点名的 "square panel" 坑）。
    // 方角面板不需要 SDF，直接全不透明。
    if corner <= 0.0 {
        return 255;
    }
    let half = [w * 0.5, h * 0.5];
    let p = [x + 0.5 - half[0], y + 0.5 - half[1]];
    let sdf = glass_sdf(p, half, corner);
    let cov = (0.5 - sdf).clamp(0.0, 1.0);
    (cov * 255.0).round() as u8
}

/// 可分离高斯模糊（RGBA，边界夹取）。
///
/// 两趟一维卷积：先横后竖，复杂度从 `O(r²)` 降到 `O(2r)`。权重直接取
/// `glass_backdrop::blur::normalized_gaussian_weights`，与上游 WGSL 侧的
/// 卷积核是同一份定义 —— 改一边忘了另一边，常量测试会先红。
///
/// ## 为什么先拆平面
///
/// 交错 RGBA 里同一通道相邻像素隔 4 个字节，卷积内层要按步长取数，编译器
/// 无法向量化；拆成三个连续平面后内层就是"切片 × 权重"的点积。第二趟
/// 按转置写出（跨行写，一次卷积只写一个值，不心疼），于是两趟都不需要
/// 额外的转置内存。实测 640×400 / r=12 从 40 ms 降到 20 ms。
#[must_use]
pub fn blur_rgba(src: &[u8], w: usize, h: usize, radius: u32) -> Vec<u8> {
    let need = w.saturating_mul(h).saturating_mul(4);
    if w == 0 || h == 0 || src.len() < need {
        return Vec::new();
    }
    let r = clamp_radius(radius) as usize;
    let weights = normalized_gaussian_weights(radius, sigma_for_radius(radius));
    let n = w * h;

    let mut plane = [vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]];
    for i in 0..n {
        plane[0][i] = f32::from(src[i * 4]);
        plane[1][i] = f32::from(src[i * 4 + 1]);
        plane[2][i] = f32::from(src[i * 4 + 2]);
    }

    // 横向：按转置写 → `mid[x * h + y]` 就是"原图第 x 列"的第 y 个值。
    let mut mid = [vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]];
    for c in 0..3 {
        for y in 0..h {
            conv_row(
                &plane[c][y * w..(y + 1) * w],
                &mut mid[c],
                y,
                h,
                &weights,
                r,
            );
        }
    }

    // 纵向：把"列"当行走一遍，同样按转置写 → 恢复成正常的 w×h 布局。
    let mut out = [vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]];
    for c in 0..3 {
        for x in 0..w {
            conv_row(&mid[c][x * h..(x + 1) * h], &mut out[c], x, w, &weights, r);
        }
    }

    let mut rgba = vec![0u8; need];
    for i in 0..n {
        rgba[i * 4] = out[0][i].round().clamp(0.0, 255.0) as u8;
        rgba[i * 4 + 1] = out[1][i].round().clamp(0.0, 255.0) as u8;
        rgba[i * 4 + 2] = out[2][i].round().clamp(0.0, 255.0) as u8;
        rgba[i * 4 + 3] = 255;
    }
    rgba
}

/// 一行一维卷积：`out[base + x * stride] = Σ row[x - r + k] * weights[k]`。
///
/// 左右各 `r` 个像素越界，按边界夹取（等于把边缘像素当常数延拓）。中间段
/// 没有夹取也没有分支，是这条路径上唯一需要跑快的部分。
fn conv_row(row: &[f32], out: &mut [f32], base: usize, stride: usize, weights: &[f32], r: usize) {
    let w = row.len();
    let taps = weights.len();
    if w >= taps {
        for x in 0..r {
            out[base + x * stride] = clamp_taps(row, x, r, weights);
        }
        for x in r..w - r {
            let win = &row[x - r..x - r + taps];
            let mut acc = 0.0f32;
            for (a, b) in win.iter().zip(weights) {
                acc += a * b;
            }
            out[base + x * stride] = acc;
        }
        for x in w - r..w {
            out[base + x * stride] = clamp_taps(row, x, r, weights);
        }
    } else {
        // 图比卷积核还窄：整行都按夹取算，没有"中间段"可言。
        for x in 0..w {
            out[base + x * stride] = clamp_taps(row, x, r, weights);
        }
    }
}

/// 带夹取的一维卷积（只用于左右边缘与窄图）。
#[inline]
fn clamp_taps(row: &[f32], x: usize, r: usize, weights: &[f32]) -> f32 {
    let last = row.len() - 1;
    let mut acc = 0.0f32;
    for (k, wt) in weights.iter().enumerate() {
        let sx = (x + k).saturating_sub(r).min(last);
        acc += row[sx] * wt;
    }
    acc
}

/// 把圆角 alpha 与预乘写进模糊结果。
///
/// 只有四个角需要算 SDF：圆角半径以外的像素覆盖率恒为 255。四角方块按
/// 半径截断，整幅图跑一遍 SDF 在实时路径上（每秒十次）是纯浪费。
pub(crate) fn apply_round_alpha(rgba: &mut [u8], w: usize, h: usize, corner: f32) {
    if corner <= 0.0 {
        return; // `blur_rgba` 已经把 alpha 写成 255
    }
    let (fw, fh) = (w as f32, h as f32);
    let n = (corner.ceil() as usize + 1).min(w).min(h);
    if 2 * n > w || 2 * n > h {
        // 面板小到四个角重叠：退回整幅扫描，避免同一像素被乘两次 alpha。
        for y in 0..h {
            for x in 0..w {
                let a = rounded_coverage(x as f32, y as f32, fw, fh, corner);
                put_alpha(rgba, (y * w + x) * 4, a);
            }
        }
        return;
    }
    for y in 0..n {
        for x in 0..n {
            for (px, py) in [
                (x, y),
                (w - 1 - x, y),
                (x, h - 1 - y),
                (w - 1 - x, h - 1 - y),
            ] {
                let a = rounded_coverage(px as f32, py as f32, fw, fh, corner);
                put_alpha(rgba, (py * w + px) * 4, a);
            }
        }
    }
}

/// 写一个像素的 alpha，并把它乘进 RGB。
///
/// egui 的 `ColorImage` 在这一步之后按**预乘**解释：圆角外侧 alpha→0 时
/// 若 RGB 还留着亮色，线性采样会把亮边拖进圆角。
fn put_alpha(rgba: &mut [u8], off: usize, a: u8) {
    rgba[off + 3] = a;
    if a < 255 {
        let k = f32::from(a) / 255.0;
        for c in 0..3 {
            rgba[off + c] = (f32::from(rgba[off + c]) * k).round() as u8;
        }
    }
}

/// CPU fallback: captured bytes -> blur -> rounded alpha -> egui image.
/// Also the golden reference for the GPU compute path in tests.
pub(crate) fn compose(bits: &[u8], w: u32, h: u32, spec: Spec, ppp: f32) -> Option<ColorImage> {
    let (w, h) = (w as usize, h as usize);
    let need = w.checked_mul(h)?.checked_mul(4)?;
    if w == 0 || h == 0 || bits.len() < need {
        return None;
    }
    let mut rgba = blur_rgba(&bits[..need], w, h, spec.radius);
    if rgba.is_empty() {
        return None;
    }
    let scale = f32::from(CAPTURE_PCT) / 100.0;
    apply_round_alpha(&mut rgba, w, h, spec.corner_pt * ppp * scale);
    Some(ColorImage::from_rgba_premultiplied([w, h], &rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 常量图模糊后不变（权重归一化，和必须为 1）。
    #[test]
    fn constant_image_stays_constant() {
        let src = vec![90u8; 5 * 5 * 4];
        let out = blur_rgba(&src, 5, 5, 4);
        assert_eq!(out.len(), src.len());
        assert_eq!(out[0], 90);
        assert_eq!(out[1], 90);
        assert_eq!(out[2], 90);
        assert_eq!(out[3], 255);
    }

    /// 单点会被摊开，且能量守恒（近似：所有像素之和 ≈ 原点值）。
    #[test]
    fn impulse_spreads_and_conserves_energy() {
        let w = 9;
        let h = 9;
        let mut src = vec![0u8; w * h * 4];
        for px in src.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        src[(4 * w + 4) * 4] = 255;
        let out = blur_rgba(&src, w, h, 3);
        let sum: u32 = out.as_chunks::<4>().0.iter().map(|p| u32::from(p[0])).sum();
        assert!((220..290).contains(&sum), "能量应接近 255，实际 {sum}");
        assert!(out[(4 * w + 4) * 4] < 255, "中心应当被摊薄");
        assert!(out[(4 * w + 3) * 4] > 0, "核半径内的邻居该分到能量");
        assert_eq!(out[0], 0, "核半径外不该有能量");
    }

    /// 非方图（宽 ≠ 高）也要模糊得对：转置写不能让轴搞混。
    ///
    /// 横向放一条亮线、纵向全暗，模糊后亮带应当保持在中间几行，不会跑到列上去。
    #[test]
    fn non_square_keeps_axes_straight() {
        let (w, h) = (24usize, 8usize);
        let mut src = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                src[(y * w + x) * 4] = if y == h / 2 { 200 } else { 0 };
            }
        }
        let out = blur_rgba(&src, w, h, 2);
        let row = |y: usize| u32::from(out[(y * w + w / 2) * 4]);
        assert!(row(h / 2) > 100, "亮带应留在原来的行上");
        assert!(row(0) < 20 && row(h - 1) < 20, "角落不该被点亮");
    }

    /// 与朴素二维卷积逐像素对拍（±1 舍入）。
    ///
    /// 可分离 + 平面 + 转置写的实现改过好几处索引，光看代码很难确信没把
    /// 横竖搞混：这里用最直白的 `Σ row[y][x] * w * w` 二维卷积当基准，
    /// 覆盖非方图与奇数半径 —— 一旦观感变了，先红的是这条。
    #[test]
    fn separable_blur_matches_naive_2d() {
        let (w, h, radius) = (13usize, 9usize, 2u32);
        // 确定的伪随机内容（不用外部 crate，也不依赖时间）。
        let mut seed = 0x1234_5678u32;
        let mut src = vec![0u8; w * h * 4];
        for (i, px) in src.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let mut next = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 24) as u8
            };
            px[0] = next();
            px[1] = next();
            px[2] = next();
            px[3] = 255;
            let _ = i;
        }

        let fast = blur_rgba(&src, w, h, radius);
        let weights = normalized_gaussian_weights(radius, sigma_for_radius(radius));
        let r = clamp_radius(radius) as i32;
        let taps = weights.len() as i32;
        assert_eq!(taps, 2 * r + 1, "权重个数必须是 2r+1");

        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    // 真正的二维卷积：核是两个一维权重的外积（不是对角线和）。
                    let mut acc = 0.0f32;
                    for (j, wy) in weights.iter().enumerate() {
                        let sy = (y as i32 + j as i32 - r).clamp(0, h as i32 - 1) as usize;
                        for (k, wx) in weights.iter().enumerate() {
                            let sx = (x as i32 + k as i32 - r).clamp(0, w as i32 - 1) as usize;
                            acc += f32::from(src[(sy * w + sx) * 4 + c]) * wy * wx;
                        }
                    }
                    let want = acc.round().clamp(0.0, 255.0) as i32;
                    let got = i32::from(fast[(y * w + x) * 4 + c]);
                    assert!(
                        (got - want).abs() <= 1,
                        "({x},{y}) 通道 {c}：可分离={got} 朴素={want}"
                    );
                }
            }
        }
    }

    /// 合法性：非法尺寸早退。
    #[test]
    fn malformed_buffers_do_not_panic() {
        assert!(blur_rgba(&[], 0, 0, 4).is_empty());
        assert!(blur_rgba(&[0u8; 4], 10, 10, 4).is_empty());
    }

    /// 圆角：中心与内部不透明，四角透明，边缘是渐变。
    #[test]
    fn rounded_mask_keeps_center_and_cuts_corners() {
        let (w, h, r) = (100.0f32, 100.0f32, 12.0f32);
        assert_eq!(rounded_coverage(50.0, 50.0, w, h, r), 255);
        assert_eq!(rounded_coverage(0.0, 0.0, w, h, r), 0);
        assert_eq!(rounded_coverage(99.0, 0.0, w, h, r), 0);
        // 方角（radius=0）不能走 SDF：上游那个 SDF 在内部恒为 -radius，
        // 半径 0 时处处返回 0，会把整块面板钉在 50% 覆盖率。
        assert_eq!(rounded_coverage(0.0, 0.0, w, h, 0.0), 255);
    }

    /// 只算四角的 alpha 必须和整幅扫描完全一致 —— 这是实时路径的性能前提，
    /// 一旦角块尺寸算少了，圆角外侧就会出现没被扣掉的亮边。
    #[test]
    fn corner_only_alpha_matches_full_scan() {
        for (w, h, corner) in [(120usize, 80usize, 12.0f32), (200, 150, 7.3), (60, 60, 4.0)] {
            // 模拟 `blur_rgba` 的产物：RGB 是内容，alpha 已经全 255。
            let start: Vec<u8> = (0..w * h).flat_map(|_| [200u8, 200, 200, 255]).collect();
            let mut fast = start.clone();
            apply_round_alpha(&mut fast, w, h, corner);

            // 整幅扫描的参考实现（预乘写全，不看 `put_alpha`）。
            let mut slow = start;
            for y in 0..h {
                for x in 0..w {
                    let a = rounded_coverage(x as f32, y as f32, w as f32, h as f32, corner);
                    let off = (y * w + x) * 4;
                    slow[off + 3] = a;
                    let k = f32::from(a) / 255.0;
                    for c in 0..3 {
                        slow[off + c] = (200.0f32 * k).round() as u8;
                    }
                }
            }
            assert_eq!(
                fast, slow,
                "{w}x{h} corner={corner} 的四角裁剪应等价于整幅扫描"
            );
        }
    }

    /// 方角面板一个字节都不该动（模糊已经写成 255），更不能做预乘。
    #[test]
    fn square_panel_leaves_alpha_alone() {
        let start: Vec<u8> = (0..8 * 8).flat_map(|_| [180u8, 90, 30, 255]).collect();
        let mut buf = start.clone();
        apply_round_alpha(&mut buf, 8, 8, 0.0);
        assert_eq!(buf, start);
    }

    /// 毛玻璃底色确实比实心底色更透。
    #[test]
    fn frost_background_is_more_transparent() {
        assert!(panel_bg(true, true).a() < panel_bg(true, false).a());
        assert_eq!(panel_bg(true, false).a(), 250);
    }

    fn frame_with_cols(x0: u32, x1: u32, w: u32) -> FrostFrame {
        FrostFrame {
            bits: Arc::new(vec![0u8; 4]),
            w,
            h: 1,
            x0,
            x1,
            field: Arc::new(vec![[0.0; 3]; GRID_W * GRID_H]),
        }
    }

    /// 丢帧时脏列必须取并集，否则中间帧改过的那一竖条会永久残留。
    #[test]
    fn dropped_frames_union_their_dirty_columns() {
        let merged = merge_frames(frame_with_cols(10, 20, 100), frame_with_cols(30, 40, 100));
        assert_eq!((merged.x0, merged.x1), (10, 40));
    }

    /// 只要有一帧是全量，合并结果就必须是全量。
    #[test]
    fn a_full_frame_keeps_the_merge_full() {
        let merged = merge_frames(frame_with_cols(10, 20, 100), frame_with_cols(0, 100, 100));
        assert_eq!((merged.x0, merged.x1), (0, 100));
        let merged = merge_frames(frame_with_cols(0, 100, 100), frame_with_cols(30, 40, 100));
        assert_eq!((merged.x0, merged.x1), (0, 100));
    }

    /// 色场只保留低频：常量图处处同色，左右分色要分得开。
    #[test]
    fn color_field_keeps_low_frequency_colour() {
        let (w, h) = (48usize, 32usize);
        let mut flat = vec![0u8; w * h * 4];
        for px in flat.as_chunks_mut::<4>().0 {
            px[0] = 200;
            px[1] = 100;
            px[2] = 50;
            px[3] = 255;
        }
        let f = color_field(&flat, w, h);
        assert_eq!(f.len(), GRID_W * GRID_H);
        assert!(f.iter().all(|c| (c[0] - 200.0).abs() < 0.01
            && (c[1] - 100.0).abs() < 0.01
            && (c[2] - 50.0).abs() < 0.01));

        let mut split = flat;
        for y in 0..h {
            for x in w / 2..w {
                let i = (y * w + x) * 4;
                split[i] = 10;
                split[i + 1] = 20;
                split[i + 2] = 230;
            }
        }
        let g = color_field(&split, w, h);
        assert!(g[0][0] > 190.0, "左半应保持红色");
        assert!(g[GRID_W - 1][2] > 220.0, "右半应变成蓝色");
        assert!(field_delta(&f, &g) > 20.0);
        assert_eq!(field_delta(&f, &f), 0.0);
    }

    /// 脏列检测：只报真正改过的列；无变化与坏输入都退回全量。
    #[test]
    fn dirty_columns_finds_the_changed_band() {
        let (w, h) = (8usize, 4usize);
        let mut a = vec![0u8; w * h * 4];
        let mut b = a.clone();
        for y in 0..h {
            for x in [3usize, 5] {
                b[(y * w + x) * 4] = 9;
            }
        }
        assert_eq!(dirty_columns(&a, &b, w, h), (3, 6));
        assert_eq!(dirty_columns(&a, &a, w, h), (0, w as u32));
        assert_eq!(dirty_columns(&[], &b, w, h), (0, w as u32));
        // 尺寸不同时（例如窗口刚 resize）不能直接按列比较。
        assert_eq!(dirty_columns(&a, &b, w + 1, h), (0, (w + 1) as u32));
        a.truncate(4);
        assert_eq!(dirty_columns(&a, &b, w, h), (0, w as u32));
    }
}
