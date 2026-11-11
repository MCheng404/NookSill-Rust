# 协作约定

这个仓库目前只有一个人开发，所以规则的目的**不是**流程管控，而是两件事：

1. 让"改坏了"在提交前暴露，而不是在下次跑起来时才发现；
2. 让历史可读 —— 半年后回来看 `git log`，能知道每次改动**为什么**发生。

技术栈的取舍见 [`DECISIONS.md`](DECISIONS.md)，模块职责见
[`ARCHITECTURE.md`](ARCHITECTURE.md)，这里只讲怎么提交。

---

## 一、克隆后必做一次

```bash
git config core.hooksPath .githooks   # 启用提交前门禁
```

不做这一步的话 `pre-commit` 不会跑，提交照样能过 —— 门禁是**约定**，
不是保险。另外确认 `core.autocrlf` 是 `false`（换行由 `.gitattributes`
决定，两者同时生效会打架）：

```bash
git config core.autocrlf false
```

## 二、门禁分三级

| 时机 | 命令 | 跑什么 | 耗时量级 |
|---|---|---|---|
| 每次提交（自动） | `.githooks/pre-commit` | 注释语言 + 单文件规模 + `fmt --check` + `clippy -D warnings` | 秒 |
| 提交前 / 合并前 | `./scripts/build.sh --gate` | 上面四项 + `cargo test` | 十几秒 |
| 发版 | `./scripts/build.sh --release` | release 构建（LTO + `opt-level=z`） | 一分钟 |

两个文本门禁是纯扫描，不依赖 MSVC，本地钩子、CI 的 `gate` job 与
`--gate` 跑的是同一批脚本（都先跑 `--self-test` 再扫仓库）。

`pre-commit` **故意不含测试**：全量测试要链接 7 个 crate 的测试二进制，
放进提交钩子的结果一定是被 `--no-verify` 绕过，那还不如不放。

跳过门禁的两种方式，语义不同：

```bash
SKIP_GATE=1 git commit ...   # 明确表示"这次不管"，钩子自己打印一行提示
git commit --no-verify ...   # git 原生逃生口，钩子完全不执行
```

## 三、分支

单人项目，不强制 Git Flow：

- **文档、单文件小修**：直接提 `main`。
- **跨 crate 改动、重构、改公共 API**：开分支，合回 `main` 时保留 merge
  commit（`git merge --no-ff`），这样 `git log --first-parent` 能看到
  "这是一整块工作"，而不是一串碎片。
- 分支名用 `类型/短描述`：`refactor/layout-dedup`、`fix/clock-offset`。

## 四、提交信息

格式：`类型: 一句话`（中文，不超过 72 字符，句末不加句号）。

| 类型 | 用于 |
|---|---|
| `feat` | 新功能（对应 PORTING.md 里的某个 P 项） |
| `fix` | 修缺陷。**正文里写清楚"原来错在哪"** |
| `refactor` | 不改行为的结构调整 |
| `perf` | 只改性能（附实测数字，见下） |
| `docs` | 只改文档 |
| `test` | 只改测试 |
| `chore` | 构建、依赖、仓库配置 |

正文（可选，但 `fix` / `perf` / `refactor` 建议写）回答三个问题：

```
fix: 时钟开启时栏体右端被裁

原因：calc_metrics 把 clock_extra 算进了逻辑宽度，但布局没减掉它。
影响：GDI+ 版与 GPU 版都偏右，宽度越窄越明显。
验证：cargo test -p nooksill-core layout
```

性能类改动必须带**同机、同配置**的前后数字，否则下一个人没法判断
该不该保留。本仓库的做法是 `./scripts/probe-mem.sh` 与 `BUILD.md` 的实测表。

## 五、什么不该进版本库

| 路径 | 原因 |
|---|---|
| `target/` | 构建产物，GB 级 |
| `reference/` | 上游 C 源码。**版权未授予再分发权**（见 README），只允许本地对照 |
| `scratch/` | 临时验证程序（含编译出来的 exe） |
| `.workbuddy-ai/` | 协作工具的工作区数据，只对当前机器有意义 |
| `NookSill.ini` / `icons/` | 运行期数据，便携模式下会写在这里 |

`Cargo.lock` **要提交**：这是二进制程序，不是库，依赖必须可复现。

## 六、代码备注

- **纯英文、ASCII**。中文只允许出现在面向用户的字符串与测试断言的期望消息里。
- **一行说清**。一行写不完，说明这段代码该拆函数，而不是该加注释。
- **只写读代码看不出来的东西**：Win32/驱动的行为怪癖、单位与坐标系、
  执行顺序约束、性能约束、不变量。
- **不写**：代码正在做什么（名字已经说了）、改动历史、踩坑故事、
  ADR/CHANGELOG 编号、"为什么这么设计"的长篇论证、强调性修辞。
- 原因只写一句，直接陈述，不加"这是因为""必须注意"之类的引导语。
- 模块头 `//!` 一到三行：这个文件负责什么、边界在哪。
- 门禁 `scripts/check-comments.sh` 扫 `crates/` 下 `*.rs` 与 `*.wgsl` 的行注释
  （含行尾）、文档注释与块注释；字符串里的 `http://` 不当注释。`--self-test`
  用固定样例守住门禁本身，新增注释形态或文件类型时先补样例。

## 七、单文件规模

- **一个 `.rs` 只放一个职责**。生产代码单文件目标 600 行，超过 800 行必须拆。
- 拆分方式：`foo.rs` → `foo/{mod,state,paint,input}.rs`。
  `mod.rs` 放类型与子模块声明；`impl` 块可以分散在子模块里。
- 跨子模块调用的私有方法标 `pub(super)`，不要为了省事把整个类型改 `pub`。
- 测试超过 400 行独立成 `tests.rs`，再多按被测模块拆。
- 这条对**所有** Rust 项目生效，不限于本仓库。
- 门禁 `scripts/check-size.sh` 按「排除末尾 `#[cfg(test)]` 模块后的行数」判定，
  生产代码与独立测试文件上限都是 600 行；`--self-test` 守住 600 / 601 的边界。
  拆完仍需 `cargo check --workspace --all-targets` 验证公开路径没变。

## 八、文档与版本

- 每次改动写进 [`CHANGELOG.md`](CHANGELOG.md) 的**当前小节**，只记
  「改了什么 + 依据 + 验证」，不写过程叙事、不写排查流水账。
- 新的技术取舍追加到 [`DECISIONS.md`](DECISIONS.md) 的末尾，格式固定为
  「决定 / 依据 / 代价」，**不删旧条目** —— 错误的决策留着，才有人知道
  为什么不能重犯。
- 版本号只有一个来源：`scripts/version.rs`（构建期跑一次）。**改版本 = 改
  `Cargo.toml` 的 workspace `version`**，并在 `CHANGELOG.md` 里开一个同名小节；
  其它地方不要再写死版本号。Debug 的 `-dev.<构建编号>` 由构建脚本自己加。
- 发版＝版本号对齐后打 `v*` 标签，CI 会校验标签与 `Cargo.toml` 一致再出包。
  流程见 [`BUILD.md`](BUILD.md) 第八节。
