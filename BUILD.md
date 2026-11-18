# 构建

## 一、实测环境（2026-10-27 核对）

| 组件 | 版本 | 位置 |
|---|---|---|
| rustc / cargo | 1.99.1 | `C:\Users\Cookies\.cargo\bin` |
| 工具链 | `stable`（默认 gnu，另装 msvc） | `rustup toolchain list` |
| MSVC 工具集 | 14.52.36629（VS 18 Insiders） | `D:\Program Files\Microsoft Visual Studio\18\Insiders\VC\Tools\MSVC\` |
| Windows SDK | 10.0.28000.0（另有 26100.0） | `C:\Program Files (x86)\Windows Kits\10` |
| MinGW-w64 | 16.2.0（仅用于编译 C 版对照） | WinGet `BrechtSanders.WinLibs.POSIX.UCRT` |

目标三元组固定 `x86_64-pc-windows-msvc`，`rustflags = ["-C","target-feature=+crt-static"]`。

**为什么是 MSVC 而不是 GNU**：系统 DLL（GDI+/DWM/shell32）都是 MSVC ABI 的第一等公民；
`+crt-static` 把 CRT 静态链进去，产物不依赖 `VCRUNTIME140.dll`；崩溃转储与 WinDbg 配合最好。
代价是本机 VS 装在 `D:\Program Files`（非常规路径），系统 PATH 里没有 `cl.exe`。

## 二、构建命令

```bash
source scripts/env.sh              # 必须。注入 INCLUDE/LIB/PATH + CC/CXX/AR
./scripts/build.sh                 # debug
./scripts/build.sh --release       # release（opt-level=z + fat LTO + strip）
./scripts/build.sh --test          # 全部测试（跳过 doctest）
./scripts/build.sh --clippy        # -D warnings
./scripts/build.sh --fmt           # 仅检查格式
./scripts/build.sh --gate          # 六步门禁：注释 + 规模 + 提交信息 + fmt + clippy + 测试
./scripts/check-comments.sh        # 只跑注释门禁
./scripts/check-size.sh            # 只跑单文件规模门禁
./scripts/check-commit-msg.sh <file> # 只跑提交信息检查（--self-test 跑自测）
./scripts/build.sh --exe           # 打印产物路径
./scripts/build.sh --version       # 打印本次构建钉住的版本号
./scripts/package.sh [--release]   # 构建 + 按版本号打包到 dist/（CI 用同一脚本）
```

PowerShell 等价入口：`powershell -ExecutionPolicy Bypass -File scripts\build.ps1 -Mode test`。

**测试会弹系统授权框的坑（Windows 11 24H2）**：`WlanQueryInterface` 取
`wlan_intf_opcode_current_connection` 被纳入**精确位置**授权，首次调用弹
「Allow … to access your location?」并**阻塞**测试进程。cargo 每次改依赖都换
测试 exe 名，系统认成新应用，于是每次门禁都弹。`test_env::consent_probes()`
默认关掉该探测，要跑就 `NOOKSILL_CONSENT_PROBES=1`。生产 exe 路径固定，只弹一次。
依据：<https://learn.microsoft.com/zh-cn/windows/win32/nativewifi/wi-fi-access-location-changes>

`env.sh` 的三条注入路径（按优先级）：
1. PATH 里已有 `cl.exe`（CI 的 `msvc-dev-cmd`、已初始化的 vcvars shell）→ 只补 `CC/CXX/AR`，不动 `INCLUDE/LIB`；
2. `vswhere` 定位任意 VS 实例 → 取该实例下版本号最大的工具集；
3. 目录扫描 `Program Files / D:\Program Files` 兜底。

SDK 版本**自动取最新的**，不写死小版本号——写死是换机后最先失效的东西。

## 三、依赖版本

`rust-version = 1.95`（egui 0.37 / egui-wgpu 0.37 的下限）；工具链只声明
`channel = stable`，不锁具体版本号。

| crate | 版本 | 说明 |
|---|---|---|
| `egui` | 0.37 | 只开 `default_fonts`；中文字体运行时装载 |
| `egui-wgpu` | 0.37 | 提供 `Renderer` 与 `wgpu` **重导出** |
| `wgpu` | 30.0.1 | **不单独声明版本**，用 `egui_wgpu::wgpu`，避免与 egui 侧漂移 |
| `egui_kittest` | 0.37 | 仅 dev-dependency，不进产物 |
| `windows` | 0.62.2 | 官方绑定，feature 并集写在 workspace |
| `rayon` | 1.12 | **唯一用途**是图标像素处理（ADR-007 / ADR-037） |
| `pollster` | 1.0 | 启动时同步等 `request_device` / `enumerate_adapters` |
| `glass-backdrop` | git rev `747457c` | 只用 capture / blur / glass_sdf 三个数据模块（ADR-057） |
| `winresource` | 0.1.34 | 仅 build-dependency，把 `assets/nooksill.ico` 编进 exe |

`windows` 的 feature 并集在 workspace `Cargo.toml` 一次声明；i18n 只多用了
`Win32_Globalization::GetUserDefaultUILanguage`（读系统显示语言，返回 LANGID）。

**没有 eframe、没有 winit**。理由见 [`DECISIONS.md`](DECISIONS.md) ADR-002。

三处历史遗留，别照着旧文档加回来：

- `crossbeam-channel` / `arc-swap` 已在 ADR-037 删除 —— 跨线程传的是值拷贝与
  `std::sync::mpsc`，队列深度只有个位数，用不上共享快照。
- `raw-window-handle` 不在依赖里：ADR-008 之后不走 `wgpu::Surface`，
  没有需要传给后端的窗口句柄。
- `Win32_Graphics_GdiPlus` feature 已随 GDI+ 老实现一起删掉（ADR-035）。

## 四、版本号与产物

### 4.1 版本号怎么来的

**唯一来源是 `scripts/version.rs`**，被两个 `build.rs` `include!` 进来：

| 条件 | 版本 | 用在哪 |
|---|---|---|
| `NOOKSILL_VERSION` 有值 | 原样使用 | CI 显式指定（预留） |
| `PROFILE=release` | `X.Y.Z`（`Cargo.toml` 的 workspace 版本） | Release 稳定版 |
| 其它（debug） | `X.Y.Z-<构建编号>-<提交号>` | Debug 版（如 `1.1.0-66-fe678af`） |

构建编号取 `NOOKSILL_BUILD`，没有就取 `git rev-list --count HEAD`（随提交单调递增），
再没有就是 `0`。提交号是 7 位短哈希：CI 用 `GITHUB_SHA`，本地 `git rev-parse --short`。
CI 不另设构建号：它多带一个 `fetch-depth: 0` 的 checkout，用同一串提交数，
所以同一个提交在本地和在 CI 得到同一个号。

三处必须一致，所以只在构建期算一次、分发下去：

```
scripts/version.rs ──┬── crates/nooksill-core/build.rs  →  env!("NOOKSILL_VERSION") → nooksill_core::VERSION（关于页）
                     ├── crates/nooksill-gpu/build.rs   →  exe 的 VERSIONINFO（属性 → 详细信息）
                     └── target/<triple>/<profile>/NOOKSILL_VERSION  →  package.sh 的产物名
