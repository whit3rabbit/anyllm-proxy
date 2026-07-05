//! Cache-safety guard: the renderer must emit byte-identical PNGs across
//! rebuilds and dependency bumps, or the Anthropic prompt cache busts and the
//! whole feature net-loses money. A pinned sha is the only way to catch drift
//! that a same-process "render twice" check cannot (e.g. a miniz_oxide upgrade
//! changing the deflate stream). If this fails after a dep bump, that bump
//! changed the encoded bytes — decide deliberately, then re-pin.

use sha2::{Digest, Sha256};

const FIXTURE: &str = "\
fn main() {
    let path = \"src/lib.rs\";
    println!(\"{path}\");
}
// SHA a1b2c3d4 — PROJ-1482";

fn sha_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn render_output_is_pinned() {
    let pages = anyllm_pxpipe::render_text(FIXTURE, anyllm_pxpipe::RenderOpts::default());
    assert_eq!(pages.len(), 1);
    let got = sha_hex(&pages[0].png);
    // Pinned 2026-07-05. Re-pin ONLY after a deliberate, understood change.
    const PINNED: &str = "cad3f2b46713b3c680f4bdd728424e6a8c8b34817938ce0889628a69aa8e3e41";
    assert_eq!(
        got, PINNED,
        "renderer output drifted — this busts the prompt cache. If a dep bump caused it, re-pin intentionally."
    );
}
