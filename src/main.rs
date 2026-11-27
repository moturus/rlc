mod cli;
mod config;
mod count;
mod diagnostic;
mod embedding;
mod glob;
mod json;
mod language;
mod output;
mod stats;
mod walk;
mod wire;

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use cli::{Action, Streaming};
use language::{Detection, Lang};
use stats::{Languages, Report};

fn main() -> io::Result<ExitCode> {
    let mut args = match cli::parse() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            return Ok(ExitCode::from(2));
        }
    };

    let mut out = io::BufWriter::new(io::stdout().lock());
    match args.action {
        Action::Help => out.write_all(cli::HELP.as_bytes())?,
        Action::Version => writeln!(out, "{}", cli::VERSION)?,
        Action::Languages => output::languages(&mut out)?,
        Action::Count => return run(&mut args, &mut out),
    }
    out.flush()?;
    Ok(ExitCode::SUCCESS)
}

fn run(args: &mut cli::Args, out: &mut impl Write) -> io::Result<ExitCode> {
    diagnostic::configure(args.verbose);
    let config = config::Config::load(args);
    if let Some(input) = &args.input
        && !wire::valid_input(input)
    {
        wire::input_error(input);
        return Ok(ExitCode::FAILURE);
    }
    for path in &args.paths {
        if fs::metadata(path).is_err() {
            eprintln!("Error: '{}' not found.", path.display());
            return Ok(ExitCode::FAILURE);
        }
    }

    if args.streaming == Some(Streaming::Simple) {
        output::stream_header(out)?;
    }

    let mut languages = Languages::new();
    let mut result = Ok(());
    walk::files(
        &args.paths,
        &args.walk,
        |path| {
            if result.is_err() {
                return;
            }
            let Some(lang) = detect(path) else {
                return;
            };
            if args
                .types
                .as_ref()
                .is_some_and(|types| !types.contains(&lang))
            {
                return;
            }
            let language = languages.entry(lang).or_default();
            match fs::read(path) {
                Ok(contents) => {
                    let stats = count::count(
                        lang,
                        &count::decode(&contents),
                        config.doc_strings_as_comments.unwrap_or(false),
                    );
                    let report = Report {
                        path: path.to_owned(),
                        stats,
                    };
                    if let Some(format) = args.streaming {
                        result =
                            output::stream(out, format, lang, &report).and_then(|()| out.flush());
                    }
                    language.add(report);
                }
                Err(err) => {
                    language.inaccurate = true;
                    diagnostic::error(
                        "rlc::utils::fs",
                        format_args!("Error reading {}:\n{err}", path.display()),
                    );
                }
            }
        },
        |path, err| {
            if path.as_os_str().is_empty() {
                diagnostic::error("rlc::utils::fs", format_args!("{err}"));
            } else {
                diagnostic::error(
                    "rlc::utils::fs",
                    format_args!("{err} reading {}", path.display()),
                );
            }
        },
    );
    result?;

    if args.streaming.is_none() {
        if args.json {
            out.write_all(wire::output(&languages).as_bytes())?;
        } else {
            output::table(out, args, &mut languages)?;
        }
    }
    out.flush()?;
    Ok(ExitCode::SUCCESS)
}

fn detect(path: &Path) -> Option<Lang> {
    const TARGET: &str = "rlc::language::language_type";
    match Lang::detect(path) {
        Detection::Language(lang) => return Some(lang),
        Detection::UnknownExtension(ext) => {
            diagnostic::warn(TARGET, format_args!("Unknown extension: {ext}"))
        }
        Detection::UnknownInterpreter(interpreter) => {
            diagnostic::warn(TARGET, format_args!("Unknown environment: {interpreter:?}"))
        }
        Detection::Unrecognized => (),
    }
    None
}
