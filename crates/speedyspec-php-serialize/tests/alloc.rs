//! Hostile bytes cannot make `unserialize` reserve far more memory than they hold.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use speedyspec_php_serialize::unserialize;

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = NOW.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
        PEAK.fetch_max(now, Ordering::SeqCst);
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        NOW.fetch_sub(layout.size(), Ordering::SeqCst);
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

#[test]
fn nested_large_counts_stay_bounded() {
    // Each level claims 1024 members; the input ends before any of them.
    let input = "a:1024:{i:0;".repeat(4000).into_bytes();
    let before = NOW.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    assert!(unserialize(&input).is_err());
    let used = PEAK.load(Ordering::SeqCst) - before;
    assert!(
        used < 64 * input.len(),
        "{used} bytes reserved for {} bytes of input",
        input.len()
    );
}
