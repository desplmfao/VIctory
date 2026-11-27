//! Pipelines whose creation faults, survived (`VICTORY_PIPELINE_SOFT_CRASH`).
//!
//! Creating a host pipeline runs the host's shader compilers on the calling
//! thread (a translation layer's, if any, then the driver's).
//! Some translated shaders make one of them fault (an access violation, an
//! illegal instruction, a stack overflow). With this switch on, such a fault
//! on a pipeline worker ends that creation instead of the process: the
//! worker resumes after the call with the exception code, the pipeline is
//! refused (and listed in the pipeline journal, see `journal`), and the
//! worker hands its place to a fresh thread.
//!
//! How: [`guarded`] calls through a small assembly frame that saves the
//! callee-saved registers on its own stack and records where it resumes. A
//! vectored exception handler that sees a fault on a thread inside such a
//! call points the faulting context back at that frame. Nothing between the
//! fault and the frame is unwound: no destructor runs, nothing is freed.
//!
//! Risks, which is why this is opt-in:
//!
//! - locks the host held when it faulted stay held. The next creation that
//!   needs one of them waits for good; draws keep running without it
//!   (pipelines are waited for within a budget), but no pipeline is made;
//! - memory the abandoned frames owned leaks, and a fault caused by a
//!   corrupted heap leaves it corrupted;
//! - a frame-based handler of the host's own that would have handled the
//!   fault is not reached (vectored handlers run first). Only faults of the
//!   error kind are taken, not C++ exceptions or other software ones.
//!
//! After [`LIMIT`] faults, faults are left alone again: the process ends as
//! it would have, and the journal's note names the pipeline at the next
//! start.

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

/// Faults survived in one run; the next ends the process.
pub(crate) const LIMIT: u32 = 8;

/// The stack a pipeline worker gets: compilers that recurse over a shader's
/// control flow need far more than the 2 MiB Rust gives a thread. Reserved
/// only; pages are committed as they are used.
pub(crate) const WORKER_STACK: usize = 256 << 20;

/// Stack kept for exception handling when a thread's stack overflows.
const GUARANTEE: u32 = 64 << 10;

/// A fault that ended a guarded call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fault {
    /// The exception code (`0xC0000005` an access violation, `0xC00000FD`
    /// a stack overflow, `0xC000001D` an illegal instruction).
    pub code: u32,
    /// The faulting instruction's address.
    pub address: usize,
}

impl Fault {
    /// The fault as an `HRESULT` (`HRESULT_FROM_NT`), for error paths that
    /// carry one.
    pub(crate) fn error(self) -> windows::core::Error {
        windows::core::Error::from_hresult(windows::core::HRESULT((self.code | 0x1000_0000) as i32))
    }
}

static CAUGHT: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// Whether faults in guarded calls on this thread are taken.
    static ARMED: Cell<bool> = const { Cell::new(false) };
    /// Inside a guarded call: the frame's stack pointer and resume address.
    static RESUME: Cell<[usize; 2]> = const { Cell::new([0, 0]) };
    /// The fault the handler took, until the guarded call returns it.
    static TAKEN: Cell<Option<Fault>> = const { Cell::new(None) };
    /// The last fault a guarded call returned, for the caller that logs it.
    static LAST: Cell<Option<Fault>> = const { Cell::new(None) };
    /// This thread took a fault: its stack may have lost its guard page and
    /// it should end.
    static SPENT: Cell<bool> = const { Cell::new(false) };
}

/// Whether `VICTORY_PIPELINE_SOFT_CRASH` is set.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var_os("VICTORY_PIPELINE_SOFT_CRASH").is_some_and(|v| !v.is_empty() && v != "0");
        if on {
            tracing::warn!(
                limit = LIMIT,
                "VICTORY_PIPELINE_SOFT_CRASH: a pipeline whose creation faults is refused and the run goes on; locks or memory the host held then may be lost"
            );
        }
        on
    })
}

