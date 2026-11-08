//! OLE drop target, so the insertion slot shows while a drag is in progress.
//! `WM_DROPFILES` only fires on release and gives no `DragOver`. Callbacks run
//! on the UI thread, so the `Box<Dock>` is reachable without synchronization.

use windows::core::{implement, Ref, Result as WinResult};
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::{POINT, POINTL};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, ReleaseStgMedium, CF_HDROP, DROPEFFECT, DROPEFFECT_COPY,
    DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWLP_USERDATA};

use crate::dock::Dock;

/// Drop target registered on the dock window; `#[implement]` generates the
/// COM vtable and the fields are reached through Deref.
#[derive(Debug)]
#[implement(IDropTarget)]
pub struct DockDropTarget {
    hwnd: HWND,
}

impl DockDropTarget {
    #[must_use]
    pub const fn new(hwnd: HWND) -> Self {
        Self { hwnd }
    }

    /// The UI thread's `Box<Dock>`; the wndproc clears it before the window dies.
    fn dock(&self) -> *mut Dock {
        // SAFETY: Same contract as the wndproc's `dock_ptr`; callers null-check
        // the result before dereferencing.
        unsafe { GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *mut Dock }
    }

    /// `POINTL` is in screen coordinates; return client-area physical pixels.
    fn client(&self, pt: &POINTL) -> (f32, f32) {
        let mut p = POINT { x: pt.x, y: pt.y };
        // SAFETY: p is a valid, writable POINT on this stack frame.
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut p);
        }
        (p.x as f32, p.y as f32)
    }

    fn over(&self, pt: &POINTL) {
        let (x, y) = self.client(pt);
        let p = self.dock();
        if !p.is_null() {
            // SAFETY: Runs on the UI thread with the Dock alive and not mutably
            // borrowed: the main loop is not re-entered during `DragOver`.
            unsafe { (*p).set_external_drop(x, y) }
        }
    }
}

fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// Accept file lists only (`CF_HDROP`): desktop, Explorer and Task Manager
/// drags all use it; plain text or URL drags are rejected.
fn has_files(data: &IDataObject) -> bool {
    // SAFETY: fmt is a complete stack FORMATETC; a null ptd means the default
    // target device.
    unsafe { data.QueryGetData(&hdrop_format()) }.is_ok()
}

/// Extract the dropped file paths. `ReleaseStgMedium` runs unconditionally.
fn paths_from(data: &IDataObject) -> Vec<String> {
    let mut medium = match unsafe { data.GetData(&hdrop_format()) } {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    // SAFETY: After a successful GetData the medium holds a valid HGLOBAL, and
    // ReleaseStgMedium runs before return, so no COM memory leaks.
    unsafe {
        if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            let hdrop = HDROP(medium.u.hGlobal.0);
            let n = DragQueryFileW(hdrop, u32::MAX, None);
            for i in 0..n {
                let len = DragQueryFileW(hdrop, i, None);
                let mut buf = vec![0_u16; len as usize + 1];
                let got = DragQueryFileW(hdrop, i, Some(buf.as_mut_slice()));
                if got > 0 {
                    buf.truncate(got as usize);
                    out.push(String::from_utf16_lossy(&buf));
                }
            }
        }
        ReleaseStgMedium(&mut medium);
    }
    out
}

/// `pdweffect: *mut DROPEFFECT` is fixed by the `IDropTarget_Impl` signature and
/// cannot become a safe reference; OLE guarantees it is writable.
#[expect(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "IDropTarget_Impl signatures are fixed by the COM ABI"
)]
impl IDropTarget_Impl for DockDropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> WinResult<()> {
        let ok = pdataobj.as_ref().is_some_and(has_files);
        // SAFETY: OLE guarantees pdweffect is writable.
        unsafe { *effect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        if ok {
            self.over(pt);
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> WinResult<()> {
        unsafe { *effect = DROPEFFECT_COPY };
        self.over(pt);
        Ok(())
    }

    fn DragLeave(&self) -> WinResult<()> {
        let p = self.dock();
        if !p.is_null() {
            unsafe { (*p).clear_external_drop() }
        }
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> WinResult<()> {
        let (x, y) = self.client(pt);
        let files = pdataobj.as_ref().map(paths_from).unwrap_or_default();
        let p = self.dock();
        if !p.is_null() {
            // Clear the reserved slot before consuming the paths, otherwise the
            // insertion marker survives into the next frame.
            unsafe {
                (*p).clear_external_drop();
                if !files.is_empty() {
                    (*p).push_dropped_paths(files.clone(), x, y);
                }
            }
        }
        unsafe {
            *effect = if files.is_empty() {
                DROPEFFECT_NONE
            } else {
                DROPEFFECT_COPY
            }
        };
        Ok(())
    }
}
