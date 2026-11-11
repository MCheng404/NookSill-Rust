# 技术决策记录（ADR）

本文件记录不可逆的技术取舍，格式为决定 / 依据 / 代价。
编号与标题按历史原样保留（ADR-049 有两条，分别记设置界面与点击链路）。

---

## ADR-001 · 渲染后端选 wgpu，不选 D3D11/D2D

**决定**：`wgpu 30`，经 `egui_wgpu::wgpu` 重导出，不单独锁版本。

**依据**：
- Rust 生态已在此层收敛（Firefox WebGPU、Bevy、egui、iced、Dioxus 原生端），DX12/Vulkan/Metal 的驱动怪癖由上游吸收。
- 原生 D3D11/D2D 更轻（30–50 MB vs 60–120 MB），但 Rust 侧没有框架用它做 UI：文本 shaping、IME、控件、测试全要自己写，且没有同行排雷。
- 实测常驻 DX12 164 MB（AMD 610M）/ 87 MB（5070），原生 D3D11 34/39 MB；差额来自 wgpu-hal + D3D12 运行时，不是驱动固有问题。

**代价**：常驻内存与体积上一个量级，Dock 的"极轻"定位正式放弃。换后端的提议已由 ADR-011 否决。

---

## ADR-002 · UI 框架选 egui，且**不用 eframe**

**决定**：`egui 0.37` + 自持 Win32 消息循环 + `egui-wgpu`。

**依据**：
- Dock 是画布：状态 → 每帧重画，即时模式天然贴合；iced 的 Elm 架构要把缓动/拖拽塞进 `update`，是逆着框架用。
- eframe 会接管事件循环，而托盘、全局热键、AppBar、`WM_NCHITTEST` 都必须在 `WndProc` 里说话。
- 附带得到一个循环、一个 device，原生弹窗（菜单/组/DWM 预览）可共存。

**代价**：输入翻译自写约 300 行；IME 需桥接。

---

## ADR-003 · 设置界面独立成 crate，不依赖 wgpu

**决定**：`nooksill-settings` 只依赖 `egui` + `nooksill-core`。

**依据**：
- 这是"可测试性"的全部来源：9 页、约 40 个控件若与 wgpu 耦合就只能靠人眼看，拆开后 kittest 可在无显示器环境点完每一页。
- 边界廉价，收益随设置项增长。

**代价**：多一层 crate 边界与 `Command` 枚举（UI 不能直接调副作用）。

---

## ADR-004 · 中文字体用系统 TTC，不打包子集

**决定**：运行时读 `C:\Windows\Fonts\msyh.ttc`（`FontData.index = 0`），按 `msyh → Deng → simhei → simsun` 顺序尝试。

**依据**：
- 打包 Noto Sans SC 子集要 +1–3 MB，而系统字体本来就在；`epaint::FontData` 支持 TTC 的 face 索引。

**代价**：无字体的机器会崩——egui 0.37 缺字形是 `panic`（`MissingGlyphPolicy::Panic`）而不是豆腐块，所以必须在第一帧之前装字体，失败时明确报错。

---

## ADR-005 · 逐像素命中用 `WM_NCHITTEST`，不用轮询

**决定**：本条已作废。ADR-008 改用分层窗口后，`UpdateLayeredWindow` 的逐像素 alpha 重新生效，alpha=0 的像素由系统自动穿透，`overlay.rs` 里手写的命中代码已删除。

**依据**：
- 这是"GPU 窗口 + 不分层"分支上的正确结论：那种形态下没有 alpha 穿透，`winit::set_cursor_hittest` 只有整窗开关，而 `WM_NCHITTEST` 是系统每次点击都会问的同步钩子，没有"关掉穿透才收得到消息"的鸡生蛋问题。
- 一旦将来放弃分层窗口、改 DirectComposition 自建交换链，这条会重新适用。

**代价**：不分层时要自己维护命中（圆角、图标形状、气泡溢出区）。

---

## ADR-006 · IME 先叠原生 `EDIT`，不硬啃 `WM_IME_*`

**决定**：设置页需要键盘输入处先叠原生 `EDIT` 控件，IME 桥接后置；现已由 ADR-044 的 `WM_IME_*` 完整实现取代。

**依据**：
- 原版 C 就是这么做输入框的；IME 组合串、候选窗定位、TSF 兼容是"看起来简单、边界极多"的活。

**代价**：原生控件不是 egui 画的，需对齐主题（圆角/内边距/字体）。

---

## ADR-007 · CPU 并行用 rayon + 专用 IO 线程，不引入 tokio

**决定**：数据并行（缩放/预乘/模糊 fallback）用 rayon；窗口扫描、favicon 下载、缩略图抓取用固定线程 + 通道。

**依据**：
- 这里没有大量并发连接，只有几条长驻线程 + 一个线程池；async 运行时的调度开销与每任务栈占用都不划算，调试也更难。
- 跨线程数据用 `ArcSwap` 存只读快照（UI 线程无锁读）；GPU 上传只在 UI 线程做（纹理与 device 绑定）。

**代价**：无。

---

## ADR-008 · 透明窗口：wgpu 表面做不到，改离屏 + 分层窗口

**决定**：wgpu 渲到 `Rgba8UnormSrgb` 纹理 → copy 到 buffer → 预乘后写 DIB → `UpdateLayeredWindow`。M3 再评估 DirectComposition（`WS_EX_NOREDIRECTIONBITMAP` + DComp visual + 自建 DXGI 交换链）。

**依据**：
- 本机 `wgpu::Instance` 选 Vulkan 时 `alpha_modes` 无 `PreMultiplied`/`PostMultiplied`；强制 DX12 仍只有 `Opaque`——wgpu 的 DX12 表面不提供 alpha 模式。
- `LoadOp::Clear(TRANSPARENT)` 的结果是黑色，不是透出桌面。
- 离屏回读是对现有渲染代码的加法；改 D3D11+D2D 会推翻 ADR-001 并重写渲染。

**代价**：每帧一次回读（Dock 尺寸约 1–6 MB）；分层窗口与 DXGI 交换链不能共存，呈现路径要整体替换。

---

## ADR-009 · 设置窗口可以是不透明的普通窗口

**决定**：设置窗口用普通无边框窗口（不透明、可拖动、带阴影），不做毛玻璃背景。

**依据**：
- 透明只对 Dock 是必需品；设置界面是模态性质的临时窗口，透明没有功能收益，却会强迫它继承分层窗口的复杂度。

**代价**：无。

---

## ADR-010 · 保留 GDI+ 实现到 M2 结束

**决定**：`nooksill-gfx` / `nooksill-ui` 保留到 M2 图标迁移完成，并排截图差分后再删。该删除已由 ADR-035 执行。

**依据**：
- 图标形状、圆角、文字基线这些细节，最可靠的对照物是能跑的旧实现。

**代价**：工作区同时构建 wgpu 与 GDI+，编译时间变长。

---

## ADR-011 · 渲染后端改用 `egui_glow`（OpenGL）替代 `egui-wgpu`【已否决】

**决定**：不换，保留 `egui-wgpu` 单栈。

**依据**：
- 换 OpenGL 要自写 FBO 离屏回读（约 250 行），并为 RDP/虚拟机落到 GL 1.1 再准备一条回退路径。
- 换来的是常驻 150–270 MB 降到 55–80 MB，但当前优先级是减少维护面而不是压内存。
- 同卡实测：wgpu-DX12 空设备 87–164 MB，原生 D3D11 34–39 MB，OpenGL 52 MB；数据保留，内存重新成为硬指标时可直接翻案。

**代价**：混合显卡上 GL 上下文可能落独显（功耗高于核显渲染），钉核显要 `WGL_NV_gpu_affinity`（仅 NVIDIA）。

---

## ADR-012 · 显卡选择按名字匹配，默认自动

**决定**：`[Graphics] Adapter` 存适配器名字，空串 = 自动；运行时在 DX12 实例里按名字匹配（先精确忽略大小写，再退化到互相包含）；匹配不到回退自动，并把候选写进日志与设置界面提示。

**依据**：
- 不存索引：DXGI 与 wgpu 的枚举顺序都不保证稳定（换驱动、插拔外接卡、系统更新），迟早出现"选了 A 实际用了 B"且无法从配置看出。
- 不用系统级 `UserGpuPreferences`：它只在进程启动时读，还要写 HKCU，便携版不该留这种痕迹。
- 失配必须回退而不是报错：配置跟着 ini 走，换机器后名字必然失效，不能因为一个显示偏好起不来。

**代价**：两块同名同型号的卡无法区分（SLI/多卡工作站）；驱动改名加了后缀时仍会回退。

---

## ADR-013 · 内存调优只动 `DeviceDescriptor`，不换框架

**决定**：只改两处——`max_non_sampler_bindings` 1_000_000 → **4096**，`memory_hints` `Performance` → **`MemoryUsage`**；空设备 164 → 102 MB（−38%），实际 spike 271 → 111 MB（−59%）。

**依据**：
- 两条都是 wgpu 文档点名的吃内存项，且都是设备创建时的一次性成本，与渲染逻辑无关，改完像素核对一致。
- 同时把 `Limits::downlevel_defaults()`（`max_texture_dimension_2d` 只有 2048）改成 `..adapter.limits()`，否则 2560 宽的 Dock 在 150% DPI 下需要 3840 px 离屏纹理，超宽屏会建纹理失败。

**代价**：`MemoryUsage` 让分配器少留余量；本项目每帧分配是几个 KB 级，影响应在噪声内。

---

## ADR-014 · 重绘由 egui 的 `repaint_delay` 驱动，不用固定间隔轮询

**决定**：`repaint_delay` 作调度信号——`ZERO` 排到 `now + 16ms`（60 fps 节流），有限值排到 `now + delay`，`MAX` 则 `None` 阻塞在 `MsgWaitForMultipleObjectsEx(INFINITE)`；每轮先把消息队列抽干再渲染。

**依据**：
- 输入不走这条路：`WndProc` 直接置 `needs_frame`，下一轮立即出帧，不牺牲响应速度。
- 实测空闲 30 秒 CPU 增量 **0.000 秒**，设置窗静止时进程完全阻塞在 `INFINITE`。

**代价**：`Duration::ZERO` 被节流到 16 ms，需要 120 Hz 或无缝拖拽动画时要改 `FRAME_PACING` 按刷新率取值。判空必须用 `is_some_and`，写成 `is_none_or` 会把"没有待办"当成"立刻要画"，空闲 CPU 从 0% 涨到 1.5%。

---

## ADR-015 · 单实例用命名互斥体，第二次启动只唤起窗口

**决定**：`SingleInstance::acquire()`——`Ok(Some)` 正常启动且 guard 活到进程结束；`Ok(None)` 时 `find_window("NookSillOverlay")` + `SetForegroundWindow` 后以 0 退出；`Err` 时照常启动。

**依据**：
- 两个实例各画一份分层窗口时，外部观测表现为"空闲还占 8% CPU"，容易被误判成渲染循环的问题。
- 互斥体本身失败宁可多一份 Dock，也不要起不来。

**代价**：窗口类名成了跨进程契约（`overlay::CLASS_NAME`），改名要同步。

---

## ADR-016 · 回读位图按 u32 搬运，不逐字节赋值

**决定**：两个缓冲都按 `&[u32]` 处理，一次位运算完成 R/B 交换：`*d = (*s & 0xFF00_FF00) | ((*s & 0x0000_00FF) << 16) | ((*s & 0x00FF_0000) >> 16)`。

**依据**：
- 1320×1080 = 140 万像素，实测 `0.80 ms/帧` vs 逐字节 `1.16 ms/帧`（1.45x）。
- 真正的开销在 GPU 回读与 `UpdateLayeredWindow`（数毫秒级），再用 `std::simd` 属优化错对象且要 nightly。

**代价**：无。

---

## ADR-017 · 配置键表表驱动，跟上游同步的代价是"加一行"

**决定**：41 个"裸 int"键放进静态表 `NUMERIC_KEYS`，`Key::Numeric(i)` 指向表内行；类型化的 35 个键（主题、边缘、枚举）仍是普通变体。

**依据**：
- 那 41 个键在上游 C 版里就是裸 `int`，没有枚举语义；表驱动之后加一个上游键 = 加一行数据，而不是改 8 处 match。
- 键的**顺序**仍显式写在 `Key::ALL`，因为写回 ini 的行序必须与 C 版一致，由 `key_table_matches_c_g_tab` 守住。

**代价**：具名常量写死了表下标，表一重排就全错（由 `numeric_constants_match_table_rows` 守）；数值键的取值域由我们自己定（上游对这些 int 完全不夹紧，越界直接进绘制代码），值域取自上游设置行。

---

## ADR-018 · 命中与放大映射按 C 版语义，不按"直觉"

**决定**：照抄 C 版两处语义——命中取"从左到右第一个"（`[slotL − g/2, slotR + g/2)` 内即返回），不是最近的那个；放大权重用 `MapActualToBase()` 把实际位置映射回基准位置再算余弦。

**依据**：
- 放大后相邻图标会重叠，两个都覆盖鼠标点时"最近"与"第一个"会选**不同的图标**。
- 直接用实际位置会让"鼠标属于哪个图标"随放大程度漂移。

**代价**：`hit_index` 参数变 9 个，但每个都对应 C 版一个全局量，拆开更难对照。条目数量变化时放弃映射（对应上游 `prevN != nItems` 的保护）。

---

## ADR-019 · egui 的原生缩放走 `ViewportInfo::native_pixels_per_point`

