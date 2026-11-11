//! The Dock window: a real dock, not the settings window.
//!
//! Reuses the presentation pipeline from [`nooksill_gpu::frame::FrameRenderer`]
//! (offscreen render, readback, `UpdateLayeredWindow`) and only swaps the
//! content for [`nooksill_gpu::dock::Dock`].
//!
//! Esc quits. Hovering magnifies an icon and shows its label bubble.

// GUI subsystem: double clicking the exe must not open a console window.
#![windows_subsystem = "windows"]

use std::time::{Duration, Instant};

use nooksill_core::clock;
use nooksill_core::clock::ClockLine;
use nooksill_core::config::{Config, DockMode, Edge, Key};
use nooksill_core::group;
use nooksill_core::ini::Ini;
use nooksill_core::item::{Item, ItemKind};
use nooksill_core::layout;
use nooksill_core::layout::Rect as LRect;
use nooksill_core::running;
use nooksill_core::smoke::{Tuning, PALETTE};
use nooksill_core::store;
use nooksill_gpu::calendar_win::CalendarWindow;
use nooksill_gpu::cc_win::CcWindow;
use nooksill_gpu::dock::{Dock, DockItem};
use nooksill_gpu::edit_win::{EditField, EditRequest, EditSlot};
use nooksill_gpu::frame::FrameRenderer;
use nooksill_gpu::frost::Spec as FrostSpec;
use nooksill_gpu::gpu::Gpu;
use nooksill_gpu::group_win::{GroupAction, GroupKid, GroupWindow};
use nooksill_gpu::launchpad_win::LaunchpadWindow;
use nooksill_gpu::menu::{Menu, MenuCmd};
use nooksill_gpu::note_win::{NoteAction, NoteWindow};
use nooksill_gpu::resmon_win::ResMonWindow;
use nooksill_gpu::settings_win::{self, SettingsWindow};
use nooksill_gpu::smoke::{SmokeFx, View as SmokeView};
use nooksill_gpu::tool_win::ToolWindow;
use nooksill_gpu::worker::{
    DiskWriter, IconLoader, IconRequest, OpenJob, Opener, ScanInput, Scanner, Waker, WriteJob,
    WM_APP_WAKE,
};
use nooksill_platform::app_window;
use nooksill_platform::appbar::{self, AppBar};
use nooksill_platform::display;
use nooksill_platform::launch;
use nooksill_platform::occlusion;
use nooksill_platform::shortcut;
use nooksill_platform::single_instance::SingleInstance;
use nooksill_platform::sysact;
use nooksill_platform::theme;
use nooksill_platform::Paths;
use nooksill_settings::{Command, ItemEdit, SettingsState};
use std::pin::Pin;
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, ScreenToClient, UpdateWindow, PAINTSTRUCT,
};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{IDropTarget, OleInitialize, RegisterDragDrop, RevokeDragDrop};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetKeyState, ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE,
    TRACKMOUSEEVENT, VK_SHIFT,
};
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetWindowLongPtrW,
    GetWindowRect, MessageBoxW, MsgWaitForMultipleObjectsEx, PeekMessageW, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, GWLP_USERDATA, HWND_TOPMOST, MA_NOACTIVATE, MB_ICONERROR, MB_OK, MSG,
    MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW,
    SW_SHOWNA, WM_CAPTURECHANGED, WM_DESTROY, WM_DROPFILES, WM_ERASEBKGND, WM_HOTKEY, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_QUIT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SETTINGCHANGE, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

fn main() {
    // A GUI subsystem process has no console, so a trace flag attaches to the
    // parent console to keep `eprintln!` visible. A double click has no parent
    // console and the failed attach is harmless.
    if trace::any() {
        // SAFETY: attaching to the parent console has no side effect when it
        // fails, which is the double click case.
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
    // Panics and early errors must surface as a window, otherwise a double
    // click looks like nothing happened. Only the main thread pops up: a
    // worker panic must not block the user behind a modal dialog.
    let main_thread = std::thread::current().id();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("{info}");
        if std::thread::current().id() == main_thread {
            fatal(&format!("程序异常：{info}"));
        }
    }));
    if let Err(e) = run() {
        fatal(&e.to_string());
    }
}

