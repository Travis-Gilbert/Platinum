//! Frontmatter: YAML properties, typed as Obsidian types them.
use crate::ids::is_note_id;
use crate::model::{Frontmatter, PropertyType};
use indexmap::IndexMap;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]{4}-[01][0-9]-[0-3][0-9]$").unwrap());
static DATE_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9]{4}-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]$").unwrap()
});

/// The YAML between the `---` lines of the front matter comrak returns.
pub(crate) fn body(block: &str) -> &str {
    let block = block.trim_end_matches(['\r', '\n']);
    let Some((_, rest)) = block.split_once('\n') else {
        return "";
    };
    rest.rsplit_once('\n').map_or("", |(yaml, _)| yaml)
}

/// The properties in `yaml`, in file order; an error says why it is not a map.
pub(crate) fn parse_yaml(yaml: &str) -> Result<IndexMap<String, Value>, String> {
    if yaml
        .lines()
        .all(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
    {
        return Ok(IndexMap::new());
    }
    // Obsidian reads YAML 1.2, where `y`, `yes` and `on` are strings.
    let options = serde_saphyr::options! { strict_booleans: true };
    serde_saphyr::from_str_with_options(yaml, options).map_err(|e| e.to_string())
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A list property, or a string split where `sep` says; blanks dropped.
fn list(value: Option<&Value>, sep: fn(char) -> bool) -> Vec<String> {
    let items: Vec<String> = match value {
        Some(Value::Array(items)) => items.iter().filter_map(scalar).collect(),
        Some(Value::String(s)) => s.split(sep).map(str::to_string).collect(),
        Some(other) => scalar(other).into_iter().collect(),
        None => Vec::new(),
    };
    items
        .into_iter()
        .map(|i| i.trim().to_string())
        .filter(|i| !i.is_empty())
        .collect()
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.properties.get(key)
    }

    /// Written strings in the case-insensitive `tags` property. A scalar is one tag,
    /// never a comma/space-separated list; entries containing spaces are not tags.
    pub fn tags(&self) -> Vec<String> {
        let value = self
            .properties
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("tags"))
            .map(|(_, value)| value);
        let values: Vec<&str> = match value {
            Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
            Some(Value::String(s)) => vec![s],
            _ => Vec::new(),
        };
        values
            .into_iter()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.contains(' '))
            .map(|s| s.strip_prefix('#').unwrap_or(s).to_string())
            .collect()
    }

    /// `aliases`, and the older `alias`.
    pub fn aliases(&self) -> Vec<String> {
        ["aliases", "alias"]
            .iter()
            .flat_map(|key| list(self.get(key), |c| c == ','))
            .collect()
    }

    pub fn css_classes(&self) -> Vec<String> {
        ["cssclasses", "cssclass"]
            .iter()
            .flat_map(|key| list(self.get(key), char::is_whitespace))
            .collect()
    }

    /// An id Theorem wrote into the note; any other `theorem-id` is not one.
    pub fn theorem_id(&self) -> Option<&str> {
        self.get("theorem-id")?.as_str().filter(|id| is_note_id(id))
    }
}

impl PropertyType {
    /// The type a property has when the vault has not declared one for its name.
    pub fn infer(key: &str, value: &Value) -> Self {
        match (key.to_lowercase().as_str(), value) {
            ("tags", _) => Self::Tags,
            ("aliases", _) => Self::Aliases,
            ("cssclasses", _) => Self::List,
            (_, Value::Bool(_)) => Self::Checkbox,
            (_, Value::Number(_)) => Self::Number,
            (_, Value::Array(items)) if items.iter().all(|v| v.is_null() || v.is_string()) => {
                Self::List
            }
            (_, Value::Object(_) | Value::Array(_)) => Self::Unknown,
            (_, Value::String(s)) if DATE_TIME.is_match(s) => Self::DateTime,
            (_, Value::String(s)) if DATE.is_match(s) => Self::Date,
            _ => Self::Text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offsets::Span;
    use serde_json::json;

    fn fm(yaml: &str) -> Frontmatter {
        Frontmatter {
            span: Span::default(),
            properties: parse_yaml(yaml).unwrap(),
            error: None,
        }
    }

    #[test]
    fn body_drops_the_delimiter_lines() {
        assert_eq!(body("---\na: 1\nb: 2\n---\n"), "a: 1\nb: 2");
        assert_eq!(body("---\r\na: 1\r\n---\r\n"), "a: 1\r");
        assert_eq!(body("---\n---\n"), "");
    }

    #[test]
    fn properties_keep_file_order_and_types() {
        let f = fm(
            "title: Plan\ncount: 3\nratio: 0.5\ndone: true\nrelated: [a, b]\ndue: 2026-01-02\nnothing:\n",
        );
        let keys: Vec<_> = f.properties.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "title", "count", "ratio", "done", "related", "due", "nothing"
            ]
        );
        assert_eq!(f.get("count"), Some(&json!(3)));
        assert_eq!(f.get("ratio"), Some(&json!(0.5)));
        assert_eq!(f.get("done"), Some(&json!(true)));
        assert_eq!(f.get("related"), Some(&json!(["a", "b"])));
        assert_eq!(f.get("due"), Some(&json!("2026-01-02")));
        assert_eq!(f.get("nothing"), Some(&Value::Null));
    }