**决定**：自持 Win32 循环处（`input.rs`、`nooksill-dock`）构造 `RawInput` 时都补 `raw.viewports.insert(ViewportId::ROOT, ViewportInfo { native_pixels_per_point: Some(ppp), .. })`。

**依据**：
- egui 0.37 不再从 `RawInput` 直接读 `pixels_per_point`；不补时按 1.0 缩放，界面只占表面的 1/ppp，表现为"整体偏左上、尺寸偏小"，150% DPI 下文字放大后发虚。
- `screen_rect` 决定画布大小、`native_pixels_per_point` 决定缩放，两者缺一不可且症状不同（一个偏、一个虚），容易只修一个。

**代价**：无。

---

## ADR-020 · 圆角 + 渐变要自己搭网格

**决定**：`rounded_gradient()` 自建 `Mesh`：从矩形中心向圆角轮廓拉三角扇，顶点色按渐变方向线性插值。

**依据**：
- `Shape::gradient_rect()` 只是一个 4 顶点四边形，没有半径参数；支持圆角的 `rect_filled()` 只接受单色，两者不可叠加。
- 线性渐变的颜色是位置的仿射函数，三角形内的重心插值对仿射函数是**精确**的，所以这不是近似。

**代价**：`Shape::Mesh` 走 tessellator 直通分支，不做 epaint 的 1px 羽化，边缘会锯齿；补偿是在轮廓外再放一圈同色全透明顶点。

---

## ADR-021 · 搬 C 的颜色一律走 `argbc()`

**决定**：新增 `fn argbc(a, r, g, b) -> Color32`，参数顺序与 C 的 `ARGBC` 一致，所有移植颜色只走它；配 `argbc_keeps_alpha_first` 钉死语义。

**依据**：
- `Color32::from_rgba_unmultiplied(r, g, b, a)` 是 alpha 在后，逐条手抄会安静地画成另一个颜色——不崩溃、不报警，只是难看。分割线、运行指示点外圈、气泡底、图标底盘曾同时画错。
- 凡是"两边参数顺序不同"的 API，都该包一层适配函数而不是逐处小心。

**代价**：无。

---

## ADR-022 · 时钟文字归 `core::clock`，日期有横竖两套写法

**决定**：新增 `core::clock`（`ClockParts` / `clock_lines(cfg, parts)` / `ClockLine::is_date`），横排 `2026年10月30日 周五`，竖排 `10月30日` + 周几单独一行；绘制端按行数均分卡片高度（对应上游 `lh = (ch0 − 8) / nl`），时间组与日期组之间画细线。

**依据**：
- 原版 `ClockLines()` 按停靠方向分两套：竖排一行只有 21 px，塞年份会挤成一团；抄了竖排那套，横排就丢了年份。
- 把两行位置写死成 12%–56% / 54%–86% 时，`ClockShow=1/2`（只显示时间或只显示日期）会错位。

**代价**：`strftime` 之类的格式化库用不上（原版就是手拼），多语言日期格式要自己加分支；当前只做中文。

---

## ADR-023 · Dock 的输入层：鼠标用物理像素、脏标记驱动重布局、点击认"同一图标"

**决定**：
- `Dock::mouse` 存**物理像素**，只在绘制时经 `Proj::k` 换算一次；`metrics.lw/lh` 也是物理像素（`CreateWindowExW` 收的就是它）。
- 加 `dirty` 标记，`WM_MOUSEMOVE` / `WM_MOUSELEAVE` / `WM_LBUTTONDOWN` / `WM_LBUTTONUP` 都调 `mark_dirty()`，循环条件是 `dirty || anim`。
- `press_at` / `release_at` 成对判定"按下与抬起在同一图标"，主循环用返回的下标启动，不用 `hot`。
- `nooksill-dock` 也接 `SingleInstance::acquire()`，第二次启动直接退出。

**依据**：
- 逻辑点与物理像素混用时，150% DPI 下命中区整体偏移 1.5 倍（"鼠标移到图标右边才放大"）。
- `anim` 收敛后不再调 `update()`，`hot` 不会重算——悬停放大在静止状态下完全不工作。
- 少了"同一图标"这条，从图标上按下、拖到别处松手也会启动。

**代价**：无。

---

## ADR-024 · 拖拽重排：状态收进一个结构体，重排只挪一格

**决定**：
- 拖拽态收进 `Option<DragState>`（`index / pt / active / press_pt`）。
- 起拖阈值 6 px（对应上游 `abs(dx) > 6`）。
- 重排一次只挪一格，由"数有多少条目的实际中心在光标左边"得出目标下标。
- `place_for` 显式排除被拖项（传参，不从已 `take()` 的状态里读）。
- 拖动后松手不算点击；重排结果用 `pending_move: Option<(from, to)>` 单向下发给主循环。

**依据**：
- 拆成多个字段会出现"`dragging` 为真但 `dragIdx` 是 −1"的半截状态，收进 `Option` 后这种状态在类型上不存在。
- 每帧最多 `remove` + `insert` 一次，位移变化连续，`slide` 动画才自然。

**代价**：拖出栏体删除、合并成组未做；`removing` 状态位不加，避免留下"能显示却不会生效"的字段。

---

## ADR-025 · 自动隐藏：必须轮询光标，且不能每轮都重绘

**决定**：
- 开启自动隐藏时把等待超时从 `INFINITE` 改成 100 ms，每轮 `GetCursorPos()` 取一次光标。
- 收起态判据是"贴住 Dock 所在屏幕边（±1~2 px）且在窗口横向范围内"，展开态是"栏体外扩一圈"。
- 宽限期 450 ms；`busy` 为真时强制显示并刷新计时。
- 用 `need_frame` 标记按需出帧：收到消息 / `update` 跑过 / 隐藏状态变化时才画。

**依据**：
- 收起后那片区域 alpha 为 0，分层窗口按 alpha 判定命中，鼠标事件直接穿透，永远等不到 `WM_MOUSEMOVE`——只看消息 Dock 就再也弹不回来。
- 收起态若按"靠近栏体"判断则永远够不着（栏体已不在屏幕上）。
- 450 ms 用于避免鼠标划过屏幕底边去点任务栏时误弹出；每轮都重绘会变成 10 fps 的无谓渲染。

**代价**：收起态空闲 10 秒 CPU 增量 **0.000 秒**，代价是每 100 ms 醒一次。`SmartHide` 在本条落地时未实现，按常显处理并在启动日志明说。

---

## ADR-026 · 桌面预留走 AppBar，且必须 RAII 注销

**决定**：`platform::appbar::AppBar` 封装 `SHAppBarMessage` 四步（`ABM_NEW` → `ABM_QUERYPOS` → 修厚度 → `ABM_SETPOS`），`Drop` 时 `ABM_REMOVE`；每次都走 `ABM_QUERYPOS`（纯查询），结果真的变了才 `SETPOS`。

**依据**：
- AppBar 登记是系统级的，登记后 `rcWork` 永久少一块；Windows 会在进程正常或异常退出后回收，所以 RAII 不是为了兜底崩溃，而是为了"退出前就归还"，避免退出瞬间别的程序按缩小后的工作区排列窗口。
- C 版用 `hash(边, 厚度, 显示器矩形)` 判断要不要重新提交，但别的 AppBar 退出、任务栏换边、分辨率变化都不在 key 里，Dock 会卡在旧位置。

**代价**：多一次调用。正常预留时 Dock 自己的窗口位置**不变**（AppBar 从 `rcWork` 内部切走一块，两者共享同一条外边），变的是别的窗口能不能用那块地方；已有别的 AppBar 占同一侧时，必须用 `ABM_QUERYPOS` 返回的 `ab.rc` 定位，用 `rcWork` 会两个 AppBar 叠在一起。

---

## ADR-027 · 智能隐藏的遮挡判定必须枚举窗口

**决定**：`platform::occlusion` 用 `EnumWindows` 枚举顶层窗口，过滤后与栏体屏幕矩形求交，任一相交即视为被遮挡。

**依据**：
- 分层窗口在命中测试里算"在上面"，`WindowFromPoint` 会返回 Dock 自己，拿不到被它盖住的窗口。
- 过滤项缺一不可：自身窗口（自己挡自己）、`!IsWindowVisible` / `IsIconic`（最小化窗口矩形仍在原位）、`WS_EX_TOOLWINDOW` / `WS_EX_NOACTIVATE`（输入法候选、悬浮工具栏会让 Dock 莫名躲起来）、有 `GW_OWNER`、标题为空、DWM cloaked（UWP 幽灵窗口）、`Progman` / `WorkerW` / `Shell_TrayWnd`（铺满全屏会让 Dock 永远被遮挡）。

**代价**：`GetWindowTextLengthW == 0` 会误杀少数无标题全屏窗口（个别游戏/播放器），取舍是"宁可 Dock 不躲，也不要莫名其妙地躲"。智能隐藏与自动隐藏共用同一条 `tick_auto_hide`，只在入口把 `covered=false` 折算成 `forced near`；`covered` 在纯自动隐藏模式下必须忽略，否则两者互相污染。

---

## ADR-028 · 运行状态：精确匹配 exe，不做文件名猜测

**决定**：分三层——`platform::app_window::running_apps()`（枚举顶层窗口 → exe 小写路径 → 窗口）、`core::running::running_key(item)`（纯逻辑）、`core::running::refresh_running()`（返回是否有变化）。匹配只认 `item.target`（`.lnk` 解析出的真实 exe）与非空且以 `.exe` 结尾的 `item.path`，其余条目不参与匹配。

**依据**：
- 退化成"按文件名猜"会把 `网易云音乐.lnk` 与 `网易云音乐.exe` 硬凑一起，而它实际指向 `cloudmusic.exe`；猜错的结果是**给没运行的程序点亮指示点**，比"该亮没亮"更糟。
- `refresh_running` 必须返回"是否有变化"：每 1.2 秒扫一次，无条件返回 true 就是无条件重绘，空闲 CPU 从 0% 直接失守。
- 扫描周期要参与等待超时计算，否则"时间过去"不是事件，`MsgWaitForMultipleObjectsEx` 永远不返回，指示点不再更新。

**代价**：Win11 的记事本/计算器/画图是 Store 应用，`notepad.exe` 只是应用执行别名，这类条目匹配不上（指示点不亮、点击走系统转发），支持它要解析 `AppExecutionAlias` 包信息。

---

## ADR-029 · `.lnk` 解析绕过 windows crate 的 wrapper，直接调 vtable

**决定**：本条作废，见 ADR-063。手工 vtable 调用已整体删除，改用 `windows` 生成的 wrapper。

**依据**：
- 当时判定的"wrapper 只传两个参数"来自另一个接口 `IKnownFolder::GetPath`——两个接口都在 `Win32::UI::Shell` 下且同名，按名字搜文档时串台了。
- `windows 0.62.2` 的 `IShellLinkW::GetPath(&self, &mut [u16], *mut WIN32_FIND_DATAW, u32)` 与 COM ABI 一一对应。

**代价**：原条目按"不删历史"保留，但以 ADR-063 为准。

---

## ADR-030 · 四边停靠，以及"配置不生效"的排查顺序

**决定**：窗口定位抽成纯函数 `layout::window_origin(edge, work, reserved, win_w, win_h)`，尺寸抽成 `layout::physical_size(edge, lw, lh)`（竖排要转置）；`Reserve` 只在 `Mode=0`（常显）生效，与隐藏模式冲突时启动日志明说"当前已忽略"。

**依据**：
- `physical_size` 被"建窗口"与"AppBar 厚度"两处共用，写错一处竖排 Dock 就只有一条细缝；四个方向都有测试。
- 会滑走的模式下留一块空白更怪，但用户既然打开了开关就该知道它没被采纳——不静默忽略是这一系列改动的共同原则。

**代价**：无。

---

## ADR-031 · 补齐缺失的交互：焦点、捕获、强制新实例、弹跳、快捷键、全屏

**决定**：
- `WM_MOUSEACTIVATE` 返回 `MA_NOACTIVATE`。
- `WM_CAPTURECHANGED` 调 `cancel_drag()`，清状态且**不**产出点击。
- 中键与 Shift+点击走同一条 `click: Option<(usize, bool)>` 产出的 `force` 位，跳过"激活已运行窗口"；Shift 在**按下**时读。
- 弹跳曲线照抄上游 `BounceOff()`：`x = t / 1100`，`off = |sin(x·2π)| · (1 − x·0.55) · S · 0.48`。
- `force_hide`（用户手动，优先级更高、不受鼠标位置影响）与 `hide_target` 分开存；推进条件收进 `Dock::needs_hide_tick()`（含 `hide_cur > 0`）。
- 前台是"覆盖整个显示器且无 `WS_CAPTION`"的窗口时 `SW_HIDE`，检测周期 1 s。
- `WM_SETTINGCHANGE` 收到 `ImmersiveColorSet` 立即重读主题。

**依据**：
- 不拦 `WM_MOUSEACTIVATE` 会把焦点从用户正在用的窗口抢走，"点一下整个桌面闪一下"。
- 捕获被抢走时抬起事件永远不到，不清 `press` 的话下一次点击会带着上一次的"按下"，表现为"点一下启动了上一个图标"。
- `force_hide` 与 `hide_target` 都变 false 而 `hide_cur` 仍为 1 时，动画再也不会被推进，Dock 永久消失——"要不要继续动画"应由持有动画状态的一方给出。
- `WS_CAPTION` 这一条是关键：普通最大化窗口也覆盖全屏，但不该让 Dock 消失；无边框全屏才是沉浸态的信号。

**代价**：全屏检测把空闲等待从 `INFINITE` 改成 1000 ms，实测空闲 10 秒 CPU 增量仍是 **0.000 秒**。判定逻辑只有一份（`check_fullscreen()`），诊断与实际行为共用，避免"比没有诊断更糟"。

