use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::language::Lang;
use crate::walk::WalkOptions;

pub(crate) const HELP: &str = include_str!("cli-help.txt");
pub(crate) const VERSION: &str = "rlc 0.1.0 compiled with serialization support: json";
const USAGE: &str = "Usage: rlc [OPTIONS] [input]...";
const FOOTER: &str = "For more information, try '--help'.";

#[derive(Clone, Copy)]
pub(crate) enum Sort {
    Files,
    Lines,
    Blanks,
    Code,
    Comments,
}

impl Sort {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "files" => Some(Self::Files),
            "lines" => Some(Self::Lines),
            "blanks" => Some(Self::Blanks),
            "code" => Some(Self::Code),
            "comments" => Some(Self::Comments),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Streaming {
    Simple,
    Json,
}

#[derive(Default)]
pub(crate) enum Action {
    #[default]
    Count,
    Help,
    Version,
    Languages,
}

#[derive(Default)]
pub(crate) struct Args {
    pub(crate) action: Action,
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) walk: WalkOptions,
    pub(crate) columns: Option<usize>,
    pub(crate) list_files: bool,
    pub(crate) input: Option<String>,
    pub(crate) json: bool,
    pub(crate) streaming: Option<Streaming>,
    pub(crate) sort: Option<Sort>,
    pub(crate) reverse: bool,
    pub(crate) types: Option<Vec<Lang>>,
    pub(crate) compact: bool,
    pub(crate) separator: Option<char>,
    pub(crate) verbose: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Columns,
    Exclude,
    Files,
    Input,
    Hidden,
    Languages,
    NoIgnore,
    NoIgnoreParent,
    NoIgnoreDot,
    NoIgnoreVcs,
    Output,
    Streaming,
    Sort,
    Rsort,
    Types,
    Compact,
    NumFormat,
    Verbose,
    Help,
    Version,
}

struct OptionSpec(Key, Option<char>, &'static str, Option<&'static str>);

use Key::*;

const OPTIONS: &[OptionSpec] = &[
    OptionSpec(Columns, Some('c'), "columns", Some("columns")),
    OptionSpec(Exclude, Some('e'), "exclude", Some("exclude")),
    OptionSpec(Files, Some('f'), "files", None),
    OptionSpec(Input, Some('i'), "input", Some("file_input")),
    OptionSpec(Hidden, None, "hidden", None),
    OptionSpec(Languages, Some('l'), "languages", None),
    OptionSpec(NoIgnore, None, "no-ignore", None),
    OptionSpec(NoIgnoreParent, None, "no-ignore-parent", None),
    OptionSpec(NoIgnoreDot, None, "no-ignore-dot", None),
    OptionSpec(NoIgnoreVcs, None, "no-ignore-vcs", None),
    OptionSpec(Output, Some('o'), "output", Some("output")),
    OptionSpec(Streaming, None, "streaming", Some("streaming")),
    OptionSpec(Sort, Some('s'), "sort", Some("sort")),
    OptionSpec(Rsort, Some('r'), "rsort", Some("rsort")),
    OptionSpec(Types, Some('t'), "types", Some("types")),
    OptionSpec(Compact, Some('C'), "compact", None),
    OptionSpec(NumFormat, Some('n'), "num-format", Some("num_format_style")),
    OptionSpec(Verbose, Some('v'), "verbose", None),
    OptionSpec(Help, Some('h'), "help", None),
    OptionSpec(Version, Some('V'), "version", None),
];

impl OptionSpec {
    fn label(&self) -> String {
        let mut label = format!("--{}", self.2);
        if self.0 == Verbose {
            label.push_str("...");
        }
        if let Some(param) = self.3 {
            label.push_str(&format!(" <{param}>"));
        }
        label
    }
}

