use super::*;
use crate::config::Config;

fn cfg96() -> Config {
    Config::defaults()
}

fn work() -> Rect {
    Rect::new(0, 0, 1920, 1040)
}

fn kinds(n: usize) -> Vec<SlotKind> {
    vec![SlotKind::Icon; n]
}

mod hit;
mod metrics;
mod slots;
mod strip;
mod window;
