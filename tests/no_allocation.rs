//! Computing a period must not touch the heap.
//!
//! Every field's values live in a 64-bit mask, the calendar strategies
//! stream their firing days through an accumulator rather than collecting
//! them, and parsing fills a fixed array. Nothing in that chain should
//! allocate, and this test is what keeps it that way: a counting
//! allocator, and an assertion that the count does not move.
//!
//! Only rejection allocates, because a `CronError` carries an owned
//! message.
#![expect(
    unsafe_code,
    reason = "a global allocator cannot be written in safe Rust, and counting \
              allocations is the only way to check the property from outside"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    /// Allocations made by this thread.
    ///
    /// Per-thread, not global: the test harness runs tests in parallel,
    /// and a global counter would charge one test for another's
    /// allocations — which it did, intermittently, until this was fixed.
    ///
    /// `const`-initialised so that reading it cannot itself allocate and
    /// re-enter the allocator, and `Cell<usize>` has no destructor to
    /// register.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// Counts one allocation against the current thread. A thread that is
/// tearing down may no longer have the value, and missing a count there
/// is harmless.
fn note_allocation() {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get().saturating_add(1)));
}

fn allocation_count() -> usize {
    ALLOCATIONS.try_with(Cell::get).unwrap_or(0)
}

/// Passes everything through to the system allocator, counting requests.
struct Counting;

// SAFETY: every method forwards to `System` with the same arguments and
// the same contract; the only addition is a relaxed counter increment.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        // SAFETY: `layout` is forwarded unchanged from the caller.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` and `layout` are forwarded unchanged, and
        // `pointer` came from `System.alloc` above.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_allocation();
        // SAFETY: arguments are forwarded unchanged from the caller.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Allocations made on this thread while running `work`.
fn allocations_during(work: impl FnOnce()) -> usize {
    let before = allocation_count();
    work();
    allocation_count().saturating_sub(before)
}

#[test]
fn computing_a_period_never_allocates() {
    // One schedule per strategy: the 7-day cycle, the one-year calendar,
    // the full Gregorian walk, and `@every`.
    let schedules = [
        "* * * * *",
        "*/5 8-17 * * 1-5",
        "0 0 * * *",
        "0 9 * * MON-FRI",
        "0 0 1,15 * *",
        "0 0 1-7 1,4,7,10 *",
        "0 9 1 * 1",
        "0 0 29 2 *",
        "0 9 13 3 5",
        "@every 1h30m",
        "@monthly",
    ];

    // Warm every schedule first. The first calls in a process can
    // allocate for reasons that have nothing to do with us — lazy runtime
    // setup, the test harness — and warming the whole set rather than one
    // at a time keeps that from landing on whichever ran first.
    for expr in schedules {
        assert!(cronslop::try_max_period_seconds(expr).is_ok(), "{expr} should parse");
    }

    for expr in schedules {
        let mut period = 0;
        let allocations = allocations_during(|| {
            period = cronslop::try_max_period_seconds(expr).unwrap_or(0);
        });
        assert_eq!(allocations, 0, "computing '{expr}' allocated {allocations} time(s)");
        assert!(period > 0, "{expr} should have a period");
    }
}

#[test]
fn rejection_allocates_only_its_message() {
    let _ = cronslop::try_max_period_seconds("60 0 * * *");

    let allocations = allocations_during(|| {
        assert!(cronslop::try_max_period_seconds("0 0 * * MONDAY").is_err());
    });
    // The error owns one `String`; anything beyond that is a strayed
    // buffer worth knowing about.
    assert!(allocations <= 1, "rejection allocated {allocations} times, expected at most 1");
}