type Seen = Vec<(&'static OptionSpec, Option<String>)>;

pub(crate) fn parse() -> Result<Args, String> {
    let mut args = Args::default();
    let mut input = env::args_os().skip(1).peekable();
    let mut seen = Seen::new();
    let mut positional = false;
    let mut first_input = None;

    while let Some(word) = input.next() {
        let word = utf8(word)?;
        if positional {
            first_input.get_or_insert(seen.len());
            args.paths.push(word.into());
        } else if word == "--" {
            positional = true;
        } else if let Some(long) = word.strip_prefix("--") {
            let (key, value) = long
                .split_once('=')
                .map_or((long, None), |(key, val)| (key, Some(val)));
            let spec = OPTIONS
                .iter()
                .find(|opt| opt.2 == key)
                .ok_or_else(|| unexpected(&format!("--{key}"), &seen, !args.paths.is_empty()))?;
            let value = value_for(spec, value, &mut input, &seen, first_input.is_some())?;
            if record(&mut args, &mut seen, spec, value)? {
                return Ok(args);
            }
        } else if word.starts_with('-') && word.len() > 1 {
            let short = &word[1..];
            for (index, key) in short.char_indices() {
                let spec = OPTIONS
                    .iter()
                    .find(|opt| opt.1 == Some(key))
                    .ok_or_else(|| unexpected(&format!("-{key}"), &seen, !args.paths.is_empty()))?;
                let rest = &short[index + key.len_utf8()..];
                let value = if spec.3.is_some() && !rest.is_empty() {
                    Some(rest.strip_prefix('=').unwrap_or(rest))
                } else {
                    None
                };
                let value = value_for(spec, value, &mut input, &seen, first_input.is_some())?;
                if record(&mut args, &mut seen, spec, value)? {
                    return Ok(args);
                }
                if spec.3.is_some() {
                    break;
                }
            }
        } else {
            first_input.get_or_insert(seen.len());
            args.paths.push(word.into());
        }
    }

    conflicts(&seen, first_input)?;
    for (spec, value) in seen {
        apply(&mut args, spec, value)?;
    }
    if args.paths.is_empty() {
        args.paths.push(".".into());
    }
    Ok(args)
}

fn utf8(word: OsString) -> Result<String, String> {
    word.into_string()
        .map_err(|_| usage_error("invalid UTF-8 was detected in one or more arguments", USAGE))
}

fn value_for(
    spec: &OptionSpec,
    inline: Option<&str>,
    input: &mut std::iter::Peekable<impl Iterator<Item = OsString>>,
    seen: &Seen,
    paths: bool,
) -> Result<Option<String>, String> {
    if spec.3.is_none() {
        if let Some(value) = inline {
            return Err(usage_error(
                &format!(
                    "unexpected value '{value}' for '{}' found; no more were expected",
                    spec.label()
                ),
                USAGE,
            ));
        }
        return Ok(None);
    }
    if let Some(value) = inline {
        return Ok(Some(value.to_owned()));
    }
    if let Some(next) = input.peek() {
        let next = next.to_string_lossy();
        if !next.starts_with('-') || next == "-" {
            return utf8(input.next().unwrap()).map(Some);
        }
        if next != "--"
            && !OPTIONS.iter().any(|opt| {
                next.strip_prefix("--")
                    .is_some_and(|long| long.split('=').next() == Some(opt.2))
                    || opt.1.is_some_and(|short| {
                        next.strip_prefix('-')
                            .is_some_and(|word| word.starts_with(short))
                    })
            })
        {
            return Err(unexpected(next.split('=').next().unwrap(), seen, paths));
        }
    }
    Err(format!(
        "error: a value is required for '{}' but none was supplied\n\n{FOOTER}",
        spec.label()
    ))
}

fn record(
    args: &mut Args,
    seen: &mut Seen,
    spec: &'static OptionSpec,
    value: Option<String>,
) -> Result<bool, String> {
    match spec.0 {
        Help => args.action = Action::Help,
        Version => args.action = Action::Version,
        Exclude | Types | Verbose => (),
        key => {
            if seen.iter().any(|(opt, _)| opt.0 == key) {
                return Err(usage_error(
                    &format!(
                        "the argument '{}' cannot be used multiple times",
                        spec.label()
                    ),
                    USAGE,
                ));
            }
        }
    }
    seen.push((spec, value));
    Ok(matches!(args.action, Action::Help | Action::Version))
}

fn apply(args: &mut Args, spec: &OptionSpec, value: Option<String>) -> Result<(), String> {
    let value = value.as_deref().unwrap_or_default();
    match spec.0 {
        Columns => {
            args.columns = Some(value.parse().map_err(|err: std::num::ParseIntError| {
                invalid(spec, value, &format!(": {err}"))
            })?)
        }
        Exclude => args.walk.excludes.push(value.to_owned()),
        Files => args.list_files = true,
        Input => args.input = Some(value.to_owned()),
        Hidden => args.walk.hidden = true,
        Languages => args.action = Action::Languages,
        NoIgnore => args.walk.no_ignore = true,
        NoIgnoreParent => args.walk.no_ignore_parent = true,
        NoIgnoreDot => args.walk.no_ignore_dot = true,
        NoIgnoreVcs => args.walk.no_ignore_vcs = true,
        Output if value == "json" => args.json = true,
        Output => return Err(invalid(spec, value, &format!(": {}", output_error(value)))),
        Streaming => {
            args.streaming = Some(match value.to_ascii_lowercase().as_str() {
                "json" => crate::cli::Streaming::Json,
                "simple" => crate::cli::Streaming::Simple,
                _ => return Err(invalid_choice(spec, value, &["simple", "json"])),
            });
        }
        Sort | Rsort => {
            args.sort = Some(crate::cli::Sort::parse(value).ok_or_else(|| {
                invalid_choice(
                    spec,
                    value,
                    &["files", "lines", "blanks", "code", "comments"],
                )
            })?);
            args.reverse = spec.0 == Rsort;
        }
        Types => args
            .types
            .get_or_insert_with(Vec::new)
            .extend(value.split(',').filter_map(Lang::parse)),
        Compact => args.compact = true,
        NumFormat => {
            args.separator = match value {
                "plain" => None,
                "commas" => Some(','),
                "dots" => Some('.'),
                "underscores" => Some('_'),
                _ => {
                    return Err(invalid_choice(
                        spec,
                        value,
                        &["commas", "dots", "plain", "underscores"],
                    ));
                }
            }
        }
        Verbose => args.verbose = args.verbose.saturating_add(1),
        Help | Version => unreachable!(),
    }
    Ok(())
}

fn conflicts(seen: &Seen, first_input: Option<usize>) -> Result<(), String> {
    let paths = first_input.is_some();
    for (index, (spec, _)) in seen.iter().enumerate() {
        let conflict = seen.iter().skip(index + 1).find(|(other, _)| {
            matches!(
                (spec.0, other.0),
                (Sort, Rsort)
                    | (Rsort, Sort)
                    | (Output, Columns | NumFormat)
                    | (Columns | NumFormat, Output)
            )
        });
        let other = if let Some((other, _)) = conflict {
            other.label()
        } else if paths && spec.0 == Languages {
            if first_input.is_some_and(|first| first <= index) {
                let present = seen
                    .iter()
                    .filter(|(opt, _)| opt.0 != Languages)
                    .map(|(opt, _)| (*opt, None))
                    .collect();
                return Err(usage_error(
                    "the argument '[input]...' cannot be used with '--languages'",
                    &usage(&present, true, None),
                ));
            }
            "[input]...".to_owned()
        } else {
            continue;
        };
        let usage = usage(seen, paths, Some(spec.0));
        return Err(usage_error(
            &format!(
                "the argument '{}' cannot be used with '{other}'",
                spec.label()
            ),
            &usage,
        ));
    }
    Ok(())
}

fn usage(seen: &Seen, paths: bool, conflict: Option<Key>) -> String {
    if seen.is_empty() && !paths {
        return USAGE.to_owned();
    }
    let mut text = "Usage: rlc".to_owned();
    let mut used = Vec::new();
    for (spec, _) in seen {
        if used.contains(&spec.0) {
            continue;
        }
        used.push(spec.0);
        if conflict.is_some_and(|key| {
            matches!(
                (key, spec.0),
                (Sort, Rsort)
                    | (Rsort, Sort)
                    | (Output, Columns | NumFormat)
                    | (Columns | NumFormat, Output)
            )
        }) {
            continue;
        }
        text.push(' ');
        text.push_str(&spec.label());
    }
    text.push_str(if paths && conflict != Some(Languages) {
        " <input>..."
    } else {
        " [input]..."
    });
    text
}

fn usage_error(message: &str, usage: &str) -> String {
    format!("error: {message}\n\n{usage}\n\n{FOOTER}")
}

fn unexpected(value: &str, seen: &Seen, paths: bool) -> String {
    let suggested = value
        .strip_prefix("--")
        .and_then(|flag| similar(flag, OPTIONS.iter().map(|opt| opt.2)))
        .and_then(|name| OPTIONS.iter().find(|opt| opt.2 == name));
    let (tip, usage) = if let Some(spec) = suggested {
        let mut present = seen.clone();
        present.push((spec, None));
        (
            format!("a similar argument exists: '--{}'", spec.2),
            usage(&present, paths, None),
        )
    } else {
        (
            format!("to pass '{value}' as a value, use '-- {value}'"),
            usage(seen, paths, None),
        )
    };
    usage_error(
        &format!("unexpected argument '{value}' found\n\n  tip: {tip}"),
        &usage,
    )
}

fn invalid_choice(spec: &OptionSpec, value: &str, choices: &[&str]) -> String {
    let mut description = format!("\n  [possible values: {}]", choices.join(", "));
    if let Some(candidate) = similar(value, choices.iter().copied()) {
        description.push_str(&format!("\n\n  tip: a similar value exists: '{candidate}'"));
    }
    invalid(spec, value, &description)
}

fn similar<'a>(value: &str, choices: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let chars: Vec<_> = value.chars().collect();
    choices
        .map(|candidate| {
            (
                jaro(&chars, &candidate.chars().collect::<Vec<_>>()),
                candidate,
            )
        })
        .filter(|(confidence, _)| *confidence > 0.7)
        .max_by(|(a, _), (b, _)| a.total_cmp(b))
        .map(|(_, value)| value)
}