/// Last resort without a console: pop a window instead of exiting silently.
fn fatal(message: &str) {
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = "NookSill"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both pointers refer to NUL-terminated UTF-16 buffers on the
    // stack of this function.
    unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(text.as_ptr()),
            windows::core::PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut app) = App::boot()? else {
        // `None` means another instance is running, which is a normal exit.
        return Ok(());
    };
    let outcome = {
        // OLE drop target: `DragOver` drives the live insertion gap, `Drop`
        // parses CF_HDROP. After a successful `RegisterDragDrop` OLE wins and
        // `WM_DROPFILES` stops firing; if it fails, the `DragAcceptFiles`
        // registered in `boot()` takes over without the live gap.
        // SAFETY: this thread is the UI thread (STA), so the OLE callbacks run
        // on the same thread as the message loop.
        unsafe {
            let _ = OleInitialize(None);
        }
        let target: IDropTarget = nooksill_gpu::drop_target::DockDropTarget::new(app.hwnd).into();
        let registered = unsafe { RegisterDragDrop(app.hwnd, &target) }.is_ok();
        if !registered {
            eprintln!("OLE 拖放注册失败，回退到 WM_DROPFILES（无动态让位）");
        }
        let r = app.run();
        if registered {
            // Revoke while the window is still alive, so no COM callback
            // reaches a destroyed window.
            unsafe {
                let _ = RevokeDragDrop(app.hwnd);
            }
        }
        // `target` is dropped at the end of the block, after RevokeDragDrop.
        r
    };
    // Drop `App` first (its destructor unregisters the AppBar, writes the
    // config and releases the single-instance lock), then hand over the
    // replacement script: it waits for this process to exit.
    let pending = app.pending_update.take();
    drop(app);
    if let Some(staged) = pending {
        nooksill_platform::update::apply_and_restart(&staged)?;
    }
    outcome
}

mod trace {
    use std::sync::LazyLock;

    fn flag(name: &'static str) -> bool {
        std::env::var_os(name).is_some()
    }

    /// Detailed logs for geometry, reordering, clicks and scanning.
    pub fn geom() -> bool {
        static V: LazyLock<bool> = LazyLock::new(|| flag("NOOKSILL_GEOM"));
        *V
    }

    /// Logs the fullscreen detection decisions.
    pub fn fs() -> bool {
        static V: LazyLock<bool> = LazyLock::new(|| flag("NOOKSILL_FS_TRACE"));
        *V
    }

    /// Logs the message flow of the menu window.
    pub fn menu() -> bool {
        static V: LazyLock<bool> = LazyLock::new(|| flag("NOOKSILL_MENU_TRACE"));
        *V
    }

    /// Logs the decisions of the click-outside-to-close rule.
    pub fn popup() -> bool {
        static V: LazyLock<bool> = LazyLock::new(|| flag("NOOKSILL_POPUP_TRACE"));
        *V
    }

    /// Prints one line per second with the real frame count (`NOOKSILL_FPS=1`).
    ///
    /// This is how the idle promise is verified: a fully hidden Dock must
    /// print nothing at all.
    pub fn fps() -> bool {
        static V: LazyLock<bool> = LazyLock::new(|| flag("NOOKSILL_FPS"));
        *V
    }

    /// Whether any trace flag is on.
    ///
    /// Only then is attaching the parent console worth it: a GUI subsystem
    /// process has no console and would drop every `eprintln!`.
    pub fn any() -> bool {
        static V: LazyLock<bool> = LazyLock::new(|| {
            geom()
                || fs()
                || menu()
                || popup()
                || fps()
                || flag("NOOKSILL_GEOM")
                || flag("NOOKSILL_TRACE")
        });
        *V
    }
}

