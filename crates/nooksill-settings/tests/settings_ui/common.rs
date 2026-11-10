//! Shared kittest harness: state container, window size and shell stub.

use egui_kittest::Harness;
use nooksill_core::config::Config;
use nooksill_settings::{settings_ui, Command, SettingsState};

#[derive(Debug)]
pub(super) struct St {
    pub(super) cfg: Config,
    pub(super) ui: SettingsState,
    pub(super) commands: Vec<Command>,
}

pub(super) fn new_harness() -> Harness<'static, St> {
    harness(false)
}

/// With `shell = true`, emulate `gpu::settings_win::shell`: draw a title bar
/// first, then hand the remaining area to `settings_ui`.
///
/// The variant exists because `max_rect()`, which `settings_ui` once used, does
/// not shrink with content, so both columns covered the title. Without a shell
/// stand-in a test cannot catch that.
pub(super) fn harness(shell: bool) -> Harness<'static, St> {
    // Tests locate widgets by their Chinese labels; pin the language so the
    // assertions do not depend on the host's display language.
    nooksill_core::i18n::set_language(nooksill_core::i18n::Language::ZhCn);
    let state = St {
        cfg: Config::defaults(),
        ui: SettingsState {
            config_dir: r"C:\Users\test\AppData\Roaming\NookSill".to_owned(),
            ..SettingsState::default()
        },
        commands: Vec::new(),
    };
    // Generous size, so the ScrollArea does not crop widgets out of reach.
    let size = if shell {
        [900.0, 720.0]
    } else {
        [900.0, 1200.0]
    };
    let builder = Harness::builder()
        .with_size(size)
        // Fonts must be installed before the first frame: egui 0.37 panics on a
        // missing glyph (MissingGlyphPolicy::Panic) instead of drawing tofu.
        .with_context_setup(|ctx| {
            assert!(
                nooksill_settings::fonts::install_cjk(ctx),
                "本机找不到中文字体，界面无法渲染"
            );
        });

    if shell {
        builder.build_ui_state(
            |ui, st: &mut St| {
                ui.heading("NookSill");
                ui.separator();
                run(ui, st);
            },
            state,
        )
    } else {
        builder.build_ui_state(|ui, st: &mut St| run(ui, st), state)
    }
}

fn run(ui: &mut egui::Ui, st: &mut St) {
    let outcome = settings_ui(ui, &mut st.cfg, &mut st.ui);
    if let Some(cmd) = outcome.command {
        st.commands.push(cmd);
    }
}