---

## ADR-032 · 右键菜单必须有自己的窗口

**决定**：菜单是独立顶层分层窗口，共用 GPU 设备；给 `FrameRenderer` 加 `resize()`，只重建离屏目标与 DIB、保留 ctx 与管线，菜单窗口懒建一次之后只 resize。模态用 `SetCapture` + `WS_EX_NOACTIVATE` + `MA_NOACTIVATE`，不激活窗口。菜单只放当前真能执行的命令。

**依据**：
- 菜单要弹出到栏体之外，而分层窗口不会因为内容画到矩形外就被裁剪——画出去的部分直接不可见。
- 每次开菜单重建 `FrameRenderer` 要重跑 `egui::Context::default()` + 解析 CJK 字体 + 编译 wgpu 管线，几十毫秒，菜单会明显卡一下；固定最大尺寸会让下方透明区穿透，"点看不见的空白"语义混乱。
- 激活会抢焦点，用户关掉菜单后原来在用的窗口已经失焦，还得再点一次才能继续输入。

**代价**：`WM_CAPTURECHANGED` 也要当作"取消"，否则菜单会挂在屏幕上等一个永远不来的取消事件。"从 Dock 移除""添加分割线"要改条目数量 → 改窗口尺寸，属于配置热更新（ADR-033），本轮不做；分割线上右键不弹菜单，因为弹一个空窗口是对用户的欺骗。

---

## ADR-033 · 配置热更新：窗口尺寸是会变的

**决定**：链路固定为「改 `raw`（真源）→ 写回 ini → `dock.replace_items()` → 下一帧 `update()` 重算几何 → 主循环比对 `physical_size_i32()` 发现变化 → `frame.resize()` → `SetWindowPos`」；`resize()` 必须**先于** `SetWindowPos`。

**依据**：
- 谁都不直接改窗口：尺寸是几何的**结果**，几何由 `update()` 算，让调用方去算就会出现两套几何来源。
- 窗口变小而离屏目标还是旧尺寸时，`UpdateLayeredWindow` 提交的是旧尺寸位图——"只有左上角一部分被画出来"或右边留下残影。
- `replace_items` 只整份替换会让所有图标退回"首字占位"再重新加载（整条 Dock 闪一下），所以要按 `path` 把旧 `IconTex` 搬到新下标。
- `raw.len() <= 1` 时拒绝删除：Dock 变成空壳后用户没有任何可点的入口恢复它。

**代价**：重复路径（同一快捷方式放两份）只保住第一份的纹理，另一份重新加载，不影响正确性。

---

## ADR-034 · 配置格式 v2：按语义分段，键名自己定

**决定**：保留 ini 语法，换成语义分段（`[meta]` / `[appearance]` / `[appearance.background]` / `[appearance.plate]` / `[appearance.reflect]` / `[layout]` / `[behavior]` / `[behavior.group]` / `[theme]` / `[widgets.clock]` / `[menu]` / `[advanced]`，`[Items]` 与 `[Graphics]` 不变）；`Mode` → `hide_mode`，`BgMode` → `fill`；41 个数值键只分区、暂改名。

**依据**：
- `nooksill-core` 是零依赖且 `unsafe_code = "forbid"` 的，引 serde + toml 会打破整条分层基础（core 要能在无 Windows SDK 的环境单测）；自己写 TOML 子集要几百行外加一堆边角。
- 真正要脱钩的是**键名兼容**，不是语法——`Ini` 是通用 key=value 分区格式，不属于上游。
- `Mode` 实际是"隐藏方式"、`BgMode` 实际是"填充方式"，和 `hide_mode` 摆在一起才看得出谁管什么。
- 数值键大多对应尚未实现的功能（`FanArc`、`GroupFrost`），给没写过实现的功能定名字，大概率等实现时又要改。

**代价**：数值键名仍留上游的；段顺序按首次出现排列，升级后的文件里 `[Items]` 会排在 `[meta]` 前面（不影响解析）。迁移是"读到就升级"（判定条件是**没有** `[meta] version` **且** `[Settings]` 非空，只看 `[Settings]` 会误判新格式），只写一次盘。

---

## ADR-035 · 删除 GDI+ 老实现（nooksill / gfx / ui）

**决定**：删除 `crates/nooksill`（83 行）、`crates/nooksill-gfx`（969 行）、`crates/nooksill-ui`（948 行），合计 2000 行（占当时 16604 行的 12%），workspace 从 7 个 crate 减到 4 个。

**依据**：
- 它们从来不是行为基准——真正的基准是上游 C 源码 `reference/minidock.c`；这三个 crate 只是 M0 之前一次不完整的移植尝试（没有应用组、扇面、磨砂），拿半成品当参照比没有参照更容易误导。
- 两份实现已经开始漂移（GDI+ 版用动画中的 `cur` 算基准宽度导致槽位每帧漂移），"哪份是对的"每次都要重新判断。
- 它让 `cargo build` 产出两个都能启动、读同一份配置的 Dock，调试先要确认"我跑的是哪个"。

**代价**：可执行文件名从 `NookSill.exe` 变成 `nooksill-dock.exe`；要沿用旧名应在 `nooksill-gpu` 里给 bin 加 alias，而不是把老 crate 捡回来。不留 `--features legacy-gdi`（两份实现共存的成本会以另一种形式回来），不建 `archive/` 分支（git 历史可取回）。

---

## ADR-036 · 设置窗口接进 Dock 进程

**决定**：设置窗口是 Dock 进程内的第二个分层窗口——非模态、要焦点、`WS_EX_APPWINDOW` 进任务栏 + `WS_EX_TOPMOST`；`SettingsWindow::new` 直接返回 `Pin<Box<Self>>` 后再 `attach()`；写盘 400 ms 防抖且写前重新 `Ini::load`；`(changed, remount)` 分开返回。

**依据**：
- 独立进程要改完走 IPC 或重启 Dock，对"拖滑块看栏体变化"不可用；上游 C 版本来也是进程内对话框。
- 别的窗口盖上来后用户既点不到它、又没有托盘图标能唤回，所以必须置顶。
- 先 `attach()` 后移动结构体会把已释放的栈帧地址写进 `GWLP_USERDATA`，下次消息就是野指针访问（“点一下设置窗口进程就崩”，无 Rust panic）。
- `write_ini` 只管 `[meta]` 之后的配置段，直接覆盖会把 `[Items]` 全部丢掉。

**代价**：Dock 进程多一个分层窗口与一份 UI 状态。停靠边/预留/隐藏模式变更要走 `remount` 回到主循环，否则 `tick_settings` 会变成十几个参数的函数。原型 `overlay.rs`（约 600 行）与 `bin/spike.rs` 一并删除。

---

## ADR-037 · 把阻塞调用挪出 UI 线程，但只在该用多核的地方用多核

**决定**：`nooksill-gpu::worker` 起四个专用线程，各一条 `mpsc`——`IconLoader`（shell/COM 取图标，串行）、`Scanner`（运行状态与遮挡合并，两者都要 `EnumWindows`）、`DiskWriter`（串行写 ini，天然保序）、`Opener`（`ShellExecuteW` 与激活已有窗口）；结果用 `PostMessage(WM_APP_WAKE)` 敲一下，`pump` 看到它只打断等待、不强制出帧。

**依据**：
- UI 线程同时负责消息泵和出帧，五条路径（取图标、运行状态 1.2 s、遮挡 100 ms、写 ini、`ShellExecuteW`）任何一次同步系统调用都直接变成掉帧。
- 四类工作性质不同（串行 / 周期 / 保序 / 一次性），塞进一个池子会让"保存配置"排在"取 20 个图标"后面；轮询每 250 ms 又会让空闲线程不再真睡。
- 多核只用在图标像素处理（每张 256×256 要过 6 万多个像素），其余四条是 IO 等待，并行只增线程不缩短墙钟——`crossbeam-channel` / `arc-swap` 两个"计划中的"依赖因此删掉。

**代价**：结果晚一帧到几帧，图标从"启动时全部就位"变成"窗口先出来、图标陆续补上"；拖拽写盘变成停手 400 ms 后写，强杀会丢最后一次顺序调整；退出时不 join 工作线程（shell 调用可能卡在慢速路径）。顺带把 `ensure_com` 从进程级 `static Once` 改 `thread_local!`——否则只有第一个线程被初始化，其余线程的 `CoCreateInstance` 以 `CO_E_NOTINITIALIZED` 失败，表现为"图标偶尔取不到"。

---

## ADR-038 · 吸收上游 v1.6.0：23 个新键与小组件条带

**决定**：配置 76 → 99 键（23 个新键追加进 `NUMERIC_KEYS`，默认值与值域照抄上游 `DefaultCfg()`）；布局从"栏体右边挂一个时钟卡片"泛化成 `core::layout::WidgetKind` + `WidgetStrip`（复刻上游 `ord[]`、`WOrder` 交换、宽度公式 `barH × 0.92 / 0.72 / 1.16 / 2.2 / 1.6` 与 `g_ce` 汇总）；`Metrics.clock_extra` 改名 `widget_extra`。

**依据**：
- `upgrade_legacy` 会删掉 `[Settings]` 整段再按 `Key::ALL` 重写，表里没有的键在删段时一起消失——症状是"用户从 1.4 升到 1.6，配置被清空一半"。
- `WidgetKind::index()` 必须与上游枚举同序：`WideMask` 是位掩码按序号取位，换序会让老配置的双倍宽设置错位。
- v1.4.0 的几何已逐项对拍通过，所以只开时钟且 `WSide=0` 时 `widget_extra` 必须恒等于 `clock_w + gap + 6`，实测重构前后 `GEOM` 输出逐字节相同。

**代价**：五个未实现的小组件（通知 / 控制中心 / 资源监视器 / 计时器 / 专注模式）即使配置为开也不占位（`is_implemented()` 只对时钟为真），否则栏体上会留四段画不出东西的空白。`[Tool]` / `[M]` 段保留但不解析。

---

## ADR-039 · 资源监视器小组件：条带的第一个新成员

**决定**：`platform::sysinfo` 只读三源——CPU `GetSystemTimes`（累计值，用带 `prev` 的 `CpuSampler` 求差）、内存 `GlobalMemoryStatusEx`、磁盘 `GetDiskFreeSpaceExW("C:\\")`（对应上游数据源）；定时采样 + 数值变了才出帧；卡片文字按栏高定字号并量真实字宽后缩；详情面板是进程内第二个分层窗口，用 `painter` 显式定位；卡片命中走 `Dock::widget_at` 独立路径。

**依据**：
- `GetSystemTimes` 单次调用算不出占用率，第一次必须返回 0（无基线）；两个查询都不便宜，而 CPU 占用率本身只有秒级意义。
- 上游用 `GdipDrawString` 传矩形，GDI+ 会自动把字缩小到装得下，egui 不会，文字直接溢出卡片；卡片宽 `barH × 1.16`，而 `barH` 会随条目数被压缩（128 条时 76 → 37），固定字号会被裁成 "PU%"。
- `with_layout(right_to_left)` 在 `horizontal` 里拿不到剩余宽度——实测三条进度条标签都在、数值一个都没画出来。
- 卡片在栏体外侧、不在条目槽位里，必须先判卡片再判条目；面板不传带 `screen_rect` 的输入会让背景被裁、右边文字落在裁剪区外。

**代价**：`row_height` 只给行高、量不出字宽，只能走 `layout_no_wrap`。`NOOKSILL_GEOM=1` 直接打印卡片的屏幕坐标，自动化脚本不必自己推窗口偏移。

---

## ADR-040 · 计时器 / 待办小组件

**决定**：`core::tool` 不调 `Instant::now()`，所有时间相关函数收 `now_ms` 参数；待办存储只剥**一层** `d|` 前缀，不循环剥；比上游多两件事——时长可调（1..99 分钟）、面板高度按 `330 + n × 25` 算；落盘与配置分开防抖（120 ms vs 400 ms），`WriteJob` 带提交时刻的时间戳。

**依据**：
- 收 `now_ms` 让"倒计时归零"这类边界能用固定数字复现，不必 sleep 等真实时间。
- 上游只在 `v[0] == '1'` 时剥前缀，`done=0` 的条目每轮"存 → 读 → 存"都多叠一层（`0|买菜 → 0|0|买菜 → …`），而每次打勾都会存盘，正常使用中稳定恶化；循环剥会误伤合法文本（用户写 `1|foo` 会被吃成 `foo`），单层已根治累积。
- 面板有按钮和列表，命中区必须自己算（`Hits` + 逐个数）；在写盘线程里重新取时间会让秒表数字跳变。

**代价**：面板 UI 不用 egui 控件，矩形全自己算；`✓` / `✕` 不在打包字体里，改用线段绘制；分层窗口不响应 `WM_NCHITTEST`，手型光标要在 `WM_SETCURSOR` 里设。

---

## ADR-041 · 日历 / 农历：纯算法 + 一个弹层

**决定**：`core::calendar` 无 Win32 调用；农历表由 `scratch/gen_calendar_tables.py` 从上游 `minidock.c` 提取（脚本对每张表断言项数 201 / 24 / 30 / 12 / 10 / 12 / 12 / 7，文件头注明"不要手改"）；节气用平气公式不换 VSOP87；弹层 dismiss 从"鼠标移出就关"改成 `SetCapture` + "点到自己外面才关"；窗口的 8 px 透明边距要压住卡片边缘 4 px。

