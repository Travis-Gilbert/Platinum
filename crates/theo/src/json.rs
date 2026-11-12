//! JSON that keeps the order its objects were written in.
//!
//! A program's `set` is read key by key, and each key may draw from the
//! seeded generator, so the order of keys is part of what a program means.
//! Numbers are read with the standard library's correctly rounded parser, so
//! a number written by JavaScript is read back as the same number; that too
//! is part of replaying a program as the web plays it. `serde_json` gives
//! each of these only behind a feature that would switch on for the whole
//! workspace, so this small reader stands in for it and the crate depends on
//! nothing.
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut reader = Reader {
            bytes: text.as_bytes(),
            at: 0,
            depth: 0,
        };
        let value = reader.value()?;
        reader.space();
        if reader.at != reader.bytes.len() {
            return Err(reader.wrong("something after the value"));
        }
        Ok(value)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Json::Num(value) => Some(*value),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Json::Str(value) => Some(value),
            _ => None,
        }
    }

    pub fn bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn list(&self) -> Option<&[Json]> {
        match self {
            Json::List(items) => Some(items),
            _ => None,
        }
    }

    pub fn obj(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Obj(entries) => Some(entries),
            _ => None,
        }
    }

    /// A number at a key, if there is one.
    pub fn num_at(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Json::num)
    }

    pub fn str_at(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Json::str)
    }
}

impl From<f64> for Json {
    fn from(value: f64) -> Self {
        Json::Num(value)
    }
}

impl From<&str> for Json {
    fn from(value: &str) -> Self {
        Json::Str(value.to_owned())
    }
}

/// How deep a value may nest. A program is refused long before this; it is here so no text can exhaust the stack.
const MAX_DEPTH: usize = 128;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: usize,
}

impl Reader<'_> {
    fn wrong(&self, what: &str) -> String {
        format!("Not JSON: {what} at byte {}.", self.at)
    }

    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.at += 1;
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.wrong("an unknown word"))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.space();
        match self.bytes.get(self.at) {
            None => Err(self.wrong("the end")),
            Some(b'n') => self.word("null", Json::Null),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => self.nested(Self::list),
            Some(b'{') => self.nested(Self::object),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.wrong("an unexpected character")),
        }
    }

    fn nested(&mut self, read: fn(&mut Self) -> Result<Json, String>) -> Result<Json, String> {
        if self.depth >= MAX_DEPTH {
            return Err(self.wrong("nesting too deep"));
        }
        self.depth += 1;
        self.at += 1;
        let value = read(self);
        self.depth -= 1;
        value
    }

    /// After an item: `true` for a comma, `false` for the closing character.
    fn more(&mut self, close: u8) -> Result<bool, String> {
        self.space();
        match self.bytes.get(self.at) {
            Some(b',') => {
                self.at += 1;
                Ok(true)
            }
            Some(byte) if *byte == close => {
                self.at += 1;
                Ok(false)
            }
            _ => Err(self.wrong("no comma or close")),
        }
    }

    fn list(&mut self) -> Result<Json, String> {
        let mut items = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Json::List(items));
        }
        loop {
            items.push(self.value()?);
            if !self.more(b']')? {
                return Ok(Json::List(items));
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        let mut entries: Vec<(String, Json)> = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Json::Obj(entries));
        }
        loop {
            self.space();
            if self.bytes.get(self.at) != Some(&b'"') {
                return Err(self.wrong("no key"));
            }
            let key = self.string()?;
            self.space();
            if self.bytes.get(self.at) != Some(&b':') {
                return Err(self.wrong("no colon"));
            }
            self.at += 1;
            let value = self.value()?;
            // A key written twice keeps its first place and its last value, as JSON.parse does.
            match entries.iter_mut().find(|(name, _)| *name == key) {
                Some(entry) => entry.1 = value,
                None => entries.push((key, value)),
            }
            if !self.more(b'}')? {
                return Ok(Json::Obj(entries));
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        let digits = |reader: &mut Self| {
            let from = reader.at;
            while matches!(reader.bytes.get(reader.at), Some(b'0'..=b'9')) {
                reader.at += 1;
            }
            reader.at > from
        };
        if self.bytes.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        let whole = self.at;
        if !digits(self) || (self.bytes[whole] == b'0' && self.at > whole + 1) {
            return Err(self.wrong("a bad number"));
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            if !digits(self) {
                return Err(self.wrong("a bad number"));
            }
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !digits(self) {
                return Err(self.wrong("a bad number"));
            }
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .map(Json::Num)
            .ok_or_else(|| self.wrong("a bad number"))
    }

    fn hex4(&mut self) -> Result<u16, String> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u16::from_str_radix(digits, 16).ok())
            .ok_or_else(|| self.wrong("a bad escape"))?;
        self.at += 4;
        Ok(digits)
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut text = String::new();
        loop {
            let start = self.at;
            while !matches!(
                self.bytes.get(self.at),
                None | Some(b'"' | b'\\' | 0..=0x1f)
            ) {
                self.at += 1;
            }
            // The text was a `str`, and a run ends only at an ASCII byte, so each run is whole characters.
            text.push_str(
                std::str::from_utf8(&self.bytes[start..self.at])
                    .map_err(|_| self.wrong("bad text"))?,
            );
            match self.bytes.get(self.at) {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(text);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = *self
                        .bytes
                        .get(self.at)
                        .ok_or_else(|| self.wrong("the end"))?;
                    self.at += 1;
                    match escape {
                        b'"' => text.push('"'),
                        b'\\' => text.push('\\'),
                        b'/' => text.push('/'),
                        b'b' => text.push('\u{8}'),
                        b'f' => text.push('\u{c}'),
                        b'n' => text.push('\n'),
                        b'r' => text.push('\r'),
                        b't' => text.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let unit = if (0xd800..0xdc00).contains(&first)
                                && self.bytes[self.at..].starts_with(b"\\u")
                            {
                                self.at += 2;
                                let second = self.hex4()?;
                                char::decode_utf16([first, second]).next()
                            } else {
                                char::decode_utf16([first]).next()
                            };
                            // A lone surrogate has no character; it reads as the replacement one.
                            text.push(unit.and_then(Result::ok).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(self.wrong("a bad escape")),
                    }
                }
                _ => return Err(self.wrong("an unfinished string")),
            }
        }
    }
}

