use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

use crate::cli::{Args, Sort};
use crate::json;
use crate::language::Lang;

#[derive(Default)]
pub(crate) struct Config {
    columns: Option<usize>,
    hidden: Option<bool>,
    no_ignore: Option<bool>,
    no_ignore_parent: Option<bool>,
    no_ignore_dot: Option<bool>,
    no_ignore_vcs: Option<bool>,
    sort: Option<Sort>,
    types: Option<Vec<Lang>>,
    pub(crate) doc_strings_as_comments: Option<bool>,
}

impl Config {
    pub(crate) fn load(args: &mut Args) -> Self {
        let home = env::home_dir();
        let xdg = env::var_os("XDG_CONFIG_HOME")
            .filter(|path| !path.is_empty())
            .map(Into::into)
            .or_else(|| home.as_ref().map(|path| path.join(".config")));
        let mut merged = Self::default();
        for dir in [xdg, home, env::current_dir().ok()].into_iter().flatten() {
            if let Some(config) = Self::read(&dir) {
                merged.merge(config);
            }
        }

        args.columns = args.columns.or(merged.columns);
        args.sort = args.sort.or(merged.sort);
        args.types = args.types.take().or_else(|| merged.types.take());
        args.walk.hidden |= merged.hidden.unwrap_or(false);
        args.walk.no_ignore |= merged.no_ignore.unwrap_or(false);
        args.walk.no_ignore_parent |= merged.no_ignore_parent.unwrap_or(false);
        args.walk.no_ignore_dot |= merged.no_ignore_dot.unwrap_or(false);
        args.walk.no_ignore_vcs |= merged.no_ignore_vcs.unwrap_or(false);
        merged
    }

    fn merge(&mut self, config: Self) {
        self.columns = config.columns.or(self.columns);
        self.hidden = config.hidden.or(self.hidden);
        self.no_ignore = config.no_ignore.or(self.no_ignore);
        self.no_ignore_parent = config.no_ignore_parent.or(self.no_ignore_parent);
        self.no_ignore_dot = config.no_ignore_dot.or(self.no_ignore_dot);
        self.no_ignore_vcs = config.no_ignore_vcs.or(self.no_ignore_vcs);
        self.sort = config.sort.or(self.sort);
        self.types = config.types.or_else(|| self.types.take());
        self.doc_strings_as_comments = config
            .doc_strings_as_comments
            .or(self.doc_strings_as_comments);
    }

    fn read(dir: &Path) -> Option<Self> {
        let text = fs::read_to_string(dir.join("rlc.toml"))
            .or_else(|_| fs::read_to_string(dir.join(".rlcrc")))
            .ok()?;
        let mut config = Self::default();
        for (key, value) in assignments(&text)? {
            match key.as_str() {
                "columns" => config.columns = Some(integer(&value)?),
                "hidden" => config.hidden = Some(value.parse().ok()?),
                "no_ignore" => config.no_ignore = Some(value.parse().ok()?),
                "no_ignore_parent" => config.no_ignore_parent = Some(value.parse().ok()?),
                "no_ignore_dot" => config.no_ignore_dot = Some(value.parse().ok()?),
                "no_ignore_vcs" => config.no_ignore_vcs = Some(value.parse().ok()?),
                "treat_doc_strings_as_comments" => {
                    config.doc_strings_as_comments = Some(value.parse().ok()?)
                }
                "sort" => config.sort = Some(Sort::parse(&string(&value)?)?),
                "types" => config.types = Some(types(&value)?),
                _ => (),
            }
        }
        Some(config)
    }
}

// rlc settings are top-level scalars and a list of language names.
fn assignments(text: &str) -> Option<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    let mut statement = String::new();
    let mut depth = 0usize;
    for line in text.lines() {
        let mut quote = None;
        let mut escape = false;
        let line = line.trim();
        let mut end = line.len();
        if statement.is_empty() && line.starts_with('[') {
            break;
        }
        for (i, c) in line.char_indices() {
            if let Some(delimiter) = quote {
                if escape {
                    escape = false;
                } else if c == '\\' && delimiter == '"' {
                    escape = true;
                } else if c == delimiter {
                    quote = None;
                }
            } else {
                match c {
                    '"' | '\'' => quote = Some(c),
                    '[' | '{' => depth += 1,
                    ']' | '}' => depth = depth.checked_sub(1)?,
                    '#' => {
                        end = i;
                        break;
                    }
                    _ => (),
                }
            }
        }
        if quote.is_some() {
            return None;
        }
        statement.push_str(&line[..end]);
        if depth != 0 {
            statement.push('\n');
            continue;
        }
        if !statement.trim().is_empty() {
            let (key, value) = statement.split_once('=')?;
            let key = key.trim();
            let key = if key.starts_with(['"', '\'']) {
                string(key)?
            } else {
                key.to_owned()
            };
            let value = value.trim();
            if value.is_empty() || values.insert(key, value.to_owned()).is_some() {
                return None;
            }
        }
        statement.clear();
    }
    (depth == 0).then_some(values)
}

fn string(value: &str) -> Option<String> {
    if let Some(literal) = value.strip_prefix('\'') {
        let literal = literal.strip_suffix('\'')?;
        return (!literal.contains('\'')).then(|| literal.to_owned());
    }
    json::parse(value)?.string().map(str::to_owned)
}

fn types(value: &str) -> Option<Vec<Lang>> {
    let mut entries = value.strip_prefix('[')?.strip_suffix(']')?.trim();
    let mut types = Vec::new();
    while !entries.is_empty() {
        let quote = entries.chars().next()?;
        if !matches!(quote, '"' | '\'') {
            return None;
        }
        let mut escape = false;
        let end = entries[1..].char_indices().find_map(|(i, c)| {
            if escape {
                escape = false;
            } else if c == '\\' && quote == '"' {
                escape = true;
            } else if c == quote {
                return Some(i + 2);
            }
            None
        })?;
        types.push(Lang::named(&string(&entries[..end])?)?);
        entries = entries[end..].trim_start();
        if !entries.is_empty() {
            entries = entries.strip_prefix(',')?.trim_start();
        }
    }
    Some(types)
}

fn integer(value: &str) -> Option<usize> {
    let value = value.replace('_', "");
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(digits) = value.strip_prefix(prefix) {
            return usize::from_str_radix(digits, radix).ok();
        }
    }
    value.parse().ok()
}
