use std::borrow::Cow;

use crate::diagnostic;
use crate::embedding::{Embedded, LineContext, notebook, target_language};
use crate::language::{Definition, Lang};
use crate::stats::CodeStats;

pub(crate) fn decode(bytes: &[u8]) -> Cow<'_, [u8]> {
    if let Some(bytes) = bytes.strip_prefix(b"\xef\xbb\xbf") {
        return Cow::Borrowed(bytes);
    }
    let (bytes, little_endian) = match bytes {
        [0xff, 0xfe, rest @ ..] => (rest, true),
        [0xfe, 0xff, rest @ ..] => (rest, false),
        _ => return Cow::Borrowed(bytes),
    };
    let (words, remainder) = bytes.as_chunks::<2>();
    let odd_byte = !remainder.is_empty();
    let text: String = char::decode_utf16(words.iter().copied().map(|word| {
        if little_endian {
            u16::from_le_bytes(word)
        } else {
            u16::from_be_bytes(word)
        }
    }))
    .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
    .chain(odd_byte.then_some(char::REPLACEMENT_CHARACTER))
    .collect();
    Cow::Owned(text.into_bytes())
}

pub(crate) fn count(lang: Lang, bytes: &[u8], doc_strings_as_comments: bool) -> CodeStats {
    if lang.definition().id == "Jupyter" {
        return notebook(bytes, doc_strings_as_comments).unwrap_or_default();
    }
    let mut scanner = Scanner {
        definition: lang.definition(),
        quote: None,
        verbatim: false,
        comments: Vec::new(),
        doc_strings_as_comments,
        lf_target: (lang.definition().id == "LinguaFranca")
            .then(|| target_language(bytes).unwrap_or(lang)),
    };
    let mut stats = CodeStats::default();
    let trace = diagnostic::tracing();
    let mut start = 0;
    let trailing_space = bytes.iter().rposition(|&b| !space(b)).map_or(0, |i| i + 1);
    while start < bytes.len() {
        let end = line_end(bytes, start);
        let line = &bytes[start..end];
        let trimmed = trim(line);
        if trace {
            diagnostic::trace(
                "rlc::language::language_type",
                format_args!("{}", String::from_utf8_lossy(trimmed)),
            );
        }
        if scanner.plain() && trimmed.is_empty() {
            stats.counts.blanks += 1;
            if trace {
                diagnostic::trace(
                    "rlc::language::syntax",
                    format_args!("Blank No.{}", stats.counts.blanks),
                );
            }
        } else if scanner.plain() && !scanner.important(trimmed) {
            if trace {
                diagnostic::trace("rlc::language::syntax", format_args!("^ Skippable"));
            }
            let line = if matches!(scanner.definition.id, "FortranLegacy" | "FortranModern") {
                line
            } else {
                trimmed
            };
            if scanner.definition.literate || scanner.starts_line_comment(line) {
                stats.counts.comments += 1;
                if trace {
                    diagnostic::trace(
                        "rlc::language::syntax",
                        format_args!("Comment No.{}", stats.counts.comments),
                    );
                }
            } else {
                stats.counts.code += 1;
                if trace {
                    diagnostic::trace(
                        "rlc::language::syntax",
                        format_args!("Code No.{}", stats.counts.code),
                    );
                }
            }
        } else {
            let started_in_comment = !scanner.comments.is_empty()
                || (scanner.doc_strings_as_comments && scanner.in_doc_quote());
            if let Some(embedded) = scanner.scan_line(bytes, start, end, trailing_space) {
                start = embedded.end;
                embedded.add_to(&mut stats);
                continue;
            }

            if scanner.definition.literate || scanner.is_comment(trimmed, started_in_comment) {
                stats.counts.comments += 1;
                if trace {
                    diagnostic::trace(
                        "rlc::language::syntax",
                        format_args!("Comment No.{}", stats.counts.comments),
                    );
                }
            } else {
                stats.counts.code += 1;
                if trace {
                    diagnostic::trace(
                        "rlc::language::syntax",
                        format_args!("Code No.{}", stats.counts.code),
                    );
                }
            }
        }
        start = end;
    }
    stats
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QuoteKind {
    String,
    Verbatim,
    Doc,
}

struct Scanner {
    definition: &'static Definition,
    quote: Option<(&'static str, QuoteKind)>,
    verbatim: bool,
    comments: Vec<&'static str>,
    doc_strings_as_comments: bool,
    lf_target: Option<Lang>,
}

impl Scanner {
    fn plain(&self) -> bool {
        self.quote.is_none() && self.comments.is_empty()
    }

    fn in_doc_quote(&self) -> bool {
        self.quote.is_some_and(|(_, kind)| kind == QuoteKind::Doc)
    }

    fn starts_line_comment(&self, bytes: &[u8]) -> bool {
        self.definition
            .line_comments
            .iter()
            .any(|token| bytes.starts_with(token.as_bytes()))
    }

    fn important(&self, line: &[u8]) -> bool {
        let d = self.definition;
        d.quotes
            .iter()
            .chain(d.doc_quotes)
            .chain(d.block_comments)
            .chain(d.nested_comments)
            .map(|&(start, _)| start)
            .chain(d.important.iter().copied())
            .any(|token| find(line, token.as_bytes()).is_some())
    }

    fn scan_line(
        &mut self,
        bytes: &[u8],
        start: usize,
        end: usize,
        limit: usize,
    ) -> Option<Embedded> {
        let context = LineContext::new(self.definition.id, &bytes[start..end], self.lf_target);
        let mut index = start;
        while index < end.min(limit) {
            let rest = &bytes[index..];
            if let Some(skip) = self.end_delimiter(rest) {
                index += skip;
                continue;
            }
            if self.quote.is_none() {
                if self.comments.is_empty()
                    && let Some(embedded) =
                        context.at(bytes, start, index, self.doc_strings_as_comments)
                {
                    return Some(embedded);
                }
                if let Some(skip) = self.start_delimiter(rest) {
                    index += skip;
                    continue;
                }
                if self.comments.is_empty() && self.starts_line_comment(rest) {
                    break;
                }
            }
            index += 1;
        }
        None
    }

    fn end_delimiter(&mut self, rest: &[u8]) -> Option<usize> {
        if let Some((end, _)) = self.quote
            && rest.starts_with(end.as_bytes())
        {
            self.quote = None;
            return Some(end.len());
        }
        if !self.verbatim
            && let Some(escaped) = rest.strip_prefix(b"\\")
            && (escaped.starts_with(b"\\")
                || self
                    .definition
                    .quotes
                    .iter()
                    .any(|(q, _)| escaped.starts_with(q.as_bytes())))
        {
            return Some(2);
        }
        if let Some(&end) = self.comments.last()
            && rest.starts_with(end.as_bytes())
        {
            self.comments.pop();
            return Some(end.len());
        }
        None
    }

    fn start_delimiter(&mut self, rest: &[u8]) -> Option<usize> {
        let d = self.definition;
        if self.comments.is_empty() {
            for (delimiters, kind) in [
                (d.doc_quotes, QuoteKind::Doc),
                (d.verbatim_quotes, QuoteKind::Verbatim),
                (d.quotes, QuoteKind::String),
            ] {
                if let Some(&(start, end)) = delimiters
                    .iter()
                    .find(|(s, _)| rest.starts_with(s.as_bytes()))
                {
                    self.quote = Some((end, kind));
                    self.verbatim = kind == QuoteKind::Verbatim;
                    return Some(start.len());
                }
            }
        }
        for &(start, end) in d.block_comments.iter().chain(d.nested_comments) {
            if rest.starts_with(start.as_bytes()) {
                if self.comments.is_empty() || d.nested || d.nested_comments.contains(&(start, end))
                {
                    self.comments.push(end);
                }
                return Some(start.len());
            }
        }
        None
    }

    fn is_comment(&self, line: &[u8], started_in_comment: bool) -> bool {
        if self.quote.is_some() {
            return self.doc_strings_as_comments && self.in_doc_quote();
        }
        started_in_comment
            || self.starts_line_comment(line)
            || self
                .definition
                .block_comments
                .iter()
                .chain(self.definition.nested_comments)
                .any(|(start, end)| {
                    line.starts_with(start.as_bytes())
                        && (line.ends_with(end.as_bytes()) || self.comments.last() == Some(end))
                })
    }
}

pub(crate) fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    let (&first, _) = needle.split_first()?;
    let candidates = bytes.get(..=bytes.len().checked_sub(needle.len())?)?;
    candidates
        .iter()
        .enumerate()
        .find(|&(index, &byte)| byte == first && bytes[index..].starts_with(needle))
        .map(|(index, _)| index)
}

pub(crate) fn line_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |i| start + i + 1)
}

pub(crate) fn space(b: u8) -> bool {
    b == b' ' || (b'\t'..=b'\r').contains(&b)
}

pub(crate) fn trim_start(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|&b| !space(b)).unwrap_or(bytes.len());
    &bytes[start..]
}

pub(crate) fn trim(bytes: &[u8]) -> &[u8] {
    let bytes = trim_start(bytes);
    let end = bytes.iter().rposition(|&b| !space(b)).map_or(0, |i| i + 1);
    &bytes[..end]
}