/// Takes faults in guarded calls on this thread from now on, and keeps
/// stack for handling an overflow of it.
pub(crate) fn arm() {
    install();
    let mut guarantee = GUARANTEE;
    // SAFETY: changes this thread's own guarantee, through a live local.
    unsafe {
        windows_sys::Win32::System::Threading::SetThreadStackGuarantee(&mut guarantee);
    }
    ARMED.set(true);
}

/// Whether this thread took a fault and should end (a pipeline worker).
pub(crate) fn spent() -> bool {
    SPENT.get()
}

/// The fault the last guarded call on this thread ended with, once.
pub(crate) fn take_fault() -> Option<Fault> {
    LAST.take()
}

/// Calls `f`. On a thread [`arm`]ed, a fault inside `f` ends the call with
/// `Err` instead of the process. Panics pass through.
///
/// `f` must hold no lock or other state of ours that a fault, which ends it
/// without unwinding, would leave behind.
pub(crate) fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, Fault> {
    if !ARMED.get() || CAUGHT.load(Ordering::Relaxed) >= LIMIT {
        return Ok(f());
    }
    imp::guarded(f).inspect_err(|&fault| {
        SPENT.set(true);
        LAST.set(Some(fault));
    })
}

/// Registers the handler once, ahead of the crash reporter's.
fn install() {
    use windows_sys::Win32::System::Diagnostics::Debug::AddVectoredExceptionHandler;
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: the handler only reads the exception and, on an armed thread
    // inside a guarded call, redirects it to that call's frame.
    ONCE.call_once(|| unsafe {
        AddVectoredExceptionHandler(1, Some(handler));
    });
}

/// Exceptions taken: faults (severity error, not customer codes such as
/// C++ exceptions), except those that say the process is beyond saving.
fn taken(code: u32) -> bool {
    const HEAP_CORRUPTION: u32 = 0xC000_0374;
    const STACK_BUFFER_OVERRUN: u32 = 0xC000_0409;
    code & 0xF000_0000 == 0xC000_0000 && !matches!(code, HEAP_CORRUPTION | STACK_BUFFER_OVERRUN)
}

unsafe extern "system" fn handler(
    info: *mut windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH,
    };
    // Kept small: after a stack overflow this runs on what little is left.
    // SAFETY: the system passes the exception being dispatched.
    let (record, context) = unsafe { (&*(*info).ExceptionRecord, &mut *(*info).ContextRecord) };
    let code = record.ExceptionCode as u32;
    let [rsp, rip] = RESUME.get();
    if rsp == 0 || !ARMED.get() || !taken(code) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    if CAUGHT.fetch_add(1, Ordering::Relaxed) >= LIMIT {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    RESUME.set([0, 0]);
    TAKEN.set(Some(Fault {
        code,
        address: record.ExceptionAddress as usize,
    }));
    imp::resume(context, rsp, rip, code);
    EXCEPTION_CONTINUE_EXECUTION
}

#[cfg(target_arch = "x86_64")]
mod imp {
    use super::*;