**依据**：
- 算法与窗口分开后，"农历算得对不对"可以穷举验证：1901-01-01 到 2099-12-31 共 72 684 天逐日「公历 → 农历 → 公历」往返、逐日连续性、以及 2020–2026 春节与三个闰月的公开锚点。
- 201 个十六进制数手抄必然错位，而错了不报错，只会让某几年的农历静默错一两天。
- 上游的 `for (i = 0x8000; i > 0x8; i >>= 1)` 照抄成 `0x8000..=0x8` 是空区间，循环一次都不跑，年长度恒为 348 + 闰月，整个农历错位两年多；正确写法按位移数遍历 `0..12`。
- 平气与本世纪真实节气最多差一天，而验收基准是"与上游对得上"，不是与紫金山天文台对得上。

**代价**：弹层开着时 Dock 收不到鼠标消息（图标悬停会冻住），与菜单一致；Esc 关不掉（`WS_EX_NOACTIVATE` 永远拿不到键盘焦点，写了也是死代码）。只能看 1901–2099。

---

## ADR-042 · 控制中心：真音量与"拉系统面板"

**决定**：分三层——`core::control`（磁贴顺序、行数、开关语义，纯数据）、`platform::audio` / `platform::sysact`（真调用）、`gpu::cc_win`（只画与判命中）。音量走 `IAudioEndpointVolume`；Wi‑Fi / 蓝牙 / 热点模拟 Win+A 拉出系统快速设置；深色模式改 `HKCU\...\Themes\Personalize` 两个 DWORD 后广播 `ImmersiveColorSet`；滑块 y 按实际行数算。

**依据**：
- `VK_VOLUME_UP/DOWN` 会改"应用程序音量"分组、在系统 OSD 上闪一下，且拿不到真实数值，滑块没法反映当前音量。
- 这三格没有稳定的公开开关接口（WinRT `Radio` 要 UWP 上下文，`netsh` 太慢且改不了蓝牙），就地切换要赌未文档化接口，赌输是"点了没反应"，更糟。
- 注册表 `= 0` 才是深色（语义是反的）；上游把滑块 y 写成"2 行磁贴之后"，但节能与热点默认都开着就是 8 个、3 行，滑块直接压在第三行。

**代价**：点这三格会弹系统面板，不是就地切换。COM 必须在调用线程初始化（忽略 `RPC_E_CHANGED_MODE`），接口指针不能跨线程，所以 `Audio` 不是 `Send`/`Sync`，由编译器守住；无音频设备时 `Audio::new()` 返回 `None`，滑块画成禁用态。有线网卡检测用 `GetAdaptersAddresses` 找 `IfType == ETHERNET_CSMACD` 且 `OperStatus == IfOperStatusUp`，建窗与每次 `show_at()` 都重查，任何失败降级为 `false`。

---

## ADR-043 · 起动台：把 700 行 C 拆成"模型 + 窗口"

**决定**：三层——`core::launchpad`（应用/文件夹/格子顺序/隐藏列表 + 拖放规则 + `launchpad.dat` 读写，零 Win32，15 条单测）、`platform::launchpad`（扫两个 `Programs` 目录，深度 4，过滤卸载器/readme）、`gpu::launchpad_win`（全屏窗口，只管画与收输入）。`drop_at(src, target, on_icon)` 把"落在图标本体（合并）还是格子空白（重排）"的判断留在 UI；文件夹成员为 0 → 整格消失、为 1 → 散开、否则保持。

**依据**：
- 700 行 C 里唯一难验证的是"拖到图标上会不会正确合并"，而它恰好最容易写错，所以像素不进模型层；少了 `on_icon`，"把 A 拖到 B 的位置想换序"永远变成"合并成文件夹"。
- 散开这一步容易漏，漏了会出现"点开一个只有一个图标的文件夹"，看起来像 bug。
- 一百个应用一次性取图标会让打开卡住几秒，所以复用 `IconLoader` 的**实现**、但自起一条线程只给当前页排任务；图标未到画字母占位，不引 loading 动画。

**代价**：添加方式改成 `WM_DROPFILES` 拖入（更符合直觉，省掉三个系统对话框），点击「＋」只打印提示；右键只保留"文件夹里右键 = 移出文件夹"，重命名与"添加到 Dock"在 ADR-044 才补。搜索框自己收 `WM_CHAR`，所以窗口必须能拿焦点。呈现失败会被 `TexturesDelta` 的 drop 断言盖住，所以 `FrameRenderer::render` 把"读回 + 呈现"包进闭包，先无条件清理 delta 再传播错误。

---

## ADR-044 · 中文输入法、起动台的菜单与重命名

**决定**：接 `WM_IME_*` 四条消息（`STARTCOMPOSITION` 清组合串并把候选窗挪到光标处、`COMPOSITION` 取 `GCS_COMPSTR` / `GCS_RESULTSTR`、`ENDCOMPOSITION` 清空、`SETCONTEXT` 必须交回 `DefWindowProc` 否则系统不画候选窗）；候选列表不自绘；组合中的拼音与已上屏文字拼起来画并加下划线；起动台的右键菜单与重命名框画在**自己窗口内**，"添加到 Dock"通过 `LaunchpadCmd::AddToDock` 抛给主循环。

**依据**：
- `WM_CHAR` 只在选词上屏那一刻给一个字符，拼音过程中按键被输入法吃掉，窗口什么都收不到——用户看到的是"程序不响应"。
- 候选框用系统那一套：用户熟悉、词库一致、翻页键不用重学；自绘候选要接 `ITfTextInputProcessor`，另一个量级且换不来可感知的好处。
- 组合部分加下划线不是装饰：用户需要看到"字还在输入法里"，否则会以为没打进去而重复输入。
- 窗口只管交互、配置改动集中在 `bin/dock.rs`，所以起动台不直接改 Dock 配置。

**代价**：`ImmGetCompositionStringW` 是"先问字节数、再取内容"，返回 UTF-16 **字节数**，0 或负数都表示没有，容易当字符数用。命中区有两个列表（格子 / 图标），只 push `icon_rects` 曾导致点图标根本不会启动应用。

---

## ADR-045 · 光标为什么一直是"忙"

**决定**：新增 `gpu::cursor` 一处维护映射——`class_cursor()` 填窗口类的 `hCursor`、`apply(icon)` / `set_hand_if(bool)` 在窗口过程里设、`client_pos(hwnd)` 自己取一次鼠标位置；七个窗口类全部给 `hCursor`，五个窗口过程补 `WM_SETCURSOR`，**持有捕获的窗口在 `WM_MOUSEMOVE` 里也设一次**。

**依据**：
- Windows 对"没有类光标、又不回 `WM_SETCURSOR`"的窗口不是退回箭头，而是退回 `IDC_WAIT`；所有窗口类都是 `..Default::default()`（即 `NULL`）。
- `SetCapture` 期间系统根本不发 `WM_SETCURSOR`，而"弹出即 `SetCapture`"的东西（模态菜单、日历、控制中心、按住图标拖动）几乎每个自绘窗口都有，所以症状"哪儿都有"。
- `WM_SETCURSOR` 的 `lparam` 低字是命中测试码不是鼠标坐标；也不能用上一次 `WM_MOUSEMOVE` 存的位置（系统先发 `SETCURSOR` 再发 `MOUSEMOVE`，会慢一格）。

**代价**：无。设置窗口原先在 `WM_SETCURSOR` 里一律设箭头，文本框上永远见不到 I 型光标，现在记住上一帧 egui 想要的形状。

---

## ADR-046 · 吸收 v1.6.5 的 19 个键 + 启动宽限期

**决定**：配置 99 → 118 键；`HPad` 在 `Key::ALL` 里照上游插在第 4 位（决定 ini 行序），在 `NUMERIC_KEYS` 里**追加在末尾**（决定 `Key::Numeric(i)` 下标）；新增纯函数 `occlusion::in_launch_grace(last_launch_ms, now_ms)`，宽限期 1.5 s，两个启动入口都打时间戳。

**依据**：
- `upgrade_legacy` 删整段重写，表里没有的键会静默归零——这是 ADR-038 同一个洞的第二次复发，所以功能可以后做，配置丢了找不回来。
- 行序必须跟上游，下标只能跟自己：`NUMERIC_KEYS` 插在中间会让 64 个具名常量全部错位，而错位编译期发现不了（`Key::Numeric(4)` 依然合法，只是指向了别的键）。
- 新启动的应用常先以最大化/全屏姿态出现（浏览器、游戏启动器），没有宽限期时点一下图标 Dock 就闪没、等窗口缩回才弹回；`now < last`（时钟异常）按"在宽限期内"处理。
- `MenuFrost` 与 `SepScale` 在上游 `DefaultCfg()` 里没有赋值，`Cfg cfg` 是静态零初始化，所以默认是 0（关）——照代码不照发布说明。

**代价**：无。

---

## ADR-047 · 合并胶囊 / HPad / 托盘小组件

**决定**：横向内边距从 `pad + 2` 改为独立键 `HPad`（默认 `Sc(12)`）；合并胶囊（`WMerge`，默认开）只在绘制层做——每张卡片的底盘改走 `paint_card_bg`，合并时直接 return，胶囊本身在 `paint` 里先于所有卡片画一次；托盘小组件 `W_TRAY` 宽 `barH × 0.72`、位置永远在最外侧，点击只做 `Win+B` 再 120 ms 后 `Enter`。

**依据**：
- 1.6.0 的 `M.padX` 恒为 10，1.6.5 默认 12，栏体两侧各宽 2 px、里面图标整体右移 2 px——这是跟上游走的有意变化，"与 v1.4.0 逐像素一致"的基线到此为止，对拍基准换成 1.6.5。
- 在布局层合并（把间隔压成 0）会让位置和上游对不上，还会牵动 `widget_at` 的命中区；上游 `RenderFrame` 画的是"首尾成员的外接框"，成员之间的 `gap + 6` 本来就留在胶囊内部。
- 上游的跨进程点击要 `SendMessage` 到 Explorer 的 `ToolbarWindow32` 再 `mouse_event`，要求两个进程同位数、工具栏结构一致，坐标算错就点到别处，而收益只省一次 Enter。

**代价**：`TrayW` 默认 1，条带默认多一个成员，所有"数成员个数"的测试要么显式 `TRAY_W = 0`、要么期望值 +1。

---

## ADR-048 · 环境烟雾：把 Taskbar Smoke 的流体模拟搬进 Dock

**决定**：把 Taskbar Smoke 的三块核心搬进 `nooksill-core::smoke` 并接到 egui——可平铺域扭曲 FBM（`Noise`）、Stam 稳定流体（`Fluid::step`）、网格 → 预乘 RGBA（`Fluid::raster`）；每帧只画一条栏体；不做 `style=texture` 风格；发射区改成贴轮廓 3 格（15 px）的窄带，掩码只一份（`Fluid::edge`）供源与光栅化共用；烟画在栏体**下面**。

**依据**：
- 烟不是"贴图平移"：密度场被速度场输运、速度场由浮力与噪声湍流驱动、涡量约束让小漩涡不肯散开、光标把烟推开——这几条叠加才有"活的"观感，预渲染动画一循环就假。
- egui 没有带纹理的 stroke，硬做只能预渲染几十张圆角环。
- 原版任务栏胶囊只有 40 px 高，整块内部当发射区没问题；Dock 栏体高 100 px，照抄会让烟被浮力顶到上沿、被边界困住，只看得见沿顶边一条细线。
- 栏体就是胶囊，烟画在上面的图标会被染成紫色；画在下面只看得见溢出的那圈光晕，这恰好是想要的。

**代价**：只在底部停靠生效；条目很少时表现为"一圈不均匀的光环"而不是"一片飘动的烟"。噪声表 40 ms（一次性），模拟 0.6 ms/帧、光栅 1.3 ms/帧（156×24 网格、390×60 位图，release），限速 30 fps 约 6% 单核——限速不只省 CPU，`FrameRenderer` 每帧要同步回读整块画面，60 fps 等于把回读翻倍；不启用时零成本。

---

## ADR-049 · 设置界面改成「左导航 + 右侧卡片」，一页一个文件

**决定**：布局换成左侧导航 + 右侧多卡片；一页一个文件（`pages/general.rs` … `pages/add.rs`），共享控件收进 `pages/widgets.rs`，加一页 = 加一个文件 + `Page` 枚举加一项；视觉基准是 `docs/settings-prototype.html`，改设计先改雏形再改 Rust。手工切两块 `Rect`（`ui.new_child(max_rect)`），起点用 `available_rect_before_wrap()`。

**依据**：
- 原来 9 页全写在 `pages.rs` 一个文件里（653 行），问题不在长而在"改一页要读全部"。
- `SidePanel` 按**根 Ui** 的可用区域布局，在子 Ui 里开面板会跟标题栏抢地方；`max_rect()` 不随已用内容收缩，拿它当起点会让两列从窗口顶端算起、直接盖住标题。
- 卡片与侧栏的表面色必须硬编码：`visuals.panel_fill` 在浅色主题下和窗口底色几乎同值，卡片会"消失"；只覆盖表面色、控件与文字仍走 `visuals`，换主题时对比度不会失控。

**代价**：`card()` 要强制撑满，否则一行 label 的卡片会缩成一小块、和同列对不齐；派生开关（磨砂 = `bg_mode == Frost`、"扫描运行中应用"同时改两个键）需要 `toggle_derived`，这类"设置项与配置键不是一对一"的映射是重构里最容易错位的地方。验收靠 `FrameRenderer::render_rgba` + `examples/settings_shot.rs` 在无显示器环境出 BMP，走与真实窗口完全相同的渲染路径。

---

## ADR-049 · 点击图标没反应：捕获、前台锁与"再点一次收起"

