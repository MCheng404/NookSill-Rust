# 架构

## 一、一个 UI 线程 + 四个工作线程

```
┌─ UI 线程 ──────────────────────────────────────────────────┐
│ Win32 消息循环（热键 / AppBar / 捕获 / 全屏检测 / 自动隐藏轮询）│
│   ↓ 消息                                                   │
│ Dock 状态（一个 Box<Dock>，wndproc 与主循环共用同一份）      │
│   ↓                                                       │
│ egui::Context::run_ui → tessellate → egui-wgpu → 离屏纹理   │
│   ↓                                                       │
│ 回读 → 预乘 BGRA 写入 DIB → UpdateLayeredWindow            │
└────────────────────────────────────────────────────────────┘
   ↑ try_recv（从不等）           ↑ PostMessage(WM_APP_WAKE)
┌──┴──────────────┬──────────────┬──────────────┬────────────┐
│ IconLoader      │ Scanner      │ DiskWriter   │ Opener     │
│ shell 取图标    │ 运行状态     │ 写 ini       │ 启动应用   │
│ + rayon 并行    │ + 遮挡检测   │              │            │
└─────────────────┴──────────────┴──────────────┴────────────┘
```

**所有会阻塞的系统调用都在下面那四个线程里**（ADR-037）。UI 线程只做
`try_recv`，永远不等；工作线程有结果时 `PostMessage` 敲一下，主循环的
`MsgWaitForMultipleObjectsEx` 立刻返回 —— 空闲时仍然真睡，不轮询。

没有 `wgpu::Surface` —— Windows 上没有带 alpha 的交换链（ADR-008），
所以"呈现"是每帧一次同步回读 + `UpdateLayeredWindow`。这条回读是 UI 线程
上唯一剩下的阻塞点，也是将来要提帧率时的第一个目标。

线程里**绝对不做**的事：不碰 `egui::Context`（纹理注册与图集上传必须由
UI 线程做）、不创建窗口、不改窗口状态。`wgpu` compute 是例外：
`Device` / `Queue` 是 `Send + Sync`，图标圆角遮罩在工作线程提交并回读。

rayon 只用来并行派发图标批；像素处理本身已移到 GPU compute，
毛玻璃卷积也不再占 CPU 核。其余线程都是 IO 等待。

| 工作 | 线程 | 代价 |
|---|---|---|
| 图标取用 | `IconLoader` | 启动时窗口先出来，图标陆续补上；遮罩走 GPU compute |
| 窗口枚举 | `Scanner` | 1.2 s / 100 ms 一轮，都在后台 |
| 写 ini | `DiskWriter` | 停手 400 ms 后才写 |
| 毛玻璃抓屏 | `frost::Live` | 变化时 50 ms / 静止退避到 800 ms；另有 25 ms 像素探针负责提前唤醒 |
| 毛玻璃模糊 | **UI 线程**（`shaders::frost` compute） | 只重算变化的列，零回读 |
| 烟雾光栅 | **UI 线程**（`shaders::smoke` compute） | 每帧上传流场，输出直接注册为 egui 纹理 |

## 二、crate 依赖

```
nooksill-core        （无 Win32 依赖，纯逻辑）
   ↑        ↑
   │        └── nooksill-settings （egui，无 wgpu）── kittest 测试
   │                    ↑
   └── nooksill-platform│
              ↑         │
         nooksill-gpu ──┘  （wgpu + Win32 窗口/输入/命中 + 工作线程）
```

依赖方向是单向的：`gpu → settings → core`、`gpu → platform`。
`settings` 不认识 `wgpu`，所以它的测试不需要 GPU/显示器；
`core` 不认识 Windows，所以它的测试能跑在任何宿主上。

### 界面语言：`core::i18n`

```
[general] Language ─┐
GetUserDefaultUILanguage ─┴→ i18n::configure → AtomicU8 → t() / fmt()
                                                     ↑
                          en.rs（底层） ← 回退 ← zh_cn.rs / zh_tw.rs
```

文案放在 `core` 是因为四个 crate 都依赖它，而 `core` 零依赖、不碰 Win32；
系统语言由 `platform::locale` 读一次，启动时与配置合成进程级语言。英文文案
自身就是键，同时是底层语言：翻译表按英文键排序、二分查找，缺项回退英文。
每种语言一个骨架 `.rs`：英文与简体中文的键表按字母分段放在 `en/` 与 `zh_cn/`
（`first.rs` / `second.rs`），由骨架里的 const 块在编译期拼回**单一** `TEXT`
切片，所以 `lookup` 仍是二分查找、公开路径不变（ADR-076）。加语言 =
加一个骨架 + `Language` 加一项。动态文案走 `fmt(key, args)` 的 `{0}` 占位，
译文可以自己决定语序；`i18n::tests` 扫全部源码，保证每个调用点都登记在
`en.rs::TEXT`、翻译表不含野键，并守住分段拼接后的有序性。

不随语言切换的三类：农历 / 节气 / 干支的名称（文化数据，面板标签已本地化）、控制台日志与 Win32
诊断串、测试断言消息。繁体表有意保持部分翻译，用于验证回退链。

## 三、渲染管线

一帧的完整路径在 **`frame/render.rs::FrameRenderer::draw_frame`** 里，`render` 与
`render_rgba` 共用同一份实现 —— 这条链路原先两边各写一遍，逐行相同，任何
API 变动都要改两处。

像素缓冲按「设备级 / 窗口级」分家（ADR-117）：egui Context、字形图集与 wgpu
管线属设备级，建一次留终身；`offscreen`（GPU 目标 + 回读缓冲）与 `presenter`
（分层 DIB）属窗口级，各自一个可空槽位。`new()` 两个都不分配，首帧才建；
窗口隐藏时只 `release_dib()` 释放 DIB —— 它是唯一**释放后真能换回**进程工作集
的缓冲，实测 7 个窗口共 13.5 MiB。GPU 目标分配时确实抬 WS（+31 / +17 MiB），
但释放一点不还、再分配也不再花钱：内存被图形分配器池化留着（ADR-117）。
`render` 前 `ensure_targets()` 补齐，
`render_rgba` 只 `ensure_offscreen()`（离屏截图没有窗口，不需要 DIB）。

1. 调用方组装 `egui::RawInput`（含 `screen_rect`、`time`、`viewports`），
   传给 `render(raw, hwnd, pos, |ui| ...)`。
2. `ctx.run_ui(raw, build)`：闭包里只负责画。
3. `ctx.tessellate(shapes, ppp)` → `Vec<ClippedPrimitive>`。
4. `textures_delta.set` 逐项 `renderer.update_texture`（字形图集、图标纹理）。
5. `renderer.update_buffers` 写顶点/索引缓冲。
6. `begin_render_pass`，`LoadOp::Clear(Color::TRANSPARENT)` —— **透明清屏是窗口能透出桌面的关键**。
7. `renderer.render(&mut pass, ...)`。
8. `copy_texture_to_buffer` → `queue.submit` → `textures_delta.free` + `clear()`
   （不 clear 会在 Drop 时断言 panic）。