    // `victory_gpu_guarded_call(f, data, resume)`: saves the callee-saved
    // registers, MXCSR and the x87 control word in its frame, writes its
    // stack pointer and resume address to `resume`, and calls `f(data)`;
    // returns 0. The handler resumes a faulted call at `2:` with the code in
    // `eax`, on this frame's stack pointer. Unwind data describes the frame
    // for any stack walk through it.
    std::arch::global_asm!(
        ".globl victory_gpu_guarded_call",
        ".p2align 4",
        "victory_gpu_guarded_call:",
        ".seh_proc victory_gpu_guarded_call",
        "push rbp",
        ".seh_pushreg rbp",
        "push rbx",
        ".seh_pushreg rbx",
        "push rdi",
        ".seh_pushreg rdi",
        "push rsi",
        ".seh_pushreg rsi",
        "push r12",
        ".seh_pushreg r12",
        "push r13",
        ".seh_pushreg r13",
        "push r14",
        ".seh_pushreg r14",
        "push r15",
        ".seh_pushreg r15",
        "sub rsp, 216",
        ".seh_stackalloc 216",
        "movaps xmmword ptr [rsp + 32], xmm6",
        ".seh_savexmm xmm6, 32",
        "movaps xmmword ptr [rsp + 48], xmm7",
        ".seh_savexmm xmm7, 48",
        "movaps xmmword ptr [rsp + 64], xmm8",
        ".seh_savexmm xmm8, 64",
        "movaps xmmword ptr [rsp + 80], xmm9",
        ".seh_savexmm xmm9, 80",
        "movaps xmmword ptr [rsp + 96], xmm10",
        ".seh_savexmm xmm10, 96",
        "movaps xmmword ptr [rsp + 112], xmm11",
        ".seh_savexmm xmm11, 112",
        "movaps xmmword ptr [rsp + 128], xmm12",
        ".seh_savexmm xmm12, 128",
        "movaps xmmword ptr [rsp + 144], xmm13",
        ".seh_savexmm xmm13, 144",
        "movaps xmmword ptr [rsp + 160], xmm14",
        ".seh_savexmm xmm14, 160",
        "movaps xmmword ptr [rsp + 176], xmm15",
        ".seh_savexmm xmm15, 176",
        ".seh_endprologue",
        "stmxcsr dword ptr [rsp + 192]",
        "fnstcw word ptr [rsp + 196]",
        "mov qword ptr [r8], rsp",
        "lea rax, [rip + 2f]",
        "mov qword ptr [r8 + 8], rax",
        "mov rax, rcx",
        "mov rcx, rdx",
        "call rax",
        "xor eax, eax",
        "jmp 3f",
        "2:",
        "fninit",
        "fldcw word ptr [rsp + 196]",
        "ldmxcsr dword ptr [rsp + 192]",
        "3:",
        "movaps xmm6, xmmword ptr [rsp + 32]",
        "movaps xmm7, xmmword ptr [rsp + 48]",
        "movaps xmm8, xmmword ptr [rsp + 64]",
        "movaps xmm9, xmmword ptr [rsp + 80]",
        "movaps xmm10, xmmword ptr [rsp + 96]",
        "movaps xmm11, xmmword ptr [rsp + 112]",
        "movaps xmm12, xmmword ptr [rsp + 128]",
        "movaps xmm13, xmmword ptr [rsp + 144]",
        "movaps xmm14, xmmword ptr [rsp + 160]",
        "movaps xmm15, xmmword ptr [rsp + 176]",
        "add rsp, 216",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rsi",
        "pop rdi",
        "pop rbx",
        "pop rbp",
        "ret",
        ".seh_endproc",
    );

    unsafe extern "C" {
        fn victory_gpu_guarded_call(
            f: unsafe extern "C" fn(*mut c_void),
            data: *mut c_void,
            resume: *mut [usize; 2],
        ) -> u32;
    }

    struct Call<F, T> {
        f: Option<F>,
        out: Option<std::thread::Result<T>>,
    }