    #[test]
    fn empty_and_comment_only_yaml_has_no_properties() {
        assert!(parse_yaml("").unwrap().is_empty());
        assert!(parse_yaml("# nothing\n\n").unwrap().is_empty());
    }

    #[test]
    fn yaml_that_is_not_a_map_is_an_error() {
        assert!(parse_yaml("- a\n- b").is_err());
        assert!(parse_yaml("just text").is_err());
        assert!(parse_yaml("a: [unclosed").is_err());
    }

    #[test]
    fn yaml_one_one_booleans_stay_strings() {
        let f = fm("tags: [y, n, yes, no, on, off, true]\nflag: yes");
        assert_eq!(f.get("flag"), Some(&json!("yes")));
        assert_eq!(f.tags(), ["y", "n", "yes", "no", "on", "off"]);
    }

    #[test]
    fn tags_come_from_a_list_or_a_string() {
        assert_eq!(fm("tags: [a, '#b/c', ünï]").tags(), ["a", "b/c", "ünï"]);
        assert!(fm("tags: a, b/c d").tags().is_empty());
        assert!(fm("tag: solo").tags().is_empty());
        assert_eq!(fm("tags: solo").tags(), ["solo"]);
        assert_eq!(fm("tags:\n  - x\n  - y").tags(), ["x", "y"]);
        assert!(fm("tags:").tags().is_empty());
    }

    #[test]
    fn aliases_split_at_commas_only() {
        assert_eq!(
            fm("aliases: [One, Two Words]").aliases(),
            ["One", "Two Words"]
        );
        assert_eq!(
            fm("aliases: One, Two Words").aliases(),
            ["One", "Two Words"]
        );
        assert_eq!(fm("alias: Solo").aliases(), ["Solo"]);
        assert_eq!(fm("aliases: [1984]").aliases(), ["1984"]);
    }

    #[test]
    fn css_classes_split_at_spaces() {
        assert_eq!(fm("cssclasses: wide dark").css_classes(), ["wide", "dark"]);
        assert_eq!(fm("cssclasses: [wide]").css_classes(), ["wide"]);
    }

    #[test]
    fn only_theorems_own_ids_are_honoured() {
        assert_eq!(
            fm("theorem-id: vf_01J9ZK3Q4M5N6P7R8S9T0V1W2X").theorem_id(),
            Some("vf_01J9ZK3Q4M5N6P7R8S9T0V1W2X")
        );
        assert_eq!(fm("theorem-id: abc").theorem_id(), None);
        assert_eq!(
            fm("theorem-id: vf_01J9ZK3Q4M5N6P7R8S9T0V1W2U").theorem_id(),
            None
        );
    }

    #[test]
    fn types_are_inferred_from_the_value_unless_the_name_fixes_them() {
        let t = |key: &str, v: Value| PropertyType::infer(key, &v);
        assert_eq!(t("x", json!("hi")), PropertyType::Text);
        assert_eq!(t("x", json!(3)), PropertyType::Number);
        assert_eq!(t("x", json!(true)), PropertyType::Checkbox);
        assert_eq!(t("x", json!(["a"])), PropertyType::List);
        assert_eq!(t("x", json!("2026-01-02")), PropertyType::Date);
        assert_eq!(t("x", json!("2026-01-02T10:30")), PropertyType::DateTime);
        assert_eq!(t("x", json!("2026-01-02 10:30:00")), PropertyType::Text);
        assert_eq!(t("x", json!(null)), PropertyType::Text);
        assert_eq!(t("tags", json!("a")), PropertyType::Tags);
        assert_eq!(t("aliases", json!("a")), PropertyType::Aliases);
        assert_eq!(t("cssclasses", json!("a")), PropertyType::List);
    }
}