9. 之后分两路：
   - `render`：`with_mapped_pixels` 把带 256 字节行对齐的回读缓冲**直接**
     写进 DIB（同时去对齐 + RGBA→BGRA），再 `UpdateLayeredWindow`。
     没有整帧中间缓冲，也没有每帧 1–6 MB 的堆分配。
   - `render_rgba`：`read_pixels` 返回紧密 RGBA，不呈现 —— 离屏截图
     （`examples/settings_shot.rs`）与像素级回归走这条路。

面板与起动台的标签统一走 `gpu::text`：`LayoutJob` 一次排版出带省略号的
`Galley`，再 `painter.galley` 画。`painter.text` 内部会再排版一次，所以
「量一次宽 + text 画」是两次 shaping（不变式 14 的同一条约束）。

返回 `FrameOutcome { cursor, repaint_delay }`，同时把 `repaint_delay` 记成
`FrameRenderer` 的截止时间：主循环用 `repaint_due()` 决定是否出帧、用
`repaint_in()` 决定等多久。只改其中一半会漏帧或让等待退化成 1 ms 空转；
Dock 完全隐藏时屏蔽残留期限。依据见 ADR-075。

毛玻璃背景不走上屏纹理上传：`frost::Live` 线程只抓屏（顺带算脏列范围与
低频色场）并把字节用 `Arc` 交给 UI 线程；`FrameRenderer` 在 `draw_frame`
前调用 `blur_gpu` 的 WGSL compute（两趟可分离高斯 + 圆角 coverage），
输出 `Rgba8Unorm` 纹理直接注册给 egui，**零回读**。高斯逐像素独立，
所以只 dispatch 变化列 `[x-r, x+r)`，其余像素留在纹理里不动。
CPU `blur_rgba` / `compose` 保留为回退路径与像素级对照基准。

磨砂只有这一条路径。实验性的 `illusion` 色场重投影已整条移除，
`FrostIllusion` 键保留在 ini 里以兼容旧配置，但不再读取（ADR-094）。

### GPU shader 框架：`gpu/shaders/`

所有逐像素着色器集中在这里。`ShaderRuntime` 缓存 compute pipeline、管理
输出 `Rgba8Unorm` 纹理、统一注册为 egui native texture；WGSL 分文件存放，
每条 GPU 路径都有 CPU 参考实现和 `#[ignore]` 真机逐通道对照（差 ≤1）。

| 模块 | GPU 做什么 | CPU 参考 |
|---|---|---|
| `frost.wgsl` | 两趟可分离高斯 + 圆角 + 预乘 + 抖动（支持脏列范围） | `frost::blur_rgba` |
| `smoke.wgsl` | 双线性放大 + 三层噪声 + 调色 + pre/post | `Fluid::raster` / `post` |
| `icon.wgsl` | 图标圆角 alpha 遮罩 | `core::icon::apply_round_mask` |
| `puff.wgsl` | 64×64 软边烟团贴图 | `SmokeFx::puff_rgba_cpu` |

CPU 版本只作回退与测试基准。**保留在 CPU 的不是着色器**：
`Fluid::step`（流体物理模拟）、`Noise` 表（共享数据源，启动上传一次）、
`fit_icon` / bbox（标量几何）、`platform::icon::bgra_to_rgba`（GDI 格式
重排，platform 层按架构不依赖 wgpu）、`present::swap_rb`（GPU→CPU 呈现
桥接，`UpdateLayeredWindow` 要求 CPU DIB，M3 换 DirectComposition 才能消除）。

### 帧节拍与常驻内存

活跃态由 `App::wait()` 按 `perf::pacing_ms` 唤醒：默认不限（1 ms 泵），
`[performance] FrameCap` 可设上限，`VSync=1` 时按
`DwmGetCompositionTimingInfo` 的实际刷新率对齐（本机 16 ms）。空闲态
仍睡满 `FS_CHECK_MS`，保持「空闲 0 帧」。设置窗口用 `repaint_in()` 把
egui 的重绘期限并入等待，不再靠 1 秒兜底；反过来它的连续重绘按显示器
刷新节流，且这一窗口在动画时栏体当轮停画（不变量 31）。

MSAA 默认按面积自适应：> 2 M 物理像素用 2x（2560×1600 省约 31 MiB
常驻显存），小窗口仍 4x；`Msaa` 可强制 1/2/4（下次启动生效）。
CJK 字体（`msyh.ttc`，8.04 MiB）由 `OnceLock<Arc<FontData>>` 全进程
共享，不再每个 `FrameRenderer` 各读一份。

### 槽位布局：`core::layout`

"每个图标画在哪、多大、鼠标命中谁"全部是纯函数，住在 `core::layout`：

| 函数 | 职责 |
|---|---|
| `calc_metrics` | 由配置 + 工作区算出 `Metrics`（图标边长、栏高、时钟占位……） |
| `base_centers` | 未放大时的槽位中心（放大权重必须以它为基准） |
| `magnify_targets` | 本帧每个槽位的目标边长（余弦衰减） |
| `lerp_to` | 向目标值指数逼近一帧，返回"还需不需要下一帧" |
| `place_slots` | 放大后的实际槽位 `bar_l/bar_w/slot_l/slot_r/act_c` |
| `bars` | 纵向基线 `bar_top/bar_bottom/icon_bottom` |
| `hit_test` | 命中下标（从左到右第一个，见下） |

GPU 版与 GDI+ 版都只是"取结果 → 画"。这套逻辑曾经两边各写一遍，
已经漂移过（GDI+ 版拿动画中的 `cur` 去算基准宽度，导致基准槽位每帧移动）。

### 设置界面：左导航 + 右侧卡片

`nooksill-settings` 是**纯 UI**：没有 HWND、没有 device、没有线程，所以能被
kittest 无头驱动。结构按"一页一个文件"拆开：

```
crates/nooksill-settings/src/
  pages/
    mod.rs         Page 枚举、SettingsState、Command、两列骨架
    nav.rs         一级导航项与分组 -> 内容标签的映射
    preview.rs     由 Config 每帧驱动的实时预览 Dock（纯 egui）
    update.rs      更新策略页、网络页、状态卡片与更新弹窗内容
    widgets/       共享控件：card / row / toggle / slider / combo / 侧栏 / modal
    options.rs     下拉选项表（纯数据）
    items.rs       「条目」页：既有条目的改名/排序/归组/删除（数据由主程序同步快照）
    general.rs     常规      appearance.rs  外观     position.rs   位置
    icons.rs       图标      interaction.rs 交互     extras.rs     附件
    effect.rs      效果      performance.rs 性能     library.rs    图标库
    add.rs         添加      about.rs       关于
```

