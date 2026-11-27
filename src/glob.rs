// Ignore patterns use slash-separated paths and byte-oriented wildcards.
pub(crate) struct Rule {
    pattern: Pattern,
    basename_only: bool,
    directory_only: bool,
    ignore: bool,
}

impl Rule {
    pub(crate) fn parse(mut text: &str, exclude: bool) -> Result<Option<Self>, &'static str> {
        if !exclude && text.starts_with('#') {
            return Ok(None);
        }
        if !text.ends_with("\\ ") {
            text = text.trim_end();
        }
        if text.is_empty() && !exclude {
            return Ok(None);
        }
        let ignore = exclude || !text.starts_with('!');
        if !ignore {
            text = &text[1..];
        }
        let anchored = text.starts_with('/');
        if anchored {
            text = &text[1..];
        }
        let directory_only = text.ends_with('/');
        if directory_only {
            text = text[..text.len() - 1]
                .strip_suffix('\\')
                .unwrap_or(&text[..text.len() - 1]);
        }
        let basename_only = !anchored && !text.contains('/');
        let pattern = Pattern::parse(text)?;
        Ok(Some(Self {
            pattern,
            basename_only,
            directory_only,
            ignore,
        }))
    }

    pub(crate) fn matched(&self, path: &[u8], name: &[u8], is_dir: bool) -> Option<bool> {
        if self.directory_only && !is_dir {
            return None;
        }
        let bytes = if self.basename_only { name } else { path };
        self.pattern.matches(bytes).then_some(self.ignore)
    }
}

enum Pattern {
    Literal(Vec<u8>),
    Wildcard(Vec<Token>),
}

impl Pattern {
    fn parse(text: &str) -> Result<Self, &'static str> {
        let mut parser = Glob {
            rest: text.as_bytes(),
        };
        let tokens = parser.sequence(false)?;
        if tokens.iter().all(|t| matches!(t, Token::Byte(_))) {
            Ok(Self::Literal(
                tokens
                    .into_iter()
                    .filter_map(|t| {
                        if let Token::Byte(byte) = t {
                            Some(byte)
                        } else {
                            None
                        }
                    })
                    .collect(),
            ))
        } else {
            Ok(Self::Wildcard(tokens))
        }
    }

    fn matches(&self, bytes: &[u8]) -> bool {
        match self {
            Self::Literal(expected) => expected == bytes,
            Self::Wildcard(tokens) => {
                let mut current = vec![false; bytes.len() + 1];
                current[0] = true;
                match_tokens(tokens, bytes, current)[bytes.len()]
            }
        }
    }
}

enum Token {
    Byte(u8),
    Any,
    Star,
    RecursivePrefix,
    RecursiveSuffix,
    Class {
        negated: bool,
        ranges: Vec<(u8, u8)>,
    },
    Alternatives(Vec<Vec<Self>>),
}

struct Glob<'a> {
    rest: &'a [u8],
}

impl Glob<'_> {
    fn take(&mut self) -> Option<u8> {
        let (&first, rest) = self.rest.split_first()?;
        self.rest = rest;
        Some(first)
    }

    fn sequence(&mut self, in_group: bool) -> Result<Vec<Token>, &'static str> {
        let mut tokens = Vec::new();
        let mut component_start = true;
        while let Some(&byte) = self.rest.first() {
            if in_group && matches!(byte, b',' | b'}') {
                break;
            }
            self.take();
            let token = match byte {
                b'\\' => Token::Byte(self.take().ok_or("dangling '\\'")?),
                b'?' => Token::Any,
                b'*' => {
                    let mut stars = 1;
                    while self.rest.starts_with(b"*") {
                        self.take();
                        stars += 1;
                    }
                    if component_start && stars == 2 && self.rest.starts_with(b"/") {
                        self.take();
                        Token::RecursivePrefix
                    } else if component_start && stars == 2 && self.rest.is_empty() {
                        Token::RecursiveSuffix
                    } else {
                        Token::Star
                    }
                }
                b'[' => self.character_class()?,
                b'{' if !in_group => self.alternatives()?,
                b'{' => return Err("nested alternate groups are not allowed"),
                b'}' => return Err("unopened alternate group; missing '{'"),
                _ => Token::Byte(byte),
            };
            component_start = matches!(token, Token::Byte(b'/') | Token::RecursivePrefix);
            tokens.push(token);
        }
        Ok(tokens)
    }

    fn character_class(&mut self) -> Result<Token, &'static str> {
        let negated = self.rest.starts_with(b"!") || self.rest.starts_with(b"^");
        if negated {
            self.take();
        }
        let mut ranges = Vec::new();
        let mut first = true;
        while let Some(byte) = self.take() {
            if byte == b']' && !first {
                return Ok(Token::Class { negated, ranges });
            }
            first = false;
            if self.rest.starts_with(b"-") && self.rest.get(1).is_some_and(|&b| b != b']') {
                self.take();
                let high = self.take().unwrap();
                if high < byte {
                    return Err("invalid range in character class");
                }
                ranges.push((byte, high));
            } else {
                ranges.push((byte, byte));
            }
        }
        Err("unclosed character class; missing ']'")
    }

    fn alternatives(&mut self) -> Result<Token, &'static str> {
        let mut alternatives = Vec::new();
        loop {
            let tokens = self.sequence(true)?;
            if !tokens.is_empty() {
                alternatives.push(tokens);
            }
            match self.take() {
                Some(b',') => (),
                Some(b'}') => return Ok(Token::Alternatives(alternatives)),
                _ => return Err("unclosed alternate group; missing '}'"),
            }
        }
    }
}

fn match_tokens(tokens: &[Token], bytes: &[u8], mut current: Vec<bool>) -> Vec<bool> {
    let mut next = vec![false; current.len()];
    for token in tokens {
        next.fill(false);
        match token {
            Token::Star => {
                next[0] = current[0];
                for (i, &b) in bytes.iter().enumerate() {
                    next[i + 1] = current[i + 1] || (next[i] && b != b'/');
                }
            }
            Token::RecursivePrefix => {
                let mut seen_start = current[0];
                next[0] = seen_start;
                for (i, &b) in bytes.iter().enumerate() {
                    next[i + 1] = current[i + 1] || (seen_start && b == b'/');
                    seen_start |= current[i + 1];
                }
            }
            Token::RecursiveSuffix => {
                let mut seen_start = false;
                for (output, &input) in next.iter_mut().zip(&current) {
                    seen_start |= input;
                    *output = seen_start;
                }
            }
            Token::Alternatives(alternatives) => {
                for alternative in alternatives {
                    let matched = match_tokens(alternative, bytes, current.clone());
                    for (out, matched) in next.iter_mut().zip(matched) {
                        *out |= matched;
                    }
                }
                if alternatives.is_empty() {
                    next.copy_from_slice(&current);
                }
            }
            _ => {
                for (i, &b) in bytes.iter().enumerate().filter(|(i, _)| current[*i]) {
                    next[i + 1] = match token {
                        Token::Byte(byte) => b == *byte,
                        Token::Any => b != b'/',
                        Token::Class { negated, ranges } => {
                            ranges.iter().any(|&(low, high)| (low..=high).contains(&b)) != *negated
                        }
                        _ => unreachable!(),
                    };
                }
            }
        }
        std::mem::swap(&mut current, &mut next);
    }
    current
}
