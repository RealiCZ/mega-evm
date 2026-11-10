//! A strict reader of JSON text (RFC 8259) for the chain configuration.
//!
//! The configuration's keys and values are read off the text itself, not off a general-purpose
//! deserializer's data model: a number keeps the digits it was written with, so an integer is read
//! exactly whatever its size (no floating-point step, and no dependence on a deserializer's
//! optional arbitrary-precision mode), and an object keeps every copy of a repeated key in the
//! order written, so the reader decides which copy counts. Only JSON values exist here: an object
//! that some deserializer hands a large number over as (`serde_json`'s
//! `{"$serde_json::private::Number": "<digits>"}`) is an object like any other.

use std::{string::String, vec::Vec};

/// The deepest nesting of arrays and objects the reader follows; deeper text is refused rather
/// than recursed into. A genesis file nests a handful of levels.
const MAX_DEPTH: usize = 128;

/// A JSON value: a number keeps its text, an object its members in the order written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Json<'a> {
    Null,
    Bool(bool),
    /// A number's text exactly as written; the grammar is checked.
    Number(&'a str),
    String(String),
    Array(Vec<Json<'a>>),
    /// An object's members in the order written: a key given twice appears twice.
    Object(Vec<(String, Json<'a>)>),
}

impl<'a> Json<'a> {
    /// What the value is, for an error message.
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "a boolean",
            Self::Number(_) => "a number",
            Self::String(_) => "a string",
            Self::Array(_) => "an array",
            Self::Object(_) => "an object",
        }
    }

    /// The members of an object, `None` for any other value.
    pub(crate) fn members(&self) -> Option<&[(String, Json<'a>)]> {
        match self {
            Self::Object(members) => Some(members),
            _ => None,
        }
    }

    /// The last copy of `key` in an object: a key given twice is read as its last copy.
    pub(crate) fn get(&self, key: &str) -> Option<&Json<'a>> {
        self.members()?.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The value as an unsigned integer of at most `u64::MAX`: a number written as digits alone
    /// (no sign, fraction or exponent) that fits. `None` for anything else, `-0`, `1.0` and `1e3`
    /// among them.
    pub(crate) fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(text) if text.bytes().all(|b| b.is_ascii_digit()) => text.parse().ok(),
            _ => None,
        }
    }
}

/// Text that is not one JSON value: where (a byte offset into the text) and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonError {
    /// The byte offset into the text where reading stopped.
    pub offset: usize,
    /// What was wrong there.
    pub message: &'static str,
}

impl core::fmt::Display for JsonError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