加一页设置 = 加一个文件 + `Page` 枚举加一项；加一个设置项 = `core::config`
加一个键 + 对应页面加一行。**控件长什么样、怎么排版只在 `widgets/` 里改**。

### 二级内容与弹窗

一级导航只有 6 项（常规 / Dock 栏 / 性能 / 内容与图标库 / 更新与网络 / 关于）。
打开分组时 `SettingsState::sub` 接管：左栏临时渲染该组的内容标签，底部固定
「< 返回」，右栏顶部渲染**实时预览 Dock**（`pages/preview.rs`，每帧读同一份
`Config`，不碰 wgpu），下面是该标签原有的页面函数。`normalize_nav()` 保证
直接设置 `page` 也会落到正确分组，所以一页仍只有一个渲染函数。
Dock 组的六个标签（含「效果」）都带预览，符合「效果页也渲染预览」的要求。

`Modal` 是纯数据：标题、正文、按钮动作（`Option<Command>`）与一个可选的
「不再提示」配置键。弹窗在 `settings_ui` 末尾统一绘制，遮罩与散焦光晕盖住整窗，
`settings_win::shell` 再叠一层柔化底色；动作只写 `Command`，替换文件、写盘与
重启仍在主程序。详见 ADR-084。

布局不是 `SidePanel` 而是手工切两块 `Rect`（`ui.new_child(max_rect)`）：
设置窗口外面还套着标题栏，`SidePanel` 按根 Ui 布局会跟标题抢地方。起点用
`available_rect_before_wrap()` —— `max_rect()` 不随已用内容收缩，用它会把
两列画到标题上。

视觉基准是 `settings-prototype.html` 雏形：页面、卡片、行与 Rust 端一一对应，
改设计先改雏形。该雏形属于非源码内容，已移出公开仓库、保存在本机私有设计
目录（见 `CONTRIBUTING.md` 第五节）。**不靠人肉截图验收** ——

```bash
cargo run -p nooksill-gpu --example settings_shot -- 出图.bmp 外观 [dark]
```

走的是与真实窗口完全相同的 `FrameRenderer` + `settings_win::shell`，只把最后
一步的 `UpdateLayeredWindow` 换成写 BMP。

### 组件面板：声明式注册表 + 运行时

七个面板原先各写一套定位（锚点 / 居中 / 自己算「上方居中」）和一套背景：
日历与控制中心锚在卡片旁，计时器 / 资源 / 速记读 `centered_on_target`，
应用组另算一遍；面板的 `FrameRenderer` 从未启用磨砂。现在两层收口：

```
core::config::panel     REGISTRY / PanelKind / Placement / Backdrop / Prefs（纯数据）
      ↓ 每帧 sync_prefs
gpu::widgets/framework  prefs（进程级） → placement（纯几何） → runtime（Win32 + 抓屏绑定）
      ↓
widgets/{calendar,control,timer,resource,notes,group,editor}   只声明 kind，共用 paint_frame
```

位置三档（跟随组件 / 屏幕居中 / 自动）由 `framework::placement::place` 一个纯函数
决定，`Auto` 比较未钳制的理想矩形与工作区；背景四档（跟随栏体 / 纯色 / 渐变 /
磨砂）由 `PanelStyle` 绘制，`runtime::apply_backdrop` 负责挂实时抓屏并在收起时释放。
新增一个组件 = `REGISTRY` 加一行 + 面板打开时调一次 `place_panel` /
`apply_backdrop`，`nooksill_gpu::calendar_win` 等公开路径不变（ADR-088）。

资源监视器是唯一「行集合可变」的面板：`core::resource` 定义六种读数与 ini 顺序，
`nooksill-platform::sysinfo` 负责采样（累计计数器做差、句柄 RAII），
`widgets::resource` 按配置顺序生成行几何并在打开时 `resize` 离屏目标。标题下不画
分隔线；占用率三项有进度条，其余只有数值与说明（ADR-098）。

控制中心的亮度条始终占位：`platform::brightness` 先查 Windows WMI
（`WmiMonitorBrightness`，内置屏），失败再试 DDC/CI（外接屏）；成功用强调色并写入
系统，失败灰显（拖动只重试探测）。写入按 50 ms 节流，面板高度不随探测结果变化
（ADR-101）。

磁贴状态由 `platform::radio` 在面板打开时读取：WiFi 走 WLAN 软件无线电并带回
SSID / 信号，蓝牙走 WinRT `Radio`，定位读用户同意存储。热点与定位没有公开的写入
接口，点击仍打开系统页；WiFi / 蓝牙的点击是真开关（ADR-102）。

## 四、帧调度：空闲不提交

```rust
loop {
    if ctx.has_requested_repaint() { render()?; sleep(8ms); continue; }
    GetMessageW(...)   // 阻塞，直到有消息
    TranslateMessage / DispatchMessage
}
```

egui 只在有动画、悬停、输入时请求重绘；其余时间线程阻塞在 `GetMessageW`，
CPU 占用为 0。这条性质是产品承诺（"空闲无定时器"），**不能被任何"为了流畅"
的常驻 vsync 循环破坏**。M1 引入的动画必须走 `ctx.request_repaint()`，
不允许自建 60fps 定时器。

## 五、输入与命中

- 鼠标/滚轮/键盘由 `input.rs` 翻译成 `egui::Event`。这份翻译是"不用 eframe"
  的代价，M0 只覆盖鼠标、滚轮、Esc/Enter/Tab/方向键。
- **命中测试在 `WM_NCHITTEST` 里同步完成**：把光标坐标换成逻辑像素，
  判断是否落在面板矩形内，是则 `HTCLIENT`，否则 `HTTRANSPARENT`。
  系统每次点击都会问，不需要轮询、不存在"鸡生蛋"问题。
- M1 要补：按圆角/图标实际形状判定（现在是矩形近似）、`WM_MOUSELEAVE`、
  IME（`WM_IME_*` → `egui::Event::Ime`），以及输入框的原生 `EDIT` 兜底。

## 六、线程模型

| 线程 | 职责 | 禁止事项 |
|---|---|---|
| UI | 消息泵、egui、GPU 提交、命中 | 不做磁盘 IO、不枚举窗口 |
| rayon 池 | 图标像素处理（缩放判定 + 圆角遮罩） | 不碰窗口/device |
| `IconLoader` | shell/COM 取图标，**串行** | 不碰 UI 状态 |
| `Scanner` | 运行状态 + 遮挡检测 | 不产生 `wgpu` 对象 |
| `sampler::SysSampler` | 资源读数（CPU / GPU / 内存 / 磁盘 / 网络 / 进程 / 温度），每秒一次 | 不碰窗口、不碰 `egui` |
| `DiskWriter` | 写 ini（400 ms 防抖、串行保序） | 不碰窗口 |
| `Opener` | `ShellExecuteW` / 激活已有窗口 | 不碰窗口 |
| `nooksill-frost` | GDI 抓屏 + CPU 可分离高斯（面板可见时） | 不碰窗口/device |