/// All mutable state of a Dock session.
struct App {
    hwnd: HWND,
    gpu: Gpu,
    frame: FrameRenderer,
    /// Owns the global toggle hotkey; `Drop` unregisters it.
    global_hotkey: nooksill_platform::hotkey::GlobalHotkey,
    /// Combo and enabled flag last applied, so rebinding runs only on change.
    hotkey_applied: (nooksill_core::hotkey::Hotkey, bool),
    /// Must be on the heap: `wndproc` stores its address in `GWLP_USERDATA`,
    /// and the `Box` content does not move when `App` itself does.
    dock: Box<Dock>,
    paths: Paths,
    /// Full items with launch paths and resolved `.lnk` targets. `dock.items`
    /// holds only the display subset; both must stay in sync or a click
    /// launches the wrong thing.
    raw: Vec<Item>,
    dpi: i32,
    ppp: f32,
    /// Work area of the target monitor, in physical pixels.
    work: LRect,
    appbar: AppBar,
    /// Rect reserved by the AppBar; `None` means nothing was reserved.
    reserved: Option<LRect>,
    /// Screen coordinates of the window origin, in physical pixels.
    pos: (i32, i32),
    /// Window size, in physical pixels.
    size: (i32, i32),
    menu_slot: MenuSlot,
    /// Edit popup (rename, create). Modal like the menu, so it reuses the
    /// same slot type.
    edit_slot: EditSlot,
    /// Group panel, created lazily: most sessions never use groups.
    group_slot: Option<Pin<Box<GroupWindow>>>,
    /// Index of the group shown by the group panel within `raw`.
    group_index: Option<usize>,
    /// Notes panel, created lazily.
    note_slot: Option<Pin<Box<NoteWindow>>>,
    /// All notes; the source of truth, written back as a whole `[Notes]`.
    notes: nooksill_core::note::NoteBook,
    settings_slot: Option<Pin<Box<SettingsWindow>>>,
    /// Channel of the background update check and download thread. The UI
    /// thread only `try_recv`.
    update_rx: Option<std::sync::mpsc::Receiver<UpdateMsg>>,
    /// The release info received last, needed for the download.
    update_release: Option<nooksill_platform::update::Release>,
    /// The downloaded file that passed verification.
    update_staged: Option<std::path::PathBuf>,
    /// Pending replacement to run after exit. It must not call
    /// `process::exit` inside `apply_update`: that skips the destructors, so
    /// the AppBar reservation and the config write are lost.
    pending_update: Option<std::path::PathBuf>,

    // Background threads. The UI thread only `try_recv`, it never waits.
    /// Icon loading (shell and COM, serialised).
    icons: IconLoader,
    /// Run state and occlusion detection, both need `EnumWindows` and share
    /// one thread.
    scanner: Scanner,
    /// When the system samples were last refreshed, for the resource monitor
    /// card.
    last_sysinfo: std::time::Instant,
    /// Resource monitor detail panel, created lazily.
    resmon_slot: Option<Pin<Box<ResMonWindow>>>,
    /// Timer and todo panel, also created lazily.
    tool_slot: Option<Pin<Box<ToolWindow>>>,
    /// Calendar popup, created lazily.
    calendar_slot: Option<Pin<Box<CalendarWindow>>>,
    /// Control centre panel, created lazily: it holds COM interfaces.
    cc_slot: Option<Pin<Box<CcWindow>>>,
    /// Launchpad, created lazily and only once: it owns an icon thread.
    launchpad_slot: Option<Pin<Box<LaunchpadWindow>>>,
    /// `NOOKSILL_DEBUG_OPEN` is handled exactly once.
    debug_opened: bool,
    /// When the last frame was laid out. Animations advance by real elapsed
    /// time, not by frame count.
    last_layout: Instant,
    /// Whether the system timer precision is currently raised to 1 ms.
    timer_raised: bool,
    /// Click-outside-to-close for the panels, by polling global mouse buttons.
    ///
    /// No mouse capture is used, see the note at the top of
    /// [`nooksill_gpu::popup`]: only the foreground window can capture the
    /// mouse and the Dock never takes the foreground.
    watcher: nooksill_gpu::popup::Watcher,
    /// Whether a panel was open in the previous round, used to reset
    /// `watcher` at the moment a panel opens.
    panel_was_open: bool,
    /// When the last app was launched, in milliseconds since process start.
    ///
    /// See [`LAUNCH_GRACE_MS`]: a freshly launched app often appears maximized
    /// or fullscreen, and an immediate check would make the Dock hide and
    /// come back.
    last_launch_ms: Option<u64>,
    /// When the timer panel changes should hit the disk, debounced together
    /// with the config.
    tool_save_at: Option<Instant>,
    /// The last printed widget geometry, printed only on change.
    widgets_dump: String,
    /// Frame count of the current second and its start time.
    fps_frames: u32,
    fps_at: Instant,
    /// Ambient smoke around the bar.
    smoke: SmokeFx,
    /// The DWM accent colour, read once at startup: it only changes when the
    /// user switches the theme, which sends `WM_SETTINGCHANGE`.
    accent: [f32; 3],
    /// Cached **system** dark mode. `theme::is_dark()` reads the registry
    /// every time, but a theme change also wakes the loop, so there is no
    /// need to ask every frame. `cfg.dock_is_dark()` only reads a config
    /// field and is still evaluated per frame.
    system_dark: bool,
    /// Number of appearance changes handled so far (see [`Dock::look_epoch`]).
    look_epoch: u32,
    /// ini writer (serial, so ordering is automatic).
    disk: DiskWriter,
    /// Launches and activates apps through `ShellExecuteW`.
    opener: Opener,

