//! A small JSON reader for probe inputs ([`crate::probe`]), so that this crate needs no JSON
//! library: objects, arrays, strings (with escapes), `true`, `false`, `null` and non-negative
//! integers, kept as their digits so that a value above `u64` reaches its parser whole. Fractions,
//! exponents and negative numbers are refused; no probe field takes one.

/// A parsed JSON value. Object members keep their order; a repeated key is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    /// A non-negative integer, as written.
    Int(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// What the value is, for error messages.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Json::Null => "null",
            Json::Bool(_) => "a boolean",
            Json::Int(_) => "an integer",
            Json::Str(_) => "a string",
            Json::Arr(_) => "a list",
            Json::Obj(_) => "an object",
        }
    }
}

/// Parses `text` as one JSON value (surrounding whitespace allowed).
pub(crate) fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing characters after the value"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

/// How deep values may nest (a probe nests four levels at most).
const MAX_DEPTH: usize = 64;

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        format!("JSON at byte {}: {what}", self.i)
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\r' | b'\n') {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected `{}`", c as char)))
        }
    }

    fn word(&mut self, w: &str, v: Json) -> Result<Json, String> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            Err(self.err("unknown literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err(self.err("nested too deeply"));
        }
        match self.peek() {
            None => Err(self.err("expected a value")),
            Some(b'{') => {
                self.i += 1;
                let mut members: Vec<(String, Json)> = Vec::new();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(members));
                }
                loop {
                    if self.peek() != Some(b'"') {
                        return Err(self.err("expected a member name"));
                    }
                    let key = self.string()?;
                    if members.iter().any(|(k, _)| *k == key) {
                        return Err(self.err(&format!("repeated member `{key}`")));
                    }
                    self.eat(b':')?;
                    let v = self.value(depth + 1)?;
                    members.push((key, v));
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(members));
                        }
                        _ => return Err(self.err("expected `,` or `}`")),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return Err(self.err("expected `,` or `]`")),
                    }
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'0'..=b'9') => {
                let start = self.i;
                while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                    self.i += 1;
                }
                if matches!(self.s.get(self.i), Some(b'.' | b'e' | b'E')) {
                    return Err(self.err("only integers are read"));
                }
                let digits = std::str::from_utf8(&self.s[start..self.i]).unwrap().to_owned();
                if digits.len() > 1 && digits.starts_with('0') {
                    return Err(self.err("an integer with a leading zero"));
                }
                Ok(Json::Int(digits))
            }
            Some(b'-') => Err(self.err("negative numbers are not read")),
            Some(_) => Err(self.err("expected a value")),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return Err(self.err("unterminated string"));
            };
            self.i += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return Err(self.err("unterminated escape"));
                    };
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex =
                                self.s.get(self.i..self.i + 4).ok_or(self.err("short \\u"))?;
                            let code = std::str::from_utf8(hex)
                                .ok()
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .ok_or(self.err("bad \\u escape"))?;
                            self.i += 4;
                            out.push(char::from_u32(code).ok_or(self.err("a surrogate \\u"))?);
                        }
                        _ => return Err(self.err("unknown escape")),
                    }
                }
                c if c < 0x20 => return Err(self.err("a control character in a string")),
                _ => {
                    // Copy one UTF-8 character whole.
                    let start = self.i - 1;
                    let len = match c {
                        0x00..=0x7f => 1,
                        0xc0..=0xdf => 2,
                        0xe0..=0xef => 3,
                        _ => 4,
                    };
                    let ch = self
                        .s
                        .get(start..start + len)
                        .and_then(|b| std::str::from_utf8(b).ok())
                        .ok_or(self.err("invalid UTF-8"))?;
                    out.push_str(ch);
                    self.i = start + len;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reads_the_values_a_probe_writes() {
        let v = parse(r#" {"a": [1, "x\"yA", true, null], "b": {}, "c": 18446744073709551616} "#)
            .unwrap();
        assert_eq!(
            v,
            Json::Obj(vec![
                (
                    "a".into(),
                    Json::Arr(vec![
                        Json::Int("1".into()),
                        Json::Str("x\"yA".into()),
                        Json::Bool(true),
                        Json::Null
                    ])
                ),
                ("b".into(), Json::Obj(vec![])),
                ("c".into(), Json::Int("18446744073709551616".into())),
            ])
        );
        assert_eq!(parse("\"é\"").unwrap(), Json::Str("é".into()));
    }

    #[test]
    fn test_refuses_what_it_does_not_read() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":1,}",
            "{\"a\":1 \"b\":2}",
            "1.5",
            "-1",
            "1e3",
            "01",
            "tru",
            "\"a",
            "{\"a\":1,\"a\":2}",
            "[1] 2",
            "{a:1}",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} parsed");
        }
        let deep = "[".repeat(100) + &"]".repeat(100);
        assert!(parse(&deep).is_err());
    }
}