跨线程数据用 `std::sync::mpsc`（任务与结果各一条通道）+ `PostMessage` 唤醒。
**没有 `ArcSwap` / `crossbeam-channel`**：本项目跨线程传的是值拷贝，不存在
读多写少的共享快照，队列深度也只有个位数（ADR-037）。
不用 tokio：这里没有大量并发连接，只有几条长驻线程 + 一个线程池，
async 运行时带来的调度开销与栈占用都不划算。

## 七、不变式（违反即 bug）

1. **字体先于第一帧**。egui 0.37 缺字形直接 `panic`（`MissingGlyphPolicy::Panic`），
   不是画豆腐块。`fonts::install_cjk` 必须在 `run_ui` 之前调用。
2. **`GWLP_USERDATA` 里存的是 `Box<Dock>` 内的地址**。`Dock` 必须一直待在堆上：
   一旦被移出 `Box`（比如改成 `App` 的裸字段），窗口过程手里的指针立刻失效，
   表现为"点一下进程就崩"、且没有任何 Rust panic 输出（ADR-036 记过同一个坑）。
   同理，`SettingsWindow::new` 直接返回 `Pin<Box<Self>>`。
3. **设置界面不产生副作用**。它只能改 `Config` 并返回 `Command`；
   文件、注册表、对话框由主程序执行。这条保证了 kittest 的可断言性。
4. **一个 Device**。禁止为第二个窗口再建 device —— 纹理与图集无法共享，
   内存会翻倍。菜单 / 设置窗口各自有自己的 `FrameRenderer`，但共用同一个
   `Gpu`（device + queue）。
5. **配置写入只有一个出口**。所有值必须经 `Config::apply_i32`（内含夹紧），
   不允许直接改字段。原版崩溃的一半来自"某处忘了夹紧"。
6. **自定义 `WM_APP` 消息号必须全局查重**。它们都发到 Dock 的同一个
   hwnd，`pump()` 又按 `WM_APP_WAKE` 区分"要不要出帧"：
   `WM_APP+1 = worker::WM_APP_WAKE`、`WM_APP+2 = tray::WM_APP_TRAY`、
   `WM_APP+3 = single_instance::WM_APP_SHOW`、
   `0x8000+0x4142 = appbar::WM_APP_AB`。撞号会让托盘点击被静默吞掉，
   而且不会有任何编译错误。加新消息前先 `grep -rn WM_APP crates/`。
7. **窗口过程只碰 `Dock`**。`GWLP_USERDATA` 挂的是 `Box<Dock>` 的地址，
   wndproc 拿不到 `App`。托盘点击、拖放路径这类事件先记在 `Dock` 的
   一次性标志/队列里，由主循环取走执行 —— 不要在 WndProc 里开窗口
   或跑模态循环。
8. **IME 文本链路只有一条**：`WM_CHAR` 的可打印字符 → `Event::Text`；
   组合串 → `ImeEvent::Preedit`；上屏串 → `ImeEvent::Commit`。上屏
   **不能**退回 `Event::Text`，否则预编辑的拼音会残留在正文里。同一帧
   里 Commit 必须先于 Preedit（一条 `WM_IME_COMPOSITION` 可能同时带上屏
   与下一轮组合）。另外 egui 的 `Event::Text("\n")` 会被忽略，换行只能
   靠 `Event::Key{Enter}` —— 多行框别再往 `WM_CHAR` 里塞换行。
9. **拖放落点走 `GetCursorPos + ScreenToClient`**，不用 `DragQueryPoint`
   （坐标系文档不明确）。窗口像素先在 `Dock::logical_point` 转成逻辑轴，
   `drop_index_at` 只认沿栏的 `u`；四边不再各写一套 x/y 分支。一批
   `WM_DROPFILES` 只认第一条落点。拿不到光标就退回追加末尾，不能退化成
   插到第 0 位。
10. **没有周期动画的面板，`render()` 末尾必须把 `next_repaint` 推到未来**。
    这些窗口全靠事件把闹钟设回 `now`；若只设不推进，`due()` 在可见期间
    恒为真，主循环会把整个 Dock 拖进 60 fps 常驻提交。要周期动画的
    （计时器）则由 `render()` 自己按 `TICK` 推进，两者不能混。
11. **图标请求去重的键是 `(下标, 路径)`，不能只用路径**。分组条目的
    `path` 恒为空串，只用路径会让多个分组互相抑制。`change_icon` /
    `reset_icon` 必须调 `IconLoader::forget` 解绑，否则换图标不生效。
12. **自动隐藏是「非对称迟滞」而不是两个独立定时器**：
    `hide_delay_ms` 从「最后一次靠近」起算「持续离开」，
    `show_delay_ms` 从「进入热区」起算「持续停留」，中途任一方向反悔
    都要清空对应计时。两档按前台窗口是否最大化切换（`IsZoomed`，
    与 `is_fullscreen_foreground` 的无边框全屏判定互不等价）。
    默认值 450 / 0 必须保留旧行为，否则所有既有隐藏测试都要重写。
13. **小组件卡片的视觉参数只有一个来源**：`dock/mod.rs::WidgetStyle`
    （深浅配色、内边距、图标/字号比例、最小文本宽度）。
    `argbc(a,r,g,b)` 是 **alpha 在前**，与 egui 的
    `from_rgba_unmultiplied(r,g,b,a)` 顺序相反 —— 混用会画出
    "红色 + 近乎全透明"的浅色图标。所有卡片前景必须走
    `WidgetStyle::ink/ink_sub/rule`。
14. **卡片文本先量后排，且必须复用 galley**。`painter.text()` 内部会
    再排一次版，所以量宽后应 `painter.galley()` 画。缩字号前必须检查
    `avail > MIN_TEXT_WIDTH` 并把字号夹到 `>= 1.0`：卡片宽度退化到 0 时
    负字号会让 epaint 的 glyph atlas 直接 panic。
15. **几何量必须先夹非负，再喂给 `f32::clamp`**。`clamp(0.0, x)` 在
    `x < 0` 时直接 panic（"min > max"）。`place_slots` 的 `bar_w` 来自
    `total.min(lw - 4 - widget_extra)`，空条目 + 多个小组件会让它变负。
    所有几何计算里"检查尺寸 <= 0 就早退"必须写在 clamp **之前**，
    并同时挡 `NaN` / 无穷。可变上界的 clamp（如 `w as f32 - 1.001`）
    也要先保证上界非负。