/// Reads `text`, which must be exactly one JSON value surrounded by optional whitespace.
pub(crate) fn parse(text: &str) -> Result<Json<'_>, JsonError> {
    let mut parser = Parser { text, bytes: text.as_bytes(), pos: 0, depth: 0 };
    parser.skip_whitespace();
    let value = parser.value()?;
    parser.skip_whitespace();
    if parser.pos != parser.bytes.len() {
        return Err(parser.error("trailing characters after the value"));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn error(&self, message: &'static str) -> JsonError {
        JsonError { offset: self.pos, message }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// Consumes `byte`, or fails with `message`.
    fn expect(&mut self, byte: u8, message: &'static str) -> Result<(), JsonError> {
        if self.peek() != Some(byte) {
            return Err(self.error(message));
        }
        self.pos += 1;
        Ok(())
    }

    fn value(&mut self) -> Result<Json<'a>, JsonError> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("expected a value")),
            None => Err(self.error("unexpected end of text")),
        }
    }

    fn literal(&mut self, word: &'static str, value: Json<'a>) -> Result<Json<'a>, JsonError> {
        if !self.bytes[self.pos..].starts_with(word.as_bytes()) {
            return Err(self.error("expected a value"));
        }
        self.pos += word.len();
        Ok(value)
    }

    fn enter(&mut self) -> Result<(), JsonError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        Ok(())
    }

    fn object(&mut self) -> Result<Json<'a>, JsonError> {
        self.enter()?;
        self.pos += 1;
        let mut members = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a string key"));
            }
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':', "expected ':' after a key")?;
            self.skip_whitespace();
            let value = self.value()?;
            members.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(self.error("expected ',' or '}' in an object")),
            }
        }
    }

    fn array(&mut self) -> Result<Json<'a>, JsonError> {
        self.enter()?;
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']' in an array")),
            }
        }
    }

    /// A string, its escapes decoded; the opening quote is at `pos`.
    fn string(&mut self) -> Result<String, JsonError> {
        self.pos += 1;
        let mut out = String::new();
        let mut run = self.pos;
        loop {
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    out.push_str(&self.text[run..self.pos]);
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    out.push_str(&self.text[run..self.pos]);
                    self.pos += 1;
                    self.escape(&mut out)?;
                    run = self.pos;
                }
                Some(0x00..=0x1f) => return Err(self.error("control character in a string")),
                // The text is UTF-8 and every delimiter is ASCII, so a run of other bytes is
                // copied whole, never split inside a character.
                Some(_) => self.pos += 1,
            }
        }
    }

    /// One escape after its backslash.
    fn escape(&mut self, out: &mut String) -> Result<(), JsonError> {
        let Some(byte) = self.peek() else { return Err(self.error("unterminated string")) };
        self.pos += 1;
        let c = match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let first = self.hex4()?;
                let code = match first {
                    0xd800..=0xdbff => {
                        if !self.bytes[self.pos..].starts_with(b"\\u") {
                            return Err(self.error("lone leading surrogate in an escape"));
                        }
                        self.pos += 2;
                        let second = self.hex4()?;
                        if !(0xdc00..=0xdfff).contains(&second) {
                            return Err(self.error("lone leading surrogate in an escape"));
                        }
                        0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)
                    }
                    0xdc00..=0xdfff => {
                        return Err(self.error("lone trailing surrogate in an escape"))
                    }
                    code => code,
                };
                char::from_u32(code).ok_or_else(|| self.error("invalid escape"))?
            }
            _ => {
                self.pos -= 1;
                return Err(self.error("invalid escape"));
            }
        };
        out.push(c);
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut code = 0;
        for _ in 0..4 {
            let digit = self
                .peek()
                .and_then(|b| (b as char).to_digit(16))
                .ok_or_else(|| self.error("expected four hex digits in an escape"))?;
            code = code * 16 + digit;
            self.pos += 1;
        }
        Ok(code)
    }

    /// A number's text: `-? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?`.
    fn number(&mut self) -> Result<Json<'a>, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.error("expected a digit")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("expected a digit after the decimal point"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("expected a digit in the exponent"));
            }
            self.digits();
        }
        Ok(Json::Number(&self.text[start..self.pos]))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_values_and_whitespace() {
        let v = parse(" {\"a\": [1, -2.5e+3, true, false, null, \"x\"], \"b\": {}} ").unwrap();
        assert_eq!(
            v.get("a"),
            Some(&Json::Array(vec![
                Json::Number("1"),
                Json::Number("-2.5e+3"),
                Json::Bool(true),
                Json::Bool(false),
                Json::Null,
                Json::String("x".into()),
            ]))
        );
        assert_eq!(v.get("b"), Some(&Json::Object(vec![])));
        assert_eq!(parse("[]").unwrap(), Json::Array(vec![]));
    }

    #[test]
    fn test_a_repeated_key_is_kept_and_read_as_its_last_copy() {
        let v = parse(r#"{"k": 1, "k": 2}"#).unwrap();
        assert_eq!(v.members().unwrap().len(), 2);
        assert_eq!(v.get("k"), Some(&Json::Number("2")));
    }

    #[test]
    fn test_escapes_decode() {
        let v = parse(r#""a\"\\\/\b\f\n\r\t\u0041\u00e9\ud83d\ude00""#).unwrap();
        assert_eq!(v, Json::String("a\"\\/\u{8}\u{c}\n\r\tAé😀".into()));
        assert_eq!(parse("\"é😀\"").unwrap(), Json::String("é😀".into()));
    }

    #[test]
    fn test_integers_read_exactly() {
        let n = |t: &str| parse(t).unwrap().as_u64();
        assert_eq!(n("0"), Some(0));
        assert_eq!(n("18446744073709551615"), Some(u64::MAX));
        assert_eq!(n("18446744073709551616"), None);
        for t in ["-0", "-1", "1.0", "1e3", "1E3", "0.5"] {
            assert_eq!(n(t), None, "{t}");
        }
        assert_eq!(Json::Null.as_u64(), None);
        assert_eq!(Json::String("1".into()).as_u64(), None);
    }

    #[test]
    fn test_malformed_text_is_refused() {
        for (text, message) in [
            ("", "unexpected end of text"),
            ("01", "trailing characters after the value"),
            ("1.", "expected a digit after the decimal point"),
            ("1e", "expected a digit in the exponent"),
            ("-", "expected a digit"),
            ("+1", "expected a value"),
            ("{\"a\" 1}", "expected ':' after a key"),
            ("{\"a\": 1,}", "expected a string key"),
            ("{a: 1}", "expected a string key"),
            ("[1 2]", "expected ',' or ']' in an array"),
            ("{\"a\": 1 \"b\": 2}", "expected ',' or '}' in an object"),
            ("\"abc", "unterminated string"),
            ("\"a\u{1}\"", "control character in a string"),
            ("\"\\x\"", "invalid escape"),
            ("\"\\u12\"", "expected four hex digits in an escape"),
            ("\"\\ud800\"", "lone leading surrogate in an escape"),
            ("\"\\ud800\\u0041\"", "lone leading surrogate in an escape"),
            ("\"\\udc00\"", "lone trailing surrogate in an escape"),
            ("tru", "expected a value"),
            ("NaN", "expected a value"),
            ("{} {}", "trailing characters after the value"),
        ] {
            assert_eq!(parse(text).map(|_| ()).unwrap_err().message, message, "{text:?}");
        }
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert_eq!(parse(&deep).unwrap_err().message, "nesting too deep");
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(&ok).is_ok());
    }
}
