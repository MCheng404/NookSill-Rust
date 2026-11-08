//! Window creation, visibility and the periodic save timer.

use super::win::{class_pcwstr, wndproc};
use super::*;

impl LaunchpadWindow {
    /// Creates the window (hidden by default). `dat` is the full path of
    /// `launchpad.dat`.
    pub fn new(gpu: &Gpu, ppp: f32, dark: bool, dat: &Path) -> Result<Pin<Box<Self>>, GpuError> {
        let hinst =
            unsafe { GetModuleHandleW(None) }.map_err(|_| GpuError::Win32("GetModuleHandleW"))?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: windows::Win32::Foundation::HINSTANCE(hinst.0),
            lpszClassName: class_pcwstr(),
            hCursor: crate::cursor::class_cursor(),
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&raw const wc);
        }

        // Cover the whole primary monitor.
        let mon = nooksill_platform::display::monitor_rect(0);
        let (pw, ph) = (mon.right - mon.left, mon.bottom - mon.top);
        let size = (pw as f32 / ppp, ph as f32 / ppp);

        let hwnd = unsafe {
            // `WS_EX_LAYERED` is mandatory: presentation goes through
            // `UpdateLayeredWindow` (see `present::LayeredPresenter`), which
            // fails without it.
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                class_pcwstr(),
                windows::core::w!("起动台"),
                WS_POPUP | WS_VISIBLE,
                mon.left,
                mon.top,
                pw,
                ph,
                None,
                None,
                Some(windows::Win32::Foundation::HINSTANCE(hinst.0)),
                None,
            )
        }
        .map_err(|_| GpuError::Win32("CreateWindowExW（起动台）"))?;

        // Hide it right away: `WS_VISIBLE` is only there so the window
        // procedure receives messages; `open` shows it again.
        unsafe {
            let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
            // Allow files to be dropped in (WM_DROPFILES).
            DragAcceptFiles(hwnd, true);
        }

        let renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;

        // Read the file, then scan the system.
        let mut lp = match std::fs::read_to_string(dat) {
            Ok(text) => Launchpad::parse(&text),
            Err(_) => Launchpad::new(),
        };
        lp.absorb(&nooksill_platform::launchpad::scan());

        let mut win = Box::pin(Self {
            hwnd,
            renderer,
            input: crate::input::Input::default(),
            pos: (mon.left, mon.top),
            ppp,
            size,
            lp,
            grid: (7, 4),
            dat: dat.to_path_buf(),
            page: 0,
            query: String::new(),
            open_folder: None,
            hover: None,
            hits: Vec::new(),
            icon_rects: Vec::new(),
            down: (0.0, 0.0),
            cur: (0.0, 0.0),
            press: None,
            dragging: false,
            drag_src: None,
            drag_page: 0,
            icons: HashMap::new(),
            asked: HashMap::new(),
            loader: IconLoader::spawn(Waker::new(hwnd), Some(gpu)).ok(),
            ime: Ime::new(),
            menu: None,
            menu_at: (0.0, 0.0),
            sel: None,
            cat: 0,
            renaming: None,
            rename_buf: String::new(),
            cmds: Vec::new(),
            visible: false,
            dark,
            dirty_save: false,
            next_repaint: Instant::now(),
        });
        win.attach();
        // The window needs an IME context, otherwise pinyin input never
        // arrives.
        Ime::enable(hwnd);
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
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Opens the launchpad, takes the focus and the keyboard.
    ///
    /// With `frost` the backdrop is captured **before** `ShowWindow`, while
    /// the window is still off screen (GDI would otherwise capture the panel
    /// itself), and the window is excluded from screen capture so the live
    /// capture thread reads the desktop behind it. A failed capture falls
    /// back to a solid background.
    pub fn open(&mut self, frost: Option<crate::frost::Spec>) {
        match frost {
            Some(spec) => {
                spec.apply(&mut self.renderer, self.hwnd, self.ppp);
                // Must happen before `ShowWindow`: one frame later would
                // capture the panel's own mirror.
                crate::frost::exclude_from_capture(self.hwnd);
                self.renderer.start_live_frost(self.hwnd, spec, self.ppp);
            }
            None => {
                self.renderer.clear_frost();
                self.renderer.stop_live_frost();
            }
        }
        unsafe {
            // A fullscreen overlay has to sit above other windows, including
            // other TOPMOST ones: otherwise the foreground window covers it
            // and the launchpad looks like it never opened.
            let _ = SetWindowPos(
                self.hwnd,
                Some(windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST),
                0,
                0,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
        }
        self.visible = true;
        self.query.clear();
        self.page = 0;
        self.open_folder = None;
        self.hover = None;
        self.sel = None;
        self.cat = 0;
        self.menu = None;
        self.renaming = None;
        self.rename_buf.clear();
        self.next_repaint = Instant::now();
    }

    /// Closes the launchpad.
    pub fn close(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        // Stop the capture before hiding: the other order grabs one more
        // frame between hiding and the thread exiting (harmless, but
        // pointless), and `allow_capture` has to run while the window is
        // still visible.
        self.renderer.stop_live_frost();
        crate::frost::allow_capture(self.hwnd);
        unsafe {
            let _ = ShowWindow(self.hwnd, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
        }
        if self.dirty_save {
            self.save();
        }
    }

    /// Takes one command for the main loop to handle.
    pub fn take_cmd(&mut self) -> Option<LaunchpadCmd> {
        if self.cmds.is_empty() {
            None
        } else {
            Some(self.cmds.remove(0))
        }
    }

    /// Writes the layout to disk.
    pub fn save(&mut self) {
        self.dirty_save = false;
        let text = self.lp.serialize();
        if let Err(e) = std::fs::write(&self.dat, text) {
            eprintln!("[launchpad] 写 {} 失败: {e}", self.dat.display());
        }
    }

    #[must_use]
    pub fn due(&self) -> bool {
        self.visible && self.next_repaint <= Instant::now()
    }
}
