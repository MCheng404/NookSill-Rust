//! Process-wide panel policy.
//!
//! Panel constructors take no `Config`, so the main loop publishes the policy
//! here once per frame; a settings edit takes effect on the next open.

use nooksill_core::config::panel::{Backdrop, Placement, Prefs};
use nooksill_core::config::{BgMode, Config};
use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};

static GLOBAL: AtomicU8 = AtomicU8::new(0);
static OVERRIDES: AtomicI32 = AtomicI32::new(0);
static BACKDROP: AtomicU8 = AtomicU8::new(0);
static BLUR: AtomicU8 = AtomicU8::new(6);
static BAR: AtomicU8 = AtomicU8::new(2);
static MATERIAL: AtomicU8 = AtomicU8::new(3);

/// Publish the current config. Called by `App::render_frame`.
pub fn sync_prefs(cfg: &Config) {
    let p = Prefs::from_cfg(cfg);
    GLOBAL.store(p.global.code() as u8, Ordering::Relaxed);
    OVERRIDES.store(p.overrides, Ordering::Relaxed);
    BACKDROP.store(p.backdrop.code() as u8, Ordering::Relaxed);
    BLUR.store(p.blur as u8, Ordering::Relaxed);
    BAR.store(cfg.bg_mode.code() as u8, Ordering::Relaxed);
    MATERIAL.store(p.material(cfg.bg_mode).code() as u8, Ordering::Relaxed);
}

/// Snapshot of the published policy.
#[must_use]
pub fn current() -> Prefs {
    Prefs {
        global: Placement::from_code(i32::from(GLOBAL.load(Ordering::Relaxed))),
        overrides: OVERRIDES.load(Ordering::Relaxed),
        backdrop: Backdrop::from_code(i32::from(BACKDROP.load(Ordering::Relaxed))),
        blur: i32::from(BLUR.load(Ordering::Relaxed)),
    }
}

/// Bar material, needed to resolve `FollowBar` outside the config owner.
#[must_use]
pub fn bar() -> BgMode {
    BgMode::from_code(i32::from(BAR.load(Ordering::Relaxed)))
}

/// Resolved material of a widget panel.
#[must_use]
pub fn material() -> Backdrop {
    Backdrop::from_code(i32::from(MATERIAL.load(Ordering::Relaxed)))
}

/// Whether a panel needs the live desktop capture.
#[must_use]
pub fn frost_wanted() -> bool {
    material() == Backdrop::Frost
}

#[cfg(test)]
mod tests {
    use super::*;
    use nooksill_core::config::Key;

    /// One test owns the process-wide atomics; parallel writers would race.
    #[test]
    fn published_prefs_round_trip_and_follow_the_bar() {
        let mut cfg = Config::default();
        for (code, want) in [
            (0, Backdrop::Solid),
            (1, Backdrop::Gradient),
            (2, Backdrop::Frost),
        ] {
            cfg.apply_i32(Key::BgMode, code);
            sync_prefs(&cfg);
            assert_eq!(bar(), cfg.bg_mode);
            assert_eq!(material(), want);
            assert_eq!(frost_wanted(), want == Backdrop::Frost);
        }
        cfg.apply_i32(Key::PANEL_POS, 1);
        cfg.apply_i32(Key::Blur, 9);
        sync_prefs(&cfg);
        let p = current();
        assert_eq!(p.global, Placement::ScreenCenter);
        assert_eq!(p.backdrop, Backdrop::Frost, "the bar material decides");
        assert_eq!(p.blur, 9);
        assert_eq!(material(), Backdrop::Frost);
        assert!(frost_wanted());
    }
}
