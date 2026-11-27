use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::{diagnostic, glob::Rule};

#[derive(Default)]
pub(crate) struct WalkOptions {
    pub(crate) hidden: bool,
    pub(crate) no_ignore: bool,
    pub(crate) no_ignore_parent: bool,
    pub(crate) no_ignore_dot: bool,
    pub(crate) no_ignore_vcs: bool,
    pub(crate) excludes: Vec<String>,
}

struct Scope {
    dir: PathBuf,
    git_root: bool,
    rules: [Vec<Rule>; 4],
}

struct Walker<'a> {
    options: &'a WalkOptions,
    scopes: Vec<Scope>,
    excludes: Vec<Rule>,
    global: Vec<Rule>,
}

pub(crate) fn files(
    paths: &[PathBuf],
    options: &WalkOptions,
    mut visit: impl FnMut(&Path),
    mut error: impl FnMut(&Path, io::Error),
) {
    let excludes = options
        .excludes
        .iter()
        .filter_map(|text| match Rule::parse(text, true) {
            Ok(rule) => rule,
            Err(err) => {
                error(
                    Path::new(""),
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("error parsing glob '!{text}': {err}"),
                    ),
                );
                None
            }
        })
        .collect();
    let mut walker = Walker {
        options,
        scopes: Vec::new(),
        excludes,
        global: if options.no_ignore || options.no_ignore_vcs {
            Vec::new()
        } else {
            global_rules()
        },
    };
    for path in paths {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => visit(path),
            Ok(metadata) if metadata.is_dir() => {
                let absolute = path.canonicalize().unwrap_or_else(|_| path.clone());
                if !options.no_ignore && !options.no_ignore_parent {
                    let parents: Vec<_> = absolute.ancestors().skip(1).collect();
                    for parent in parents.into_iter().rev() {
                        walker.push(parent);
                    }
                }
                walker.directory(path, &absolute, &mut visit, &mut error);
                walker.scopes.clear();
            }
            Ok(_) => (),
            Err(err) => error(path, err),
        }
    }
}

impl Walker<'_> {
    fn push(&mut self, absolute: &Path) {
        if self.options.no_ignore {
            return;
        }
        let mut scope = Scope {
            dir: absolute.to_path_buf(),
            git_root: false,
            rules: Default::default(),
        };
        if !self.options.no_ignore_dot {
            scope.rules[0] = read_rules(&absolute.join(".rlcignore"));
            scope.rules[1] = read_rules(&absolute.join(".ignore"));
        }
        if !self.options.no_ignore_vcs {
            let git = absolute.join(".git");
            scope.git_root = git.exists() || absolute.join(".jj").exists();
            scope.rules[2] = read_rules(&absolute.join(".gitignore"));
            if scope.git_root {
                scope.rules[3] = read_rules(&git_dir(&git).join("info/exclude"));
            }
        }

        if scope.git_root || scope.rules.iter().any(|rules| !rules.is_empty()) {
            self.scopes.push(scope);
        }
    }

    fn directory(
        &mut self,
        path: &Path,
        absolute: &Path,
        visit: &mut impl FnMut(&Path),
        error: &mut impl FnMut(&Path, io::Error),
    ) {
        let Ok(entries) = fs::read_dir(path).map_err(|err| error(path, err)) else {
            return;
        };
        let depth = self.scopes.len();
        self.push(absolute);
        // Directory entries and equal sort keys have stable order in rlc.
        let mut children = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => children.push(entry),
                Err(err) => error(path, err),
            }
        }
        children.sort_by_key(fs::DirEntry::file_name);
        for entry in children {
            let path = entry.path();
            let Ok(kind) = entry.file_type().map_err(|err| error(&path, err)) else {
                continue;
            };
            if !kind.is_dir() && !kind.is_file() {
                continue;
            }

            let name = entry.file_name();
            let resolved = absolute.join(&name);
            let name = name.as_encoded_bytes();
            if self.ignored(&path, &resolved, name, kind.is_dir()) {
                continue;
            }
            if kind.is_dir() {
                self.directory(&path, &resolved, visit, error);
            } else {
                visit(&path);
            }
        }
        self.scopes.truncate(depth);
    }

    fn ignored(&self, path: &Path, absolute: &Path, name: &[u8], is_dir: bool) -> bool {
        let path = path.strip_prefix(".").unwrap_or(path);
        if let Some(skip) = matched(&self.excludes, path, name, is_dir) {
            return skip;
        }
        let has_repo = self.scopes.iter().any(|scope| scope.git_root);
        // Rule source precedence is independent of directory depth.
        for source in 0..if has_repo { 4 } else { 2 } {
            for scope in self.scopes.iter().rev() {
                if let Ok(relative) = absolute.strip_prefix(&scope.dir)
                    && let Some(skip) = matched(&scope.rules[source], relative, name, is_dir)
                {
                    return skip;
                }
                if source >= 2 && scope.git_root {
                    break;
                }
            }
        }
        if has_repo && let Some(skip) = matched(&self.global, path, name, is_dir) {
            return skip;
        }
        !self.options.hidden && name.starts_with(b".")
    }
}

fn matched(rules: &[Rule], path: &Path, name: &[u8], is_dir: bool) -> Option<bool> {
    let bytes = path.as_os_str().as_encoded_bytes();
    rules
        .iter()
        .rev()
        .find_map(|rule| rule.matched(bytes, name, is_dir))
}

fn read_rules(path: &Path) -> Vec<Rule> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    diagnostic::debug(
        "ignore::gitignore",
        format_args!("opened gitignore file: {}", path.display()),
    );
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .enumerate()
        .filter_map(|(i, line)| {
            let line = if i == 0 {
                line.trim_start_matches('\u{feff}')
            } else {
                &line
            };
            Rule::parse(line, false).ok().flatten()
        })
        .collect()
}

fn git_dir(dot_git: &Path) -> PathBuf {
    if let Ok(config) = fs::read_to_string(dot_git)
        && let Some(path) = config.trim().strip_prefix("gitdir: ")
    {
        let dir = dot_git.with_file_name(path);
        return match fs::read_to_string(dir.join("commondir")) {
            Ok(common) => dir.join(common.trim()),
            Err(_) => dir,
        };
    }
    dot_git.to_owned()
}

fn global_rules() -> Vec<Rule> {
    let home = env::home_dir();
    let config = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    let mut configs = Vec::new();
    if let Some(home) = &home {
        configs.push(home.join(".gitconfig"));
    }
    if let Some(config) = &config {
        configs.push(config.join("git/config"));
    }
    for path in configs {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        if let Some(value) = text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            key.trim()
                .eq_ignore_ascii_case("excludesfile")
                .then_some(value.trim().trim_matches('"'))
        }) {
            let path = if let Some(path) = value.strip_prefix("~/") {
                home.as_ref().map(|home| home.join(path))
            } else {
                Some(PathBuf::from(value))
            };
            if let Some(path) = path {
                return read_rules(&path);
            }
        }
    }
    config.map_or_else(Vec::new, |dir| read_rules(&dir.join("git/ignore")))
}
