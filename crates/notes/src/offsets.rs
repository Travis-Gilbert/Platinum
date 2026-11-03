//! Offsets into a note: comrak counts UTF-8 bytes, Theorem sessions Unicode scalar values, macOS text input UTF-16 units.
use ropey::Rope;
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// A range of a note in UTF-8 bytes (end exclusive); `line` is the 1-based line of `start`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
}

impl Span {
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }

    pub fn slice<'a>(&self, text: &'a str) -> &'a str {
        text.get(self.range()).unwrap_or_default()
    }
}

/// The one place offsets change units. Every method clamps an offset past the end to the end, and
/// rounds an offset inside a character down to that character's start.
pub struct Offsets {
    rope: Rope,
    // Start byte of each line; lines end at LF, CR or CRLF, as comrak counts them (ropey's own
    // line index also breaks at U+2028 and others).
    line_starts: Vec<usize>,
}

impl Offsets {
    pub fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut line_starts = vec![0];
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' || (b == b'\r' && bytes.get(i + 1) != Some(&b'\n')) {
                line_starts.push(i + 1);
            }
        }
        Self {
            rope: Rope::from_str(text),
            line_starts,
        }
    }

    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The 1-based line containing `byte`.
    pub fn line_of(&self, byte: usize) -> u32 {
        self.line_starts.partition_point(|&start| start <= byte) as u32
    }

    pub fn span(&self, start: usize, end: usize) -> Span {
        let start = start.min(self.len());
        Span {
            start,
            end: end.clamp(start, self.len()),
            line: self.line_of(start),
        }
    }

    /// The span of a comrak source position: 1-based line, 1-based inclusive byte column.
    /// An end column of 0 ends at the line's start, as comrak writes it for a block that
    /// swallows its final newline.
    pub fn sourcepos(
        &self,
        (start_line, start_col): (usize, usize),
        (end_line, end_col): (usize, usize),
    ) -> Span {
        let at = |line: usize| {
            self.line_starts
                .get(line.saturating_sub(1))
                .copied()
                .unwrap_or(self.len())
        };
        self.span(
            at(start_line) + start_col.saturating_sub(1),
            at(end_line) + end_col,
        )
    }

    /// The first byte of `line` (1-based), or the end of the text past the last line.
    pub fn line_start(&self, line: u32) -> usize {
        self.line_starts
            .get((line as usize).saturating_sub(1))
            .copied()
            .unwrap_or(self.len())
    }

    pub fn byte_to_scalar(&self, byte: usize) -> usize {
        self.rope.byte_to_char(byte.min(self.len()))
    }

    pub fn scalar_to_byte(&self, scalar: usize) -> usize {
        self.rope.char_to_byte(scalar.min(self.rope.len_chars()))
    }

    pub fn byte_to_utf16(&self, byte: usize) -> usize {
        self.scalar_to_utf16(self.byte_to_scalar(byte))
    }

    pub fn utf16_to_byte(&self, unit: usize) -> usize {
        self.scalar_to_byte(self.utf16_to_scalar(unit))
    }

    pub fn scalar_to_utf16(&self, scalar: usize) -> usize {
        self.rope
            .char_to_utf16_cu(scalar.min(self.rope.len_chars()))
    }

    pub fn utf16_to_scalar(&self, unit: usize) -> usize {
        self.rope
            .utf16_cu_to_char(unit.min(self.rope.len_utf16_cu()))
    }

    /// Convert a seam edit's old-text byte range to session scalar positions.
    pub fn byte_range_to_scalar(&self, range: Range<usize>) -> Range<usize> {
        self.byte_to_scalar(range.start)..self.byte_to_scalar(range.end)
    }

    /// Convert session scalar positions to a seam byte range in this text.
    pub fn scalar_range_to_byte(&self, range: Range<usize>) -> Range<usize> {
        self.scalar_to_byte(range.start)..self.scalar_to_byte(range.end)
    }

    /// Convert a seam byte range to macOS text-input UTF-16 units.
    pub fn byte_range_to_utf16(&self, range: Range<usize>) -> Range<usize> {
        self.byte_to_utf16(range.start)..self.byte_to_utf16(range.end)
    }

    /// Convert macOS text-input UTF-16 units to a seam byte range.
    pub fn utf16_range_to_byte(&self, range: Range<usize>) -> Range<usize> {
        self.utf16_to_byte(range.start)..self.utf16_to_byte(range.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // (text, byte offset of the character after "x", scalar offset, UTF-16 offset)
    const FIXTURES: &[(&str, &str, usize, usize, usize)] = &[
        ("ascii", "abcx", 3, 3, 3),
        ("emoji", "a😀x", 5, 2, 3),
        ("family emoji", "👨‍👩‍👧x", 18, 5, 8),
        ("combining mark", "e\u{301}x", 3, 2, 2),
        ("cjk", "日本語x", 9, 3, 3),
        ("right to left", "שלוםx", 8, 4, 4),
    ];

    #[test]
    fn offsets_match_reference_at_every_byte_and_utf16_unit() {
        for &(name, text, ..) in FIXTURES {
            let offsets = Offsets::new(text);
            for target in 0..text.len() + 3 {
                let byte = text
                    .char_indices()
                    .map(|(byte, _)| byte)
                    .chain([text.len()])
                    .take_while(|&byte| byte <= target)
                    .last()
                    .unwrap();
                assert_eq!(
                    offsets.byte_to_utf16(target),
                    text[..byte].encode_utf16().count(),
                    "{name}: byte {target}"
                );
            }
            for target in (0..text.encode_utf16().count() + 3).rev() {
                let byte = text
                    .char_indices()
                    .map(|(byte, _)| byte)
                    .chain([text.len()])
                    .take_while(|&byte| text[..byte].encode_utf16().count() <= target)
                    .last()
                    .unwrap();
                assert_eq!(
                    offsets.utf16_to_byte(target),
                    byte,
                    "{name}: utf16 {target}"
                );
            }
        }
    }

    #[test]
    fn offsets_are_independent_between_documents() {
        let before = Offsets::new("ab😀");
        let after = Offsets::new("😀ab");
        assert_eq!(before.byte_to_utf16(2), 2);
        assert_eq!(after.byte_to_utf16(2), 0);
        assert_eq!(after.utf16_to_byte(2), 4);
        assert_eq!(Offsets::new("👨‍👩‍👧").byte_to_utf16(18), 8);
        assert_eq!(Offsets::new("e\u{301}").utf16_to_byte(1), 1);
        assert_eq!(before.byte_to_utf16(2), 2);
    }

    #[test]
    fn edit_ranges_round_trip_through_session_and_mac_units() {
        for &(name, text, bytes, scalars, units) in FIXTURES {
            let offsets = Offsets::new(text);
            assert_eq!(offsets.byte_range_to_scalar(0..bytes), 0..scalars, "{name}");
            assert_eq!(offsets.scalar_range_to_byte(0..scalars), 0..bytes, "{name}");
            assert_eq!(offsets.byte_range_to_utf16(0..bytes), 0..units, "{name}");
            assert_eq!(offsets.utf16_range_to_byte(0..units), 0..bytes, "{name}");
            // Insertions must remain empty in all units.
            assert_eq!(
                offsets.byte_range_to_scalar(bytes..bytes),
                scalars..scalars,
                "{name}"
            );
        }
    }

    #[test]
    fn units_agree_before_the_marker() {
        for &(name, text, bytes, scalars, units) in FIXTURES {
            let o = Offsets::new(text);
            assert_eq!(text.len() - 1, bytes, "{name}: fixture");
            assert_eq!(o.byte_to_scalar(bytes), scalars, "{name}: byte to scalar");
            assert_eq!(o.scalar_to_byte(scalars), bytes, "{name}: scalar to byte");
            assert_eq!(o.byte_to_utf16(bytes), units, "{name}: byte to utf16");
            assert_eq!(o.utf16_to_byte(units), bytes, "{name}: utf16 to byte");
            assert_eq!(o.scalar_to_utf16(scalars), units, "{name}: scalar to utf16");
            assert_eq!(o.utf16_to_scalar(units), scalars, "{name}: utf16 to scalar");
        }
    }

    #[test]
    fn mixed_direction_text_round_trips_at_every_boundary() {
        let text = "ab مرحبا cx 😀 e\u{301} 日本";
        let o = Offsets::new(text);
        for (byte, _) in text.char_indices().chain([(text.len(), ' ')]) {
            let scalar = text[..byte].chars().count();
            let unit = text[..byte].encode_utf16().count();
            assert_eq!(o.byte_to_scalar(byte), scalar);
            assert_eq!(o.byte_to_utf16(byte), unit);
            assert_eq!(o.scalar_to_byte(scalar), byte);
            assert_eq!(o.utf16_to_byte(unit), byte);
        }
    }

    #[test]
    fn an_offset_inside_a_character_rounds_down() {
        let o = Offsets::new("a😀b");
        // Bytes 2 to 4 are inside the emoji; UTF-16 unit 2 is its low surrogate.
        assert_eq!(o.byte_to_scalar(3), 1);
        assert_eq!(o.utf16_to_scalar(2), 1);
        assert_eq!(o.utf16_to_byte(2), 1);
    }

    #[test]
    fn an_offset_past_the_end_clamps() {
        let o = Offsets::new("a😀");
        assert_eq!(o.byte_to_scalar(99), 2);
        assert_eq!(o.scalar_to_byte(99), 5);
        assert_eq!(o.byte_to_utf16(99), 3);
        assert_eq!(o.utf16_to_byte(99), 5);
        assert_eq!(
            o.span(3, 99),
            Span {
                start: 3,
                end: 5,
                line: 1
            }
        );
    }

    #[test]
    fn lines_end_at_lf_cr_and_crlf_only() {
        let text = "a\nb\r\nc\rd\u{2028}e\u{85}f";
        let o = Offsets::new(text);
        assert_eq!(o.line_of(0), 1);
        assert_eq!(o.line_of(2), 2);
        assert_eq!(o.line_of(5), 3);
        assert_eq!(o.line_of(7), 4);
        assert_eq!(o.line_of(text.len()), 4);
        assert_eq!(o.line_start(3), 5);
        assert_eq!(o.line_start(99), text.len());
    }

    #[test]
    fn comrak_positions_become_byte_spans() {
        // A BOM counts as three bytes in comrak's columns.
        let text = "\u{feff}# T 😀\n\n- [x] done\n";
        let o = Offsets::new(text);
        let heading = o.sourcepos((1, 4), (1, 11));
        assert_eq!(heading.slice(text), "# T 😀");
        let item = o.sourcepos((3, 1), (3, 10));
        assert_eq!(item.slice(text), "- [x] done");
        assert_eq!(item.line, 3);
        // An end column of 0 ends at the start of that line.
        assert_eq!(o.sourcepos((3, 1), (4, 0)).slice(text), "- [x] done\n");
        // A line past the end of the text clamps.
        assert_eq!(o.sourcepos((9, 1), (9, 5)).start, text.len());
    }

    #[test]
    fn spans_slice_safely() {
        let text = "a😀b";
        assert_eq!(
            Span {
                start: 1,
                end: 5,
                line: 1
            }
            .slice(text),
            "😀"
        );
        // Not on character boundaries: no panic, no text.
        assert_eq!(
            Span {
                start: 2,
                end: 4,
                line: 1
            }
            .slice(text),
            ""
        );
    }
}