/// A number as JavaScript writes it, for messages that quote one.
pub fn js_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let size = value.abs();
    if size >= 1e21 || size < 1e-6 {
        let written = format!("{value:e}");
        return match written.split_once('e') {
            Some((digits, power)) if !power.starts_with('-') => format!("{digits}e+{power}"),
            _ => written,
        };
    }
    format!("{value}")
}

/// Text as JSON writes it, quotes and all.
pub fn quote(text: &str) -> String {
    Json::Str(text.to_owned()).to_string()
}

fn write_string(text: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("\"")?;
    for character in text.chars() {
        match character {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            other if (other as u32) < 0x20 => write!(f, "\\u{:04x}", other as u32)?,
            other => write!(f, "{other}")?,
        }
    }
    f.write_str("\"")
}

/// Writes the value as JSON on one line. A number that is not finite is written `null`, as JavaScript writes it.
impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(value) => write!(f, "{value}"),
            Json::Num(value) if !value.is_finite() => f.write_str("null"),
            Json::Num(value) => f.write_str(&js_number(*value)),
            Json::Str(value) => write_string(value, f),
            Json::List(items) => {
                f.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Json::Obj(entries) => {
                f.write_str("{")?;
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        f.write_str(",")?;
                    }
                    write_string(key, f)?;
                    write!(f, ":{value}")?;
                }
                f.write_str("}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Json;

    #[test]
    fn keeps_the_order_and_the_numbers() {
        let text = r#"{"b":1,"a":[0.981050967471674,-2.5e-3,true,null],"b":"x\u00e9\n"}"#;
        let value = Json::parse(text).unwrap();
        let keys: Vec<&str> = value
            .obj()
            .unwrap()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(keys, ["b", "a"]);
        assert_eq!(value.str_at("b"), Some("x\u{e9}\n"));
        assert_eq!(
            value.get("a").unwrap().list().unwrap()[0].num(),
            Some(0.981050967471674)
        );
        assert_eq!(Json::parse(&value.to_string()).unwrap(), value);
    }

    #[test]
    fn refuses_what_is_not_json() {
        for text in [
            "",
            "{",
            "[1,]",
            "01",
            "1.",
            "\"a",
            "{\"a\" 1}",
            "nul",
            "1 2",
            "{a:1}",
        ] {
            assert!(Json::parse(text).is_err(), "{text:?}");
        }
        assert!(Json::parse(&"[".repeat(500)).is_err());
    }
}
