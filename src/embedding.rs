use crate::count::{count, find, line_end, space, trim, trim_start};
use crate::json::{self, Value};
use crate::language::Lang;
use crate::stats::CodeStats;

const FENCE: &[u8] = b"```";

pub(crate) struct Embedded {
    pub(crate) end: usize,
    lang: Lang,
    stats: CodeStats,
    code: usize,
    comments: usize,
}

impl Embedded {
    pub(crate) fn add_to(self, stats: &mut CodeStats) {
        stats.counts.code += self.code;
        stats.counts.comments += self.comments;
        stats.blobs.entry(self.lang).or_default().merge(self.stats);
    }
}

pub(crate) enum LineContext<'a> {
    None,
    Rust,
    Markdown(Option<(usize, usize)>),
    LinguaFranca(Lang, Option<usize>),
    Html([Option<Tag<'a>>; 3]),
}

pub(crate) struct Tag<'a> {
    start: usize,
    end: usize,
    language: Option<&'a [u8]>,
}

impl<'a> LineContext<'a> {
    pub(crate) fn new(id: &str, line: &'a [u8], lf_target: Option<Lang>) -> Self {
        match id {
            "Rust" => Self::Rust,
            "Markdown" | "UnrealDeveloperMarkdown" => Self::Markdown(markdown_opening(line)),
            "LinguaFranca" => Self::LinguaFranca(lf_target.unwrap(), find(line, b"{=")),
            "Html" | "RubyHtml" | "Svelte" | "Vue" | "GlimmerJs" | "GlimmerTs" => Self::Html([
                tag(line, b"<script", b"type=\""),
                tag(line, b"<style", b"lang=\""),
                tag(line, b"<template", b"lang=\""),
            ]),
            _ => Self::None,
        }
    }

    pub(crate) fn at(&self, bytes: &[u8], line: usize, i: usize, doc: bool) -> Option<Embedded> {
        let offset = i - line;
        match self {
            Self::Rust => rust_doc(bytes, i, doc),
            Self::Markdown(Some((start, end))) if *start == offset => {
                let lang = trim(&bytes[i + 3..line + end])
                    .split(|&b| b == b',')
                    .find_map(|name| Lang::parse(&String::from_utf8_lossy(name)))?;
                let body = &bytes[line + end..];
                let (length, closing) = find(body, FENCE).map_or((body.len(), 0), |pos| {
                    let after = &body[pos + FENCE.len()..];
                    (pos, FENCE.len() + whitespace_width(after))
                });
                Some(Embedded {
                    end: line + end + length + closing,
                    lang,
                    stats: count(lang, trim(&body[..length]), doc),
                    code: 0,
                    comments: if closing == 0 { 1 } else { 2 },
                })
            }
            Self::LinguaFranca(lang, Some(start)) if *start == offset => {
                let body = &bytes[i + 2..];
                let length = find(body, b"=}").unwrap_or(body.len());
                Some(Embedded {
                    end: i + 2 + length,
                    lang: *lang,
                    stats: count(*lang, trim_outer_lines(&body[..length]), doc),
                    code: 0,
                    comments: 0,
                })
            }
            Self::Html(tags) => {
                let (kind, tag) = tags.iter().enumerate().find_map(|(kind, tag)| {
                    tag.as_ref()
                        .filter(|tag| tag.start == offset)
                        .map(|tag| (kind, tag))
                })?;
                let (terminator, default) = [
                    (b"</script>".as_slice(), "JavaScript"),
                    (b"</style>".as_slice(), "CSS"),
                    (b"</template>".as_slice(), "HTML"),
                ][kind];
                let body = &bytes[line + tag.end..];
                let length = find(body, terminator)?;
                let body = &body[..length];
                if trim(body).is_empty() {
                    return None;
                }

                let lang = tag
                    .language
                    .and_then(|s| {
                        let value = String::from_utf8_lossy(trim(s));
                        if kind == 0 {
                            Lang::mime(&value)
                        } else {
                            Lang::parse(&value)
                        }
                    })
                    .unwrap_or_else(|| Lang::named(default).unwrap());
                Some(Embedded {
                    end: line + tag.end + length,
                    lang,
                    stats: count(lang, trim_outer_lines(body), doc),
                    code: 1,
                    comments: 0,
                })
            }
            _ => None,
        }
    }
}

fn rust_doc(bytes: &[u8], mut start: usize, doc: bool) -> Option<Embedded> {
    let rest = trim_start(&bytes[start..]);
    let marker = [b"///", b"//!"]
        .into_iter()
        .find(|&marker| rest.starts_with(marker))?;
    let mut markdown = Vec::new();
    while start < bytes.len() && trim_start(&bytes[start..]).starts_with(marker) {
        let end = line_end(bytes, start);
        let line = trim_start(&bytes[start..end]);
        markdown.extend_from_slice(&line[marker.len().min(line.len())..]);
        start = end;
    }
    let lang = Lang::named("Markdown").unwrap();
    Some(Embedded {
        end: start,
        lang,
        stats: count(lang, trim(&markdown), doc),
        code: 0,
        comments: 0,
    })
}

