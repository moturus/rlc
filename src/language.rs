use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

#[path = "language_data.rs"]
mod data;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Lang(usize);

pub(crate) type Delimiters = &'static [(&'static str, &'static str)];

pub(crate) struct Definition {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) extensions: &'static [&'static str],
    filenames: &'static [&'static str],
    suffixes: &'static [&'static str],
    shebangs: &'static [&'static str],
    env: &'static [&'static str],
    mime: &'static [&'static str],
    pub(crate) line_comments: &'static [&'static str],
    pub(crate) block_comments: Delimiters,
    pub(crate) nested_comments: Delimiters,
    pub(crate) quotes: Delimiters,
    pub(crate) verbatim_quotes: Delimiters,
    pub(crate) doc_quotes: Delimiters,
    pub(crate) important: &'static [&'static str],
    pub(crate) nested: bool,
    pub(crate) literate: bool,
}

impl Definition {
    const EMPTY: Self = Self {
        id: "",
        name: "",
        extensions: &[],
        filenames: &[],
        suffixes: &[],
        shebangs: &[],
        env: &[],
        mime: &[],
        line_comments: &[],
        block_comments: &[],
        nested_comments: &[],
        quotes: &[],
        verbatim_quotes: &[],
        doc_quotes: &[],
        important: &[],
        nested: false,
        literate: false,
    };
}

#[derive(Default)]
struct Lookup {
    names: HashMap<&'static str, Lang>,
    folded: HashMap<String, Lang>,
    filenames: HashMap<&'static str, Lang>,
    extensions: HashMap<&'static str, Lang>,
    shebangs: HashMap<&'static str, Lang>,
    mime: HashMap<&'static str, Lang>,
    suffixes: Vec<(&'static str, Lang)>,
    env: Vec<(&'static str, Lang)>,
}

fn lookup() -> &'static Lookup {
    static LOOKUP: OnceLock<Lookup> = OnceLock::new();
    LOOKUP.get_or_init(|| {
        let mut table = Lookup::default();
        for (index, d) in data::DEFINITIONS.iter().enumerate() {
            let lang = Lang(index);
            table.names.entry(d.id).or_insert(lang);
            table.names.entry(d.name).or_insert(lang);
            table.folded.entry(d.name.to_lowercase()).or_insert(lang);
            for &name in d.filenames {
                table.filenames.entry(name).or_insert(lang);
            }
            for &ext in d.extensions {
                table.extensions.entry(ext).or_insert(lang);
            }
            for &bang in d.shebangs {
                table.shebangs.entry(bang).or_insert(lang);
            }
            for &mime in d.mime {
                table.mime.entry(mime).or_insert(lang);
            }
            table.suffixes.extend(d.suffixes.iter().map(|&s| (s, lang)));
            table.env.extend(d.env.iter().map(|&s| (s, lang)));
        }
        table
    })
}

pub(crate) enum Detection {
    Language(Lang),
    UnknownExtension(String),
    UnknownInterpreter(String),
    Unrecognized,
}

impl Lang {
    pub(crate) fn all() -> &'static [Definition] {
        data::DEFINITIONS
    }

    pub(crate) fn definition(self) -> &'static Definition {
        &data::DEFINITIONS[self.0]
    }

    pub(crate) fn name(self) -> &'static str {
        self.definition().name
    }

    pub(crate) fn named(name: &str) -> Option<Self> {
        lookup().names.get(name).copied()
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        lookup().folded.get(&name.to_lowercase()).copied()
    }

    pub(crate) fn extension(ext: &str) -> Option<Self> {
        lookup().extensions.get(ext).copied()
    }

    pub(crate) fn mime(mime: &str) -> Option<Self> {
        lookup().mime.get(mime).copied()
    }

    pub(crate) fn detect(path: &Path) -> Detection {
        let table = lookup();
        if let Some(filename) = path.file_name() {
            let filename = filename.to_string_lossy().to_lowercase();
            if let Some(&lang) = table.filenames.get(filename.as_str()) {
                return Detection::Language(lang);
            }
            if let Some(&(_, lang)) = table.suffixes.iter().find(|(s, _)| filename.ends_with(s)) {
                return Detection::Language(lang);
            }
        }

        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            return match Self::extension(&ext) {
                Some(lang) => Detection::Language(lang),
                None => Detection::UnknownExtension(ext),
            };
        }

        Self::detect_shebang(path, table)
    }

    fn detect_shebang(path: &Path, table: &Lookup) -> Detection {
        let mut bytes = [0; 128];
        let Ok(len) = File::open(path).and_then(|mut file| file.read(&mut bytes)) else {
            return Detection::Unrecognized;
        };
        let line = bytes[..len].split(|&byte| byte == b'\n').next().unwrap();
        let Ok(line) = str::from_utf8(line) else {
            return Detection::Unrecognized;
        };
        let mut words = line.split_whitespace();
        let Some(interpreter) = words.next() else {
            return Detection::Unrecognized;
        };

        if let Some(&lang) = table.shebangs.get(interpreter) {
            return Detection::Language(lang);
        }

        if interpreter == "#!/usr/bin/env"
            && let Some(word) = words.next()
        {
            return match table
                .env
                .iter()
                .find(|(prefix, _)| word.starts_with(prefix))
            {
                Some(&(_, lang)) => Detection::Language(lang),
                None => Detection::UnknownInterpreter(word.to_owned()),
            };
        }

        Detection::Unrecognized
    }
}
