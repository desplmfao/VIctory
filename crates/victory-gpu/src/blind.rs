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
//! Frame gap (testing aid): with `VICTORY_FRAME_CUT=1` each frame drops one
//! window of a tenth of its draws and dispatches (in submission order);
//! everything before and after it runs, the final composite included.
//! Numpad minus moves the window a tenth earlier, numpad plus later, numpad
//! star turns it off (all run); F10, F11 and F12 do the same.
//! The keys come from GameInput's host poll (every 8 ms, [`keys`]), so a tap
//! counts however slow frames are. Each change logs the cut and the pipeline at it:
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
/// Pipelines dropped in the window this frame (the first few), to name
/// them once.
static IN_GAP: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
/// The pipeline at the cut in the last frame, to name it once.
static AT_CUT: AtomicUsize = AtomicUsize::new(0);
static NAMED: AtomicBool = AtomicBool::new(true);

fn cut_wanted() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var_os("VICTORY_FRAME_CUT").is_some_and(|v| v == "1");
        if on {
            tracing::warn!("VICTORY_FRAME_CUT: a tenth of each frame is dropped from where numpad - / + (F10/F11) put it; numpad * (F12) off");
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
    if !cut_wanted() {
        return false;
    }
    let n = OPS.fetch_add(1, Ordering::Relaxed);
    let cut = CUT.load(Ordering::Relaxed);
    if cut == u64::MAX {
        return false;
    }
    let width = (LAST_OPS.load(Ordering::Relaxed) / 10).max(1);
    let dropped = n >= cut && n < cut + width;
    if n == cut {
        AT_CUT.store(pipeline, Ordering::Relaxed);
    }
    if dropped && !NAMED.load(Ordering::Relaxed) {
        let mut gap = IN_GAP.lock().unwrap_or_else(|e| e.into_inner());
        if gap.len() < 64 && !gap.contains(&pipeline) {
            gap.push(pipeline);
        }
    }
    dropped
}

/// At present: frame cut keys and bookkeeping.
fn poll_cut() {
    if !cut_wanted() {
        return;
    }
    // Every draw and dispatch is counted, cut or not: the frame's size.
    LAST_OPS.store(OPS.swap(0, Ordering::Relaxed), Ordering::Relaxed);
    if !NAMED.swap(true, Ordering::Relaxed) {
        let gap = std::mem::take(&mut *IN_GAP.lock().unwrap_or_else(|e| e.into_inner()));
        #[cfg(feature = "diagnostics")]
        let shaders: Vec<String> = gap.iter().map(|&p| crate::pipeline::shaders_of(p as crate::object::This)).collect();
        #[cfg(not(feature = "diagnostics"))]
        let shaders: Vec<String> = gap.iter().map(|p| format!("{p:#x}")).collect();
        let _ = AT_CUT.load(Ordering::Relaxed);
        tracing::warn!(at = %caption(), from = CUT.load(Ordering::Relaxed), of = LAST_OPS.load(Ordering::Relaxed), pipelines = gap.len(), "VICTORY_FRAME_CUT: window dropped");
        for (i, s) in shaders.iter().enumerate() {
            tracing::warn!(i, "VICTORY_FRAME_CUT dropped {s}");
        }
    }

}


/// The host keys down now (virtual-key codes), from GameInput's poll: a
/// press of numpad minus / F10 moves the cut earlier, numpad plus / F11
/// later, numpad star / F12 turns it off.
pub fn keys(down: &[u8]) {
    if !cut_wanted() {
        return;
    }
    static DOWN: [AtomicBool; 3] = [AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)];
    for (i, keys) in [[0x6D, 0x79], [0x6B, 0x7A], [0x6A, 0x7B]].into_iter().enumerate() {
        let now = down.iter().any(|k| keys.contains(k));
        if !now || DOWN[i].swap(now, Ordering::Relaxed) {
            DOWN[i].store(now, Ordering::Relaxed);
            continue;
        }
        let total = LAST_OPS.load(Ordering::Relaxed).max(1);
        let step = (total / 10).max(1);
        let cut = CUT.load(Ordering::Relaxed);
        let new = match (i, cut) {
            // Off: minus starts at the last tenth, plus at the first.
            (0, u64::MAX) => total.saturating_sub(step),
            (1, u64::MAX) => 0,
            (0, c) => c.saturating_sub(step),
            (1, c) if c.saturating_add(step) >= total => u64::MAX,
            (1, c) => c + step,
            _ => u64::MAX,
        };
        CUT.store(new, Ordering::Relaxed);
        NAMED.store(new == u64::MAX, Ordering::Relaxed);
        tracing::warn!(cut = new, of = total, "VICTORY_FRAME_CUT: key");
    }
}

/// For the window title: where the frame gap is (` | gap 4/10`), or nothing.
pub(crate) fn caption() -> String {
    if !cut_wanted() {
        return String::new();
    }
    let cut = CUT.load(Ordering::Relaxed);
    if cut == u64::MAX {
        return " | gap off".into();
    }
    let total = LAST_OPS.load(Ordering::Relaxed).max(1);
    let step = (total / 10).max(1);
    format!(" | gap {}/10", (cut / step + 1).min(10))
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
