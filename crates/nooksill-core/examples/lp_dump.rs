//! Launchpad diagnostic probe: prints the tag groups and each app's group.
//!
//! Usage:
//!
//! ```bash
//! cargo run -q -p nooksill-core --example lp_dump -- <launchpad.dat>
//! ```
//!
//! Run this first when the grouping looks wrong: it shows what the tags
//! parsed to and which group each app landed in, so no counting through the
//! UI is needed.

use nooksill_core::launchpad::Launchpad;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("用法：lp_dump <launchpad.dat>");
    let text = std::fs::read_to_string(&path).expect("读文件失败");
    let lp = Launchpad::parse(&text);
    println!("标签组：{:?}", lp.tags());
    println!("应用：{} 个", lp.apps().len());
    let mut counts = vec![0usize; lp.tags().len() + 1];
    for i in 0..lp.apps().len() {
        counts[lp.category_of(i)] += 1;
    }
    for (i, c) in counts.iter().enumerate().skip(1) {
        println!("  第 {i} 组 {:?}: {c} 个", lp.tags().get(i - 1));
    }
}
