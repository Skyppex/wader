//! Open documents. Plain text handling; nothing Rill-specific.

mod line_index;

pub use line_index::{Encoding, LineIndex};

use crate::lsp::{Position, Range, TextDocumentContentChangeEvent};

#[derive(Clone, Debug)]
pub struct Document {
    pub version: i32,
    pub text: String,
    pub index: LineIndex,
}

impl Document {
    pub fn new(text: String, version: i32, encoding: Encoding) -> Document {
        let index = LineIndex::new(&text, encoding);
        Document {
            version,
            text,
            index,
        }
    }

    /// Apply edits in order. A change without a range replaces everything.
    pub fn apply(&mut self, changes: Vec<TextDocumentContentChangeEvent>, version: i32) {
        for change in changes {
            match change.range {
                None => self.text = change.text,
                Some(range) => {
                    let start = self.index.offset(&self.text, range.start) as usize;
                    let end = (self.index.offset(&self.text, range.end) as usize).max(start);
                    self.text.replace_range(start..end, &change.text);
                }
            }
            self.index = LineIndex::new(&self.text, self.index.encoding());
        }
        self.version = version;
    }

    pub fn position(&self, offset: u32) -> Position {
        self.index.position(&self.text, offset)
    }

    pub fn offset(&self, pos: Position) -> u32 {
        self.index.offset(&self.text, pos)
    }

    pub fn range(&self, start: u32, end: u32) -> Range {
        self.index.range(&self.text, start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_and_ranged_changes() {
        let mut doc = Document::new("let a = 1\nlet b = 2\n".into(), 1, Encoding::Utf16);
        doc.apply(
            vec![TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(1, 4), Position::new(1, 5))),
                text: "bee".into(),
            }],
            2,
        );
        assert_eq!(doc.text, "let a = 1\nlet bee = 2\n");
        assert_eq!(doc.position(doc.text.len() as u32), Position::new(2, 0));
        doc.apply(
            vec![TextDocumentContentChangeEvent {
                range: None,
                text: "x".into(),
            }],
            3,
        );
        assert_eq!((doc.text.as_str(), doc.version), ("x", 3));
    }
}
