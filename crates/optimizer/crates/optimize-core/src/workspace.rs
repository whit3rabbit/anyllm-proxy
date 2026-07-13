//! Per-worker reusable scratch. Acquired per request, cleared not freed, to keep the
//! hot path low-allocation. One `Workspace` per worker thread.

use crate::select::Word;

#[derive(Default)]
pub struct Workspace {
    pub words: Vec<Word>,
    pub scores: Vec<f32>,
    pub keep: Vec<bool>,
    pub edit_buf: String,
    /// Scratch for scorer subtoken ids (used by the ONNX scorer).
    pub ids: Vec<u32>,
    /// Scratch string set used by adapters / rendering.
    pub strbuf: String,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear all scratch without freeing capacity.
    pub fn clear(&mut self) {
        self.words.clear();
        self.scores.clear();
        self.keep.clear();
        self.edit_buf.clear();
        self.ids.clear();
        self.strbuf.clear();
    }
}