```

`./scripts/build.sh --version` 直接读最后那个文件；`scripts/package.sh` 也只读它，
**不重新推导** —— 产物名必须等于 exe 里那个号。

### 4.2 直接产物

| 目标 | 路径 | 说明 |
|---|---|---|
| `nooksill-dock` | `target/x86_64-pc-windows-msvc/{debug,release}/nooksill-dock.exe` | Dock 主程序。ADR-035 删除 GDI+ 老实现后**唯一**的 Dock 可执行文件 |
| `probe` | 同目录 `probe.exe` | 显存/后端探针，只建设备不建窗口（`scripts/probe-mem.sh`） |
| `settings_shot` | `cargo run -p nooksill-gpu --example settings_shot -- <出图.bmp>` | 离屏渲染任意一页设置界面，走真实 `FrameRenderer` |
| `dock_shot` | `cargo run -p nooksill-gpu --example dock_shot -- <出图.bmp> [light] [grad] [insert] [bottom/top/left/right]` | 离屏渲染 Dock 栏（含倒影/边框），倒影走与真实窗口同一条 `mirror_rows`；最后一项可选，用于核对四边停靠 |

`Cargo.toml` 里 `[[bin]]` 是显式声明的；`src/bin/` 下没有其它自动发现的可执行文件。
README 里早期提过的 `nooksill-spike`、`NookSill.exe` 都已在 ADR-035 / ADR-036 删除。

### 4.3 分发包（`scripts/package.sh`）

```
dist/NookSill-1.0.0-win64.exe
dist/NookSill-1.0.0-win64.exe.sha256
dist/NookSill-1.1.0-66-fe678af-win64-debug.exe     （--pdb 时另加同名 .pdb）
dist/NookSill-1.1.0-66-fe678af-win64-debug.exe.sha256
```

- **裸文件，不压 zip**：GitHub Release 上是 exe 与校验和两个下载项，用户下下来
  就能跑，不需要先解压。
- 校验和是 `sha256sum` 的标准格式（`<hash>  <文件名>`），
  `sha256sum -c NookSill-1.0.0-win64.exe.sha256` 直接能验。
- 包内 exe 叫 `NookSill-<版本>-win64[-debug].exe`（产品名），而 cargo 产出的
  二进制仍叫 `nooksill-dock.exe`；改名只发生在复制到 `dist/` 这一步。
  代价：`--pdb` 出来的 `.pdb` 与 exe 不同名，调试器不会自动找到它，
  需要手动加载（所以 pdb 默认不进包，它本来就是本地排查用的）。
- Debug 的 pdb 有 200 MB 量级，默认不打包。

首次 `cargo build` 要编译 wgpu（naga、wgpu-hal、DX12 后端），**冷启动 3–6 分钟**属正常；
之后增量在 10–30 秒。release 首次更久（fat LTO）。

## 五、内存：历史实测基线与可执行 NFR（实测 2026-10-27 / 2026-11-17 · AMD Radeon 610M + NVIDIA RTX 5070 Laptop · 2560×1600@150%）

体积：`nooksill-spike.exe` release **7.2 MB**（`opt-level=z` + fat LTO + strip），
debug 43 MB（符号占绝大部分）。

内存：`probe.exe` 只建设备、不建窗口、不跑 egui，由进程内 `mem.rs` 自采样
（`PROCESS_MEMORY_COUNTERS_EX` 三口径：WorkingSetSize / PeakWorkingSetSize /
PrivateUsage）。早期文档写「与界面 RSS 同一口径」已失真：当前性能页「当前占用」
只显示帧耗时 / fps / Present（`settings_win/render.rs`），资源监视器报的是系统级
内存（`platform::sysinfo`）。完整 dock 进程的常驻测量改用 `scripts/mem-guard.sh`。
`PROBE_ALLOC=1` 时再分配与真实应用等量的三块：1320×1080 渲染目标（8.4 MB）
+ 同尺寸回读（5.4 MB）+ 同尺寸 DIB（5.4 MB）。

| 适配器 | 后端 | 设备 WS | 设备+缓冲 WS | 设备 私有 | 设备+缓冲 私有 |
|---|---|---|---|---|---|
| AMD 610M（核显） | dx12 / wgpu | 164.2 | 227.7 | 358.8 | 423.5 |
| AMD 610M | vulkan / wgpu | 103.2 | 169.1 | 279.7 | 349.8 |
| AMD 610M | **d3d11 原生** | **33.7** | 33.9 | 24.8 | 44.1 |
| NVIDIA 5070（独显） | dx12 / wgpu | 86.8 | 150.8 | 328.6 | 396.1 |
| NVIDIA 5070 | vulkan / wgpu | 79.5 | 144.7 | 258.0 | 329.0 |
| NVIDIA 5070 | **d3d11 原生** | **39.1** | 40.5 | 54.8 | 72.2 |
| NVIDIA 5070 | OpenGL（WGL） | 51.9 | 51.9 | 47.3 | 58.1 |
| 软件光栅 | d3d11 WARP | 13.8 | 14.0 | 3.1 | 19.5 |

**结论一：wgpu 比原生 D3D11 贵 2.5–5 倍。** 同一块 AMD 核显 164 MB vs 34 MB，
同一块 NVIDIA 独显 87–102 MB vs 39 MB。差价在 wgpu-hal 与 D3D12 运行时，
不在业务代码 —— 这组数字是在一行渲染都没跑的空进程里测的。

**结论二：19 MB 的缓冲在 wgpu 下被放大成 +64 MB 工作集**（DX12 / Vulkan 都是）。
D3D11 下同样三块只让私有提交 +19 MB、工作集几乎不动（没写过的页不驻留）。
**两条限定**（2026-11-18 补）：① 这是 ADR-013 调优**之前**的数字，不是现值；
② 它测的是**分配方向**。`mem_surface` 实测释放方向不对称——GPU 目标分配抬
WS，释放却一点不还，再分配还不要钱（内存被分配器池化留下）。所以本条与
ADR-117「释放 GPU 目标不省 WS」**并不矛盾**，别把两者当成互相推翻。

**结论三：OpenGL 是"保住 egui 又省内存"的中间档。** 52 MB，约 wgpu-DX12 的 1/3，
原生 D3D11 的 1.3 倍。据此曾提议 ADR-011（换 `egui_glow`），**已否决**。


### 调优：两个参数把 DX12 打下来（2026-10-27，AMD 610M）

不改框架、不换后端，只调 `DeviceDescriptor` 的两个字段：

| 配置 | 工作集 | 私有提交 |
|---|---|---|
| 默认（`Limits::downlevel_defaults` + `MemoryHints::Performance`） | 164.1 | 356.7 |
| `max_non_sampler_bindings: 4096` | 133 | 266 |
| 再加 `MemoryHints::MemoryUsage` | **102** | 74–110 |
| 实际 spike（release，设置窗 1320×1080） | **271 → 111** | 429 → 113 |

两条依据都来自 wgpu 自己的文档：

- **`max_non_sampler_bindings` 默认 1_000_000**。“This limit only affects the
  d3d12 backend… at the cost of a large up-front allocation at device creation”，
  核显共享系统内存，所以这笔预分配直接算在进程上。压到 4096 省 31 MB，
  再往下（256/64）没有额外收益。
- **`memory_hints` 默认 `Performance`**，会让分配器按性能多留块。
  Dock 每帧只有一两个 bind group，`MemoryUsage` 又省 31 MB，且不改变渲染结果
  （像素核对：面板 `(29,30,34)`、圆角外透出壁纸，与调优前一致）。

**不要用 `downlevel_webgl2_defaults()` 省内存**：它的
`max_texture_dimension_2d` 只有 2048，而 2560 宽的 Dock 在 150% DPI 下
就要 3840 px 的离屏纹理。实测三档限制的内存完全一致，没有收益只有风险。

**也顺带纠正一个隐患**：原先用 `Limits::downlevel_defaults()` 同样把纹理上限
卡在 2048，只是设置窗只有 1320 宽才没暴露。现在改成 `..adapter.limits()`，
只收紧描述符预算。

**同一个探针还测了另外三条路**（都不如上面这条划算）：

| 方案 | 工作集 | 说明 |
|---|---|---|
| wgpu 自带 GL 后端（`Backends::GL`，wgpu-hal 的 WGL 实现） | 117 | 保留 egui-wgpu 只换后端，但比调优后的 DX12 还高，且落在独显上 |
| 原生 OpenGL 上下文 | 52 | 要换 `egui_glow`（ADR-011，已否决） |
| 原生 D3D11 | 34 | 要自研 UI，失去 kittest |

外部佐证：wgpu 上游 issue 与 Slint #13470 记录的同量级现象（D3D12 路径比
OpenGL 路径多 ~110 MB），以及一个独立项目把 wgpu-DX12 的空设备地板测成
~300 MB 并最终放弃压缩 —— 结论一致：**这笔钱在 D3D12 栈里，不在业务代码**。

### CPU 基线

| 场景 | 结果 |
|---|---|
| 静止（单实例，光标不在窗口上） | **0.000 秒 / 30 秒** —— 线程阻塞在 `MsgWaitForMultipleObjectsEx(INFINITE)` |
| 悬停 / 动画 | 按 `repaint_delay` 排帧，`ZERO` 时节流到 60 fps |
| 单帧 CPU（1320×1080，含 GPU 回读与呈现） | release 下 1–2 ms（见性能页） |

查"为什么还在重绘"：`NOOKSILL_TRACE=1` 运行，每秒一行，
含累计帧数、`hover` 位置与 `repaint_causes()`。

### 方法学（不写清楚就会得出错误结论）

- **必须按适配器分组。** 本机是 NVIDIA + AMD 双显卡：wgpu 的 `LowPower`
  默认选核显，`D3D11CreateDevice(NULL)` 默认选独显 —— 不钉适配器的对比里
  "87 vs 164"有一半是换了块显卡。用 `PROBE_ADAPTER=AMD|NVIDIA` 钉住，
  `PROBE_LIST=1` 先看清单。
- **工作集 ≠ 私有提交。** D3D11 下缓冲几乎不体现在工作集里（惰性提交），
  看私有列才是"承诺了多少"；wgpu 两列都高，因为它同时抬高驻留与提交。
- **驱动有 ±15 MB 抖动**（NVIDIA dx12 稳态 87 / 峰值 102），比较时看峰值列。
- **混合显卡上 GL 上下文落在独显**（本机是 NVIDIA），所以 GL 行不与 AMD 组同卡。
- 早期文档把本机写成 "Intel Iris Xe"，那是把 wgpu 打印的 `device=5710`
  （PCI id）当成了名字：`5710 = 0x164E` 是 AMD Radeon 610M。

复现：`scripts/probe-mem.sh`（`PROBE_HOLD=20` 更稳）。单点：
`PROBE_BACKEND=d3d11 PROBE_ADAPTER=NVIDIA PROBE_ALLOC=1`。探针源码
`crates/nooksill-gpu/src/bin/probe.rs`，采样逻辑 `src/mem.rs`。

### NFR：弃用绝对上限，改为三条可执行口径（ADR-105）

上表的 111 / 102 MB 是**某台基准盒某时刻的实测值，不是产品承诺**。驱动、
适配器（核显/独显）、分辨率与 DPI 都让它漂移；"工作集 < 100 MB"这类绝对
目标既不可达，也无法区分回归与 ±15 MB 驱动抖动。NFR 改为三条：

1. **无会话增长**：4 小时 soak 预热后稳态 WS 线性回归斜率 ≤ 5 MB/h 且
   R² ≤ 0.5，峰值增幅 ≤ 30 MB。命令
   `MODE=soak DURATION=14400 ./scripts/mem-guard.sh`，也可由
   `.github/workflows/mem-soak.yml` 在**带交互桌面的 self-hosted runner** 上手动跑
   （GitHub 托管 runner 无桌面会话，分层窗口路径无意义），CSV 上传 artifact。
2. **增量记账**：每个新增常驻资源的 feat 在 CHANGELOG 验证栏记
   「WS +X.X MB（同基准盒、同场景、预热后稳态中位数）」，
   用 `scripts/measure-feat.sh` 生成。噪声内写实测值，不写"零增长"。
3. **软上限回归捕捉**：`MODE=quick DURATION=300`（5 分钟）以基线稳态
   中位数 + 20 MB 为软上限。它是回归探针，不声称证明无泄漏——5 分钟尺度
   上 ±15 MB 抖动会淹没慢泄漏，真正的泄漏判定只由第 1 条给出。

**当前基线（2026-11-17，AMD 610M，release，dock 空闲 75 s，预热 45 s）**：
完整进程 WS **150 MB** / private **278 MB** / peak **157 MB**，斜率 −1.66 MB/h。
同一台机器 device 级探针只有 80 MB——差额是 7 套 FrameRenderer 的离屏纹理、
DIB、egui Context / atlas 与 worker 栈，这部分才是业务侧可优化的空间。

**隐藏窗口 DIB 释放（2026-11-18，ADR-117）**：面板 / 设置窗 / 起动台 / 右键
菜单 / 编辑窗隐藏时 drop `LayeredPresenter`，GPU 目标、egui Context 与管线全部
保留，出帧前按需重建；`FrameRenderer::new` 也不再预分配像素缓冲。

同进程 A/B 探针 `examples/mem_targets.rs`（真实分层窗口 + 真实 `render` 上屏
路径，7 个窗口 = 6 预热面板 + 设置窗，ppp 1.5 / Msaa 4，每相位 7 次采样取中位数）：

| 相位 | NVIDIA WS | AMD WS | NVIDIA private | AMD private |
|---|---|---|---|---|
| 全部持有 DIB | 152.6 MiB | 123.5 MiB | 279.3 MiB | 222.2 MiB |
| 隐藏后释放 DIB | 139.1 MiB | 110.0 MiB | 265.8 MiB | 208.7 MiB |
| 重新出帧 | 152.6 MiB | 123.5 MiB | 279.3 MiB | 222.2 MiB |

**省 13.49 MiB WS / 13.5 MiB private，两卡逐位一致，restored 与 live 漂移 0.00**
（第 2 轮起 5 轮稳定）。这 7 个窗口的 DIB 解析字节合计 13.47 MiB，与收益吻合。
单位口径：`MemStats` 除 1024×1024，所以是 **MiB**，不是 MB。

复现：`cargo run --release -p nooksill-gpu --example mem_targets -- 1.5 4 5`，
`PROBE_ADAPTER=AMD|NVIDIA` 钉卡。两条踩坑记录：

- **必须走 `render` 而不是 `render_rgba`**：离屏路径不写 DIB，未触碰的 DIB 页
  不会常驻，用 `render_rgba` 只能测出 0.5 MiB 的假收益。
- **不要连 GPU 目标一起释放**：WS 收益不变，私有提交稳态却抬高约 48 MiB
  （283 → 331），6 轮确认收敛而非泄漏。**这 48 MiB 的归属未定位**——不要写成
  "gpu-allocator 堆不回收"，源码里空块是会销毁的（见 ADR-117 勘误）。

**分解探针 `examples/mem_surface.rs`**（把两类缓冲拆开，**分配与释放分别测**）：

| 方向 | NVIDIA WS | NVIDIA priv | AMD WS | AMD priv |
|---|---|---|---|---|
| 分配 GPU 目标（81.17 MiB） | **+31.36** | +143.11 | **+16.92** | +120.11 |
| 释放 GPU 目标（3 轮） | 0.00 | 0.00 | 0.00 | 0.00 |
| 再分配（2 轮） | **0.00** | 0.00 | **0.00** | 0.00 |
| 释放 DIB | **14.46** | 18.24 | **14.14** | 13.52 |

**GPU 目标确实计入 WS，但释放拿不回来，再分配也不再花钱**——内存被分配器
池化留着。DIB 两个方向都如实进出，这才是只释放 DIB 的理由。
首轮偶有几 MiB 释放收益是采样时机造成的，多等一个周期就归零。
同一探针还测得 `CreateDIBSection`（1340×1080，200 次）p50 0.010 ms、
max 0.06–0.27 ms，以及 Dock 栏窗口 1927×233 → DIB 1.71 MiB。
复现：`cargo run --release -p nooksill-gpu --example mem_surface -- 1.5 4`。

阈值依据：驱动抖动 ±15 MB；4 小时允许总增长不超过一个抖动带（≈3.75 MB/h），
取 5 留一倍余量；R² 门槛排除随机游走。RDP / 虚拟机上 GPU 路径可能落到 WARP，
绝对值不可比——判定一律用趋势与相对差，不做跨机绝对比较。

**配置 A/B（ADR-107，`scripts/mem-ab.sh`，release，隔离 portable 目录，
预热后 WS 中位数）**：

| 变体 | WS | 相对默认 |
|---|---|---|
| 默认 | 158.8 MB | — |
| `PreloadPanels=0` | 126.8 MB | −32.0 |
| `Smoke=0` | 133.5 MB | −25.3 |
| `BgMode=0` | 139.2 MB | −19.6 |
| 三者全关 | 119.8 MB | −39.0 |

预加载与磨砂重叠：关预加载后再关磨砂只多省 3.3 MB。烟雾的 25 MB 是运行时
场缓冲与纹理，不是 pipeline——跳过烟雾 GPU pipeline 后 WS 仅降 0.3 MB。

## 六、排错

| 症状 | 原因 | 处理 |
|---|---|---|
| `linker 'link.exe' not found` | 没 source `env.sh` | `source scripts/env.sh` |
| egui 画面偏左上、只占表面 1/ppp | `RawInput` 少了 `viewport_info.native_pixels_per_point`（egui 0.37 改用它传原生缩放） |
| `/usr/bin/link: extra operand` | Git Bash 的 GNU `link` 抢在 MSVC `link.exe` 前面 | `env.sh` 已把 cl.exe 目录**前置**进 PATH；若手工配置需照做 |
| `找不到 MSVC 工具集` | VS 没装「使用 C++ 的桌面开发」 | 装工作负载，或用已初始化的 vcvars shell |
| 测试 panic `No glyph for "常"` | 没装中文字体，egui 0.37 缺字形直接 panic | 确认 `C:\Windows\Fonts\msyh.ttc` 存在；测试用 `with_context_setup` 预装 |
| 窗口透明但画面全黑 | 交换链 alpha 模式退化成 `Opaque` | 看启动日志；考虑 DirectComposition 方案 |
| `cargo fetch` 卡住 | 首次拉取 wgpu/egui 依赖树 | 需要网络；缓存后离线可构建 |
| RDP/虚拟机里没画面 | DXGI 复制或 DX12 不可用 | M3 会加 CPU 模糊 fallback；M0 仅作提示 |

## 七、测试分层

| 层 | 命令 | 依赖 |
|---|---|---|
| 逻辑层（配置/INI/布局/条目） | `cargo test -p nooksill-core` | 无 |
| 设置界面（kittest 无头） | `cargo test -p nooksill-settings` | 需要系统中文字体 |
| 平台层（显示器/注册表） | `cargo test -p nooksill-platform` | 需要桌面会话 |
| GPU 尖刺 | 手动运行 | 需要 D3D12 适配器 |

`core` 的测试**不依赖 Windows**，可以放到任意 CI runner 上跑；这是把纯逻辑
单独拆 crate 的主要动机。

排查用的两个 example，读一个 ini 或打印键表，不必启动 Dock：

```bash
cargo run -q -p nooksill-core --example read_ini -- <path/to/NookSill.ini>
cargo run -q -p nooksill-core --example config_table     # 重新生成 CONFIG.md 的键表
```

### 自动化交互测试的已知坑

- `PostMessage` 注入的鼠标坐标会被 DPI 虚拟化改写；注入前先把测试进程设成
  per-monitor aware，否则坐标只能对上 100% 缩放。
- 拖拽消息序列的有效窗口约 420 ms，慢了会被当成两次独立点击。
- 窗口在 A 线程创建、B 线程跑消息泵时，控件拿不到焦点，结论一律不可信。
- 点击前先跑一帧再断言，布局尚未提交时命中测试算的是旧矩形。

## 八、CI（`.github/workflows/build.yml`）

三段式，跑在 `windows-latest`：

| job | 触发 | 干什么 |
|---|---|---|
| `gate` | 每次 push（任意分支）/ PR | 注释 / 规模 / 提交信息三个文本门禁（bash，含 `--self-test`）→ `fmt --check` → `clippy -D warnings` → `--lib --tests`，与本地 `--gate` 同一批命令 |
| `debug` | 每次 push（任意分支）/ 手动触发 | `package.sh`（构建号 = `git rev-list --count HEAD`，与本地同一套）→ 上传 Actions 产物（`NookSill-<ver>-win64-debug.exe` + `.sha256`，保留 14 天）；默认**再发一条 rolling Pre-Release** |
| `release` | 推 `v*` 标签 | 校验标签与 `Cargo.toml` 一致 → `package.sh --release` → 发**正式 Release**（资产是裸 exe + `.sha256`；非草稿、非预发布、抢 `Latest`） |

四个 JS action 都声明 `runs.using: node24`（逐个核对上游 `action.yml`）：
`checkout@v6`、`step-security/msvc-dev-cmd@v1.13.1`、`upload-artifact@v6`、
`action-gh-release@v3`。升级时先核实目标版本的运行时与输入项，不要只按大版本号猜。

### 8.1 Debug 的两个出口

1. **Actions 产物**：无条件上传，任何分支都有。分支名不能当版本，所以特性分支
   只留在这里。
2. **Pre-Release**：只在默认分支（`main`）上发，**永远用同一个 tag `dev`**，
   标题写 `Development build <版本>`。每次发布先删掉旧 release 和 `dev` tag，
   再基于当前 commit 重建（`gh release create --target $GITHUB_SHA`），资产
   随 release 一起上传。

   > 为什么删掉重建：`gh release edit` 改不了 `published_at`，只覆盖资产会让
   > Releases 页一直显示「released 2 weeks ago」，说明里的构建时间也对不上。
   > 重建让发布时间、标题、说明、资产四者同批刷新。

   > 为什么不每次一个新 tag：一天几十次提交就是几十条 release，Releases 页
   > 会被刷屏，而且 `Latest` 旁边的"预发布"永远指向某个半成品。滚动一条，
   > 链接稳定、内容最新。

### 8.2 关掉 Pre-Release（测试流程用）

两种方式，任选：

| 方式 | 生效范围 | 怎么设置 |
|---|---|---|
| 仓库变量 `DEBUG_PUBLISH=artifact` | 长期 | Settings → Secrets and variables → Actions → **Variables** 新建 |
| 手动触发时 `publish` 选 `artifact` | 仅这一次 | Actions → build → Run workflow |

两种都关掉之后，debug 仍然构建、仍然上传 Actions 产物，只是不再动 Releases 页。

### 8.3 发版

```bash
# 1) 把 Cargo.toml 的 workspace version 与 CHANGELOG 顶层小节改成同一个号
# 2) 提交
git commit -am "release: 0.9.26"
# 3) 打标签（只认 v* 前缀，tag 与版本号不一致会直接 fail）
git tag v0.9.26 && git push origin v0.9.26
```

两个刻意的选择：

- **不用 `msvc-dev-cmd` 之外的魔法**：`env.sh` 本来就有"PATH 里已有 cl.exe 就直接用"
  这条路径，CI 上由 `ilammy/msvc-dev-cmd` 触发它，本地由 VS 开发者 shell 触发，
  同一条代码路径。
- **标签与版本号强校验**：打错标签（比如 `v0.9.26` 而 Cargo 还是 `0.9.25`）
  直接 fail，而不是发出一个版本号对不上的包。

### 8.4 两个更新通道

应用内更新把这两条发布线当成通道，由 `[advanced] UpdChannel` 选择：

| 通道 | 来源 | CI 触发 | 资产 |
|---|---|---|---|
| 稳定版（`0`） | 正式 Release | 推 `v*` 标签 | `NookSill-X.Y.Z-win64.exe` + `.sha256` |
| 测试版（`1`） | 滚动 Pre-Release | 默认分支每次提交，tag 固定 `dev` | `NookSill-X.Y.Z-<构建号>-<sha>-win64-debug.exe` + `.sha256` |

更新器只请求一次 `GET /releases?per_page=15`，按 `draft` / `prerelease` 本地分流；
ETag 落盘做条件请求，校验优先用 API 的 `assets[].digest`。所以 CI **不要**改
`per_page` 或给 `dev` 换成每次一个新 tag —— 前者会让缓存失效，后者会刷屏且
让测试通道找不到固定入口。Release 资产必须保持公开匿名可读，客户端不能带凭据。

### 8.5 工具链跟随 stable，升级时要两边验证

`rust-toolchain.toml` 只声明 `channel = "stable"`，CI 的
`dtolnay/rust-toolchain@stable` 也跟随上游最新 stable，**两边都不锁版本号**。
代价是 Rust 发版会带进新的 clippy lint，出现「本地 `--gate` 全绿、CI 挂」：
1.99.1 → 1.100 时就连续命中 `needless_range_loop` 与 `needless_bool`。

升级到新 stable 后按顺序做一次，别只依赖 CI 报错再回头修：

```bash
rustup update stable-x86_64-pc-windows-msvc   # 与 CI 对齐
./scripts/build.sh --gate                      # 同版本 lint + 全量测试
```

CI 的 `gate` job 与本地 `--gate` 跑同一批脚本、同一个编译器，结论才可比。
本机工具链落后时，`rustc --version` 与 CI 日志里的版本不一致就是第一线索。
