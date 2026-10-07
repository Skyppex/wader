//! Byte offsets <-> LSP positions.

use crate::lsp::{Position, PositionEncodingKind, Range};

/// What `Position::character` counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    /// The LSP default.
    #[default]
    Utf16,
    Utf32,
}

impl Encoding {
    /// Pick from the client's offer, preferring UTF-8 since that is what the
    /// text is stored as. Without an offer, the spec says UTF-16.
    pub fn negotiate(offered: Option<&[PositionEncodingKind]>) -> Encoding {
        let offered = offered.unwrap_or_default();
        if offered.contains(&PositionEncodingKind::utf8()) {
            Encoding::Utf8
        } else if offered.contains(&PositionEncodingKind::utf32()) {
            Encoding::Utf32
        } else {
            Encoding::Utf16
        }
    }

    pub fn kind(self) -> PositionEncodingKind {
        match self {
            Encoding::Utf8 => PositionEncodingKind::utf8(),
            Encoding::Utf16 => PositionEncodingKind::utf16(),
            Encoding::Utf32 => PositionEncodingKind::utf32(),
        }
    }

    /// Length of `s` in this encoding's units.
    pub fn len(self, s: &str) -> u32 {
        match self {
            Encoding::Utf8 => s.len() as u32,
            Encoding::Utf16 => s.chars().map(|c| c.len_utf16() as u32).sum(),
            Encoding::Utf32 => s.chars().count() as u32,
        }
    }

    fn char_len(self, c: char) -> u32 {
        match self {
            Encoding::Utf8 => c.len_utf8() as u32,
            Encoding::Utf16 => c.len_utf16() as u32,
            Encoding::Utf32 => 1,
        }
    }
}

/// Start of every line in a text, for converting positions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndex {
    /// Byte offset of each line's first character. Always starts with 0.
    starts: Vec<u32>,
    len: u32,
    encoding: Encoding,
}

impl LineIndex {
    pub fn new(text: &str, encoding: Encoding) -> LineIndex {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i as u32 + 1))
            .collect();
        LineIndex {
            starts,
            len: text.len() as u32,
            encoding,
        }
    }

    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Byte range of `line`'s content, without the line break.
    fn line_bounds(&self, text: &str, line: usize) -> (usize, usize) {
        let start = self.starts[line] as usize;
        let mut end = self
            .starts
            .get(line + 1)
            .map_or(self.len as usize, |&next| next as usize - 1);
        if end > start && text.as_bytes()[end - 1] == b'\r' {
            end -= 1;
        }
        (start, end)
    }

    pub fn position(&self, text: &str, offset: u32) -> Position {
        let offset = offset.min(self.len);
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        let (start, end) = self.line_bounds(text, line);
        let mut at = (offset as usize).min(end);
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        Position {
            line: line as u32,
            character: self.encoding.len(&text[start..at]),
        }
    }

    /// The byte offset of `pos`. Out-of-range lines go to the end of the
    /// text, out-of-range characters to the end of the line, and a position
    /// inside a character to that character's start.
    pub fn offset(&self, text: &str, pos: Position) -> u32 {
        let line = pos.line as usize;
        if line >= self.starts.len() {
            return self.len;
        }
        let (start, end) = self.line_bounds(text, line);
        let mut units = 0;
        for (i, c) in text[start..end].char_indices() {
            let next = units + self.encoding.char_len(c);
            if next > pos.character {
                return (start + i) as u32;
            }
            units = next;
        }
        end as u32
    }

    pub fn range(&self, text: &str, start: u32, end: u32) -> Range {
        Range {
            start: self.position(text, start),
            end: self.position(text, end),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn ascii() {
        let text = "ab\ncd\n";
        let ix = LineIndex::new(text, Encoding::Utf16);
        assert_eq!(ix.position(text, 0), pos(0, 0));
        assert_eq!(ix.position(text, 2), pos(0, 2));
        assert_eq!(ix.position(text, 3), pos(1, 0));
        assert_eq!(ix.position(text, 6), pos(2, 0));
        for offset in 0..=6 {
            assert_eq!(ix.offset(text, ix.position(text, offset)), offset);
        }
    }

    #[test]
    fn encodings_count_differently() {
        // `ø` is 2 bytes / 1 UTF-16 unit, `🎵` is 4 bytes / 2 units.
        let text = "aø🎵b";
        let b = text.find('b').unwrap() as u32;
        for (enc, ch) in [
            (Encoding::Utf8, 7),
            (Encoding::Utf16, 4),
            (Encoding::Utf32, 3),
        ] {
            let ix = LineIndex::new(text, enc);
            assert_eq!(ix.position(text, b), pos(0, ch), "{enc:?}");
            assert_eq!(ix.offset(text, pos(0, ch)), b, "{enc:?}");
        }
    }

    #[test]
    fn inside_a_character_snaps_to_its_start() {
        let text = "a🎵b";
        let ix = LineIndex::new(text, Encoding::Utf16);
        // Character 2 is the second half of the surrogate pair.
        assert_eq!(ix.offset(text, pos(0, 2)), 1);
        // Byte 2 is inside the note.
        assert_eq!(ix.position(text, 2), pos(0, 1));
    }

    #[test]
    fn crlf() {
        let text = "ab\r\ncd";
        let ix = LineIndex::new(text, Encoding::Utf16);
        assert_eq!(ix.position(text, 4), pos(1, 0));
        // Past the end of line 0 stops before the `\r`.
        assert_eq!(ix.offset(text, pos(0, 99)), 2);
        assert_eq!(ix.position(text, 3), pos(0, 2));
    }

    #[test]
    fn out_of_range_clamps() {
        let text = "ab\ncd";
        let ix = LineIndex::new(text, Encoding::Utf16);
        assert_eq!(ix.offset(text, pos(9, 0)), 5);
        assert_eq!(ix.offset(text, pos(1, 9)), 5);
        assert_eq!(ix.position(text, 99), pos(1, 2));
    }

    #[test]
    fn negotiation() {
        assert_eq!(Encoding::negotiate(None), Encoding::Utf16);
        let offer = [PositionEncodingKind::utf16(), PositionEncodingKind::utf8()];
        assert_eq!(Encoding::negotiate(Some(&offer)), Encoding::Utf8);
    }
}