16. **所有弹出面板的视觉样式只有一个来源**：`gpu::widgets::style::PanelStyle`。
    **底色一律走 `paint_base()`**：日历 / 控制中心 / 计时器 / 资源经
    `paint_frame()` 间接调用，速记 / 分组 / 编辑自己画叠加层与细边框，
    但底色必须调同一个方法（曾漏掉这三处，材质开关对它们静默无效）。
    文字层级统一走 `ink / ink_sub / faint / card / card_hover / line`；圆角只用
    `R_PANEL / R_CARD / R_FIELD / R_SMALL` 或 `pill(h)`，**不留锐角**。
    注意 `argbc(a,r,g,b)` 是 alpha 在前，与 egui 的
    `from_rgba_unmultiplied(r,g,b,a)` 相反 —— 这个项目已经因此错两次。
    配置经进程级 `sync_prefs` 同步，不要在单个面板里另读 `Config`。
    这些面板的物理位置统一在 `gpu::widgets/`（见不变式 17）。
17. **小组件代码按「一个面板一个模块」放在 `gpu::widgets/`**：
    `calendar / control / timer / resource / notes / group / editor / style / framework`。
    `lib.rs` 用 `pub use widgets::calendar as calendar_win` 保留旧路径，
    外部调用方可以逐步迁移；模块内部一律用 `widgets::` 新路径。
    新增小组件只需在 `widgets/mod.rs` 加一行声明，并在
    `core::config::panel::REGISTRY` 加一行描述。
18. **拖入走 `gpu::drop_target::DockDropTarget`（OLE `IDropTarget`）**，
    `DragAcceptFiles` 只是注册失败时的兜底。`DragOver` 只负责把落点写进
    [`Dock::set_external_drop`]，数据落地一律在 `Drop` 里走
    `CF_HDROP` → `push_dropped_paths` —— 与 `WM_DROPFILES` 共用同一条
    路径，别在两条分支里各写一套解析。`STGMEDIUM` 必须无条件
    `ReleaseStgMedium`。回调在 UI 线程（STA），可直接解引用
    `GWLP_USERDATA` 上的 `Dock`。
19. **插入让位是布局层职责，不是绘制层 hack**：
    `layout::place_slots_insert` 预留空槽并把 `insert_l` 交给绘制层；
    `Dock::update` 用上一帧 `slot_l` 补 `slide` 做平滑。移除预览同理，
    把被拖项宽度压 0 让两侧合拢，并用 `drag.is_some()` 自动失效。
    别在 `paint` 里硬挪图标 —— 那样命中测试会与视觉错位。
20. **取不到 shell 图标时画 `glyph::paint_kind(ItemKind)`，不读名称**。
    `Builtin::from_kind` 是穷尽匹配，新增 `ItemKind` 会被编译器逼着补
    预制图标。`paint_monogram`（名称首字）只保留给图标画廊展示，
    生产路径不得调用 —— 中文首字在 20px 下辨识度差、同名条目分不出。
21. **按住期间不重建栏体列表**。`App::sync_items` 在
    `Dock::press_active()`（按下即算，不等起拖阈值）时只置
    `items_stale`，由 `flush_items_stale` 在松手后补。`replace_items` 会
    清掉 `drag / press / hover`，边拖边重建的表现是"图标刚换位、拖动
    立刻断"。同理，`pending_moves` 是**队列**：一帧里可能连换好几次位，
    只留最后一次会让配置与栏体差一格（点 A 启动 B）。
22. **换位只在「可写回」的方向上跨段**（`Dock::clamp_segment` +
    `reorder_bar`）：前 `pinned` 项来自配置 `[Items]`，其后是运行中应用。
    栏体上"固定项夹在临时项中间"这种顺序配置装不下，所以：
    - **运行中应用 → 固定区**：允许，并在落点**就地固定**（`BarMove::PinnedNow`）。
      这就是"把正在运行的应用固定到 Dock"的手势，顺序天然可写回。
    - **固定条目 → 运行中区**：夹在固定区末尾。那不是"取消固定"的手势
      （取消固定走右键菜单），拖过去只会在松手时莫名跳回来。
    - 运行中应用之间可以换位，顺序写回 `temp_keys`（扫描线程按它保序，
      不写回会被 1.2 s 后的扫描冲掉），但**不落盘**：它是会话状态。
23. **长度键存 96 DPI 逻辑像素，物理换算只在渲染处做一次**。
    `Key::is_length()` 是唯一判定（表驱动键看 `NUMERIC_KEYS` 最后一列），
    收口只有 `layout::calc_metrics`（栏体几何）与 `Dock::pxf/pxi`（零散
    绘制点）两处。新增长度键只改表的最后一个布尔值，不要在读取处再乘
    `dpi/96`；弹出面板走 egui 的逻辑点（`pixels_per_point`），已经是
    DPI 无关的，重复缩放会让它在 150% 下变大 1.5 倍。ini 里的旧值是
    物理像素，由 `Config::migrate_units`（v3 → v4）一次性换算。

