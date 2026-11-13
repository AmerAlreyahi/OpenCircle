//! The allocator (glibc): the arena limit and giving freed heap back to the OS.

/// Call first in `main`, before any thread starts (glibc fixes the arena limit
/// at the first thread's first allocation). Frames and panel work are allocated
/// on short-lived threads; with per-thread arenas each overlay session kept a
/// whole frame (~20 MB at 3440x1440) after closing. One arena keeps the app at
/// its idle size.
pub fn limit_malloc_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: only changes an allocator setting; no memory is touched.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 1);
    }
}

/// Gives freed heap memory back to the OS (glibc keeps it otherwise). Cheap
/// (a few ms); call once big buffers are gone and the app is going idle.
pub fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim only releases free heap pages.
    unsafe {
        libc::malloc_trim(0);
    }
    // Windows: the heap hands its free pages back. The working set is left
    // alone: trimming it only moves pages out, and they fault back on next use.
    #[cfg(windows)]
    {
        use windows::Win32::System::Memory::{GetProcessHeap, HEAP_FLAGS, HeapCompact};
        // SAFETY: HeapCompact only releases unused heap pages.
        unsafe {
            if let Ok(heap) = GetProcessHeap() {
                HeapCompact(heap, HEAP_FLAGS(0));
            }
        }
    }
}

/// `trim_heap` a beat later (a window just closed: its surface and pixmaps are
/// freed by the renderer after the close, so an immediate trim misses them).
/// Runs on its own short-lived thread.
pub fn trim_heap_soon() {
    trim_after(std::time::Duration::from_millis(500), trim_heap);
}

fn trim_after(delay: std::time::Duration, f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        f();
    });
}

/// For a long-lived worker: freed memory stays in the heap (no mmap'd chunks
/// handed back, no automatic trim) until `trim_heap` is called.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn keep_free_heap() {
    // SAFETY: allocator settings only; no memory is touched.
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 256 << 20);
        libc::mallopt(libc::M_TRIM_THRESHOLD, 1 << 30);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn trim_runs_after_the_delay_not_before() {
        let (tx, rx) = std::sync::mpsc::channel();
        let t = std::time::Instant::now();
        super::trim_after(std::time::Duration::from_millis(100), move || {
            tx.send(()).unwrap()
        });
        assert!(rx.try_recv().is_err(), "ran at once");
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert!(t.elapsed() >= std::time::Duration::from_millis(100));
    }
}