**决定**：
- `release_at` 必须在 `ReleaseCapture()` **之前**结算；`cancel_drag` 不再清 `click`。
- `force_foreground` 用三级阶梯（直接置前 → `AttachThreadInput` 挂到前台线程 → ALT 连击），之后复核。
- `in_front` 三条取或：句柄相等 → 同进程 → **自己没被遮挡**（`WindowFromPoint(中心点)` 的根窗口是自己）。
- 补 `ShowRunning`：`core::running::temp_keys` 只追加不重排，临时条目挂 `items` 最后一段、上限 16、不给"移除/加分割线"；扫描按需（`ScanInput::want_running`）。

**依据**：
- `ReleaseCapture()` 会**同步**给自己的窗口发 `WM_CAPTURECHANGED`，那条分支把 `press` 清成 `None`，等 `release_at` 跑到时已没人记得按在哪个图标上——每一次点击都被吞，且没有弹跳、没有日志、没有任何副作用。
- Dock 是 `WS_EX_NOACTIVATE`，点它不会拿到前台权，`SetForegroundWindow` 经常什么也不做；实际切换不是同一时刻完成，立刻复核会误判失败。
- 只认 `GetForegroundWindow() == hwnd` 时，前台是桌面（`Progman`）、是提权窗口（标题被 UIPI 挡成空）、或是应用的多窗口之一（浏览器、Electron），判据必然为假，于是永远走"显示"分支，而窗口本来就在眼前。
- `EnumWindows` 的 Z 序每轮都变，照它排会让图标互相穿插；栏体会按条目数等比缩小，几十个临时条目会压成一条细线。

**代价**：并排摆放、都没被遮挡的两个窗口，点后面那个的图标会把它收起而不是提到前台——它可见，用户不缺"看到"它。

---

## ADR-050 · 面板「点外面收起」：不能用鼠标捕获，只能轮询

**决定**：主循环加 `step_panel_dismiss`——面板可见时每 `popup::POLL_MS`（24 ms）读一次 `GetAsyncKeyState` 的左右键，发现"按下"且落点不在任何面板里就收起；同时看最低位与当前状态；落在 Dock 上放行，点条目时由 `step_click` 主动 `hide_panels()`。

**依据**：
- **只有前台窗口能抓鼠标**。Dock 是 `WS_EX_NOACTIVATE`，面板是它的子窗口，同样从不在前台——捕获"设置成功、收不到消息"，真机上点桌面毫无反应。
- `WS_EX_NOACTIVATE` 的窗口也收不到 `WM_ACTIVATE` / `WM_KILLFOCUS`，所以"失焦即关"这条路本来也不成立。
- 最低位是"自上次调用以来按下过"，能抓住两次轮询之间的快速点击，但只看它会被别的线程调过同一 API 弄丢边沿。
- 轮询不改变事件流向：那一下点击照常落到该去的地方，比捕获式"点一下只关面板"更符合直觉，也不需要 `WH_MOUSE_LL`（要额外承担钩子被系统摘掉的风险）。

**代价**：面板开着时每秒醒 42 次（每次只读两个按键状态与不超过四个矩形，微秒级），面板全关时一步都不走；键盘无法收起日历/控制中心（拿不到焦点），只能再点一次卡片。

---

## ADR-051 · 小组件文字按逻辑点排，不再"固定物理像素"

**决定**：所有小组件文字都从**卡片的逻辑尺寸**推——时钟 `line_h = (c.height() − 2·PAD_Y) / 行数`，时间 `0.70 · line_h`、日期 `0.44 · line_h`；资源卡片 `0.27 · c.height()`；两者都保留"量真实字宽、装不下就缩"的兜底。

**依据**：
- 原先一处写 `14.0 * p.k`（固定 14 px 物理字号，不随 DPI 变），另一处写 `metrics.bar_h * 0.27`（`bar_h` 已是物理像素，当逻辑点用等于 DPI 乘了两遍），于是 150% 下时钟比悬停气泡还小一圈、资源卡片反而偏大。
- `c.height()` 是逻辑点，算出的字号在 100% / 150% / 200% 下是同一个物理尺寸，栏高被条目数压缩时又同比缩小。
- 汉字在同样字号下比数字"重"，同一档字号时日期会更抢眼，所以时间 0.70、日期 0.44。

**代价**：无。旧的像素级对拍基线只覆盖栏体几何，不涉及任何几何量。

---

## ADR-052 · 内置条目的图标用矢量画，起动台补键盘操作

**决定**：`nooksill:start` / `nooksill:launchpad` 的图标用矢量画（圆角矩形 + 几何，比例照上游四格窗 0.27/0.04、九宫格 0.2/0.075），不用渐变；起动台补键盘——方向键夹在本页（左右到头翻页）、Home/End 到本页首尾、PageUp/PageDown 翻页、Enter 打开，选中格画 accent 描边，打搜索词时清空选中。

**依据**：
- 这两个条目**没有文件**，图标是现画的。走 shell 取不到就落到"首字占位"，Dock 上立着一块写着「起」的方块；`nooksill:launchpad` 被当普通文件丢给 `ShellExecuteW` 必然失败，表现为"点起动台毫无反应"。
- emoji 字体里不一定有、缺字是豆腐块，彩色字形跟其余图标不是一套语言；字体字符依赖字体覆盖，放大到 100 px 就露馅；矢量与尺寸无关、无字体依赖、缩放动画里不会糊。
- 上下翻页需要额外约定"翻到下一页的第几列"，实际用起来更容易迷路。

**代价**：无。菜单 / 重命名 / 文件夹弹层开着时方向键不归网格。

---

## ADR-053 · 起动台分类 / 标签 / A–Z：照搬语义，不照搬关键词

**决定**：`launchpad.dat` 末尾追加 `G\t<组名>`（1..=8 组）与 `C\t<应用名>\t<组号>`；老文件没有这两行就读成默认三组（办公 / 系统 / 工具）；分类号从 1 开始、`0` 表示"全部"；分类与搜索词是"与"，过滤视图只列应用不列文件夹；右键「移动到标签组…」做成二级菜单；A–Z 排的是顶层格子。