24. **面向用户的文案只有一条出口**：`core::i18n::t / fmt`，英文文案即键。新增文案必须登记在 `i18n/en/` 的对应字母段（拼接后有序、无重复），中文放 `i18n/zh_cn/` 的对应段，缺项自动回退英文；`i18n::tests` 扫源码守住登记完整性，`options.rs` 的测试守住下拉选项常量。下拉表只存英文、由 `combo` 一处翻译；用显示文案做命令分派要写成 guard 比较，不能进 `match` 模式。窗口标题是创建期的 Win32 字符串，语言变化时已打开的设置窗口要显式 `SetWindowTextW`。
25. **控件 ID 不是显示文案**：`Id::unique` 哈希传入的内容，翻译后的 label 会随语言变化而改变控件身份。ID 盐只用语义键（下标、动作编号、固定字符串）；egui 0.37 里 `Id::new` 已弃用，不要改回去。
26. **GPU 回读的每条返回路径都要 `unmap()`**，且 `Device::poll` 必须带超时。`unmap()` 会取消此前的 `map_async`，漏掉会让下一帧提交失败；`wait_indefinitely()` 在驱动挂起时会永久卡住 UI 线程（ADR-075）。
27. **Dock 内部只有一种坐标：逻辑轴 `(u,v)`**（`u` 沿栏、`v` 离屏幕边缘）。Win32 给的是窗口像素，必须在 WndProc / `DockDropTarget` / `cursor::client_pos` 这些边界经 `logical_point` 转换；反过来屏幕矩形（`slot_screen_rect`、`widget_screen_rect`、`hot_phys`、`icon_rect`）必须走 `map_rect`。唯一例外是烟雾光标的窗口像素，用 `window_point` 转回。漏掉 `to_logical`，靠左/靠右时命中、放大、拖放与弹窗锚点会整体错位（ADR-077）。
28. **全局热键只有一个所有者**：`platform::hotkey::GlobalHotkey` 用 RAII 注册/注销，组合由 `[Hotkeys] Toggle` 经 `core::hotkey::Hotkey` 解析；`App::sync_global_hotkey` 只在配置变化时重绑，被占用时回滚并提示一次。`[Settings] Hotkey` 只是 0/1 开关，不要把组合塞回那 76 个与 C 版一一对应的键（ADR-078）。
29. **几何按目标显示器现算，不缓存主屏**：工作区、`AppBar` 预留、DPI 都从 `[Settings] Monitor` 取（`work_area` / `monitor_rect` / `monitor_dpi`），越界回退主屏；`App::sync_monitor` 在序号或 `Dock.geom_epoch`（`WM_DPICHANGED` / `WM_DISPLAYCHANGE` 自增）变化时重算，`step_relayout` 同时比较尺寸与原点。弹窗按锚点钳制（`widgets::anchor` + `display::monitor_at` / `work_area_at`），没有锚点的居中面板读 `display::target_work_area()`。任何新窗口都不要直接写 `work_area(0)` / `monitor_rect(0)`（ADR-079）。
30. **抓屏背景要求窗口把自己排除出捕获**：`frost::Live` 读的是「屏幕在该矩形上显示的内容」，活窗口不调 `exclude_from_capture`（`WDA_EXCLUDEFROMCAPTURE`）就会抓到自己；`x ← blur(0.6·tint + 0.4·x)` 的不动点是一片纯色，磨砂会静默退化成塑料。排除只在真的画磨砂时开（`Config::frost_enabled()`），关闭或退出走 `allow_capture`，代价是开着磨砂的窗口不进截图/录屏。背景按件取：`FrameRenderer::frost_backdrop` 给出 `frost::Backdrop { tex, win }`，绘制一律走 `chrome::rounded_frost`，UV 是「窗口内分数」，不要整张 `Shape::image` 贴上去——栏体上下留出的悬垂与倒影边距里没有模糊底；`draw_frame` 里那句整窗 `painter().image(...)` 由 `FrameRenderer::set_frost_whole_window` 控制，默认给面板、栏体在启动时关掉。换帧由 `Live::has_frame` 驱动（`App::step_frost` 返回 `true` 才重绘），不要用定时器轮询，否则静止时也会出帧，破坏零帧待机；只改半径走 `set_frost_spec`，不要重启线程（ADR-080）。
31. **同一条线程上的两个窗口，一次只画一个**：栏体、设置窗口、起动台和七个面板都在一条 UI 线程、一个 wgpu 设备上，一轮循环只购买一次 present。`pump()` 只把 `msg.hwnd == 栏体` 的消息算作「栏体有新消息」，其他窗口的输入只打断等待；设置窗口的连续重绘按显示器刷新节流（不是 1 ms），且它一进入连续动画（`SettingsWindow::animating`）栏体就当轮停画，`wait(yielded)` 同时收起烟雾的帧间隔唤醒。判断「谁该出帧」只看这两个事实，不要用「设置窗口可见」当条件——光标闪烁和一次性刷新必须让栏体继续动（ADR-081）。

32. **组件面板的位置与背景只有一条出口**：定位一律走
    `framework::place_panel`（内部调 `placement::place`，锚点几何只有
    `widgets::anchor` 一份；`monitor_of` 与 `work_area_of` 必须用同一个
    `center_of`，否则副屏锚点会按主屏工作区居中）。底色一律走
    `PanelStyle::paint_base`，抓屏绑定走
    `framework::{apply_backdrop, refresh_backdrop, release_backdrop}`。
    开合一律走 `framework::show_panel` / `show_panel_active` / `hide_panel`：
    定位 → `SetWindowPos` → 绑背景 → `ShowWindow` 的顺序只有这一份。面板自己
    不再调 `centered_on_target` / `SetWindowPos` 算位置，也不自己
    `start_live_frost`；窗口尺寸变化后必须 `refresh_backdrop`（旧模糊纹理会被
    拉伸），首次抓屏失败必须 `release_backdrop` 交回截图可见性。静态面板的
    `due()` 必须把 `renderer.frost_pending()` 并进判据，否则抓屏线程产出的
    新帧不会触发重绘，磨砂会停在第一帧（ADR-080 同一条约束）。模态编辑框是
    显式例外：按光标定位并在屏幕内钳制，不读 placement policy。

33. **预加载与懒加载共用同一批 slot**：`App::preload_panels()` 只填 `None`
    的六个面板 slot，创建失败的留空、由点击路径补建；预加载的窗口一律隐藏，
    不调用 `show_*`。预加载不得绕过 `new()` 里的 `attach()` ——
    `GWLP_USERDATA` 必须指向 `Pin<Box<Self>>` 的最终地址（不变式 2）。
    `reset_panels_for_scale()` 销毁后本会话回到懒加载，下次启动再预加载。
    **预加载不得读盘或读注册表**：`[Tool]` 由 `boot` 解析进 `App::tool_state`，
    暗色判定用主循环缓存的 `system_dark`；写盘提交与 DPI 重建前同步回缓存
    （ADR-090）。

34. **磨砂卷积的边缘不得钳制到边界像素**：`frost::paint::edge_taps` 与
    `frost.wgsl` 的两趟卷积都必须丢弃越界 tap 并按实际权重和归一化。
    钳制会让左上角在水平、垂直两趟同时反复采样同一个像素，放大后就成了
    纯色方块；这是所有磨砂窗口共用的内核，改一处必须两处同步，并跑
    `#[ignore]` 的 CPU/GPU 逐通道对照。`GpuBlur::Target` 把 `corner` 也算进
    重建判定，避免脏列更新时四角残留旧 alpha（ADR-091）。

35. **柔光是抓屏内容的函数，不是常量蒙版**：`frost::glow::Field` 从捕获的
    桌面降采样成 4×4 低频亮度，`mesh()` 展开成 5×5 顶点；两者与
    `soft_light()` 都是纯函数，必须保持无 Win32 依赖以便无头单测。柔光层
    画在整窗 frost sheet 之后、`build(ui)` 之前，**栏体（`frost_whole=false`）
    不画**，否则透明的悬停区与倒影边距会出现矩形亮块。强度由
    `frost::glow_strength()` 发布，`panel_style::sync_prefs` 负责同步
    `GlowStrength`（ADR-092）。

36. **8-bit 后处理输出必须抖动，磨砂只有一套开关**：`frost.wgsl` 在
    `textureStore` 前加整数哈希的 ±0.5 步噪声，CPU
    `apply_dither` 用相同的 wrapping 运算。磨砂纹理放大 4 倍显示，不抖动就会
    看到色带；改公式必须两边同步，CPU/GPU 最终图像的容差 2 就是留给抖动的
    合法余量（ADR-093）。磨砂材质只有 `BgMode`、强度只有 `Blur`，
    `frost_spec` 不再接收 per-surface key；`PanelBg` / `UiFrost` / `LpFrost` /
    `MenuFrost` / `GroupFrost` 是兼容键，`WidgetBlur` 在启动时并入 `Blur`。

