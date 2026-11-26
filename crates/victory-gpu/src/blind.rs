//! Blind frames (testing aid): with `VICTORY_BLIND=<n>` the run starts
//! drawing only every `n`th frame (`1`: none; draws, dispatches and indirect
//! executions of the other frames are dropped; copies and presents go on),
//! and F9 toggles between that and drawing every frame. A title
//! that advances its cutscenes per frame then plays them at the speed of its
//! own frames, not of ours: the intro before the prologue passes in a
//! fraction of the time. Without the variable nothing changes and F9 does
//! nothing.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Drawing is cut down now.
static BLIND: AtomicBool = AtomicBool::new(false);
/// Frames drawn while cut down: every `EVERY`th (0: none).
static EVERY: AtomicU64 = AtomicU64::new(0);
/// This frame is one of those drawn.
static DRAWN: AtomicBool = AtomicBool::new(false);

fn every(value: Option<&std::ffi::OsStr>) -> Option<u64> {
    let n = value?.to_str()?.trim().parse::<u64>().ok().filter(|&n| n >= 1)?;
    Some(if n == 1 { 0 } else { n })
}

fn wanted() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let Some(n) = every(std::env::var_os("VICTORY_BLIND").as_deref()) else {
            return false;
        };
        EVERY.store(n, Ordering::Relaxed);
        BLIND.store(true, Ordering::Relaxed);
        tracing::warn!(every = n, "VICTORY_BLIND: only every n-th frame is drawn (0: none) until F9 (F9 toggles)");
        true
    })
}

/// Whether draws and dispatches are dropped now.
#[inline]
pub(crate) fn on() -> bool {
    BLIND.load(Ordering::Relaxed) && !DRAWN.load(Ordering::Relaxed)
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
}

/// Once a frame (at present): picks whether the next frame is drawn; F9
/// toggles.
pub(crate) fn poll() {
    if !wanted() {
        return;
    }
    static FRAME: AtomicU64 = AtomicU64::new(0);
    let frame = FRAME.fetch_add(1, Ordering::Relaxed) + 1;
    let n = EVERY.load(Ordering::Relaxed);
    DRAWN.store(n != 0 && frame.is_multiple_of(n), Ordering::Relaxed);
    #[cfg(windows)]
    {
        static DOWN: AtomicBool = AtomicBool::new(false);
        const VK_F9: i32 = 0x78;
        // SAFETY: a plain query.
        let down = unsafe { GetAsyncKeyState(VK_F9) } < 0;
        if down && !DOWN.swap(true, Ordering::Relaxed) {
            let now = !BLIND.fetch_xor(true, Ordering::Relaxed);
            tracing::warn!(blind = now, "VICTORY_BLIND: F9");
        } else if !down {
            DOWN.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_switch_takes_how_often_to_draw() {
        let n = |v: &str| super::every(Some(std::ffi::OsStr::new(v)));
        assert_eq!(super::every(None), None);
        assert_eq!(n("1"), Some(0));
        assert_eq!(n("10"), Some(10));
        assert_eq!(n("0"), None);
        assert_eq!(n("x"), None);
    }
}