**依据**：
- 标签是"用户自己摆出来的东西"，跟格子顺序同类，不设单独开关文件。
- 原版 `LpCat` 的关键词里有 `\Windows\`，但它的 `path` 是 `.lnk` 解析出的目标 exe，我们存的是开始菜单 `.lnk` 路径，每一行都含 `\Windows\`——190 个应用全被判成"系统"。改成只认 `\Windows\System32\`、`\Windows\SysWOW64\` 与 `shell:` 前缀后是 办公 9 / 系统 0 / 工具 181。照搬启发式关键词前，先确认两边喂给它的**是什么字符串**。
- 主菜单已经有 5 项，再塞 9 个组会长得没法看。

**代价**：标签改名/删除、过滤视图里的拖拽排序未做。当前组标「（当前）」而不是 `✓`——字体里没有那个勾，画出来是豆腐块。

---

## ADR-054 · 起动台的改名框复用搜索框；过滤视图里拖动怎么算落点

**决定**：重命名不再画浮层，改由**搜索框就地接管**（改名时显示 `名字|` + accent 描边，右侧框外给「回车确认 · Esc 取消」），并同时给目标描一圈；名字上限 40 字，超框宽时缩字号而不截断。过滤视图新增 `move_app_before(src_ai, dst_ai)`：把源应用在顶层的位置挪到目标应用前面。

**依据**：
- 浮层固定画在 `y=96`、水平居中，而标签栏就在 `y=92..120`，两个字直接叠在一起；复用搜索框还能保住位置固定、命中区不变（点别处仍是"提交并退出"）。
- 名字是用户正在核对的东西，少一个字就可能改错。
- 过滤结果里相邻的两个应用在完整视图里可能隔着十几格，就地合成文件夹会让用户回完整视图找不到东西（原版在过滤视图里也只重排）；已在文件夹里的应用没有自己的顶层位置，挪不动。
- `visible_in` 按应用表顺序（扫开始菜单的顺序）返回，而拖动改的是顶层次序——按 `app_slot()` 排序才对得上，否则"拖了完全没有反应"。

**代价**：`launchpad.dat` 仍只在关窗时写盘，改名/删组/拖拽后强杀会丢；要改成防抖保存得先给 `save()` 加时间戳节流。自动化点击必须等一帧——`self.hits` 是上一帧的结果。

---

## ADR-055 · 控制中心磁贴顺序：存 id 不存位置

**决定**：`[widgets] Order=5,1,7,3,2,4,6,0`，逗号分隔的磁贴 **id**（0 无线/有线、1 蓝牙、2 深色、3 定位、4 静音、5 设置、6 节能、7 热点、8 有线），数值沿用上游 `[Cc] Order`；三条回退规则——关掉的磁贴保留位置、有线/无线共用一个槽位、`Order` 里没提到的追加末尾；拖动即时换位不做拖影，阈值 6 逻辑点，换过位就不再触发点击；落盘只把新顺序交给主循环走 400 ms 防抖，没拖过不写这一行。

**依据**：
- 存位置会被"开关一个磁贴"挤乱：把设置拖到第一格、后来关掉节能，用位置存会让重开的节能插到中间；用 id 存则"顺序"与"谁显示"无关。
- 3 列 × 3 行的格子只有 107×100，拖影会把底下的格子遮住一半，反而看不清会落到哪；即时换位本身就是最好的预览。
- 空 `Order=` 会让排查配置的人以为用户调过顺序（`[Graphics] Adapter` 是反例——那里空串有"自动"的语义，必须写）。

**代价**：设置界面里没有"恢复默认顺序"按钮，用户弄乱后只能手改 ini（删掉 `Order` 即可）。

---

## ADR-056 · 分割线拖动缩放：状态机放在 Dock 里，不在窗口过程里

**决定**：状态机放 `gpu::dock::Dock`，`press_at` / `drag_to` / `release_at` / `cancel_drag` 四个入口各加一个分支，窗口过程只多"设光标形状"；`press_at` 里**先于**普通拖拽判定（命中分割线且 `SepScale != 0` → 进入缩放，`press = None`、`drag = None`）；缩放比例以**按下那一刻**的 `icon0/zoom0` 为基准，`zoom` 同比后另夹 `32..220`，`(dy * 0.6) as i32` 截断不四舍五入，捕获被抢走时清缩放态但保留已改尺寸。

**依据**：
- `wndproc` 只拿得到 `Dock` 的裸指针（`GWLP_USERDATA`），拿不到 `App`，而缩放要改 `cfg.icon_size` / `cfg.zoom_size`、标脏、通知写盘。
- 顺序反了会把这一下当成"拖动分割线重排"，而分割线本来就不该被拖走；`release_at` 里缩放态必须返回 `None`，否则每次调完尺寸都会顺带启动一个应用或弹出系统面板。
- 用每帧当前值作基准会累积取整误差，来回拖回原位回不到原尺寸。

**代价**：`dy` 是客户区物理像素、`icon_size` 是配置逻辑值，高 DPI 下拖同样距离变化更慢，且栏体贴底时长高会让窗口上沿上移、对 `dy` 有阻尼——上游同样如此，保持一致比"修正手感"更重要。防闪烁靠两处：`drag_to` 里 16 ms 节流后再比较 `icon != cfg.icon_size`，主循环 `step_relayout()` 只在 `physical_size_i32()` 真变时重建（实测一次 40 px 拖动只有 3 次重建）。

---

## ADR-057 · 毛玻璃在 CPU 上做，不接 glass-backdrop 的 SPIR-V 管线

**决定**：只用 `glass-backdrop` 的三个**数据**模块——`capture`（GDI 抓屏）、`blur`（与 WGSL 同一份高斯权重）、`glass_sdf`（圆角 SDF）；模糊在 CPU 上做，结果当普通 egui 纹理画在 UI 最底层。

**依据**：
- 本项目没有交换链（ADR-008）：渲染是 `egui → 离屏 sRGB 纹理 → CPU 回读 → 预乘 BGRA DIB → UpdateLayeredWindow`，接它的 pass 等于再开一条 Vulkan 呈现路径，收益与复杂度不成比例。
- `fs_glass` 的折射 / 色散 / 2.5D 高光边与项目的纯平基调冲突（用户明确排斥渐变、折射、内发光）。
- 权重仍取自上游，`normalized_gaussian_weights` 改一边另一边会红。

**代价**：模糊是 CPU 开销，捕获分辨率必须降到 25% 才能接受（亚克力只看卷积结果，25% 与 100% 肉眼无差）；抓屏与模糊都在自己的线程上排队，不能和渲染重叠。

---

## ADR-058 · 实时背景用独立抓取线程 + `WDA_EXCLUDEFROMCAPTURE`

**决定**：面板显示前 `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`，隐藏时恢复 `WDA_NONE`；可见期间起 `nooksill-frost` 线程做「抓屏 → 与上一帧逐字节比较 → 变了才模糊 → `ColorImage` 经通道回传 → `WM_APP_WAKE` 叫醒 UI 线程」；节拍 100 ms，连续无变化时间隔翻倍退避到 800 ms，一有变化立刻复位。

**依据**：
- 面板已上屏时 GDI 抓到的是"桌面 + 面板自己"，直接当背景会得到无限嵌套的镜像。
- 实测（分层窗口铺纯红 + 底下绿窗口）：不排除时抓到 R255 G0 B0，排除后抓到 R0 G200 B0，恢复后回到前者——排除后读到的是**背后**的真实内容，不是黑色也不是自己。
- 必须在 `ShowWindow` **之前**设：之后才设，中间那一帧会抓到"桌面 + 自己"，实时层里闪一下镜像。
- 不做"抓之前临时排除、抓完恢复"：`SetWindowDisplayAffinity` 到 DWM 生效是异步的，要靠 `DwmFlush` 等一次合成（+33 ms/次），且那段时间用户截屏会缺一块。

**代价**：面板可见且开着毛玻璃时，它在**所有**屏幕捕获里消失——截屏、录屏、共享屏幕、任务栏缩略图都看不到它；关掉 `LpFrost` / `UiFrost` / `MenuFrost` 即恢复。抓屏线程自己做模糊（640×400/r=12 约 20 ms），并扣除本轮实际耗时，节拍才是稳定 100 ms 而不是"100 ms + 耗时"。

---

## ADR-059 · `blur_rgba` 改成平面布局 + 转置写

**决定**：拆成三个 `f32` 平面 → 横向卷积（结果**直接按转置写**）→ 再横向 → 交错回 RGBA。第二次卷积把"列"当行读，两趟内层都是连续切片点积。

**依据**：
- 原实现是交错 RGBA，内层按步长 4 取同一通道、每个 tap 一次 `clamp`，每次乘加约 3.7 个周期；实时路径每秒十次，40 ms/次不可接受。
- 640×400/r=12 从 40 ms 降到 20 ms，384×240 从 14 ms 降到 7 ms；`separable_blur_matches_naive_2d` 用朴素二维卷积逐像素对拍，误差 ≤1。

**代价**：多两块 `f32` 平面缓冲（640×400 各 1 MB），不值得为省这点内存改回交错。圆角遮罩只遍历四个角方块（半径以外覆盖率恒 255），`corner_only_alpha_matches_full_scan` 保证与整幅扫描逐字节相同。

---

## ADR-060 · 显示/命中判定收敛到一条"停靠带"热区

**决定**：`Dock::hot_phys()` 给出**一条**矩形——图标条 ∪ 小组件条带，外加两者之间 6 px 视觉缝与朝停靠边的投影留白（窗口物理坐标）；`layout::near_dock()` 是纯函数，两态共用同一套几何，差别只有容差（展开时多 32 px 回滞，施密特触发器）；判定输入（光标、热区、工作区、伸展量）全部显式传参，Win32 只剩 `GetCursorPos` / `GetWindowRect`。

**依据**：
- 原先展开态只认图标条 `bar_l..bar_l+bar_w`，光标停在时钟/控制中心/资源卡片上时判"不靠近"，450 ms 后收起；收起后光标仍贴屏幕底边且在窗口宽度内，立刻判"靠近"弹出——日志实测每 450 ms 翻转一次。
- 输入显式化后"光标在时钟上算不算靠近"可以写成断言，而不是靠人挪鼠标看它抖不抖。

**代价**：热区比"能看见的栏体"略大（含投影留白与 32 px 回滞），光标在 Dock 外一指宽处仍会让它保持展开——宁可多留一会儿，也别在用户手底下抽搐。同时修掉两处同源问题：遮挡判定也算小组件条带；`busy` 加上"面板/设置/起动台开着"。

---

## ADR-061 · 收起之后不再出帧：唤醒 ≠ 要出帧

**决定**：`Pumped` 增加 `Wake`——一批消息里只要出现过非 `WM_APP_WAKE` 的消息才算 `Frames`，否则只打断等待、不置 `need_frame`；出帧条件加 `!dock.fully_hidden()`（`hide_cur > 0.999` 时一帧都不画）；`step_smoke` 与 `wait()` 里的烟雾节拍同样按 `fully_hidden()` 停摆。

**依据**：
- `PostMessage(WM_APP_WAKE)` 是后台线程的统一唤醒手段，智能隐藏的扫描线程按 100 ms 回传一次，每次 `wake()`——收起状态每秒 10 条唤醒就是每秒 10 帧白画。
- `step_smoke` 在收起时直接 `return false` 而**没有推进 `next_smoke`**，`wait()` 用 `next_smoke` 算还要睡多久，于是 `left` 恒为 0、`timeout = 1 ms`，主循环以约 1000 Hz 空转——日志里完全看不出来，只有 CPU 占用降不下去。
- `!fully_hidden()` 是兜底：即使将来又有哪条路径置了脏标记，也不会画一张屏幕外的图。

**代价**：收起状态仍按 100 ms 醒一次（要轮询光标才能"探头"弹出），但每次只做几次只读判定，实测收起 8 秒 0 帧、CPU 0.26% 单核。`Wake` 不等于"什么都不用处理"：仍要 `DispatchMessage`，面板类窗口的 `WM_APP_WAKE` 就是在自己的窗口过程里把 `next_repaint` 置为现在。

---

## ADR-062 · 烟雾网格的身份只认尺寸，位置每帧更新

**决定**：`Fluid::ensure(w, h, seed, bar, thickness)`——`bar` 是格坐标矩形，**不参与身份判定**，只有 `(w, h, seed)` 变了才重建；掩码 `build_edge` 每次调用重算（O(格数) 距离场，约 0.1 ms）；网格取整窗尺寸并按 `GRID_BUCKET` 分档。

**依据**：
- 原实现把"栏体在网格里的位置"编码成四边 margin，网格尺寸跟着栏体走——悬停放大时栏体每帧变长 → 每帧重建 → 速度场与染料清零 → 烟闪烁（实测 grid 从 300x49 跳到 341x49）。
- 悬停时窗口本身也会变宽（1496 → 1720 px），不分档仍会跨档重建。
- 竖停靠与横停靠的长宽比差十倍，固定尺寸要么浪费内存要么不够用，分档是"够稳定"与"不浪费"之间的折中。

**代价**：整窗网格比原来的"栏体 + margin"大 3～12 倍，但 `raster` 对未点亮的格子提前返回，实测帧率不变（21–26 fps）。

---

## ADR-063 · 第二轮依赖库官方 API 审查：ADR-029 作废，改用生成 wrapper

**决定**：
- 删掉 `Interface::vtable` / `as_raw` / `PWSTR` 的手工 COM 调用，直接用 `windows` 生成的 wrapper。
- `platform::icon::draw_into_dib` 改走内存 DC：`SelectObject(dib)` → `DrawIconEx` → `GdiFlush` → 恢复旧对象 → `DeleteDC` → 读 `bits` → `DeleteObject`；所有可能提前 `?` 的计算挪到创建 DIB 之前。
- 回读：`poll` 返回后用 `try_recv()` 取代 `recv()`；`get_mapped_range()` 取像素包进闭包，所有路径都走到 `unmap()`；行去对齐抽成纯函数 `unpad_rows`。

**依据**：
- 依据一律取本地 cargo 缓存里的依赖源码 + Microsoft Learn，不凭印象改。逐字核对 `windows-0.62.2`：wrapper 与 COM ABI 一一对应，ADR-029 抄下的"错误签名"其实是 `IKnownFolder::GetPath`。
- 原 DIB 兜底把屏幕 DC 交给 `DrawIconEx`、从未把 DIB 选进 DC，后果是**图标画到屏幕左上角、DIB 恒全 0**，第三层兜底永远返回 `None`——前两层覆盖了绝大多数现代图标，所以这个"看着实现好了、其实一次都没跑通"的分支一直没被发现。
- `poll` 返回后回调必然已调用（`PollType::Wait` 文档），驱动异常时 `try_recv` 能给出可报告的错误而不是让 UI 线程永久挂死；`unmap` 漏掉会让**此后每一帧**的 `map_async` 都失败。

**代价**：无。核对后**不改**的六项一并记下以免重复试：`textures_delta` 顺序（官方也是先 `set` 上传、submit 后才 `free`）、`Context::run_ui`、`update_buffers` → `render` 顺序、`RendererOptions::dithering`、rayon 的 `into_par_iter().filter_map().collect()`、`PollType::wait_indefinitely()`。

---

## ADR-064 · MSRV 跟随依赖，lint 豁免改用 `#[expect]`

**决定**：`rust-version` 1.88 → 1.95；不再用 `#[allow]` 兜底——必须保留的豁免改 `#[expect(..., reason = "...")]`，不再触发的直接删，死代码删掉而不是留占位函数；新增/修改的代码注释用纯英文、简短。

**依据**：
- egui 0.37 / egui-wgpu 0.37 的 `rust-version = 1.95`，原声明低于依赖下限，在 1.88–1.94 上根本编不过。
- `#[expect]` 在 lint 不再触发时报 `unfulfilled_lint_expectations`（`-D warnings` 下是编译错误），本轮靠它抓到一处挂错函数的 `#[allow(clippy::too_many_arguments)]`：它挂在 3 参数的 `frost_spec` 上，9 参数的 `show_menu` 另有一份。

**代价**：升级 rustc / clippy 时阈值变化会让部分 `#[expect]` 变错误，需要当场判断是删豁免还是改代码——这正是要的反馈。直接依赖已是 crates.io 最新，本轮只更新传递依赖；`glass-backdrop` 的上游 HEAD 等于固定 rev，无更新空间。

---

## ADR-065 · frost 用 GPU compute，呈现路径零中间缓冲

**决定**：`blur_gpu.rs` 用 WGSL 两趟可分离高斯 + 圆角 coverage，输出 `Rgba8Unorm` storage texture，经 `Renderer::register_native_texture` 注册给 egui；权重与边界夹取沿用 `glass_backdrop`，CPU `blur_rgba` / `compose` 保留为回退与测试基准，GPU 不可用时自动退回；`Live` 线程只抓屏、字节用 `Arc` 共享、不碰 device；`Offscreen::with_mapped_pixels` + `present::upload_rgba_strided` 在映射期间直接写 DIB。

**依据**：
- CPU blur 在 640×400 / r=12 实测（debug）242 ms/次，而实时路径每 100 ms 一次，是最大的单点 CPU 开销。
- egui-wgpu 0.37 的纹理格式正是 `Rgba8Unorm`，与 CPU `ColorImage` 路径颜色空间一致，所以可以零回读且逐像素对齐。
- 实测（debug）GPU 提交 1.78 ms/帧、10 帧排空 30.66 ms，呈现 6.78 → 6.01 ms/帧（1.13x），像素一致；两条真机 `#[ignore]` 测试逐通道差 ≤1。

**代价**：每个开毛玻璃的窗口多一份 compute pipeline 与 GPU 缓冲（首次调用懒创建）；WGSL 与 CPU 公式必须同步改，否则一致性测试立刻红。

---

## ADR-066 · 所有像素着色器统一到 GPU compute 框架

**决定**：`gpu/shaders/ShaderRuntime` 统一 device/queue、pipeline 缓存、`Rgba8Unorm` 输出纹理、egui native texture 注册与回读，WGSL 分文件；frost 迁入框架，烟雾 raster + post 合成一趟 compute 并直接注册，图标圆角遮罩与烟团贴图同样走框架；CPU 实现全部保留为回退与像素级基准；工作线程允许提交 wgpu compute，不变式收敛为「不碰 `egui::Context`，纹理注册必须在 UI 线程」。

**依据**：
- CPU 侧仍有四类逐像素着色器，WGSL 与 CPU 公式分散，无法统一验证。
- 烟雾 raster/post 每帧约 1.2 MB 位图上传（30 fps ≈ 36 MB/s），合成一趟 compute 可消除。
- `Device` / `Queue` 是 `Send + Sync`，工作线程提交 compute 不需要额外机制。

**代价**：烟雾每帧仍需上传 `al|ox|oy` 三个流场（约 0.6 MB，比原 1.2 MB 少一半）——彻底消除要连 `Fluid::step` 一起 GPU 化，但流体是混沌系统，逐像素一致性不可验证。图标与烟团 GPU 化后仍需一次低频回读（≤256 KB）。`present::swap_rb` 是 GPU→CPU、Win32 DIB 是硬约束，`platform::icon::bgra_to_rgba` 是 GDI 格式重排且 platform 不依赖 wgpu，两者都不迁。质量验证是 7 条 `#[ignore]` 真机测试逐通道差 ≤1。

---

## ADR-067 · 帧率可配、取消硬上限、缩减常驻内存

**决定**：`[performance]` 新增 `FrameCap`（0 = 不限，默认）、`VSync`（按 `DwmGetCompositionTimingInfo` 实际刷新率对齐）、`Msaa`（0 自动 / 1/2/4 强制，下次启动生效），`CONFIG_VERSION` 2 → 3；删除 `SMOKE_FRAME` / `next_smoke`，活跃态统一由 `perf::pacing_ms` 决定唤醒间隔；设置窗口 `repaint_in()` 并入 `wait()` 的 timeout，面板轮询 24 → 8 ms；CJK 字体改 `OnceLock<Arc<FontData>>` 全进程共享，MSAA 按面积自适应（> 2 M 物理像素用 2x）。

**依据**：
- 原先主循环有三处硬编码帧率上限（烟雾 33 ms、动画 16 ms、设置窗口 16 ms），用户无法调整。
- 每个 `FrameRenderer` 各自把 8.04 MiB 的 `msyh.ttc` 读进内存；2560×1600 窗口单块 4x MSAA 纹理 62.5 MiB。
- 字体共享与面积自适应 MSAA 不改变像素输出；真机 DWM 刷新间隔实测 16 ms，动画最大唤醒延迟 16 → 1 ms，设置窗口最长停顿 1000 → 1 ms。

**代价**：默认不限帧率时，烟雾/动画开启会持续同步回读，CPU 占用上升，用户可用 `FrameCap` 或 `VSync` 收回；大窗口 2x 的抗锯齿略弱于 4x，`Msaa=4` 可强制恢复，且只在启动时读。空闲态仍睡满 `FS_CHECK_MS`，"空闲 0 帧"不变。

---

## ADR-068 · 呈现成本实测：暂不上 DirectComposition

