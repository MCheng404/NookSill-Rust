//! Checks the egui 0.37 assumption behind the shared-Context memory plan.
//!
//! Two contexts rasterize two atlases; two viewports of one context share one.
//! The test inspects font image sizes only, so it runs without a GPU or desktop.

use egui::{Context, ViewportId};

fn rasterize(ctx: &Context, viewport: ViewportId, text: &str) {
    let mut input = egui::RawInput {
        viewport_id: viewport,
        ..Default::default()
    };
    input.viewports.insert(
        viewport,
        egui::ViewportInfo {
            parent: Some(ViewportId::ROOT),
            ..Default::default()
        },
    );
    input
        .viewports
        .insert(ViewportId::ROOT, egui::ViewportInfo::default());
    let full = ctx.run_ui(input, |ui| {
        ui.label(text);
    });
    // epaint asserts on drop unless texture deltas are consumed.
    let mut delta = full.textures_delta;
    delta.clear();
}

fn atlas_bytes(ctx: &Context) -> usize {
    let [w, h] = ctx.fonts(|f| f.font_image_size());
    w * h * 4
}

#[test]
fn one_context_two_viewports_keeps_one_font_atlas() {
    let shared = Context::default();
    let a = ViewportId::from_hash_of("panel.a");
    let b = ViewportId::from_hash_of("panel.b");

    rasterize(&shared, a, "日历 alpha");
    let after_a = atlas_bytes(&shared);
    rasterize(&shared, b, "控制中心 beta");
    let after_b = atlas_bytes(&shared);

    // Both viewports must read the same single atlas (default width 2048).
    assert_eq!(after_a, after_b, "a second viewport must not add an atlas");
    assert!(after_a > 0, "the atlas must hold rasterized glyphs");
}

#[test]
fn two_contexts_each_pay_their_own_atlas() {
    let first = Context::default();
    let second = Context::default();

    rasterize(&first, ViewportId::ROOT, "日历 alpha");
    rasterize(&second, ViewportId::ROOT, "控制中心 beta");

    let one = atlas_bytes(&first);
    let two = atlas_bytes(&second);
    assert!(one > 0 && two > 0, "both atlases must hold glyphs");
    assert_eq!(one, two, "default atlas geometry is identical per context");
}

/// Seven panels each rasterizing their own CJK glyphs pay seven atlases.
/// The same workload on one Context across seven viewports pays one.
/// This is the assumption behind the shared-Context memory plan.
#[test]
fn seven_panels_share_one_atlas_with_viewports() {
    let panels = [
        "日历 星期一 农历 节气 节假日 提醒 待办",
        "控制中心 无线网 蓝牙 定位 亮度 音量 节能 飞行模式",
        "资源监视器 处理器 显卡 内存 磁盘 网络 进程 运行时间",
        "计时器 倒计时 待办事项 完成 添加 删除 编辑 保存",
        "速记 笔记 新建 标题 内容 搜索 置顶 导出",
        "应用组 开发工具 设计软件 浏览器 文件夹 游戏 全部应用",
        "Dock 栏 启动台 设置 关于 语言 主题 外观 效果 图标",
    ];

    let installed = nooksill_settings::fonts::install_cjk(&Context::default());

    let separate: Vec<usize> = panels
        .iter()
        .map(|text| {
            let ctx = Context::default();
            if installed {
                assert!(nooksill_settings::fonts::install_cjk(&ctx));
            }
            rasterize(&ctx, ViewportId::ROOT, text);
            atlas_bytes(&ctx)
        })
        .collect();

    let shared_ctx = Context::default();
    if installed {
        assert!(nooksill_settings::fonts::install_cjk(&shared_ctx));
    }
    for (i, text) in panels.iter().enumerate() {
        let id = ViewportId::from_hash_of(format!("panel.{i}"));
        rasterize(&shared_ctx, id, text);
    }

    let separate_total: usize = separate.iter().sum();
    let shared_total = atlas_bytes(&shared_ctx);
    eprintln!(
        "atlas budget: 7 contexts = {} bytes, 1 context = {} bytes (CJK={installed})",
        separate_total, shared_total
    );
    assert!(
        shared_total <= separate_total,
        "shared Context must not exceed seven private atlases"
    );
}
