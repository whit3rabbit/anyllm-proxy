//! Edit scripts: byte-range deletions/replacements over ONE text buffer. Extractive
//! only (Delete, or Replace-with-shorter-marker for structural truncation). Validation
//! is the safety boundary — reject the whole script on any violation (fail-open).

use std::ops::Range;

use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Delete(Range<usize>),
    Replace { range: Range<usize>, text: String },
}

impl Edit {
    fn range(&self) -> &Range<usize> {
        match self {
            Edit::Delete(r) => r,
            Edit::Replace { range, .. } => range,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditScript {
    /// MUST be sorted by start, non-overlapping. Enforced by `validate`.
    pub edits: Vec<Edit>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EditError {
    #[error("edit ranges overlap")]
    Overlap,
    #[error("edit range out of bounds or inverted")]
    OutOfBounds,
    #[error("edit range not on a char boundary")]
    NotCharBoundary,
}

impl EditScript {
    pub fn new(edits: Vec<Edit>) -> Self {
        Self { edits }
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// Validate against `src`. Non-overlapping, in-bounds, on char boundaries.
    pub fn validate(&self, src: &str) -> Result<(), EditError> {
        let mut prev_end = 0usize;
        for e in &self.edits {
            let r = e.range();
            if r.start < prev_end {
                return Err(EditError::Overlap);
            }
            if r.end > src.len() || r.start > r.end {
                return Err(EditError::OutOfBounds);
            }
            if !src.is_char_boundary(r.start) || !src.is_char_boundary(r.end) {
                return Err(EditError::NotCharBoundary);
            }
            prev_end = r.end;
        }
        Ok(())
    }

    /// Apply the (validated) script to `src`, writing into `out`. Caller must have
    /// validated first; behavior on an invalid script is unspecified but panic-free
    /// for in-bounds ranges.
    pub fn apply(&self, src: &str, out: &mut String) {
        out.clear();
        let mut cursor = 0usize;
        for e in &self.edits {
            match e {
                Edit::Delete(r) => {
                    out.push_str(&src[cursor..r.start]);
                    cursor = r.end;
                }
                Edit::Replace { range, text } => {
                    out.push_str(&src[cursor..range.start]);
                    out.push_str(text);
                    cursor = range.end;
                }
            }
        }
        out.push_str(&src[cursor..]);
    }

    /// Total bytes removed (Delete length, plus Replace shrink). Never negative for a
    /// shortening script; saturates at 0 otherwise.
    pub fn bytes_removed(&self) -> usize {
        self.edits
            .iter()
            .map(|e| match e {
                Edit::Delete(r) => r.end - r.start,
                Edit::Replace { range, text } => {
                    (range.end - range.start).saturating_sub(text.len())
                }
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_delete_and_replace() {
        let src = "hello brave new world";
        let script = EditScript::new(vec![
            Edit::Delete(6..12), // "brave "
            Edit::Replace {
                range: 16..21,
                text: "W".into(),
            }, // "world" -> "W"
        ]);
        assert!(script.validate(src).is_ok());
        let mut out = String::new();
        script.apply(src, &mut out);
        assert_eq!(out, "hello new W");
    }

    #[test]
    fn validate_rejects_overlap() {
        let src = "abcdef";
        let s = EditScript::new(vec![Edit::Delete(1..4), Edit::Delete(3..5)]);
        assert_eq!(s.validate(src), Err(EditError::Overlap));
    }

    #[test]
    fn validate_rejects_oob() {
        let src = "abc";
        let s = EditScript::new(vec![Edit::Delete(2..9)]);
        assert_eq!(s.validate(src), Err(EditError::OutOfBounds));
    }

    #[test]
    fn validate_rejects_non_char_boundary() {
        let src = "aé"; // 'é' is 2 bytes at index 1..3
        let s = EditScript::new(vec![Edit::Delete(1..2)]);
        assert_eq!(s.validate(src), Err(EditError::NotCharBoundary));
    }
}