**决定**：**不**引入 DirectComposition / swapchain，`UpdateLayeredWindow` 保持不变；新增 `crates/nooksill-gpu/examples/present_bench.rs` 长期作为呈现路径的测量工具（真实分层窗口，alpha 1/255 不可见但走完整合成路径）；`present_dirty`（`UpdateLayeredWindowIndirect` + `prcDirty`）已实现但**不接入运行时**。

**依据**：
- release 实测（1000 次/档，取 p50）：1320×1080 upload 0.79 ms + present 0.22–0.49 ms，2560×1440 分别 2.04 / 1.36 ms；整条呈现路径占 60 Hz 预算 6%（1320×1080）到 21%（2560×1440）。
- ADR-008 的 2 ms 门槛只在 2560×1440 被轻微触碰，而 DComp 要付的是呈现层重写 + ADR-008 作废。
- debug 的 6.01 ms 几乎全是 CPU 侧换序（`swap_rb`），release 只剩 0.79 ms；`swap_rb_strided` 加 AVX2（一条 `vpshufb` 换 8 个像素，行尾重叠整向量写）后降到 0.09 ms（8.7x），2560×1440 从 2.04 降到 0.44–0.60 ms。
- `prcDirty` 在 MSDN 上只是 hint，且 egui 不暴露 damage 区域，传全窗口矩形等于没优化。

**代价**：bench 只量 API 调用与位图拷贝，不含 DWM 合成线程的 GPU 工作，所以证明不了"不会掉帧"。另记一条测量前提：同一份源码 `cargo clean` 前后重新编译会有 0.7% 的字节差异（浮点收缩/FMA 影响字形 AA），"逐像素一致"只能在同一构建内比。

---

## ADR-069 · 节能磁贴读真实方案，用 API 而不是 spawn `powercfg`

**决定**：`sysact::active_power_scheme()` 用 `PowerGetActiveScheme` 读真实方案（构造时与每次打开面板时各读一次）；`PowerSaver` 进入节能前记住当时方案、退出时还原，无记忆才退回「平衡」；切换改用 `PowerSetActiveScheme`；有线网络磁贴指向 `ncpa.cpl`。

**依据**：
- `CcEnergy` 是"要不要显示这一格"，不是"现在是不是节能模式"，按它显示会在点一下之后才和系统对上。
- `powercfg.exe` 每次点都要起一个进程，慢且可能闪窗口；退出节能时一律切回「平衡」会改掉用户本来在用的「高性能」。
- `PowerGetActiveScheme` 返回 `LocalAlloc` 出来的 GUID 指针，拷一份再 `LocalFree`，耗时微秒级，与 shell 调用差三个数量级；`toggle()` 返回回读值，"界面显示与系统实际不一致"从结构上消失。
- 有线用户要的是网卡列表（禁用/启用/诊断），不是 Wi‑Fi 状态页。

**代价**：精简系统/策略禁用 `powrprof` 时读不到方案，退化为"关"（点击仍会尝试设置，失败则状态不变、不假装成功）；"进节能前的方案"只存在进程内，重启后退回「平衡」，避免把陈旧状态写回用户系统。附注：面积自适应 MSAA 选 2x 时必须请求 `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`，否则 `create_texture` 按 WebGPU 规范只认 [1,4] 而 panic——`perf::resolve_samples` 以设备实际可接受为唯一判据，不可用时**向上取 4**，不悄悄砍抗锯齿。

---

## ADR-070 · 版本号单一来源：构建期算一次，三处分发

**决定**：唯一的版本计算逻辑放 `scripts/version.rs`，被 `crates/nooksill-core/build.rs` 与 `crates/nooksill-gpu/build.rs` `include!`；规则是 `NOOKSILL_VERSION` 有值→原样用、`PROFILE=release`→`X.Y.Z`、否则 `X.Y.Z-dev.<构建编号>`（构建编号取 `NOOKSILL_BUILD` → `git rev-list --count HEAD` → 0）；分发三处——`cargo:rustc-env`（关于页）、VERSIONINFO（属性页）、`target/<triple>/<profile>/NOOKSILL_VERSION`（`scripts/package.sh` 拼产物名）；`Cargo.toml` 版本改为与 CHANGELOG 同号（0.9.25），删掉 `APP_VERSION`。

**依据**：
- 产物名、关于页、属性页三者不一致是最容易在发版时翻车的地方，而"再算一遍"是这类不一致的根源；让 build script 算一次、把结果落成文件、下游只读不推，就把问题从流程问题变成结构问题。
- 构建编号用 git 提交数而不是时间戳：同一棵树在任意机器上得到同一个号，`cargo clean` 前后不跳变，出包可复现；CI 用 `run_number` 是因为没有完整 reflog。

**代价**：debug 号随提交单调增长，与 CHANGELOG 的小版本号是不同维度；`rerun-if-changed` 挂在 `.git/logs/HEAD` 上，提交后第一次构建会重编 core 与 gpu；`include!` 共享源码不能写 `//!` 文档（E0753），所以 `scripts/version.rs` 用行注释。

---

## ADR-071 · 应用内更新：WinHTTP + 镜像测速 + DoH 只做判据

**决定**：HTTP 用 WinHTTP 不引 `reqwest`；JSON 自己解析（300 行 `json` 模块）；镜像列表写死、顺序每次由真实请求（DNS + TLS + 首字节）测速决定，够快就立刻用（默认 600 ms），直连排最后；DoH 只做判据——与系统解析结果完全不相交才判污染并禁用直连；替换自己靠一段 `cmd`（轮询 `tasklist` 等本进程退出后 `move /Y` 再 `start`，下载先落同卷的 `NookSill.update.exe`，校验不过不落盘）；两个开关藏在连点版本号 5 次解锁的高级卡片里。

**依据**：
- 三个请求（GitHub API、DoH、资产下载）都只需要 GET，WinHTTP 自带系统代理、系统信任库、重定向、HTTP/1.1，约 200 行；`reqwest` + `rustls` 会多 1 MB 以上还要自己管证书。
- 反代域名几个月换一批，写死顺序必然过期；测速用真实 HTTP 而不是 ICMP，因为反代的瓶颈在 TLS 与回源。
- WinHTTP 的 TLS 只按 `WinHttpConnect` 的域名做 SNI 与证书校验，拿 DoH 的 IP 建连必然证书不匹配；要真把 DoH 当传输得自己实现 TLS 栈。
- 更新链路的每一步失败都要能解释清楚，所以状态是四段文字（检查/测速/下载/校验）直接显示在卡片上，而不是一个转圈。

**代价**：WinHTTP 跟着系统代理走，用户开全局代理时测出来的是代理速度（这不该回避）；DoH 依赖 Cloudflare 可达，不可达时退化成只看镜像测速。`/releases/latest` 在没有正式版时返回 404 是常态，此时退回 `/releases?per_page=15`、跳过草稿与缺资产的条目、按版本号取最大；匿名接口 60 次/小时限流也翻成人话。

---

## ADR-072 · 为 Vulkan / Linux / ARM64 留的抽象边界

**决定**：不再给 wgpu 包 HAL，不为着色器做 trait 抽象；切口放在**呈现**层——Phase 2 把 `present.rs` / `frame.rs` 里的 `HWND` 收进 `trait Presenter`、`FrameRenderer::render(hwnd, pos)` 拆成 `render()` + `present()`，倒影同批改成 GPU blit 或多画一遍镜像 layer；Phase 3 把六个 `*_win.rs` 一分为二（纯 egui 的 `ui/` + `platform/win32/window.rs`）；Phase 4 Linux 平台，AppBar 用 layer-shell 的 `exclusive_zone` 表达"让出屏幕边"。

**依据**：
- wgpu 已经是 HAL，再包一层只会把 backend 差异藏起来，出问题时更难查；WGSL 本身可移植，要改的只是参数载体。
- 实测把 `wgpu::Backends::DX12` 换成可控之后，同一个 `settings_shot` 在 DX12 与 Vulkan 下出图**逐像素完全一致**（642,960 像素，最大通道差 0），WGSL、compute、离屏渲染、回读四段本来就后端无关。
- 真正的移植成本在平台层：`nooksill-platform` 5081 行全是 Win32，`nooksill-gpu` 31 个文件里 18 个直接 `use windows::Win32`。同机设备开销 DX12 464 MB vs Vulkan 114 MB，但不因此切默认后端——Windows 上的呈现路径是 `UpdateLayeredWindow + DIB`，与 DWM 合成的一致性才是理由，省 350 MB 是代价不是目标。
- 倒影现在做在 DIB 上（`present::mirror_rows` 直接改 BGRA 行），换 swapchain 后这段代码不存在，不一起改则 Linux 上没有倒影。

**代价**：Phase 2 会让 `FrameRenderer` 的调用点全部改一遍（dock、settings、menu、四个弹层窗），机械但面广；Phase 3 是六个文件各 200–400 行的搬家，判据是**新文件里不该出现 `HWND` 以外的 Win32 类型**。ARM64 换 target 即可，前提是不写 x86 假设（目前没有）。

---

## ADR-073 · 模糊延迟：探针唤醒 + 脏列卷积，实验路径用时域色彩重投影

**决定**：
- 不推翻 800 ms 退避，另加 25 ms 一步的廉价探针：读屏幕 DC 上 20 个像素点（`GetDC(NULL)` + `GetPixel`，一次 < 0.1 ms），变色立刻唤醒全量抓屏，探针失效时 300 ms 兜底。
- 变化时 50 ms 跟随，两次抓屏至少隔 40 ms。
- 高斯只算脏列：源第 x 列变化只影响输出 `[x−r, x+r]` 列，`FrostFrame` 带变化列范围、WGSL 加 `x0` 并改列优先映射，输出纹理其余像素原样保留；丢帧时取脏列并集。
- 实验路径 `[appearance] FrostIllusion`（默认 0）开窗跑一次真高斯保留结构，之后用 24×16 低频色场做色彩重投影 + 90 ms 时间插值，只有色场平均差 > 18 且距上次刷新 > 300 ms 才重跑真高斯。

**依据**：
- 延迟不在卷积（GPU 两趟高斯只要 1.8 ms 提交），在静止退避最长 800 ms 与"每次变化都整幅 640×400 重算"，哪怕只有时钟那一小块在动。
- 直接把退避砍到 50 ms，空闲时单核常年 20 ms/50 ms = 40%，违背"空闲不提交"的产品承诺。
- 每帧用 IIR 近似大半径高斯仍有拖影，桌面从亮变暗时"慢半拍"更明显；面板 tint 只透出约 22% 的模糊层、高斯又抹掉高频，人眼察觉的主要是颜色与明暗，所以让颜色立刻跟上、只缓存不影响观感的结构。

**代价**：探针是抽样，理论上会漏掉 20 个采样点之外的细微变化（300 ms 兜底保证不会永久漏）；实验路径在窗口位置/形状骤变时结构最长滞后一次刷新间隔（≥300 ms）。脏列优化依赖输出纹理跨帧持久，`ensure()` 重建目标时必须强制全量，否则新纹理未初始化区域是黑的。开关是进程级 `AtomicBool`，将来若改为多实例或每窗口独立配置要换成显式传参。原高斯与 CPU 回退全部保留，关掉即回到原路径。

---

## ADR-074 · i18n 用「英文文案即键」的集中表，不用枚举键或外部语言包

**决定**：
- 文案查表集中到 `core::i18n`：`t(key)` 静态文案、`fmt(key, args)` 动态模板。`{0}`..`{9}` 占位让译文自己决定语序。
- **英文文案本身就是键**，同时是底层语言与权威键清单（`en.rs::TEXT`）。`zh_cn.rs` / `zh_tw.rs` 是各自独立的 `.rs`，按英文键排序、二分查找；缺项逐键回退英文，未登记的键也显示传入的英文，不会出现空串或键名。
- 语言是进程级 `AtomicU8`（`Language`）。启动时 `platform::locale::ui_language_id()` 读 Win32 `GetUserDefaultUILanguage`，由 `i18n::configure(tag, lang_id)` 与配置 `[general] Language` 合成；空 / `auto` / 未知 tag 跟随系统，不支持的语言落到英文。
- 加一条文案 = 调用点写 `t("English text")` + `en.rs` 登记 + `zh_cn.rs` 给中文。繁体表允许缺项。完整性由测试守：`en::TEXT` 有序无重复、翻译表只含已登记键、源码里每个调用点都已登记、下拉选项常量全部有键。

**依据**：
- 现有 UI 有 500+ 条文案。用强类型枚举键要维护「枚举 + 英文 + 中文」三份同长列表，新增一条必须同时改三处，且枚举本身会超过单文件 600 行的约定；英文即键把三份压成两份，漏登记由测试而不是编译器发现，但回退路径仍然可读。
- `core` 零依赖且被另外三个 crate 依赖，放这里不引入新 crate、不需要 `phf` / `fluent` / 运行时加载语言文件；排序数组 + 二分查找无分配、无锁，适合逐帧即时模式 UI。
- 全局语言状态与项目已有的 `MSAA_PREF` / `panel_style::sync_prefs` / `ILLUSION` 同构；设置界面本来就是每帧重画，切换语言只需把 `changed` 传回主循环同步一次，不需要把语言塞进每个页面的 `Ctx`。
- 动态模板若继续用 `format!("中文{}")`，译文无法改语序；统一 `fmt()` 后中文表也能重排占位符。
- 「用显示文案当命令标识」的起动台菜单改成 guard 比较（`_ if label == t("…")`），而不是把译文写进 `match` 模式——后者在 Rust 里既非法也无法跨语言。