fn jaro(a: &[char], b: &[char]) -> f64 {
    let window = (a.len().max(b.len()) / 2).saturating_sub(1);
    let mut matched = vec![false; b.len()];
    let mut left = Vec::new();
    for (i, &c) in a.iter().enumerate() {
        let start = i.saturating_sub(window);
        let end = (i + window + 1).min(b.len());
        if let Some(index) = (start..end).find(|&j| !matched[j] && b[j] == c) {
            matched[index] = true;
            left.push(c);
        }
    }
    if left.is_empty() {
        return 0.0;
    }
    let right = b
        .iter()
        .zip(matched)
        .filter(|(_, found)| *found)
        .map(|(c, _)| c);
    let transposed = left.iter().zip(right).filter(|(a, b)| a != b).count() / 2;
    let n = left.len() as f64;
    (n / a.len() as f64 + n / b.len() as f64 + (n - transposed as f64) / n) / 3.0
}

fn invalid(spec: &OptionSpec, value: &str, explanation: &str) -> String {
    format!(
        "error: invalid value '{value}' for '{}'{explanation}\n\n{FOOTER}",
        spec.label()
    )
}

fn output_error(format: &str) -> String {
    if !matches!(format, "yaml" | "cbor") {
        return format!("{format:?} is not a supported serialization format");
    }
    format!(
        "This version of rlc was compiled without any '{format}' serialization support.\n\nUse '--output json' to write a JSON report.\n"
    )
}
