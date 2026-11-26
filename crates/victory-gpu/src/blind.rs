//! Blind frames (testing aid): with `VICTORY_BLIND=1` the run starts
//! drawing nothing (draws, dispatches and indirect executions are dropped;
//! copies and presents go on), and F9 toggles drawing on and off. A title
//! that advances its cutscenes per frame then plays them at the speed of its
//! own frames, not of ours: the intro before the prologue passes in a
//! fraction of the time. Without the variable nothing changes and F9 does
//! nothing.

use std::sync::atomic::{AtomicBool, Ordering};

/// Drawing is off now.
static BLIND: AtomicBool = AtomicBool::new(false);

fn wanted() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var_os("VICTORY_BLIND").is_some_and(|v| v == "1");
        if on {
            BLIND.store(true, Ordering::Relaxed);
            tracing::warn!("VICTORY_BLIND: nothing is drawn until F9 (F9 toggles)");
        }
        on
    })
}

/// Whether draws and dispatches are dropped now.
#[inline]
pub(crate) fn on() -> bool {
    BLIND.load(Ordering::Relaxed)
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
}

/// Once a frame (at present): F9 toggles drawing.
pub(crate) fn poll() {
    if !wanted() {
        return;
    }
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
