//! How well a few typed letters answer to a name.
//!
//! The letters must appear in the name in order ("lu" in "Lumio", "nt" in
//! "New Tab"). Among names that qualify, a match is worth more when it is
//! the whole name, starts the name, starts a word, or runs unbroken, and
//! worth less the more of the name it leaves over. Case and accents of case
//! are ignored; nothing else is folded.

/// The whole name, typed.
const EXACT: u32 = 1_000;
/// The name starts with what was typed.
const PREFIX: u32 = 600;
/// What was typed is in the name unbroken.
const UNBROKEN: u32 = 350;
/// The letters are there in order, but apart.
const SCATTERED: u32 = 100;
/// For each letter that starts a word of the name.
const WORD_START: u32 = 40;
/// For each letter that follows the one before it directly.
const ADJACENT: u32 = 12;
/// The most that leftover length can take away.
const LENGTH_CAP: u32 = 60;

/// How well `query` answers to `name`, or `None` when its letters are not
/// all there in order. An empty query matches everything, at nothing.
pub fn score(query: &str, name: &str) -> Option<u32> {
    let query: Vec<char> = fold(query).filter(|c| !c.is_whitespace()).collect();
    if query.is_empty() {
        return Some(0);
    }
    let name: Vec<char> = fold(name).collect();
    let spaceless: Vec<char> = name
        .iter()
        .copied()
        .filter(|c| !c.is_whitespace())
        .collect();

    let base = if spaceless == query {
        EXACT
    } else if spaceless.starts_with(&query) {
        PREFIX
    } else if contains(&spaceless, &query) {
        UNBROKEN
    } else {
        SCATTERED
    };

    // Greedy from the left, preferring a word's first letter when the next
    // letter could land on one: "nt" takes the N and the T of "New Tab".
    let mut bonus = 0;
    let mut at = 0;
    let mut last: Option<usize> = None;
    for &letter in &query {
        let rest = &name[at..];
        let word_start = rest
            .iter()
            .enumerate()
            .position(|(offset, &c)| c == letter && starts_word(&name, at + offset));
        let any = rest.iter().position(|&c| c == letter);
        let found = match (word_start, any) {
            // A word's start further on is worth taking only if the nearer
            // letter is not directly after the last one.
            (Some(word), Some(near)) if last.is_some_and(|last| at + near == last + 1) => {
                let _ = word;
                near
            }
            (Some(word), _) => word,
            (None, Some(near)) => near,
            (None, None) => return None,
        };
        let index = at + found;
        if starts_word(&name, index) {
            bonus += WORD_START;
        }
        if last.is_some_and(|last| index == last + 1) {
            bonus += ADJACENT;
        }
        last = Some(index);
        at = index + 1;
    }

    let leftover = (spaceless.len() - query.len()) as u32;
    Some(base + bonus - leftover.min(LENGTH_CAP))
}

/// The best of `score` over a name and the other words it answers to. A
/// keyword's match counts for a little less than the name's own.
pub fn best<'a>(
    query: &str,
    name: &str,
    keywords: impl IntoIterator<Item = &'a str>,
) -> Option<u32> {
    let named = score(query, name);
    let other = keywords
        .into_iter()
        .filter_map(|keyword| score(query, keyword))
        .max()
        .map(|score| score.saturating_sub(SCATTERED / 2));
    named.max(other)
}

fn fold(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().flat_map(char::to_lowercase)
}

fn contains(haystack: &[char], needle: &[char]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Whether the letter at `index` starts a word: the first letter, or one
/// after a space or a mark that separates words in names and paths.
fn starts_word(name: &[char], index: usize) -> bool {
    index == 0 || matches!(name[index - 1], ' ' | '-' | '_' | '.' | '/' | '(' | ':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_must_be_there_in_order() {
        assert!(score("lu", "Lumio Dev").is_some());
        assert!(score("ul", "Lumio Dev").is_none());
        assert!(score("xyz", "Lumio Dev").is_none());
    }

    #[test]
    fn nothing_typed_matches_everything_at_nothing() {
        assert_eq!(score("", "Anything"), Some(0));
        assert_eq!(score("   ", "Anything"), Some(0));
    }

    #[test]
    fn the_whole_name_beats_its_start_beats_its_middle_beats_scattered() {
        let exact = score("flux", "Flux").unwrap();
        let prefix = score("flux", "Flux Capacitor").unwrap();
        let middle = score("flux", "Reflux Notes").unwrap();
        let scattered = score("flux", "File Utility Export").unwrap();
        assert!(exact > prefix, "{exact} > {prefix}");
        assert!(prefix > middle, "{prefix} > {middle}");
        assert!(middle > scattered, "{middle} > {scattered}");
    }

    #[test]
    fn initials_find_a_command() {
        let initials = score("nt", "New Tab").unwrap();
        let buried = score("nt", "Console Settings").unwrap();
        assert!(initials > buried, "{initials} > {buried}");
    }

    #[test]
    fn a_shorter_name_wins_between_two_that_start_the_same() {
        let short = score("lu", "Lumio").unwrap();
        let long = score("lu", "Lumio Development Preview").unwrap();
        assert!(short > long, "{short} > {long}");
    }

    #[test]
    fn case_and_spaces_in_what_was_typed_do_not_matter() {
        assert_eq!(score("NEW TAB", "New Tab"), score("newtab", "New Tab"));
        assert_eq!(
            score("new tab", "New Tab"),
            Some(EXACT + 2 * WORD_START + 4 * ADJACENT)
        );
    }

    #[test]
    fn a_keyword_matches_for_a_little_less_than_the_name() {
        let named = best("term", "Terminal", []).unwrap();
        let aliased = best("term", "iTerm", ["Terminal"]).unwrap();
        // "iTerm" itself holds "term" unbroken; the keyword starts with it.
        assert!(aliased >= score("term", "iTerm").unwrap());
        assert!(named > score("term", "iTerm").unwrap());
        assert!(best("shell", "Terminal", ["shell", "console"]).is_some());
        assert!(best("shell", "Terminal", []).is_none());
    }

    #[test]
    fn a_path_matches_at_its_parts() {
        let part = score("snap", "notes/transaction-snapshots.md").unwrap();
        let buried = score("snap", "notes/disnapped.md").unwrap();
        assert!(part > buried, "{part} > {buried}");
    }
}
