//! Blind frames (testing aid): with `VICTORY_BLIND=<n>` the run starts
//! drawing only every `n`th frame (`1`: none; draws, dispatches and indirect
//! executions of the other frames are dropped, and those frames are not
//! shown; copies go on),
//! and F9 toggles between that and drawing every frame. A title
//! that advances its cutscenes per frame then plays them at the speed of its
//! own frames, not of ours: the intro before the prologue passes in a
//! fraction of the time. Without the variable nothing changes and F9 does
//! nothing.
//!
//! Frame cut (testing aid): with `VICTORY_FRAME_CUT=1` each frame runs only
//! its first N draws and dispatches (in submission order) and drops the
//! rest. F10 moves the cut 10% of the last frame earlier, F11 later, F12
//! turns it off (all run). Each change logs the cut and the pipeline at it:
//! moving it until a wrong layer disappears names the pass that draws it.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

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

/// Whether the frame now presented was drawn (a frame not drawn is not
/// shown: the window keeps the last drawn one instead of a blank frame).
pub(crate) fn drawn() -> bool {
    !on()
}

/// Once a frame (at present): picks whether the next frame is drawn; F9
/// toggles.
pub(crate) fn poll() {
    poll_cut();
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


/// Frame cut: ops run this frame, ops of the last frame, and the cut
/// (`u64::MAX`: none).
static OPS: AtomicU64 = AtomicU64::new(0);
static LAST_OPS: AtomicU64 = AtomicU64::new(0);
static CUT: AtomicU64 = AtomicU64::new(u64::MAX);
/// The pipeline at the cut in the last frame, to name it once.
static AT_CUT: AtomicUsize = AtomicUsize::new(0);
static NAMED: AtomicBool = AtomicBool::new(true);

fn cut_wanted() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var_os("VICTORY_FRAME_CUT").is_some_and(|v| v == "1");
        if on {
            tracing::warn!("VICTORY_FRAME_CUT: F10 cuts each frame 10% earlier, F11 later, F12 off");
        }
        on
    })
}

/// Whether the draw or dispatch with `pipeline` (the title's object) is
/// dropped: blind frames, or past the frame cut.
#[inline]
pub(crate) fn skip(pipeline: usize) -> bool {
    if on() {
        return true;
    }
    let cut = CUT.load(Ordering::Relaxed);
    if cut == u64::MAX {
        return false;
    }
    let n = OPS.fetch_add(1, Ordering::Relaxed);
    if n == cut {
        AT_CUT.store(pipeline, Ordering::Relaxed);
    }
    n >= cut
}

/// At present: frame cut keys and bookkeeping.
fn poll_cut() {
    if !cut_wanted() {
        return;
    }
    let ops = OPS.swap(0, Ordering::Relaxed);
    if CUT.load(Ordering::Relaxed) == u64::MAX {
        LAST_OPS.store(ops, Ordering::Relaxed);
    } else {
        LAST_OPS.store(LAST_OPS.load(Ordering::Relaxed).max(ops), Ordering::Relaxed);
    }
    if !NAMED.swap(true, Ordering::Relaxed) {
        let at = AT_CUT.load(Ordering::Relaxed);
        #[cfg(feature = "diagnostics")]
        let shaders = crate::pipeline::shaders_of(at as crate::object::This);
        #[cfg(not(feature = "diagnostics"))]
        let shaders = format!("{at:#x}");
        tracing::warn!(cut = CUT.load(Ordering::Relaxed), of = LAST_OPS.load(Ordering::Relaxed), at_cut = %shaders, "VICTORY_FRAME_CUT");
    }
    #[cfg(windows)]
    {
        static DOWN: [AtomicBool; 3] = [AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)];
        for (i, key) in [0x79, 0x7A, 0x7B].into_iter().enumerate() {
            // SAFETY: a plain query.
            let down = unsafe { GetAsyncKeyState(key) } < 0;
            if !down {
                DOWN[i].store(false, Ordering::Relaxed);
                continue;
            }
            if DOWN[i].swap(true, Ordering::Relaxed) {
                continue;
            }
            let total = LAST_OPS.load(Ordering::Relaxed).max(1);
            let step = (total / 10).max(1);
            let cut = CUT.load(Ordering::Relaxed);
            let new = match i {
                0 => cut.min(total).saturating_sub(step),
                1 => cut.min(total).saturating_add(step).min(total),
                _ => u64::MAX,
            };
            CUT.store(new, Ordering::Relaxed);
            NAMED.store(new == u64::MAX, Ordering::Relaxed);
            if new == u64::MAX {
                tracing::warn!("VICTORY_FRAME_CUT: off");
            }
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
