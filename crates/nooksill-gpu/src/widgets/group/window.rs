//! Group panel lifecycle: creation, opening and hide timing.

use super::win::{class_pcwstr, wndproc};
use super::*;

impl GroupWindow {
    /// Creates the window (hidden).
    pub fn new(gpu: &Gpu, ppp: f32, dark: bool) -> Result<Pin<Box<Self>>, GpuError> {
        let hinst =
            unsafe { GetModuleHandleW(None) }.map_err(|_| GpuError::Win32("GetModuleHandleW"))?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: windows::Win32::Foundation::HINSTANCE(hinst.0),
            lpszClassName: class_pcwstr(),
            hCursor: crate::cursor::class_cursor(),
            hIcon: crate::app_icon().unwrap_or_default(),
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&raw const wc);
        }

        let pw = (400.0 * ppp).round() as i32;
        let ph = (300.0 * ppp).round() as i32;
        let (x, y) = nooksill_platform::display::centered_on_target(pw, ph);
        let title = windows::core::HSTRING::from(nooksill_core::i18n::t("NookSill Group"));
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_APPWINDOW | WS_EX_TOPMOST,
                class_pcwstr(),
                windows::core::PCWSTR(title.as_ptr()),
                WS_POPUP,
                x,
                y,
                pw,
                ph,
                None,
                None,
                Some(windows::Win32::Foundation::HINSTANCE(hinst.0)),
                None,
            )
        }
        .map_err(|_| GpuError::Win32("CreateWindowExW (group panel)"))?;

        let renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;
        let mut win = Box::pin(Self {
            hwnd,
            renderer,
            input: crate::input::Input::default(),
            pos: (x, y),
            ppp,
            dark,
            visible: false,
            title: String::new(),
            kids: Vec::new(),
            fan: FanSpec::default(),
            icons: HashMap::new(),
            asked: HashSet::new(),
            loader: None,
            hover: None,
            press: None,
            released: None,
            action: GroupAction::None,
            next_repaint: None,
        });
        win.attach();
        // Own icon thread: textures cannot be reused across egui contexts.
        win.loader = IconLoader::spawn(Waker::new(hwnd), Some(gpu)).ok();
        Ok(win)
    }

    pub(super) fn attach(&mut self) {
        unsafe {
            SetWindowLongPtrW(
                self.hwnd,
                GWLP_USERDATA,
                std::ptr::from_mut::<Self>(self) as isize,
            );
        }
    }

    #[must_use]
    pub const fn hwnd(&self) -> HWND {
        self.hwnd
    }

    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Opens the panel. `anchor` is the screen rect (physical px) of the
    /// group on the Dock; the panel pops out above it.
    pub fn open(
        &mut self,
        gpu: &Gpu,
        title: &str,
        kids: Vec<GroupKid>,
        anchor: (i32, i32, i32, i32),
        fan: FanSpec,
    ) -> Result<(), GpuError> {
        self.title = title.to_owned();
        self.kids = kids;
        // A tall column may not fit the screen; the fitting step may fall back
        // to the grid, so the stored spec is what paint and hit tests use.
        let (al, at, ar, _ab) = anchor;
        let cx = (al + ar) / 2;
        let work = nooksill_platform::display::work_area_at(cx, at);
        let avail = (
            work.width() as f32 / self.ppp - 16.0,
            work.height() as f32 / self.ppp - 16.0,
        );
        self.fan = fitting_spec(self.kids.len(), fan, avail);
        // A different group invalidates the icon cache: the indices no
        // longer point at the same things.
        self.icons.clear();
        self.asked.clear();
        self.hover = None;
        self.press = None;
        self.released = None;
        self.action = GroupAction::None;

        let (lw, lh) = logical_size(self.kids.len(), &self.fan);
        let pw = (lw * self.ppp).round() as i32;
        let ph = (lh * self.ppp).round() as i32;
        self.renderer.resize(gpu, pw as u32, ph as u32)?;

        // Above the group, horizontally centered; moved down when it would
        // hit the top of the screen.
        let mut x = cx - pw / 2;
        if x < work.left {
            x = work.left;
        }
        if x + pw > work.right {
            x = work.right - pw;
        }
        let mut y = at - ph - 8;
        if y < work.top {
            y = work.top;
        }
        self.pos = (x, y);
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                pw,
                ph,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
        self.visible = true;
        self.next_repaint = Some(Instant::now());
        Ok(())
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.hover = None;
        self.press = None;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Takes the pending action.
    pub fn take_action(&mut self) -> GroupAction {
        std::mem::replace(&mut self.action, GroupAction::None)
    }

    /// Whether a repaint is due.
    #[must_use]
    pub fn due(&self) -> bool {
        self.visible && self.next_repaint.is_some_and(|t| t <= Instant::now())
    }
}
