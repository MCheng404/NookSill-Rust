//! Context menu window: class registration, message loop and painting.

use super::misc::raw_input;
use super::sys::physical_xy;
use super::*;

/// Menu window state, heap-allocated and published via `GWLP_USERDATA`.
pub(super) struct MenuWindow {
    pub(super) menu: Menu,
    pub(super) dark: bool,
    /// Modal loop exit flag.
    pub(super) done: bool,
    /// Chosen command; `None` together with `done` means cancelled.
    pub(super) cmd: Option<MenuCmd>,
    /// DPI scale: physical size derives from it, hit tests convert back.
    pub(super) ppp: f32,
    /// A new live background frame arrived; repaint once next round.
    pub(super) wake: bool,
}

pub(super) fn menu_ptr(hwnd: HWND) -> *mut MenuWindow {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut MenuWindow }
}

/// Client physical coordinates to logical points (`Menu::hit` takes logical).
pub(super) fn menu_logical(lparam: LPARAM, ppp: f32) -> (f32, f32) {
    let (x, y) = physical_xy(lparam);
    (x / ppp, y / ppp)
}

pub(super) unsafe extern "system" fn menu_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if trace::menu() {
            eprintln!("[menu-proc] msg=0x{msg:04X} wparam=0x{:X}", wparam.0);
        }
        match msg {
            // Never take focus, like the dock: focusing would defocus the
            // user's window and need an extra click after closing.
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // Live background frame: request a repaint next round.
            WM_APP_WAKE => {
                let ptr = menu_ptr(hwnd);
                if !ptr.is_null() {
                    (*ptr).wake = true;
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let ptr = menu_ptr(hwnd);
                if !ptr.is_null() {
                    let (u, v) = menu_logical(lparam, (*ptr).ppp);
                    let hit = (*ptr).menu.hit(u, v);
                    (*ptr).menu.set_hover(hit);
                    // The menu captures the mouse on open and no `WM_SETCURSOR`
                    // arrives while captured, so the cursor is set here.
                    nooksill_gpu::cursor::set_hand_if(hit.is_some());
                }
                LRESULT(0)
            }
            // The menu answers its own cursor: a null class cursor shows the
            // busy cursor. Hand over entries, arrow over separators and padding.
            WM_SETCURSOR => {
                let mut on_item = false;
                let ptr = menu_ptr(hwnd);
                if !ptr.is_null() {
                    if let Some((u, v)) = nooksill_gpu::cursor::client_pos(hwnd) {
                        let ppp = (*ptr).ppp;
                        on_item = (*ptr).menu.hit(u / ppp, v / ppp).is_some();
                    }
                }
                nooksill_gpu::cursor::set_hand_if(on_item);
                LRESULT(1)
            }
            // A press outside the menu rect cancels. This relies on capture:
            // the menu has no focus, so clicks on other windows are never
            // forwarded here.
            WM_LBUTTONDOWN => {
                let ptr = menu_ptr(hwnd);
                if !ptr.is_null() {
                    let (u, v) = menu_logical(lparam, (*ptr).ppp);
                    let (w, h) = (*ptr).menu.logical_size();
                    if u < 0.0 || v < 0.0 || u > w || v > h {
                        (*ptr).done = true;
                    }
                }
                LRESULT(0)
            }
            // Release picks. `hit` may be `None` (separator or padding) and the
            // menu must still close: that press selected nothing.
            WM_LBUTTONUP => {
                let ptr = menu_ptr(hwnd);
                if !ptr.is_null() {
                    let (u, v) = menu_logical(lparam, (*ptr).ppp);
                    (*ptr).cmd = (*ptr).menu.hit(u, v).and_then(|i| (*ptr).menu.cmd_at(i));
                    (*ptr).done = true;
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                // Esc cancels. Other keys are ignored: no keyboard navigation.
                if wparam.0 as u32 == 0x1B {
                    let ptr = menu_ptr(hwnd);
                    if !ptr.is_null() {
                        (*ptr).done = true;
                    }
                }
                LRESULT(0)
            }
            // Capture stolen (dialog, Alt+Tab): clicks can no longer arrive, so
            // close instead of waiting for a cancel that never comes.
            WM_CAPTURECHANGED => {
                let ptr = menu_ptr(hwnd);
                if !ptr.is_null() {
                    (*ptr).done = true;
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Register the menu window class; a duplicate returns 0 and is ignored.
pub(super) fn register_menu_class() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(menu_proc),
            hInstance: HINSTANCE(hinst.0),
            lpszClassName: w!("NookSillMenuWindow"),
            // Same rule: a null class cursor shows the busy cursor.
            hCursor: nooksill_gpu::cursor::class_cursor(),
            hIcon: nooksill_gpu::app_icon().unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&raw const wc);
    }
    Ok(())
}

/// Reusable menu slot: window handle plus its own renderer.
///
/// The dock's renderer is sized to the bar while the menu is a different size;
/// `resize` rebuilds only the offscreen target and DIB, keeping the egui
/// context and wgpu pipelines.
pub(super) type MenuSlot = Option<(HWND, FrameRenderer)>;

/// Return the frost config for one panel, if enabled.
/// `frost::blur_rgba` clamps the radius again (1..12).
pub(super) fn frost_spec(cfg: &Config, key: Key, corner_pt: f32) -> Option<FrostSpec> {
    if cfg.get_i32(key) == 0 {
        return None;
    }
    // Widget popups may override the global blur radius; 0 inherits `Blur`.
    // Every `frost_spec` caller (menu, launchpad, settings, widget panels)
    // picks this up without changes.
    let blur = cfg.get_i32(Key::WIDGET_BLUR);
    let radius = if blur > 0 {
        blur
    } else {
        cfg.get_i32(Key::Blur)
    }
    .max(1) as u32;
    Some(FrostSpec { radius, corner_pt })
}

#[expect(
    clippy::too_many_arguments,
    reason = "argument group is the API boundary"
)]
pub(super) fn show_menu(
    gpu: &Gpu,
    slot: &mut MenuSlot,
    menu: Menu,
    cursor: (f32, f32),
    bounds: (f32, f32, f32, f32),
    dark: bool,
    ppp: f32,
    start: Instant,
    frost: Option<FrostSpec>,
) -> Result<Option<MenuCmd>, Box<dyn std::error::Error>> {
    let (lw, lh) = menu.logical_size();
    // Round up: one missing pixel clips the rounded corner of the last row.
    let pw = (lw * ppp).ceil() as i32;
    let ph = (lh * ppp).ceil() as i32;
    let (lx, ly) = Menu::place(cursor, (lw, lh), bounds);
    let (px, py) = ((lx * ppp).round() as i32, (ly * ppp).round() as i32);

    // Lazy: build the window and renderer on first use, reuse afterwards. The
    // first open is slow (CJK fonts, wgpu pipelines); later opens just resize.
    if slot.is_none() {
        register_menu_class()?;
        let hinst = unsafe { GetModuleHandleW(None)? };
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("NookSillMenuWindow"),
                w!("NookSillMenu"),
                WS_POPUP,
                px,
                py,
                pw,
                ph,
                None,
                None,
                Some(HINSTANCE(hinst.0)),
                None,
            )?;
            let renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;
            *slot = Some((hwnd, renderer));
        }
    }

    // `Some` was just assigned above. `ok_or` rather than `expect`: a broken
    // invariant should error out, not abort the process.
    let Some((hwnd, renderer)) = slot.as_mut() else {
        return Err(nooksill_core::i18n::t("Menu window not initialised").into());
    };
    renderer.resize(gpu, pw as u32, ph as u32)?;

    // Move and resize together: the height differs on every open.
    unsafe {
        let _ = SetWindowPos(*hwnd, Some(HWND_TOPMOST), px, py, pw, ph, SWP_NOACTIVATE);
    }

    if trace::geom() {
        eprintln!(
            "[menu] items={} labels={:?} logical=({lw},{lh}) physical=({pw},{ph}) at=({px},{py})",
            menu.entries.len(),
            menu.entries
                .iter()
                .map(|e| e.label.as_str())
                .collect::<Vec<_>>()
        );
    }
    // Position and size are set before the window shows, so a desktop capture
    // here cannot include the menu itself.
    if let Some(spec) = frost {
        spec.apply(renderer, *hwnd, ppp);
        // The grab thread keeps updating after show; exclude self or it
        // captures the menu.
        nooksill_gpu::frost::exclude_from_capture(*hwnd);
        renderer.start_live_frost(*hwnd, spec, ppp);
    } else {
        renderer.clear_frost();
        renderer.stop_live_frost();
    }
    let frost_on = renderer.frost_on();

    let mut state = Box::new(MenuWindow {
        menu,
        dark,
        done: false,
        cmd: None,
        ppp,
        wake: false,
    });
    unsafe {
        SetWindowLongPtrW(*hwnd, GWLP_USERDATA, (&raw mut *state) as isize);
        let _ = ShowWindow(*hwnd, SW_SHOWNA);
        SetCapture(*hwnd);
    }

    // The first frame is mandatory: the window was just re-shown and its
    // previous content is stale.
    let mut painted: Option<Option<usize>> = None;
    let mut msg = MSG::default();
    loop {
        // Only this window's messages; dock input stays blocked while modal.
        while unsafe { PeekMessageW(&raw mut msg, Some(*hwnd), 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if state.done {
            break;
        }
        // Repaint only when the highlight changes; a live frost frame also
        // triggers one (`wake` is set by the window proc).
        if painted != Some(state.menu.hover) || state.wake {
            painted = Some(state.menu.hover);
            state.wake = false;
            let menu = &state.menu;
            let dark = state.dark;
            renderer.render(
                gpu,
                raw_input(renderer.size(), ppp, start),
                *hwnd,
                (px, py),
                |ui| menu.paint(ui, dark, frost_on),
            )?;
        }
        unsafe {
            MsgWaitForMultipleObjectsEx(None, 200, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }

    if trace::geom() {
        eprintln!("[menu] 关闭 cmd={:?}", state.cmd);
    }
    renderer.stop_live_frost();
    nooksill_gpu::frost::allow_capture(*hwnd);
    unsafe {
        if GetCapture() == *hwnd {
            let _ = ReleaseCapture();
        }
        SetWindowLongPtrW(*hwnd, GWLP_USERDATA, 0);
        let _ = ShowWindow(*hwnd, SW_HIDE);
    }
    Ok(state.cmd)
}