37. **磨砂圆角是几何，不是纹理 alpha**：25% 抓屏纹理必须保持方形全不透明，
    圆角由 `frost::round::{outline, fan, image}` 在屏幕空间裁剪，柔光 mesh 用
    同一轮廓。烘焙进低分辨率纹理的圆角放大后必然阶梯；改圆角只改
    `Spec.corner_pt` 的记录与绘制，不动卷积（ADR-097）。

38. **资源监视器的行只由 `ResOrder` 决定**：面板不得硬编码三项，也不得在绘制时
    读 ini；采样器与行几何分别留在 `platform` 与 `gpu` 的纯函数里，窗口尺寸随
    行数重建，标题下不画分隔线（ADR-098）。
39. **资源读数只有一个生产者**：`worker::SysSampler` 每秒在后台线程采样，UI 与
    面板只读 `Dock::sysinfo`，不得再调 `GetSystemTimes` / `GetIfTable2` / PDH。
    资源卡片不可见时线程暂停，空闲帧数仍为零。GPU 利用率取最忙引擎，温度读不到
    就留空——面板不显示占位数字（ADR-099）。
40. **窗口句柄在对象存在前也必须有人负责**：`CreateWindowExW` 成功后立刻交给
    `framework::WindowLease`，`FrameRenderer::new` 失败时由租约销毁；成功则
    `defuse()` 交给 `Pin<Box<Self>>`。`attach` 只在 `new` 内调用一次，预加载与
    懒加载路径不得二次挂接。`Drop` 统一走 `framework::destroy_panel`：释放抓屏 →
    清 `GWLP_USERDATA` → `DestroyWindow`，顺序不能反，否则销毁消息会解引用
    半析构对象（ADR-100）。
41. **外部程序只经 Host Link v1 清单接入**：`platform::hostlink` 按协议第 2 节
    六条通道顺序发现清单，用 `app.id` 唯一去重，只 spawn `commands.*` 里原样给出的
    exe 与 args。`launchpad::scan()` 把清单条目排在 `.lnk` 前（同名去重时保留清单
    条目），点击先问 provider，未命中才回退 `open_uri`。探测失败按报告里的
    `repair` 恢复，`EXE_CHANGED` / `LINK_STALE` / `EXE_MISSING` 重读清单；拿不到
    stdout 时读 `host-probe.json` / `host-last.json`。禁止连接私有 IPC 管道、
    硬编码应用名或把一次探测失败当永久移除（ADR-103）。
42. **磨砂包边只在几何轮廓带内做 3D**：`frost::round::image` 用三层同心环把
    已高斯的磨砂纹理沿外法线弯曲取样，迎光侧保留、背光侧压暗；面板
    `PanelStyle::paint_frame` 再叠内收暗棱与亮线。不要新增全屏折射 pass，
    也不要把包边烘焙进 25% 抓屏纹理（ADR-104）。

## 八、与 C 版的模块映射

| 原版（minidock.c） | Rust | 状态 |
|---|---|---|
| `LoadConfig` / `SaveSettingsOnly` / `g_tab[]` | `core::config` | ✅ 已迁移 + 夹紧 + 单测 |
| `ParseItemLine` / `SaveNow` / `LoadItems` | `core::item` / `core::store` | ✅ 已迁移 + 单测 |
| `CalcMetrics` / `MapR` / `ToLogical` / `HitIndex` | `core::layout` | ✅ 已迁移 + 单测 |
| `InitPaths` | `platform::paths` | ✅ |
| `GetMon` / `GetDeviceCaps` | `platform::display` / `win` | ✅ |
| `SystemDark` / `GetAutostart` | `platform::theme` | ✅ |
| `BuildRows` / `PaintSettings` / `SetProc` | `settings::pages`（12 页，一页一文件） | ✅ 布局重做（ADR-049） |
| `KidAdd` / `MergeInto` / `ExtractKid` / `DissolveGroup` | `core::group`（纯逻辑） | ✅ 归组/出组/解散，单测覆盖 |
| `BuildGroupIcon` | `core::icon::composite_quad` + `worker::group_icon` | ✅ 2×2 四宫格 |
| 组展开 | `gpu::group_win`（独立分层窗口 + 自己的 `IconLoader`） | ✅ 网格面板 |
| 重命名 / 新建对话框 | `gpu::edit_win`（模态弹窗，`WM_CHAR` + IME） | ✅ |
| 单项图标 | `bin/dock/item_edit.rs::change_icon` + `Dock::clear_icon` | ✅ 精确失效一张纹理 |
| 组内排序 | `core::group::move_kid` + `group_win` 悬停箭头 | ✅ |
| 托盘 | `platform::tray`（`Shell_NotifyIconW`）+ `bin/dock/item_cmds.rs::step_tray` | ✅ 右键菜单复用 `gpu::menu`，不用 `TrackPopupMenu` |
| 速记 | `core::note` + `gpu::note_win` + `gpu::edit_win`（多行） | ✅ 面板只做列表与动作分发，文本录入交给模态弹窗 |
| `RenderFrame` / `DrawBar` / `DrawBubbleAt` | `gpu::dock`（待写） | ⏳ M1 |
| `LoadItemIcon` / `HiconToGp` / `FitBitmap` | `platform::icons`（待写） | ⏳ M2 |
| `UpdateFrost` / `BlurPass` | `gpu::frost`（DXGI + compute） | ⏳ M3 |
| `DT_*`（OLE 拖放） | `platform::drop` | ⏳ M2 |
| `ShowMenu` / `ShowGroup` / `ShowFan` / `ShowPreview` | `gpu::popups` | ⏳ M3 |
| `SetMinHook` / `AnimStep` | `platform::minimize` | ⏳ M4 |

函数级台账与验收标准见 [`PORTING.md`](PORTING.md)。

## 九、模块划分与文件规模

一个 `.rs` 只放一个职责，生产代码单文件 ≤ 600 行；超过就按职责拆成子模块。
规则与拆分手法见 [`CONTRIBUTING.md`](CONTRIBUTING.md) 第六、七节；两条规则分别由
`scripts/check-comments.sh` 与 `scripts/check-size.sh` 守住，本地钩子、CI 的
`gate` job 与 `--gate` 跑同一批脚本。注释门禁扫 `*.rs` 与 `*.wgsl`（WGSL 的
`//` / `/* */` 语法见 W3C WGSL §3.3）；规模门禁按「排除末尾 `#[cfg(test)]` 模块
后的行数」判定，独立测试文件按总行数判定，上限同为 600。

拆分的落法是稳定的：`foo.rs` → `foo/mod.rs`（类型、`use`、子模块声明）+
`foo/{state,paint,input,...}.rs`。`impl` 块可以分散在同一 crate 的多个子模块里，
跨子模块调用的私有项标 `pub(super)`。外部路径（`nooksill_gpu::dock::Dock`）
不变，因此调用方无感。

