//! OLE 拖放目标：让 Dock 在**拖放过程中**就显示插入位。
//!
//! `DragAcceptFiles` 只在松手时给一条 `WM_DROPFILES`，拿不到 `DragOver`，
//! 所以旧版做不到「拖到两个图标之间，左右图标让出位置」。这里注册
//! `IDropTarget`：
//! - `DragOver` 把光标位置写进 [`Dock::set_external_drop`]，由布局层
//!   预留一个空槽，图标左右让位并画插入标记；
//! - `Drop` 从 `IDataObject` 的 `CF_HDROP` 取路径，复用
//!   [`Dock::push_dropped_paths`]（与旧 `WM_DROPFILES` 分支同一条落地路径）。
//!
//! 回调发生在注册窗口所在的 STA 线程（UI 线程），因此可以直接解引用
//! `GWLP_USERDATA` 上的 `Box<Dock>`，不需要跨线程同步。`WM_DROPFILES`
//! 保留作为 OLE 注册失败时的兜底。

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

/// 注册在 Dock 窗口上的拖放目标。
///
/// `#[implement]` 生成 COM vtable；`DockDropTarget_Impl` 是宏产生的
/// 实现载体，字段通过 Deref 访问。
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

    /// UI 线程上的 `Box<Dock>`；窗口销毁前会被 wndproc 清空。
    fn dock(&self) -> *mut Dock {
        // SAFETY: 与 wndproc 的 `dock_ptr` 同一套约定，先判空再解引用。
        unsafe { GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *mut Dock }
    }

    /// `POINTL` 是屏幕坐标，转成窗口客户区**物理像素**。
    fn client(&self, pt: &POINTL) -> (f32, f32) {
        let mut p = POINT { x: pt.x, y: pt.y };
        // SAFETY: p 是本栈帧有效、可写的 POINT。
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut p);
        }
        (p.x as f32, p.y as f32)
    }

    fn over(&self, pt: &POINTL) {
        let (x, y) = self.client(pt);
        let p = self.dock();
        if !p.is_null() {
            // SAFETY: 回调在 UI 线程，Dock 存活且未被同时可变借用
            //（`DragOver` 期间主循环不会被重入）。
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

/// 只接受带文件列表（`CF_HDROP`）的拖放：桌面、资源管理器、任务管理器
/// 拖出的应用/快捷方式都走这个格式；纯文本 / 网址 OLE 拖放不收。
fn has_files(data: &IDataObject) -> bool {
    // SAFETY: fmt 是栈上完整的 FORMATETC，ptd 为 null 表示默认目标设备。
    unsafe { data.QueryGetData(&hdrop_format()) }.is_ok()
}

/// 从 `IDataObject` 取出被拖的文件路径。`ReleaseStgMedium` 无条件调用。
fn paths_from(data: &IDataObject) -> Vec<String> {
    let mut medium = match unsafe { data.GetData(&hdrop_format()) } {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    // SAFETY: GetData 成功后 medium 持有合法的 HGLOBAL；无论解析结果如何，
    // 函数结束前都会 ReleaseStgMedium，不会泄漏 COM 内存。
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

/// COM 回调的 `pdweffect: *mut DROPEFFECT` 由 `IDropTarget_Impl` 签名固定，
/// 无法改成安全引用；写入前已由 OLE 保证指针有效。
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
        // SAFETY: OLE 保证 pdweffect 可写。
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
            // 先收掉让位，再落地路径 —— 否则插入标记会残留到下一帧。
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
