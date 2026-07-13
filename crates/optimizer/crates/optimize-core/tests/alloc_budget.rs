//! Allocation-count regression tests for the Workspace-reuse perf budgets (ROADMAP §M5.1,
//! ALGO §12 M5.1: "parser 0 alloc, planner O(1) reusable, renderer 1 output buffer").
//!
//! These install a counting `#[global_allocator]` for this test binary only (each file
//! under `tests/` is its own binary, so this does not affect the lib's unit tests or other
//! integration tests). Counting is gated by a thread-local "recording" flag so parallel
//! `cargo test` execution (a fresh OS thread per test) does not cross-contaminate counts.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use anyllm_optimize_core::{
    render, segment, split_words, BufferId, ContentBlock, Conversation, Edit, EditScript, Message,
    Protection, Role,
};

thread_local! {
    static RECORDING: Cell<bool> = const { Cell::new(false) };
    static ALLOC_COUNT: Cell<u64> = const { Cell::new(0) };
}

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        RECORDING.with(|r| {
            if r.get() {
                ALLOC_COUNT.with(|c| c.set(c.get() + 1));
            }
        });
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        RECORDING.with(|r| {
            if r.get() {
                ALLOC_COUNT.with(|c| c.set(c.get() + 1));
            }
        });
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Runs `f`, counting heap allocations (alloc + realloc calls) made by the current thread
/// while it runs. Touches the thread-locals before returning the closure so first-touch TLS
/// setup never lands inside the measured window.
fn count_allocs<F: FnOnce()>(f: F) -> u64 {
    ALLOC_COUNT.with(|c| c.set(0));
    RECORDING.with(|r| r.set(true));
    f();
    RECORDING.with(|r| r.set(false));
    ALLOC_COUNT.with(|c| c.get())
}

/// Parser budget: `segment` + `split_words` write only into caller-owned `Vec`s and never
/// build their own `String`/`Vec` internally (ALGO §5.2, "never allocates substrings; only
/// records byte ranges"). Once the scratch buffers are warmed to capacity, re-running the
/// scan over the same text must cost zero allocations.
#[test]
fn parser_allocates_nothing_after_warmup() {
    let text = "The quick brown fox jumps over the lazy dog. \
                 See https://example.com/path for `inline code` and more prose besides. "
        .repeat(50);

    let mut segs = Vec::new();
    let mut words = Vec::new();

    // Warm-up pass: grows `segs`/`words` to their steady-state capacity. Not measured.
    segment(&text, &mut segs);
    for seg in &segs {
        split_words(&text, seg.range.clone(), &mut words);
    }

    let allocs = count_allocs(|| {
        segs.clear();
        words.clear();
        segment(&text, &mut segs);
        for seg in &segs {
            split_words(&text, seg.range.clone(), &mut words);
        }
    });

    assert_eq!(
        allocs, 0,
        "parser (segment + split_words) allocated {allocs} time(s) on a warmed-up pass; \
         the M5.1 budget requires 0 allocations here (regression: check for a new \
         `.to_string()`/`.collect()`/local `Vec::new()` inside the scan)"
    );
}

fn conv_with_messages(n: usize, text: &str) -> Conversation {
    let messages = (0..n)
        .map(|_| Message {
            role: Role::User,
            blocks: vec![ContentBlock::Text(text.to_string())],
            protection: Protection::Mutable,
            client_cache_marker: false,
        })
        .collect();
    Conversation::new(messages)
}

fn drop_first_word() -> EditScript {
    EditScript::new(vec![Edit::Delete(0..6)]) // drops the "hello " prefix below
}

/// Renderer budget: `render` must reuse a single output buffer (via the swap-with-`buf`
/// pattern) across every edit applied in one call, not allocate a fresh buffer per edit.
/// Isolate that from the (identical-either-way) up-front `messages.clone()` cost by
/// comparing a 1-edit call against an N-edit call over the *same* conversation shape: if
/// the buffer were allocated per edit, allocation count would scale ~linearly with the
/// edit count; with correct reuse it stays flat regardless of N.
#[test]
fn renderer_edit_buffer_does_not_scale_with_edit_count() {
    let text = "hello there this is a modestly sized buffer of prose text used to exercise \
                 the renderer's edit-application buffer reuse path today.";
    let n = 40;
    let conv = conv_with_messages(n, text);

    let one_edit = vec![(0usize, BufferId(0), drop_first_word())];
    let allocs_one = count_allocs(|| {
        let _ = render(&conv, &one_edit, None);
    });

    let many_edits: Vec<_> = (0..n)
        .map(|i| (i, BufferId(0), drop_first_word()))
        .collect();
    let allocs_many = count_allocs(|| {
        let _ = render(&conv, &many_edits, None);
    });

    assert!(
        allocs_many <= allocs_one + 5,
        "renderer allocation count scaled with edit count ({allocs_one} allocs for 1 edit \
         -> {allocs_many} allocs for {n} edits); the M5.1 budget requires one shared output \
         buffer per render() call (regression: check for `String::new()` moved inside the \
         edits loop)"
    );
}
