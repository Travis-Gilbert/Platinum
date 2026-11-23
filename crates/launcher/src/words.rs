//! Reading what was typed, and saying times and sizes in words.

/// Whether what was typed reads as something to ask rather than a name to
/// look up: it ends in a question mark, or opens with a question word and
/// goes on for a few words. A wrong guess costs nothing but the order of the
/// rows: asking is still its own row, and looking up still happens.
pub fn reads_as_question(text: &str) -> bool {
    let text = text.trim();
    if text.ends_with('?') {
        return true;
    }
    let mut words = text.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    let opener = matches!(
        first.to_lowercase().as_str(),
        "who"
            | "what"
            | "when"
            | "where"
            | "why"
            | "how"
            | "which"
            | "explain"
            | "summarize"
            | "summarise"
            | "compare"
            | "draft"
            | "write"
            | "rewrite"
            | "translate"
    );
    opener && words.count() >= 2
}

/// What was typed, as an address to open, when it is one: it has a scheme,
/// or it is a host name with a dot and no spaces.
pub fn as_address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        return Some(text.to_owned());
    }
    let host = text.split(['/', '?', '#']).next().unwrap_or(text);
    let (name, top) = host.rsplit_once('.')?;
    let named = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    let top_level = top.len() >= 2 && top.chars().all(|c| c.is_ascii_alphabetic());
    (named && top_level && !name.starts_with('.') && !name.ends_with('.'))
        .then(|| format!("https://{text}"))
}

/// How long ago `then_ms` was, counted from `now_ms`, in the words a person
/// would use. Both are milliseconds since the same start.
pub fn ago(now_ms: i64, then_ms: i64) -> String {
    let seconds = (now_ms - then_ms).max(0) / 1_000;
    let (count, unit) = match seconds {
        0..60 => return "just now".to_owned(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        86_400..604_800 => (seconds / 86_400, "day"),
        604_800..2_629_800 => (seconds / 604_800, "week"),
        2_629_800..31_557_600 => (seconds / 2_629_800, "month"),
        _ => (seconds / 31_557_600, "year"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

/// A size in bytes, in the unit that keeps it short.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1_000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1_000.0;
    let mut unit = 0;
    while value >= 1_000.0 && unit + 1 < UNITS.len() {
        value /= 1_000.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_mark_or_a_question_word_and_a_few_more_reads_as_asking() {
        assert!(reads_as_question("is this right?"));
        assert!(reads_as_question("where did we discuss database rollback"));
        assert!(reads_as_question("Compare these two documents"));
        assert!(!reads_as_question("what"));
        assert!(!reads_as_question("how to"));
        assert!(!reads_as_question("Lumio Dev"));
        assert!(!reads_as_question(""));
    }

    #[test]
    fn a_host_or_a_schemed_address_is_an_address() {
        assert_eq!(
            as_address("example.com").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            as_address("docs.rs/gpui?x=1").as_deref(),
            Some("https://docs.rs/gpui?x=1")
        );
        assert_eq!(
            as_address("http://localhost:3000").as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(as_address("notes.md is here"), None);
        assert_eq!(as_address("lumio"), None);
        assert_eq!(as_address("v1.2"), None);
        assert_eq!(as_address(".com"), None);
        assert_eq!(as_address(""), None);
    }

    #[test]
    fn ages_are_said_in_the_largest_whole_unit() {
        let now = 10_000_000_000;
        assert_eq!(ago(now, now - 5_000), "just now");
        assert_eq!(ago(now, now - 60_000), "1 minute ago");
        assert_eq!(ago(now, now - 3 * 3_600_000), "3 hours ago");
        assert_eq!(ago(now, now - 2 * 86_400_000), "2 days ago");
        assert_eq!(ago(now, now - 21 * 86_400_000), "3 weeks ago");
        // A time ahead of now, from a clock that disagrees, is not negative.
        assert_eq!(ago(now, now + 60_000), "just now");
    }

    #[test]
    fn sizes_are_said_in_the_unit_that_keeps_them_short() {
        assert_eq!(size(512), "512 bytes");
        assert_eq!(size(1_500), "1.5 KB");
        assert_eq!(size(250_000), "250 KB");
        assert_eq!(size(3_200_000_000), "3.2 GB");
    }
}