fn markdown_opening(line: &[u8]) -> Option<(usize, usize)> {
    let mut offset = 0;
    while let Some(pos) = find(&line[offset..], FENCE) {
        let start = offset + pos;
        let name = start + FENCE.len();
        let mut end = name;
        while let Some((c, width)) = first_char(&line[end..]) {
            if c.is_whitespace() {
                if end > name {
                    return Some((start, end + width));
                }
                break;
            }
            end += width;
        }
        offset = start + 1;
    }
    None
}

fn first_char(bytes: &[u8]) -> Option<(char, usize)> {
    let first = *bytes.first()?;
    if first.is_ascii() {
        return Some((first as char, 1));
    }
    let width = first.leading_ones() as usize;
    let c = str::from_utf8(bytes.get(..width)?).ok()?.chars().next()?;
    Some((c, width))
}

fn whitespace_width(bytes: &[u8]) -> usize {
    first_char(bytes)
        .filter(|(c, _)| c.is_whitespace())
        .map_or(0, |(_, size)| size)
}

// Tag patterns take the first opening match and the last usable attribute.
fn tag<'a>(line: &'a [u8], opening: &[u8], attr: &[u8]) -> Option<Tag<'a>> {
    let start = find(line, opening)?;
    let rest = &line[start + opening.len()..];
    let last_end = rest.iter().rposition(|&b| b == b'>')?;
    let capture = rest[..last_end]
        .iter()
        .rposition(|&b| b == b'"')
        .and_then(|last_quote| {
            let attr_start = rest[..last_quote]
                .windows(attr.len())
                .rposition(|w| w == attr)?;
            Some((attr_start + attr.len(), last_quote))
        });
    let after = capture.map_or(0, |(_, end)| end + 1);
    let end = start + opening.len() + after + rest[after..].iter().position(|&b| b == b'>')? + 1;
    Some(Tag {
        start,
        end,
        language: capture.map(|(start, end)| &rest[start..end]),
    })
}

// Embedded HTML and Lingua Franca use the oracle's outer-line trimming.
fn trim_outer_lines(bytes: &[u8]) -> &[u8] {
    let boundary = |&b: &u8| b == b'\n' || !space(b);
    let last = bytes.len().saturating_sub(1);
    let start = bytes
        .iter()
        .position(boundary)
        .map_or(0, |i| (i + 1).min(last));
    let end = bytes.iter().rposition(boundary).map_or(last, |i| {
        if bytes[i] == b'\n' && i > 0 && bytes[i - 1] == b'\r' {
            i - 1
        } else {
            i
        }
    });
    bytes.get(start..end.saturating_add(1)).unwrap_or_default()
}

pub(crate) fn target_language(bytes: &[u8]) -> Option<Lang> {
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut offset = 0;
    while let Some(pos) = find(&bytes[offset..], b"target") {
        let start = offset + pos;
        offset = start + b"target".len();
        let rest = &bytes[offset..];
        if start > 0 && word(bytes[start - 1]) || !rest.first().is_some_and(|&b| space(b)) {
            continue;
        }
        let rest = trim_start(rest);
        let len = rest.iter().take_while(|&&b| word(b)).count();
        if len == 0 {
            continue;
        }
        // The declaration may end at a newline, semicolon, or opening brace.
        let tail = rest[len..].iter().find(|&&b| b == b'\n' || !space(b));
        if tail.is_none() || matches!(tail, Some(b'\n' | b';' | b'{')) {
            let name = str::from_utf8(&rest[..len]).ok()?;
            return Lang::named(if name == "CCpp" { "C" } else { name });
        }
    }
    None
}

pub(crate) fn notebook(bytes: &[u8], doc: bool) -> Option<CodeStats> {
    let value = json::parse(str::from_utf8(bytes).ok()?)?;
    let metadata = value.get("metadata")?;
    let kernel = metadata.get("kernelspec")?;
    let info = metadata.get("language_info")?;
    let code = kernel
        .get("language")
        .and_then(Value::string)
        .and_then(Lang::parse)
        .or_else(|| {
            info.get("file_extension")
                .and_then(Value::string)
                .and_then(Lang::extension)
        })
        .unwrap_or_else(|| Lang::named("Python").unwrap());
    let markdown = Lang::named("Markdown").unwrap();
    let mut notebook = CodeStats::default();
    for cell in value.get("cells")?.array()? {
        let lang = match cell.get("cell_type")?.string()? {
            "markdown" => markdown,
            "code" => code,
            _ => return None,
        };
        let mut text = String::new();
        for line in cell.get("source")?.array()? {
            text.push_str(line.string()?);
        }
        let stats = count(lang, text.as_bytes(), doc);
        notebook.blobs.entry(lang).or_default().merge(stats.clone());
        notebook.merge(stats);
    }
    Some(notebook)
}