当前拆过的大文件：

| 原文件 | 拆分后 |
|---|---|
| `gpu/dock.rs` | `dock/{mod,input,items,state,paint,paint_cards,chrome}.rs` |
| `gpu/bin/dock.rs` | `bin/dock/{mod,boot,run_loop,wndproc,sys,sync,clicks,drag,auto_hide,render,menu_win,satellite,item_cmds,item_edit,settings_cmd,panels_state,panels_notes,resmon,update,misc}.rs` |
| `gpu/launchpad_win.rs` | `launchpad_win/{win,nav,compose,paint,paint_folder,input,commands,lifecycle}.rs` |
| `gpu/menu.rs` | `menu/{build,layout,paint}.rs` |
| `gpu/frost.rs` | `frost/{capture,live,paint}.rs` |
| `gpu/worker.rs` | `worker/{scan,icons,disk,open}.rs` |
| `gpu/glyph.rs` | `glyph/{builtin,canvas,controls}.rs` |
| `gpu/settings_win.rs` | `settings_win/{win,window,shell,render,input}.rs` |
| `gpu/frame.rs` | `frame/{setup,render,frost_state}.rs` |
| `gpu/widgets/*.rs` | 每个面板一个目录，`mod.rs` 只放类型与子模块声明 |
| `core/config.rs` | `config/{mod,keys,key_query,apply,ini_io,migrate}.rs` |
| `core/layout.rs` | `layout/{mod,metrics,slots,hit,window}.rs` |
| `core/smoke.rs` | `smoke/{mod,fluid,noise,raster}.rs` |
| `core/launchpad.rs` | `launchpad/{mod,order,tags,apps,persist}.rs` |
| `settings/pages/widgets.rs` | `pages/widgets/{mod,card,row,button,toggle,slider,combo,pickers,sidebar,motion}.rs` |
| `gpu/widgets/notes.rs` | `notes.rs`（窗口生命周期）+ `notes/paint.rs`（行布局、绘制、分页、纯函数测试） |
| `gpu/dock/input.rs` | `input/{events,hit,drag}.rs`；`impl Dock` 分散，跨段私有项用 `pub(in crate::dock)` |
| `gpu/bin/probe.rs` | `probe/{wgpu_device,d3d11_device,gl_device}.rs`；binary crate root 的子模块用 `#[path]` 指路 |

三处拆分的外部路径都不变：`note_win::paint_preview` 由 `notes.rs` 重导出，
`Dock` 的方法仍是同一类型上的 `impl`，probe 的三个设备模块仍以 `super::`
访问 crate root 的定义。

规模门禁不允许给纯数据表留白名单：`en.rs`(599) / `zh_cn.rs`(592) 已按字母段
拆到 `en/` / `zh_cn/`，骨架各 30 行左右、数据段各约 300 行，靠 const 块拼回
单一 `TEXT`（ADR-076）。再加文案只需放进正确的字母段。

新增的 `core/src/i18n/` 按同一套手法集中组织：`mod.rs`（语言状态、查表、
`fmt`）、`en.rs` / `zh_cn.rs`（骨架 + 分段数据）、`zh_tw.rs`（部分翻译）、
`tests.rs`。`platform/src/locale.rs` 只负责读系统 LANGID，不含任何文案。

测试文件同样拆：`tests.rs` → `tests/{mod,分组}.rs`，共享辅助函数留在
`tests/mod.rs`，子模块首行 `use super::*;` 继承父模块的导入。
已拆：`gpu/dock/tests/`、`core/layout/tests/`、`core/config/tests/`。

42. **扩展设置只展示快照，不碰系统接口**：`nooksill-settings` 刻意不依赖
    `windows`，扩展页读 `SettingsState::extensions`（宿主用 `hostlink::snapshot()`
    填充），按钮只发 `Command::HostLink{Rescan,Check,Activate}`；真正的发现、
    probe 与 spawn 留在 `platform::hostlink`。两个开关来自 Config，宿主在
    设置 tick 与打开启动台时同步给 provider（ADR-104）。

43. **扩展小组件用 scheme 路径，不新增 WidgetKind**：设置页「添加到 Dock 栏」写入
    `mc.host-link:<app.id>` 的普通 `Item`；`Builtin::from_path` 识别前缀并画剪贴板
    图标，点击（含右键 Open）只调 provider 的 `activate_id`，不交给 ShellExecute。
    launchpad 条目仍用真实 exe 路径取图标，两套入口互不冲突（ADR-104）。

44. **扩展命令不在 UI 线程执行，也不持全局锁**：Dock 条目与 launchpad 点击只把
    app.id 交给 `worker::hostlink`；后台线程先从 provider 复制清单，再用独立
    provider spawn `commands.*`，最后短暂写回缓存。`ProcessRunner` 对子进程设
    2 s 上限，避免首次权限提示或忙碌的 daemon 让 Dock 卡死（ADR-104）。

45. **插件是纯数据清单，不是代码加载**：`platform::plugin` 扫描两处插件目录的
    `plugin.json`（`mc.dock-plugin/v1`），只接受已声明的数字键与 RGB 强调色，
    启动时和开关变更时经 `Config::apply_i32` 叠加到配置；没有 DLL、脚本或
    自定义 IPC。清单上限 64 KiB，列表每 5 秒自动重扫，可选 `description` 在
    设置页展示。启用列表存 `[plugins] Enabled`。两个启用插件改同一键时，
    插件行提示重叠键名；Host Link 健康码在 UI 上翻译为可读文案，原始码保留在
    tooltip（ADR-105）。

46. **真机测试不用 `#[ignore]` 藏起来**：硬件/桌面测试统一用 `test_env` 运行时
    探测，有设备就执行，headless CI 快速返回；门禁因此报告 0 ignored，
    测试数真实反映可运行项（761 项）。

47. **隐藏窗口必须交回 DIB，但只交 DIB**：任何新增的常驻窗口在隐藏路径上要调
    `FrameRenderer::release_dib()`（面板走 `framework::hide_panel` 已统一收口），
    否则每个空闲窗口都会白占一份 `w × h × 4` 的系统内存。反过来，**不要顺手把
    `offscreen` 也释放**：GPU 目标分配时确实计入 WS，但释放拿不回来、再分配
    也不再花钱（分配器池化），所以释放它只有 churn 没有收益，私有提交稳态
    还抬高约 48 MiB，且该抬高的归属尚未定位（ADR-117）。
    释放只能发生在窗口确实隐藏之后 —— 可见窗口释放会露出一帧陈旧 DWM 内容。
    尺寸由 `FrameRenderer::size` 单独记录，释放期间 `resize` 只改记录不重建，
    所以菜单每次开合仍不会多分配一次。
