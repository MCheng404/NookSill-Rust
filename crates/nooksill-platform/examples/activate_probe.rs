//! Activation probe: call `activate(exe, toggle=true)` on one exe three times
//! and print each decision and result.
//!
//! Needed because `activate` depends on who is in the foreground, which is not
//! controllable from automated clicks. Run it in its own process to see whether
//! "show, minimize" really alternates.
//!
//! Usage:
//! ```text
//! cargo run -q -p nooksill-platform --example activate_probe -- notepad.exe
//! ```

use std::time::Duration;

use nooksill_platform::app_window;

fn main() {
    let Some(name) = std::env::args().nth(1) else {
        eprintln!("用法: activate_probe <exe 路径或名字>");
        std::process::exit(2);
    };

    // A bare file name is accepted (notepad.exe); match it against all windows.
    let exe = if name.contains('\\') {
        name.clone()
    } else {
        let found = app_window::running_apps()
            .into_keys()
            .find(|k| k.ends_with(&format!("\\{}", name.to_lowercase())));
        match found {
            Some(p) => p,
            None => {
                eprintln!("没找到正在运行的 {name}");
                std::process::exit(1);
            }
        }
    };

    println!("目标: {exe}");
    for round in 1..=3 {
        println!(
            "第 {round} 次: 在前台={} 窗口={:?}",
            app_window::in_front_of(&exe),
            app_window::find_window_of(&exe).map(|h| h.0 as isize)
        );
        let r = app_window::activate(&exe, true);
        println!("           → {r:?}");
        std::thread::sleep(Duration::from_millis(600));
    }
}