    /// Earliest unsaved change; `None` means nothing is pending.
    ///
    /// Items and config **share one debounce window**: one drag can change
    /// the order, one slider drag can change dozens of values, and writing
    /// each time would rewrite the whole UTF-16 file every frame.
    dirty_at: Option<Instant>,
    /// Items to write back; `None` means the `[Items]` section is unchanged.
    dirty_items: Option<Vec<Item>>,
    /// Notes to write back; `None` means the `[Notes]` section is unchanged.
    dirty_notes: Option<Box<nooksill_core::note::NoteBook>>,
    /// Config to write back; `None` means the `[Settings]` section is unchanged.
    dirty_cfg: Option<Box<Config>>,

    /// Latest occlusion result from the scanner, used for smart hide only.
    covered: bool,
    /// Temp items for running apps that are not pinned.
    ///
    /// They live in memory only, never in the ini, but they **can be
    /// reordered**: the bar may hold nothing but running apps
    /// (`[Items] Count=0`), and a fixed order there would leave no room for
    /// drag sorting. The new order is written to `temp_keys` and the scanner
    /// thread keeps it. They are always the **last** section of `dock.items`,
    /// so `dock.items[i]` and `raw[i]` still match one to one for
    /// `i < raw.len()` and the existing click and menu indices keep working.
    temps: Vec<Item>,
    /// Lowercased exe names of the last round of temp items, used to decide
    /// whether the list has to be rebuilt and to keep the order stable.
    temp_keys: Vec<String>,
    /// The item list has to be rebuilt, but a drag is in progress.
    ///
    /// `replace_items` clears `drag`, `press` and `hover`, so rebuilding
    /// during a drag would break the drag the moment the icon moved.
    items_stale: bool,
    /// The parameters last sent to the scanner thread; `None` means none.
    ///
    /// The whole `ScanInput` is kept rather than individual fields: any
    /// change to it requires a resend, and tracking fields separately leads
    /// to a new value that never takes effect.
    scan_sent: Option<ScanInput>,

    last_fs_check: Instant,
    fs_hidden: bool,
    /// Whether the foreground window is maximized, used to switch between the
    /// normal and maximized hide delays.
    fg_maximized: bool,
    /// `NOOKSILL_FAKE_MOUSE="u,v"`: pins the cursor for unattended screenshots.
    fake_mouse: Option<(f32, f32)>,
    /// `NOOKSILL_FAKE_COVERED=0/1`: overrides the occlusion result.
    fake_covered: Option<bool>,
    /// Result of the last "near" test, logged only under `NOOKSILL_GEOM=1`
    /// to show whether the expand or hide decision flipped.
    last_near: Option<bool>,
    start: Instant,
    /// Single instance mutex. It **must be a field**: as a local of `boot`
    /// it would be released the moment `boot` returns and a second instance
    /// could start.
    _guard: Option<SingleInstance>,
    /// Tray icon, unregistered on drop; `None` when creation failed, the Dock
    /// still runs without it.
    tray: Option<nooksill_platform::tray::Tray>,
}

/// Outcome of one message pump round.
enum Pumped {
    /// A message that changes the interface was handled (mouse, keyboard,
    /// timer, repaint request), so a frame is needed.
    Frames,
    /// Only a worker thread wake-up arrived (`WM_APP_WAKE`).
    ///
    /// It only interrupts the wait, it does **not** force a frame: whether to
    /// draw is decided by the dirty flag of each step.
    Wake,
    /// The queue is empty.
    Idle,
    /// `WM_QUIT` arrived.
    Quit,
}

mod auto_hide;
mod boot;
mod clicks;
mod drag;
mod item_cmds;
mod item_edit;
mod menu_win;
mod misc;
mod panels_notes;
mod panels_state;
mod render;
mod resmon;
mod run_loop;
mod satellite;
mod settings_cmd;
mod sync;
mod sys;
mod update;
mod wndproc;

use self::menu_win::MenuSlot;
use self::update::UpdateMsg;

#[cfg(test)]
mod tests;
