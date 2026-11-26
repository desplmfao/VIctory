//! A faster clock for the title's start (testing aid).
//!
//! `VICTORY_CLOCK_SCALE=<n>` makes the title's clock run `n` times faster
//! for the first `VICTORY_CLOCK_SCALE_SECS` real seconds (180 by default),
//! then at the normal rate from where it got to: the intro cutscenes, which
//! run at one or two frames a second here, pass in seconds. The first real
//! key or button the player presses (not `VICTORY_KEYS`) ends the fast
//! stretch at once: leave the controls alone until you want to play.
//! Without the variable nothing is overridden and the title reads the
//! host's clock.
//!
//! Covers what Grand Theft Auto VI reads its time from:
//! `QueryPerformanceCounter` (`api-ms-win-core-profile-l1-1-0`) and
//! `GetTickCount64` (`api-ms-win-core-sysinfo-l1-1-0`). Wall-clock times
//! (`GetSystemTime*`) stay the host's.

#![allow(non_snake_case)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::{HleModule, hle_export};

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "QueryPerformanceCounter"]
    fn host_counter(count: *mut i64) -> i32;
    #[link_name = "QueryPerformanceFrequency"]
    fn host_frequency(frequency: *mut i64) -> i32;
}

/// The scale, and the host counter's start and end of the fast stretch.
struct Fast {
    scale: i64,
    start: i64,
    until: i64,
    frequency: i64,
}

fn settings(scale: Option<&str>, secs: Option<&str>) -> Option<(i64, i64)> {
    let scale = scale?.trim().parse::<i64>().ok().filter(|&s| s > 1)?;
    let secs = secs.and_then(|s| s.trim().parse::<i64>().ok()).filter(|&s| s > 0).unwrap_or(180);
    Some((scale, secs))
}

fn fast() -> Option<&'static Fast> {
    static FAST: OnceLock<Option<Fast>> = OnceLock::new();
    FAST.get_or_init(|| {
        let (scale, secs) = settings(
            std::env::var("VICTORY_CLOCK_SCALE").ok().as_deref(),
            std::env::var("VICTORY_CLOCK_SCALE_SECS").ok().as_deref(),
        )?;
        let (mut start, mut frequency) = (0, 0);
        // SAFETY: outputs of ours.
        unsafe {
            host_counter(&raw mut start);
            host_frequency(&raw mut frequency);
        }
        tracing::warn!(scale, secs, "VICTORY_CLOCK_SCALE: the title's clock runs fast at first");
        Some(Fast {
            scale,
            start,
            until: start.saturating_add(secs.saturating_mul(frequency)),
            frequency,
        })
    })
    .as_ref()
}

/// The host counter when the player first pressed something (`i64::MAX`
/// until then).
static TOUCHED: AtomicI64 = AtomicI64::new(i64::MAX);

/// The player pressed a key or button (gameinput's poll): the fast stretch
/// ends now.
pub(crate) fn player_input() {
    if TOUCHED.load(Ordering::Relaxed) == i64::MAX && fast().is_some() {
        let mut t = 0;
        // SAFETY: an output of ours.
        unsafe { host_counter(&raw mut t) };
        if TOUCHED.compare_exchange(i64::MAX, t, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            tracing::warn!("VICTORY_CLOCK_SCALE: player input, the title's clock runs at the normal rate from now");
        }
    }
}

/// Host counter value `t` as the title sees it.
fn scaled(f: &Fast, t: i64) -> i64 {
    scaled_until(f, t, f.until.min(TOUCHED.load(Ordering::Relaxed)))
}

fn scaled_until(f: &Fast, t: i64, until: i64) -> i64 {
    let fast_part = (t.min(until) - f.start).max(0);
    f.start + fast_part.saturating_mul(f.scale) + (t - until).max(0)
}

pub extern "system" fn QueryPerformanceCounter(count: *mut i64) -> i32 {
    let mut t = 0;
    // SAFETY: an output of ours.
    let ok = unsafe { host_counter(&raw mut t) };
    if let Some(f) = fast() {
        t = scaled(f, t);
    }
    if !count.is_null() {
        // SAFETY: the caller's output.
        unsafe { *count = t };
    }
    ok
}

pub extern "system" fn GetTickCount64() -> u64 {
    let mut t = 0;
    // SAFETY: an output of ours.
    unsafe { host_counter(&raw mut t) };
    match fast() {
        Some(f) => (scaled(f, t) as u128 * 1000 / f.frequency.max(1) as u128) as u64,
        None => {
            let mut frequency = 1;
            // SAFETY: an output of ours.
            unsafe { host_frequency(&raw mut frequency) };
            (t as u128 * 1000 / frequency.max(1) as u128) as u64
        }
    }
}

pub(crate) fn module() -> HleModule {
    let on = settings(std::env::var("VICTORY_CLOCK_SCALE").ok().as_deref(), None).is_some();
    HleModule {
        name: "clock",
        dlls: &["api-ms-win-core-profile-*", "api-ms-win-core-sysinfo-*"],
        exports: if on {
            vec![hle_export!(QueryPerformanceCounter), hle_export!(GetTickCount64)]
        } else {
            Vec::new()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_runs_fast_then_normal_without_going_back() {
        let f = Fast { scale: 100, start: 1_000, until: 2_000, frequency: 1_000 };
        let s = |t| scaled_until(&f, t, f.until);
        assert_eq!(s(1_000), 1_000);
        assert_eq!(s(1_500), 1_000 + 500 * 100);
        assert_eq!(s(2_000), 1_000 + 1_000 * 100);
        assert_eq!(s(2_010), 1_000 + 1_000 * 100 + 10);
        assert!(s(2_001) > s(2_000));
        // Ended early by the player at 1_200.
        assert_eq!(scaled_until(&f, 1_500, 1_200), 1_000 + 200 * 100 + 300);
    }

    #[test]
    fn the_switch_takes_a_factor_above_one() {
        assert_eq!(settings(None, None), None);
        assert_eq!(settings(Some("1"), None), None);
        assert_eq!(settings(Some("x"), None), None);
        assert_eq!(settings(Some("100"), None), Some((100, 180)));
        assert_eq!(settings(Some(" 50 "), Some("90")), Some((50, 90)));
        assert_eq!(settings(Some("50"), Some("0")), Some((50, 180)));
    }
}