**代价**：进程级语言意味着同一进程内不能每个窗口用不同语言（当前也不需要）；测试必须显式固定语言，否则断言会随宿主机区域漂移。窗口标题是 Win32 创建时的字符串，切换语言要给已打开的设置窗口补一次 `SetWindowTextW`。农历、节气与干支的名称属于文化数据（日历面板标签已本地化），控制台日志与诊断串不属于 UI，三者不随语言切换；繁体表有意只覆盖 49 条常用词，其余界面按约定显示英文，补齐只需往表里加行。

---

## ADR-075 · 按官方 API 语义收口文本、调度与回读，不做架构级重写

**决定**：
- 面板与起动台的文本截断统一到 `gpu::text`：`LayoutJob` + `TextWrapping::truncate_at_width` 一次排版，`painter.galley` 绘制；删掉 3 份逐字符重排。
- `FrameRenderer` 把 egui 的 `repaint_delay` 记成截止时间，`run_loop` 的「是否出帧」与 `wait()` 的「等多久」同时消费；完全隐藏时屏蔽残留期限。
- 两处 GPU 回读改用带 200 ms 超时的 `PollType::Wait`，且所有提前返回路径都 `unmap()`。
- 注释门禁覆盖 `*.wgsl`；控件 ID 盐不再使用翻译后的显示文案。
- **保持不变**：进程级 atomics（i18n / MSAA / panel style / illusion）、库 crate 的具体错误类型与主循环的 `Box<dyn Error>`、UI 只产出 `Command`、同步回读 + `UpdateLayeredWindow`、`illusion` 的线性 `p.t`、`lerp_to` 的 dt 驱动指数平滑。

**依据**（一手来源，按结论分组）：
- egui 0.37：`Painter::text` 内部会再排版一次，`galley` 画已排好的 `Arc<Galley>` → https://docs.rs/egui/0.37.0/egui/struct.Painter.html ；`TextWrapping::truncate_at_width` 单次 layout 内省略并置 `Galley::elided` → https://docs.rs/epaint/0.37.0/epaint/text/struct.TextWrapping.html ；`Id::unique` 是固定种子哈希、跨帧稳定，但 `Id::new` 已弃用 → https://docs.rs/egui/0.37.0/egui/struct.Id.html 与 https://github.com/emilk/egui/blob/0.37.0/crates/egui/src/id.rs ；`ViewportOutput.repaint_delay` 仍是 0.37 的正式字段 → https://docs.rs/egui/0.37.0/egui/viewport/struct.ViewportOutput.html ；`textures_delta` 顺序与 debug 断言 → https://docs.rs/egui/0.37.0/egui/struct.FullOutput.html 。
- wgpu 30：`PollType::Wait` 支持 `timeout`，`wait_indefinitely()` 无超时 → https://docs.rs/wgpu-types/30.0.1/wgpu_types/enum.PollType.html ；`Buffer::unmap()`「terminates the effect of all previous map_async operations」→ https://docs.rs/wgpu/30.0.1/wgpu/struct.Buffer.html ；回调触发与映射期独占 → https://docs.rs/wgpu/30.0.1/wgpu/struct.BufferSlice.html ；256 字节行对齐 → https://docs.rs/wgpu/30.0.1/wgpu/constant.COPY_BYTES_PER_ROW_ALIGNMENT.html 与 https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12device-getcopyablefootprints 。
- 呈现层：`UpdateLayeredWindow` 每帧整窗更新、窗口宜小 → https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow ；`AC_SRC_ALPHA` 要求 32bpp 预乘 → https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-blendfunction ；DirectComposition 的代价与 Win10 起改荐 Windows.UI.Composition → https://learn.microsoft.com/en-us/windows/win32/directcomp/directcomposition-portal 。
- WGSL 注释语法（行注释与可嵌套块注释）→ https://www.w3.org/TR/WGSL §3.3；多行 `bytesPerRow` 256 倍数 → https://www.w3.org/TR/webgpu/ 。
- 架构：Rust 2024「Don't use globals」给出 atomics / OnceLock / Mutex 替代 → https://doc.rust-lang.org/edition-guide/rust-2024/static-mut-references.html ；`OnceLock` 是 write-once → https://doc.rust-lang.org/std/sync/struct.OnceLock.html ；atomics 定位 → https://doc.rust-lang.org/std/sync/atomic/index.html ；`pub(in path)` 语义 → https://doc.rust-lang.org/reference/visibility-and-privacy.html ；库/应用错误分层（C-GOOD-ERR、Rust Book ch9、anyhow 与 thiserror）→ https://rust-lang.github.io/api-guidelines/interoperability.html 、 https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html 、 https://github.com/dtolnay/anyhow ；Command 模式即 Elm TEA 的 Cmd → https://guide.elm-lang.org/effects/ ；微软明言简单场景只采用有明确收益的技术 → https://learn.microsoft.com/en-us/windows/apps/develop/data-binding/data-binding-and-mvvm 。
- 算法：线性采样高斯需把 `src` 从 `array<u32>` 改成 texture+sampler 才能获益 → Intel https://www.intel.com/content/www/us/en/developer/articles/technical/an-investigation-of-fast-real-time-gpu-based-image-blur-algorithms.html 与 Rastergrid https://www.rastergrid.com/blog/2010/09/efficient-gaussian-blur-with-linear-sampling/ ；compute+LDS 未必更快 → https://lisyarus.github.io/blog/posts/compute-blur.html ；时域复用的 ghosting / 闪烁与邻域 clip → https://dl.acm.org/doi/fullHtml/10.1145/3681758.3697996 、 ARM SIGGRAPH 讲义；固定 lerp 的帧率偏差与 `1-exp(-k·dt)` → https://lisyarus.github.io/blog/posts/exponential-smoothing.html 、 https://tront.xyz/lerp 。
- 明确否决：`lerp_to` 已用 `1-(1-k)^(dt/16.67)`，改 `1-exp(-k·dt)` 属重复劳动；`illusion.p.t` 已由真实 dt 线性推进且以 `t>=1.0` 停止，改指数形式会渐近不到 1.0、导致永不停止重绘；异步/双缓冲回读与 DComp+swapchain 需重写 `Offscreen` 生命周期、帧序与倒影绑定，而小窗口同步回读常 1–2 ms，收益不足。

**代价**：文本省略由 egui 决定，断字位置可能与旧的逐字符实现略有差异（已用 `panels_shot` 7 张出图核对）；200 ms 回读超时在驱动异常时丢一帧、下帧重试，取代无限挂起；egui 请求重绘期间会开启 1 ms 定时器精度，功耗略升，空闲仍为零帧。保持同步回读意味着 UI 线程仍有每帧一次阻塞点，若将来出现持续 60 fps 全屏动画或改动 illusion 的时间参数化（破坏重绘终止条件），必须重新评估异步化；保持全局 atomics 意味着多实例场景需重新设计配置同步。

---

## ADR-076 · 翻译表分段存储，编译期拼回单一公开切片

**决定**：
- `en.rs` / `zh_cn.rs` 只留骨架；英文键与简体译文按字母顺序分到 `en/{first,second}.rs`、`zh_cn/{first,second}.rs`，每段约 300 行。
- 两个骨架用 const 块把分段拼成**单一** `TEXT` 切片；`lookup`、`has_key`、完整性测试与所有公开路径不变。
- 不用 `include!` 注入数组元素，也不把查询改成分段二分。

**依据**：
- 规模门禁把生产 `.rs` 限在 600 行，`en.rs`(599) / `zh_cn.rs`(592) 已贴线，新增一条文案即失败；放宽门禁或加白名单会让规则失去意义（ADR-075 已拒绝同类做法）。
- rustc 实测：`const X: &[&str] = &[include!("frag.rs")];` 且片段含多个逗号分隔项时报 `error: include macro expected single expression in source`（`incomplete_include` 默认 deny）。
- 分段二分要改 `Language::table()` 的返回类型，波及 `mod.rs`、`tests.rs` 与多个断言；const 块只动两个骨架文件。
- Rust const 上下文支持 while 循环与数组索引，拼接在编译期完成，运行期零分配、零额外查找。

**代价**：const 块在编译期复制约 600 个静态引用，编译开销可忽略，但骨架不如单个字面量直观；新增键必须放进正确的字母段，顺序由 `i18n::tests` 的有序性断言守住。若某一段再逼近 600 行，继续按字母细分即可。

---

## ADR-077 · 坐标只在边界转换，显卡判断只用结构化字段

**决定**：
- `Dock` 内部统一使用逻辑轴 `(u,v)`：Win32 的窗口像素在 WndProc、`DockDropTarget`、`cursor::client_pos` 三处边界经 `Dock::logical_point`（`layout::to_logical`）转换；反过来所有屏幕/窗口矩形经 `map_rect` 输出。烟雾光标是唯一需要窗口像素的消费者，用 `window_point` 转回。
- 删掉 `drop_index_at` 里「横排取 x、竖向取 y」的窗口坐标分支，四边统一用逻辑 `u`；`group_under` 把逻辑点转成窗口点再与 `icon_rect` 比较。
- 启动时设备创建读取 `[Graphics] Adapter`（`Gpu::new(&dock.cfg.adapter)`），不再硬编码空串。
- 软件适配器由 DXGI `DXGI_ADAPTER_FLAG_SOFTWARE` 位判定，不再匹配厂商/驱动名；名字匹配失败时按 PCI `(vendor, device)` 兜底；`Selection::Pinned` 保存配置名，`matches_config` 只比配置是否变化。

**依据**：
- `layout::map_rect` 与 `to_logical` 是互逆契约，`core/layout/tests/metrics.rs::edge_mapping_roundtrips` 四边验证；此前 `to_logical` 在主程序中零调用，导致靠左/靠右时鼠标窗口像素被当成逻辑轴，命中、放大、拖放、弹窗锚点整体错位。新增 `dock/tests/hit.rs` 的四边往返与矩形转置断言。
- Microsoft Learn：`DXGI_ADAPTER_FLAG_SOFTWARE = 2` 表示软件适配器，`DXGI_ADAPTER_DESC1.Flags` 是该类型的成员 → https://learn.microsoft.com/en-us/windows/win32/api/dxgi/ne-dxgi-dxgi_adapter_flag 、 https://learn.microsoft.com/en-us/windows/win32/api/dxgi/ns-dxgi-dxgi_adapter_desc1 。名字字符串不是稳定标识（不同驱动、语言、后端会变）。
- wgpu 30 `AdapterInfo.vendor/device` 是后端特异的 PCI id，`device_type` 是结构化设备类型 → https://docs.rs/wgpu-types/30.0.0/src/wgpu_types/adapter.rs.html 。DXGI 与 wgpu DX12 后端都以 PCI id 标识同一块硬件，故可做名字之外的稳定匹配。
- Win32 鼠标 `LPARAM` 低/高 16 位是有符号 `i16`（`MAKEPOINTS`），`SetCapture` 拖出窗口时必须按有符号解析，否则得到 65535。

**代价**：所有新增输入入口都必须先转换坐标，漏一处就回到旧 bug；`physical_xy` 改为有符号后菜单窗口也受影响（正向修复）。PCI 兜底只在名字匹配失败时才枚举 DXGI，启动多一次同步枚举，且在非 DX12 后端（Vulkan/GL）id 表示可能不同，名字匹配仍优先。独显/核显分档仍依赖专用显存阈值——DXGI 没有设备类型字段，这一档只影响 UI 排序与标签，不参与设备选择。

---

## ADR-078 · 热键做成配置，其余经验值只具名化

**决定**：
- `[Hotkeys] Toggle` 存全局显隐组合，默认 `Ctrl+Alt+D`。`core::hotkey::Hotkey` 是零依赖的表驱动解析器（修饰符位 + 虚拟键码），`[Settings] Hotkey` 仍是 0/1 开关。
- `platform::hotkey::GlobalHotkey` 用 RAII 拥有注册；`App::sync_global_hotkey` 在配置变化时重绑，成功即时生效、失败回滚到旧组合并只提示一次。`Drop` 负责注销。
- 设置页 General 提供文本输入，合法才写回 `Config`。新增 4 条 i18n 键，删除写死组合的旧文案。
- 其它逻辑类硬编码只改成具名常量、不改数值：`layout::metrics` 的缩放/标签/侧边预留 7 项、`platform::gpu` 的显存阈值、`frame::render` 的 illusion 重绘间隔。实现不变量（256 对齐、4096 描述符预算、`WM_APP` 号段、Win32 常量）保持原样。

**依据**：
- Win32 `RegisterHotKey` 用 `MOD_*` 位与虚拟键码标识组合，同一 `(hwnd, id)` 只能绑定一个；因此解析层用结构化字段而非字符串，重绑必须先注销再注册。
- 组合会被其它进程占用，注册失败不能让 Dock 失去快捷键，故 rebind 需要回滚路径（参考 `Gpu::Selection` 的回退语义）。
- C 版 `[Settings]` 的 76 个键是一一对应的不变式；Rust 独有设置沿用 `[Graphics]` 的独立段做法，不污染那张表。
- 把每个经验数字都做成 ini 项会显著增加配置面与迁移成本，而用户无法有意义地调整「标签气泡预留 34 px」这类值；具名常量足以让代码可读、让门禁守住，代价更小。

**代价**：热键组合是字符串配置，需要解析与回退逻辑（已用 4 条单测 + ini 往返测试守住）；新增按键名要同时更新 `NAMED_KEYS` 与文档。运行时重绑失败时 `hotkey_applied` 仍标记为已应用以避免每帧重试，用户需再次修改才会重试。老 ini 没有 `[Hotkeys]` 段时静默使用默认组合。提交信息规则同时改为纯 ASCII 英文（commit-msg 钩子 + CI），历史提交不重写。
