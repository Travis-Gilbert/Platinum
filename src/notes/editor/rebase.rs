//! Text arithmetic for the editor tab: what changed between two texts, and how
//! keystrokes the vault has not seen yet survive a head that moved under them.
//!
//! SOURCING: `diffy` does the line diff and the three-way merge. Only the
//! translation of its hunks into the editor's byte-range edits is written here.

use std::ops::Range;

use diffy::{DiffOptions, HunkRange};
use zorite_editor::TextEdit;

/// Replacements that turn `old` into `new`, in the editor's sequential form:
/// each range addresses the text after the edits before it. They come from a
/// line diff, narrowed to the characters that differ, so a caret on an
/// unchanged part of a changed line does not move. Hunks are returned last
/// first, which keeps every range valid in the original text.
pub fn edits_between(old: &str, new: &str) -> Vec<TextEdit> {
    if old == new {
        return Vec::new();
    }
    let patch = DiffOptions::new().set_context_len(0).create_patch(old, new);
    let (old_lines, new_lines) = (line_starts(old), line_starts(new));
    // A hunk with no lines on one side names the line before the gap.
    let first = |range: HunkRange| {
        if range.is_empty() {
            range.start()
        } else {
            range.start() - 1
        }
    };
    let span = |starts: &[usize], range: HunkRange| {
        starts[first(range)]..starts[first(range) + range.len()]
    };
    patch
        .hunks()
        .iter()
        .rev()
        .filter_map(|hunk| {
            let replaced = span(&old_lines, hunk.old_range());
            let inserted = span(&new_lines, hunk.new_range());
            narrowed(old, replaced, &new[inserted])
        })
        .collect()
}

/// `range` of `old` replaced by `text`, without the characters both share at
/// either end. `None` when nothing is left to change.
fn narrowed(old: &str, range: Range<usize>, text: &str) -> Option<TextEdit> {
    let replaced = &old[range.clone()];
    let prefix = replaced
        .char_indices()
        .zip(text.chars())
        .take_while(|((_, a), b)| a == b)
        .last()
        .map_or(0, |((at, a), _)| at + a.len_utf8());
    let suffix = replaced[prefix..]
        .chars()
        .rev()
        .zip(text[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    let range = range.start + prefix..range.end - suffix;
    let text = &text[prefix..text.len() - suffix];
    (!range.is_empty() || !text.is_empty()).then(|| TextEdit {
        range,
        text: text.to_owned(),
    })
}

/// The byte offset of each line of `text`, then its length: line `i` is
/// `starts[i]..starts[i + 1]`.
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
    if starts.last() != Some(&text.len()) {
        starts.push(text.len());
    }
    starts
}

/// What `ours` and `theirs`, both made from `base`, come to together. `Ok`
/// when their changes do not overlap. `Err` carries the text with both
/// versions of each overlap between conflict markers, so neither side's
/// words are dropped and a person can choose.
pub fn merge3(base: &str, ours: &str, theirs: &str) -> Result<String, String> {
    diffy::merge(base, ours, theirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &str, edits: &[TextEdit]) -> String {
        let mut text = old.to_owned();
        for edit in edits {
            text.replace_range(edit.range.clone(), &edit.text);
        }
        text
    }

    fn assert_turns_into(old: &str, new: &str) {
        let edits = edits_between(old, new);
        assert_eq!(apply(old, &edits), new, "from {old:?} to {new:?}");
    }

    #[test]
    fn edits_turn_one_text_into_the_other() {
        for (old, new) in [
            ("", ""),
            ("", "one\n"),
            ("one\n", ""),
            ("one\ntwo\nthree\n", "one\ntwo\nthree\nfour\n"),
            ("one\ntwo\nthree\n", "zero\none\ntwo\nthree\n"),
            ("one\ntwo\nthree\n", "one\nthree\n"),
            ("one\ntwo\nthree\n", "one\n2\nthree\n"),
            ("one\ntwo\nthree", "one\ntwo\nthree\n"),
            ("one\ntwo\nthree\n", "one\ntwo\nthree"),
            ("a\r\nb\r\nc\r\n", "a\r\nB\r\nc\r\n"),
            ("日本語\n😀 e\u{301}\n", "日本\n😀 e\u{301}!\n"),
            ("same\n", "same\n"),
            ("a\nb\na\nb\n", "b\na\nb\na\n"),
        ] {
            assert_turns_into(old, new);
        }
    }

    /// Texts drawn from a few lines and characters, so that every kind of
    /// overlap between lines, endings and multi-byte characters comes up.
    #[test]
    fn edits_turn_any_text_into_any_other() {
        let lines = ["", "a", "b", "日本", "😀 x", "e\u{301}", "a b", "  c"];
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % bound as u64) as usize
        };
        let text = |next: &mut dyn FnMut(usize) -> usize| {
            let count = next(7);
            let mut text: String = (0..count)
                .map(|_| format!("{}\n", lines[next(lines.len())]))
                .collect();
            if next(4) == 0 {
                text.pop();
            }
            text
        };
        for _ in 0..4000 {
            let (old, new) = (text(&mut next), text(&mut next));
            assert_turns_into(&old, &new);
        }
    }

    #[test]
    fn a_caret_outside_the_changed_words_has_nothing_to_move_it() {
        let edits = edits_between("hello brave world\n", "hello brave new world\n");
        assert_eq!(
            edits,
            [TextEdit {
                range: 12..12,
                text: "new ".into()
            }]
        );
    }

    #[test]
    fn distant_changes_stay_separate_edits() {
        let old = "one\ntwo\nthree\nfour\nfive\nsix\nseven\n";
        let new = "ONE\ntwo\nthree\nfour\nfive\nsix\nSEVEN\n";
        let edits = edits_between(old, new);
        assert_eq!(edits.len(), 2);
        assert_eq!(apply(old, &edits), new);
    }

    #[test]
    fn changes_that_do_not_overlap_merge() {
        let base = "one\ntwo\nthree\nfour\nfive\n";
        let ours = "one\ntwo\nthree\nfour\nfive and mine\n";
        let theirs = "first\ntwo\nthree\nfour\nfive\n";
        assert_eq!(
            merge3(base, ours, theirs).unwrap(),
            "first\ntwo\nthree\nfour\nfive and mine\n"
        );
    }

    #[test]
    fn the_same_change_on_both_sides_is_made_once() {
        let base = "one\ntwo\nthree\n";
        let theirs = "one\n2\nthree\n";
        let ours = "one\n2\nthree\nand more\n";
        assert_eq!(merge3(base, ours, theirs).unwrap(), ours);
    }

    #[test]
    fn overlapping_changes_keep_both_sides() {
        let marked = merge3("one\ntwo\n", "one\nmine\n", "one\ntheirs\n").unwrap_err();
        assert!(
            marked.contains("mine") && marked.contains("theirs"),
            "{marked}"
        );
    }
}
