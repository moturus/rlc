use std::collections::BTreeMap;
use std::fmt::Write as _;

pub(crate) enum Value {
    Null,
    Bool,
    Number(String),
    String(String),
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl Value {
    pub(crate) fn get(&self, key: &str) -> Option<&Self> {
        self.object()?.get(key)
    }

    pub(crate) fn object(&self) -> Option<&BTreeMap<String, Self>> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn string(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn unsigned(&self) -> Option<usize> {
        match self {
            Self::Number(text) => text.parse().ok(),
            _ => None,
        }
    }
}

pub(crate) fn parse(input: &str) -> Option<Value> {
    let mut parser = Parser { rest: input };
    let value = parser.value(0)?;
    parser.space();
    parser.rest.is_empty().then_some(value)
}

struct Parser<'a> {
    rest: &'a str,
}

impl Parser<'_> {
    fn space(&mut self) {
        self.rest = self.rest.trim_start_matches([' ', '\r', '\n', '\t']);
    }

    fn take(&mut self, prefix: &str) -> Option<()> {
        self.rest = self.rest.strip_prefix(prefix)?;
        Some(())
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth >= 128 {
            return None;
        }
        self.space();
        match *self.rest.as_bytes().first()? {
            b'{' => self.object(depth + 1).map(Value::Object),
            b'[' => self.array(depth + 1).map(Value::Array),
            b'"' => self.string().map(Value::String),
            b'n' => self.take("null").map(|()| Value::Null),
            b't' => self.take("true").map(|()| Value::Bool),
            b'f' => self.take("false").map(|()| Value::Bool),
            b'-' | b'0'..=b'9' => self.number().map(Value::Number),
            _ => None,
        }
    }

    fn object(&mut self, depth: usize) -> Option<BTreeMap<String, Value>> {
        self.take("{")?;
        self.space();
        let mut values = BTreeMap::new();
        if self.take("}").is_some() {
            return Some(values);
        }
        loop {
            self.space();
            let key = self.string()?;
            self.space();
            self.take(":")?;
            values.insert(key, self.value(depth)?);
            self.space();
            if self.take("}").is_some() {
                return Some(values);
            }
            self.take(",")?;
        }
    }

    fn array(&mut self, depth: usize) -> Option<Vec<Value>> {
        self.take("[")?;
        self.space();
        let mut values = Vec::new();
        if self.take("]").is_some() {
            return Some(values);
        }
        loop {
            values.push(self.value(depth)?);
            self.space();
            if self.take("]").is_some() {
                return Some(values);
            }
            self.take(",")?;
        }
    }

    fn string(&mut self) -> Option<String> {
        self.take("\"")?;
        let mut value = String::new();
        loop {
            let end = self
                .rest
                .bytes()
                .position(|b| b <= 31 || b == b'\\' || b == b'"')?;
            value.push_str(&self.rest[..end]);
            let byte = self.rest.as_bytes()[end];
            self.rest = &self.rest[end + 1..];
            match byte {
                b'"' => return Some(value),
                b'\\' => value.push(self.escape()?),
                _ => return None,
            }
        }
    }

    fn escape(&mut self) -> Option<char> {
        let byte = *self.rest.as_bytes().first()?;
        if !byte.is_ascii() {
            return None;
        }
        self.rest = &self.rest[1..];
        Some(match byte {
            b'"' | b'\\' | b'/' => byte.into(),
            b'b' => '\x08',
            b'f' => '\x0c',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let high = self.hex_quad()?;
                if (0xd800..=0xdbff).contains(&high) {
                    self.take("\\u")?;
                    let low = self.hex_quad()?;
                    if !(0xdc00..=0xdfff).contains(&low) {
                        return None;
                    }
                    char::from_u32(0x10000 + ((high - 0xd800) << 10) + low - 0xdc00)?
                } else {
                    char::from_u32(high)?
                }
            }
            _ => return None,
        })
    }

    fn hex_quad(&mut self) -> Option<u32> {
        let digits = self.rest.get(..4)?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.rest = &self.rest[4..];
        u32::from_str_radix(digits, 16).ok()
    }

    fn number(&mut self) -> Option<String> {
        let start = self.rest;
        self.take("-");
        if self.take("0").is_none() {
            self.digits()?;
        }
        if self.take(".").is_some() {
            self.digits()?;
        }
        if self.take("e").or_else(|| self.take("E")).is_some() {
            self.take("+").or_else(|| self.take("-"));
            self.digits()?;
        }
        Some(start[..start.len() - self.rest.len()].to_owned())
    }

    fn digits(&mut self) -> Option<()> {
        let count = self.rest.bytes().take_while(u8::is_ascii_digit).count();
        self.rest = &self.rest[count..];
        (count > 0).then_some(())
    }
}

pub(crate) fn quote(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0'..='\x1f' => write!(out, "\\u{:04x}", u32::from(c)).unwrap(),
            _ => out.push(c),
        }
    }
    out.push('"');
}
