//! Showing the modal edit dialog.

use super::layout::{logical_size, place, EditSlot};
use super::paint::paint;
use super::win::{register_class, EditState};
use super::*;

/// Opens the modal edit dialog and blocks until the user accepts or cancels.
///
/// # Panics
///
/// Never panics: an uninitialized `slot` returns an error.
pub fn show_edit(
    gpu: &Gpu,
    slot: &mut EditSlot,
    req: EditRequest,
    cursor: (f32, f32),
    bounds: (f32, f32, f32, f32),
    dark: bool,
    ppp: f32,
) -> Result<EditOutcome, Box<dyn std::error::Error>> {
    let (lw, lh) = logical_size(&req.fields);
    let pw = (lw * ppp).ceil() as i32;
    let ph = (lh * ppp).ceil() as i32;
    let (lx, ly) = place(cursor, (lw, lh), bounds);
    let (px, py) = ((lx * ppp).round() as i32, (ly * ppp).round() as i32);

    // Created lazily: the first open builds the window and renderer, later
    // opens reuse them.
    if slot.is_none() {
        register_class()?;
        let hinst = unsafe { GetModuleHandleW(None)? };
        let title = windows::core::HSTRING::from(nooksill_core::i18n::t("NookSill Edit"));
        unsafe {
            let hwnd = CreateWindowExW(
                // No WS_EX_NOACTIVATE: the input fields need keyboard focus.
                // WS_EX_APPWINDOW makes the window activatable; the Dock is
                // always NOACTIVATE and never foreground, and without this
                // bit SetForegroundWindow is more often blocked.
                WS_EX_LAYERED | WS_EX_APPWINDOW | WS_EX_TOPMOST,
                w!("NookSillEditWindow"),
                windows::core::PCWSTR(title.as_ptr()),
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

    let Some((hwnd, renderer)) = slot.as_mut() else {
        return Err(nooksill_core::i18n::t("Edit window not initialised").into());
    };
    renderer.resize(gpu, pw as u32, ph as u32)?;

    unsafe {
        let _ = SetWindowPos(*hwnd, Some(HWND_TOPMOST), px, py, pw, ph, SWP_NOZORDER);
    }

    let mut state = Box::new(EditState {
        values: req.fields.iter().map(|f| f.value.clone()).collect(),
        multiline: req.fields.iter().any(|f| f.multiline),
        req,
        done: false,
        accepted: false,
        ppp,
        dark,
        frosted: renderer.frost_on(),
        input: Input::default(),
        ime: Ime::new(),
        focused: false,
        caret: (PAD + 10.0, PAD + TITLE_H + LABEL_H + FIELD_H / 2.0),
    });
    unsafe {
        SetWindowLongPtrW(*hwnd, GWLP_USERDATA, (&raw mut *state) as isize);
        let _ = ShowWindow(*hwnd, SW_SHOW);
        // The input fields need keyboard focus, so the window must be
        // foreground.
        if GetForegroundWindow() != *hwnd {
            let _ = SetForegroundWindow(*hwnd);
        }
        // `SetForegroundWindow` is not guaranteed to succeed (foreground
        // lock); `SetFocus` is the last way to get keyboard messages.
        // Without it `WM_CHAR` goes to another window and the dialog takes
        // clicks but no typing.
        let _ = SetFocus(Some(*hwnd));
        SetCapture(*hwnd);
    }
    crate::ime::Ime::enable(*hwnd);

    let mut msg = MSG::default();
    let mut painted = false;
    loop {
        // Only messages for this window: the Dock's input is suppressed
        // during the modal loop.
        let mut got = false;
        while unsafe { PeekMessageW(&raw mut msg, Some(*hwnd), 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            got = true;
        }
        if state.done {
            break;
        }
        // Submit a frame only on input or on the first frame. The modal loop
        // wakes every 60 ms; submitting unconditionally would keep a
        // constant ~16 fps GPU cost while the dialog sits idle.
        if got || !painted {
            painted = true;
            let raw = state.input.take(renderer.size(), ppp);
            let st = &mut *state;
            renderer.render(gpu, raw, *hwnd, (px, py), |ui| {
                paint(ui, st);
            })?;
            // Move the system candidate window to the text start of the
            // focused field. `paint` just wrote that position into `caret`;
            // without this the candidate window sits in the top-left corner
            // and the user cannot see where they are typing.
            state
                .ime
                .set_caret(*hwnd, state.caret.0, state.caret.1, ppp);
        }
        if !got {
            unsafe {
                MsgWaitForMultipleObjectsEx(None, 100, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            }
        }
    }

    let values = std::mem::take(&mut state.values);
    let accepted = state.accepted;
    unsafe {
        if GetCapture() == *hwnd {
            let _ = ReleaseCapture();
        }
        SetWindowLongPtrW(*hwnd, GWLP_USERDATA, 0);
        let _ = ShowWindow(*hwnd, SW_HIDE);
    }
    Ok(EditOutcome { accepted, values })
}