    /// Runs the call in `data` (a `Call<F, T>`); a panic is kept for the
    /// caller, not unwound through the assembly frame.
    unsafe extern "C" fn run<F: FnOnce() -> T, T>(data: *mut c_void) {
        // SAFETY: `guarded` passes its own `Call<F, T>`, alive and unshared.
        let call = unsafe { &mut *data.cast::<Call<F, T>>() };
        if let Some(f) = call.f.take() {
            call.out = Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)));
        }
    }

    pub(super) fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, Fault> {
        fn go<F: FnOnce() -> T, T>(f: F) -> Result<T, Fault> {
            let mut call = Call { f: Some(f), out: None };
            let outer = RESUME.get();
            // SAFETY: the frame calls `run` with `call`, which outlives it,
            // and writes the resume point to this thread's own cell.
            let code = unsafe {
                victory_gpu_guarded_call(run::<F, T>, (&raw mut call).cast(), RESUME.with(Cell::as_ptr))
            };
            RESUME.set(outer);
            match (code, call.out.take()) {
                (0, Some(Ok(v))) => Ok(v),
                (0, Some(Err(panic))) => std::panic::resume_unwind(panic),
                _ => Err(TAKEN.take().unwrap_or(Fault { code, address: 0 })),
            }
        }
        go(f)
    }

    /// Points `context` at a guarded call's resume address.
    pub(super) fn resume(
        context: &mut windows_sys::Win32::System::Diagnostics::Debug::CONTEXT,
        rsp: usize,
        rip: usize,
        code: u32,
    ) {
        context.Rsp = rsp as u64;
        context.Rip = rip as u64;
        context.Rax = u64::from(code);
        // The direction flag clear, as calls expect.
        context.EFlags &= !0x400;
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod imp {
    use super::*;

    pub(super) fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, Fault> {
        Ok(f())
    }

    pub(super) fn resume(
        _: &mut windows_sys::Win32::System::Diagnostics::Debug::CONTEXT,
        _: usize,
        _: usize,
        _: u32,
    ) {
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Reads `address` with an instruction of our own, so the fault is not
    /// undefined behaviour of Rust code.
    pub(crate) fn read(address: usize) -> u32 {
        let value: u32;
        // SAFETY: faults on an unmapped address; the guard resumes past it.
        unsafe {
            std::arch::asm!("mov {v:e}, dword ptr [{a}]", a = in(reg) address, v = out(reg) value);
        }
        value
    }

    fn on_worker<T: Send + 'static>(stack: usize, f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::Builder::new()
            .stack_size(stack)
            .spawn(move || {
                arm();
                f()
            })
            .unwrap()
            .join()
            .unwrap()
    }

    #[test]
    fn an_access_violation_in_a_guarded_call_returns_its_code() {
        let (fault, after, spent) = on_worker(4 << 20, || {
            let fault = guarded(|| read(8));
            // The thread goes on: another call returns normally.
            let after = guarded(|| 7);
            (fault, after, spent())
        });
        let fault = fault.unwrap_err();
        assert_eq!(fault.code, 0xC000_0005);
        assert_ne!(fault.address, 0);
        assert_eq!(after, Ok(7));
        assert!(spent);
    }

    #[test]
    fn an_illegal_instruction_in_a_guarded_call_returns_its_code() {
        let fault = on_worker(4 << 20, || {
            // SAFETY: faults; the guard resumes past it.
            guarded(|| unsafe { std::arch::asm!("ud2") })
        });
        assert_eq!(fault.unwrap_err().code, 0xC000_001D);
    }

    #[test]
    fn a_stack_overflow_in_a_guarded_call_returns_its_code() {
        #[inline(never)]
        fn down(n: u64) -> u64 {
            if n == u64::MAX {
                return 0; // never: the stack ends long before
            }
            let frame = std::hint::black_box([n as u8; 4096]);
            std::hint::black_box(down(n + 1)) + u64::from(frame[0])
        }
        // A small stack, so the overflow comes soon.
        let fault = on_worker(1 << 20, || guarded(|| down(0)));
        assert_eq!(fault.unwrap_err().code, 0xC000_00FD);
    }

    #[test]
    fn a_guarded_call_returns_its_value_and_passes_panics_on() {
        let v = on_worker(4 << 20, || guarded(|| 41 + 1));
        assert_eq!(v, Ok(42));
        let panicked = std::thread::spawn(|| {
            arm();
            let _ = guarded(|| panic!("on purpose"));
        });
        assert!(panicked.join().is_err());
    }

    #[test]
    fn a_thread_not_armed_is_not_guarded() {
        // Nothing to fault here: the call just runs.
        assert_eq!(std::thread::spawn(|| guarded(|| 5)).join().unwrap(), Ok(5));
        assert!(!std::thread::spawn(spent).join().unwrap());
    }

    #[test]
    fn faults_of_the_error_kind_are_taken() {
        assert!(taken(0xC000_0005));
        assert!(taken(0xC000_00FD));
        assert!(!taken(0xE06D_7363)); // C++
        assert!(!taken(0x8000_0003)); // breakpoint
        assert!(!taken(0xC000_0374));
    }
}
