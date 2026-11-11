//! Application startup: window class, GPU/frame setup and the first layout.

use super::misc::{request_missing_icons, reserve_space, resolve_targets, work_rect};
use super::sys::{dump_geometry, fake_covered_from_env, fake_mouse_from_env};
use super::wndproc::wndproc;
use super::*;

impl App {
    /// Build the window and resources up to the point the message loop can run.
    ///
    /// `Ok(None)` means another instance is running; the caller exits normally.
    pub(super) fn boot() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        // DPI awareness must be set before window creation: otherwise Windows
        // stretches the window bitmap and the surface no longer matches the
        // client area.
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }

        // Single instance: two docks mean two layered windows and `FindWindow`
        // finds only one of them. A failing mutex never blocks startup.
        let guard = match SingleInstance::acquire() {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => {
                eprintln!("NookSill 已在运行，本次启动退出。");
                return Ok(None);
            }
            Err(e) => {
                eprintln!("单实例互斥体失败：{e}（继续启动）");
                None
            }
        };

        let paths = Paths::resolve()?;
        // Legacy migrations convert stored physical pixels, which every earlier
        // version scaled with the system DPI; the dock's own scale follows the
        // monitor it is placed on, resolved below.
        let dpi_sys = display::system_dpi();
        let mut ini = Ini::load(&paths.ini)?;
        // Upgrade the legacy ini in place. `from_ini` accepts both formats, so
        // a failed save does not affect this run.
        if Config::upgrade_legacy(&mut ini, dpi_sys) {
            match ini.save(&paths.ini) {
                Ok(()) => eprintln!(
                    "配置已从旧格式升级到 v{}（各设置保持不变）。",
                    nooksill_core::config::CONFIG_VERSION
                ),
                Err(e) => eprintln!("配置升级后保存失败（本次仍按新格式使用）：{e}"),
            }
        }
        // Length keys move from physical pixels to 96-DPI logical pixels
        // (v3 -> v4); without conversion icons jump 1.5x on a 150% display.
        if Config::migrate_units(&mut ini, dpi_sys) {
            match ini.save(&paths.ini) {
                Ok(()) => eprintln!("配置长度已换算成 96 DPI 逻辑像素（v4）。"),
                Err(e) => eprintln!("配置换算后保存失败（本次仍按新单位使用）：{e}"),
            }
        }
        let mut cfg = Config::from_ini(&ini);
        // A monitor index outlives the screen it named: unplugging the second
        // display leaves `Monitor=1` behind. Fall back to the primary instead of
        // silently drawing nothing.
        let count = display::monitors().len();
        if cfg.monitor < 0 || cfg.monitor as usize >= count.max(1) {
            eprintln!(
                "配置里的显示器序号 {} 不存在（当前 {count} 台），改用主显示器。",
                cfg.monitor
            );
            cfg.monitor = 0;
        }
        let dpi = display::monitor_dpi(cfg.monitor);
        nooksill_core::i18n::configure(&cfg.language, nooksill_platform::locale::ui_language_id());
        // Mutable: drag-reordering edits this list alongside the dock.
        let mut raw: Vec<Item> = store::load_items(&ini);
        // The notes switch appends a real item rather than a virtual one, so
        // `dock.items` and `raw` keep their index correspondence.
        if cfg.notes && !raw.iter().any(|i| i.kind == ItemKind::Note) {
            raw.push(Item::new(
                ItemKind::Note,
                nooksill_core::i18n::t("Notes"),
                "nooksill:notes",
            ));
        }
        let notes = nooksill_core::note::NoteBook::load(&ini);
        let items: Vec<DockItem> = raw.iter().map(DockItem::from_item).collect();

        // An auto-hidden taskbar on the same edge still shrinks the work area
        // by one taskbar thickness that shows nothing; anchor to the monitor
        // rect instead or the dock floats short of the screen edge.
        let taskbar = appbar::taskbar();
        let work: LRect = work_rect(&cfg, taskbar);
        let mut dock = Box::new(Dock::new(cfg, items, work));
        dock.mode = dock.cfg.mode;
        dock.update(dpi, nooksill_core::layout::REF_FRAME_MS);
        // Panels that are not anchored to a card centre themselves here.
        display::set_target_work_area(work);
        // Settings edits compare against this to re-derive the geometry.
        let monitor_for = (dock.cfg.monitor, dock.geom_epoch);

        let (win_w, win_h) = dock.physical_size_i32();
        dump_geometry(&dock, dpi);
        let ppp = dpi as f32 / 96.0;

        let hinstance = unsafe { GetModuleHandleW(None) }?;
        let wc = WNDCLASSW {
            hInstance: HINSTANCE(hinstance.0),
            lpfnWndProc: Some(wndproc),
            lpszClassName: w!("NookSillWindow"),
            // Must not be null: with a null class cursor and no `WM_SETCURSOR`
            // reply Windows falls back to the busy cursor.
            hCursor: nooksill_gpu::cursor::class_cursor(),
            // Taskbar and Alt+Tab use the icon from the exe resources.
            hIcon: nooksill_gpu::app_icon().unwrap_or_default(),
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&wc);
        }

        // Create without a reservation first: `ABM_NEW` needs the hwnd and the
        // final origin needs the rect AppBar hands back.
        let (mut x, mut y) = layout::window_origin(dock.cfg.edge, work, None, win_w, win_h);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("NookSillWindow"),
                w!("NookSill"),
                WS_POPUP,
                x,
                y,
                win_w,
                win_h,
                None,
                None,
                Some(hinstance.into()),
                None,
            )?
        };

        // Accept files dragged from Explorer; the empty-state text promises it.
        unsafe {
            DragAcceptFiles(hwnd, true);
        }

        // Reserve desktop space only in always-visible mode: a sliding dock
        // leaving a blank gap looks wrong.
        let mut appbar = AppBar::new(hwnd);
        let reserved = reserve_space(
            &mut appbar,
            &dock.cfg,
            dock.mode,
            dock.reserve_thickness(),
            taskbar,
        );
        (x, y) = layout::window_origin(dock.cfg.edge, work, reserved, win_w, win_h);
        if reserved.is_some() {
            // Move only on success; keep the old position rather than placing
            // the window off-screen.
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    x,
                    y,
                    win_w,
                    win_h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        if dock.cfg.reserve && dock.mode != DockMode::Always {
            // Deliberately ignored outside always-visible mode, but say so
            // instead of letting it look like a bug.
            eprintln!("`Reserve` 只在常显模式（Mode=0）下生效，当前已忽略。");
        }
        if trace::geom() {
            eprintln!(
                "[layout] edge={:?} reserve={} -> pos=({x},{y}) reserved={reserved:?}",
                dock.cfg.edge, dock.cfg.reserve
            );
        }

        let gpu = Gpu::new(&dock.cfg.adapter)?;
        nooksill_gpu::gpu::set_msaa_preference(dock.cfg.get_i32(Key::MSAA));
        let frame = FrameRenderer::new(&gpu, win_w as u32, win_h as u32, ppp)?;
        let start = Instant::now();

        // Resolve `.lnk` targets: both running detection and activation need
        // the real exe path. Cached in `Item.target`, never written to ini.
        // Stays on the UI thread because it runs before the window shows and
        // drives the accuracy of the first scan.
        resolve_targets(&mut raw);

        // Worker threads wake the UI thread with `PostMessage`, so the message
        // wait returns immediately instead of polling. Icon loading is the
        // largest blocking call at startup; moving it off lets the window show
        // first.
        let waker = Waker::new(hwnd);
        let icons = IconLoader::spawn(waker, Some(&gpu))?;
        let scanner = Scanner::spawn(hwnd.0 as isize, waker)?;
        let last_sysinfo = std::time::Instant::now();
        let resmon_slot: Option<Pin<Box<ResMonWindow>>> = None;
        let tool_slot: Option<Pin<Box<ToolWindow>>> = None;
        let calendar_slot: Option<Pin<Box<CalendarWindow>>> = None;
        let cc_slot: Option<Pin<Box<CcWindow>>> = None;
        let launchpad_slot: Option<Pin<Box<LaunchpadWindow>>> = None;
        let debug_opened = false;
        let last_launch_ms: Option<u64> = None;
        let tool_save_at: Option<Instant> = None;
        let widgets_dump = String::new();
        let disk = DiskWriter::spawn(paths.ini.clone(), waker)?;
        let opener = Opener::spawn()?;
        // Queue every icon up front; the worker batches them.
        request_missing_icons(&icons, &dock, &raw);

        // Global toggle: the combination comes from `[Hotkeys] Toggle`.
        let global_hotkey = nooksill_platform::hotkey::GlobalHotkey::register(
            hwnd,
            dock.cfg.hotkey_combo,
            dock.cfg.hotkey,
        );
        let hotkey_applied = (dock.cfg.hotkey_combo, dock.cfg.hotkey);

        let smoke = SmokeFx::with_gpu(&gpu);
        let mut app = Self {
            hwnd,
            gpu,
            frame,
            global_hotkey,
            hotkey_applied,
            dock,
            paths,
            raw,
            dpi,
            ppp,
            work,
            appbar,
            reserved,
            monitor_for,
            pos: (x, y),
            size: (win_w, win_h),
            menu_slot: None,
            edit_slot: None,
            group_slot: None,
            group_index: None,
            note_slot: None,
            notes,
            settings_slot: None,
            icons,
            scanner,
            last_sysinfo,
            resmon_slot,
            tool_slot,
            calendar_slot,
            cc_slot,
            launchpad_slot,
            last_layout: Instant::now(),
            timer_raised: false,
            update_rx: None,
            update_release: None,
            update_staged: None,
            pending_update: None,
            debug_opened,
            watcher: nooksill_gpu::popup::Watcher::default(),
            panel_was_open: false,
            last_launch_ms,
            tool_save_at,
            widgets_dump,
            fps_frames: 0,
            fps_at: Instant::now(),
            smoke,
            accent: theme::accent().unwrap_or(PALETTE[0]),
            system_dark: theme::is_dark(),
            look_epoch: 0,
            disk,
            opener,
            dirty_at: None,
            dirty_items: None,
            dirty_notes: None,
            dirty_cfg: None,
            covered: false,
            temps: Vec::new(),
            temp_keys: Vec::new(),
            items_stale: false,
            scan_sent: None,
            frost_excluded: false,
            frost_live: false,
            last_fs_check: Instant::now(),
            fs_hidden: false,
            fg_maximized: false,
            fake_mouse: fake_mouse_from_env(),
            fake_covered: fake_covered_from_env(),
            last_near: None,
            start,
            _guard: guard,
            tray: None,
        };

        // Smoke compute output is produced inside `run_ui`; FrameRenderer
        // registers it with egui after the pass.
        if let Some(layer) = app.smoke.layer() {
            app.frame.set_smoke_layer(layer);
        }

        // Dock pointer for wndproc; this is the heap address inside `Box`, so
        // moving `App` does not invalidate it.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&raw mut *app.dock) as isize);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = UpdateWindow(hwnd);
        }
        // Register the tray only after `GWLP_USERDATA` is set so the first
        // click lands. Explorer restarts can fail; that only loses the restore
        // entry, so log one line and carry on.
        match nooksill_platform::tray::Tray::add(hwnd, nooksill_gpu::app_icon().unwrap_or_default())
        {
            Ok(t) => app.tray = Some(t),
            Err(e) => eprintln!("托盘图标注册失败（Dock 仍可正常使用）：{e}"),
        }
        Ok(Some(app))
    }
}
