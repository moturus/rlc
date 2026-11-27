use std::env;
use std::io::{self, IsTerminal, Write};

use crate::cli::{Args, Sort, Streaming};
use crate::language::Lang;
use crate::stats::{Counts, Language, Languages, Report};

pub(crate) fn table(
    out: &mut impl Write,
    args: &Args,
    languages: &mut Languages,
) -> io::Result<()> {
    let columns = args
        .columns
        .or_else(|| args.list_files.then(terminal_columns).flatten())
        .unwrap_or(81)
        .max(81);
    let color = Color::detect();
    let options = Table {
        columns,
        color,
        separator: args.separator,
    };
    let row = "━".repeat(columns);
    let subrow = color.style("─".repeat(columns), "2");
    let mut total = Counts::default();
    let mut files = 0;
    let inaccurate = languages.values().any(|l| l.inaccurate);
    if inaccurate {
        writeln!(
            out,
            "Note: results can be inaccurate for languages marked with '(!)'"
        )?;
    }

    writeln!(out, "{row}")?;
    let width = columns - 69;
    write!(
        out,
        " {}",
        color.style(format!("{:<width$}", "Language"), "1;34")
    )?;
    for (label, width) in [
        ("Files", 14),
        ("Lines", 12),
        ("Code", 12),
        ("Comments", 12),
        ("Blanks", 12),
    ] {
        write!(out, " {}", color.style(format!("{label:>width$}"), "1;34"))?;
    }
    writeln!(out, "\n{row}")?;

    let mut ordered: Vec<_> = languages.iter_mut().collect();
    if let Some(sort) = args.sort {
        ordered.sort_by_key(|(_, l)| std::cmp::Reverse(sort_key(sort, l.counts, l.reports.len())));
        if args.reverse {
            ordered.reverse();
        }
    }

    let mut first = true;
    for show_children in [false, true] {
        for (lang, language) in &mut ordered {
            let has_children = !language.children.is_empty();
            if show_children != (has_children && !args.compact) {
                continue;
            }
            total += language.summary();
            files += language.reports.len();
            if language.counts.lines() == 0 && !has_children {
                continue;
            }
            if !first && (show_children || args.list_files) {
                writeln!(out, "{subrow}")?;
            }
            first = false;
            options.name(out, lang.name(), "", language.inaccurate)?;
            options.counts(out, Some(language.reports.len()), language.counts, false)?;
            if show_children {
                for (&child, reports) in &language.children {
                    let mut counts = Counts::default();
                    for report in reports {
                        counts += report.stats.summary();
                    }
                    options.name(out, child.name(), " |-", false)?;
                    options.counts(out, Some(reports.len()), counts, false)?;
                }
                options.report(out, "(Total)", "", language.summary(), true)?;
            }
            if args.list_files {
                writeln!(out, "{subrow}")?;
                options.files(out, **lang, language, args)?;
            }
        }
    }

    writeln!(out, "{row}")?;
    options.name(out, "Total", "", inaccurate)?;
    options.counts(out, Some(files), total, true)?;
    writeln!(out, "{row}")
}

fn sort_key(sort: Sort, counts: Counts, files: usize) -> usize {
    match sort {
        Sort::Files => files,
        Sort::Lines => counts.lines(),
        Sort::Blanks => counts.blanks,
        Sort::Code => counts.code,
        Sort::Comments => counts.comments,
    }
}

struct Table {
    columns: usize,
    color: Color,
    separator: Option<char>,
}

impl Table {
    fn name(
        &self,
        out: &mut impl Write,
        name: &str,
        prefix: &str,
        inaccurate: bool,
    ) -> io::Result<()> {
        let badge = if inaccurate { "(!)" } else { "" };
        let width = self.columns - 63 - prefix.len() - badge.len();
        let name = if name.len() > width {
            format!("{name:.precision$}|", precision = width - 1)
        } else {
            self.color.style(format!("{name:<width$}"), "1;35")
        };
        write!(out, "{prefix} {name}{badge}")
    }

    fn counts(
        &self,
        out: &mut impl Write,
        files: Option<usize>,
        counts: Counts,
        total: bool,
    ) -> io::Result<()> {
        let count = files.map(|n| number(n, self.separator)).unwrap_or_default();
        let style = |value: String| {
            if total {
                self.color.style(value, "34")
            } else {
                value
            }
        };
        write!(out, " {}", style(format!("{count:>8}")))?;
        for n in [counts.lines(), counts.code, counts.comments, counts.blanks] {
            write!(
                out,
                " {}",
                style(format!("{:>12}", number(n, self.separator)))
            )?;
        }
        writeln!(out)
    }

    fn files(
        &self,
        out: &mut impl Write,
        lang: Lang,
        language: &mut Language,
        args: &Args,
    ) -> io::Result<()> {
        let reports = &mut language.reports;
        match args.sort {
            Some(sort) if !matches!(sort, Sort::Files) => {
                reports.sort_by_key(|r| std::cmp::Reverse(sort_key(sort, r.stats.counts, 0)));
            }
            _ => reports.sort_by(|a, b| a.path.cmp(&b.path)),
        }
        let (plain, embedded): (Vec<_>, _) = reports
            .iter()
            .partition(|r| args.compact || r.stats.blobs.is_empty());
        for report in &plain {
            self.report(
                out,
                &report.path.to_string_lossy(),
                "",
                report.stats.counts,
                false,
            )?;
        }
        for (index, report) in embedded.into_iter().enumerate() {
            let name = report.path.to_string_lossy();
            if index == 0 && plain.is_empty() {
                writeln!(out, " {name}")?;
            } else {
                let dashes = "-".repeat(self.columns.saturating_sub(4 + name.len()));
                writeln!(out, "-- {name} {dashes}")?;
            }
            self.report(out, lang.name(), " |-", report.stats.counts, false)?;
            for (&child, stats) in &report.stats.blobs {
                self.name(out, child.name(), " |-", language.inaccurate)?;
                self.counts(out, None, stats.counts, false)?;
            }
            self.report(out, &name, "", report.stats.counts, true)?;
        }
        Ok(())
    }

    fn report(
        &self,
        out: &mut impl Write,
        name: &str,
        prefix: &str,
        counts: Counts,
        subtotal: bool,
    ) -> io::Result<()> {
        let width = self.columns - 56 - prefix.len();
        if subtotal {
            let sep = self.separator;
            writeln!(
                out,
                " {name:<width$} {:>14} {:>12} {:>12} {:>12}",
                number(counts.lines(), sep),
                number(counts.code, sep),
                number(counts.comments, sep),
                number(counts.blanks, sep)
            )
        } else {
            let width = width + 2;
            let shortened;
            let name = if name.len() > width {
                let cut = name.ceil_char_boundary(name.len() + 1 - width);
                shortened = format!("|{}", &name[cut..]);
                &shortened
            } else {
                name
            };
            writeln!(
                out,
                "{prefix} {name:<width$} {:>12} {:>12} {:>12} {:>12}",
                counts.lines(),
                counts.code,
                counts.comments,
                counts.blanks
            )
        }
    }
}

fn number(n: usize, separator: Option<char>) -> String {
    let digits = n.to_string();
    let Some(separator) = separator else {
        return digits;
    };
    let mut output = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            output.push(separator);
        }
        output.push(c);
    }
    output
}

#[derive(Clone, Copy)]
struct Color(bool);

impl Color {
    fn detect() -> Self {
        let enabled = |key| env::var_os(key).is_some_and(|value| value != "0");
        Self(
            enabled("CLICOLOR_FORCE")
                || (env::var_os("NO_COLOR").is_none()
                    && env::var_os("CLICOLOR").is_none_or(|value| value != "0")
                    && io::stdout().is_terminal()),
        )
    }

    fn style(&self, value: String, sgr: &str) -> String {
        if self.0 {
            format!("\x1b[{sgr}m{value}\x1b[0m")
        } else {
            value
        }
    }
}

fn terminal_columns() -> Option<usize> {
    if !io::stdout().is_terminal() && !io::stderr().is_terminal() && !io::stdin().is_terminal() {
        return None;
    }
    env::var("COLUMNS").ok()?.parse().ok()
}

pub(crate) fn languages(out: &mut impl Write) -> io::Result<()> {
    let width = terminal_columns().unwrap_or(75).saturating_sub(8);
    let (lang_width, suffix_width) = if width <= 80 {
        (width / 2, width / 2)
    } else {
        (40, width - 40)
    };
    let color = Color::detect();
    let row = color.style("━".repeat(lang_width + suffix_width + 6), "1");
    let edge = color.style("┃".to_owned(), "1");
    let cell = |value: &str, width, pad, header| {
        let string = language_cell(value, width, pad);
        if header {
            color.style(string, "1")
        } else {
            string
        }
    };
    writeln!(out, "{row}")?;
    writeln!(
        out,
        "{edge}{}{}{edge}",
        cell("Language", lang_width, 0, true),
        cell("Extensions", suffix_width, 3, true)
    )?;
    let left = color.style("━".repeat(lang_width + 2), "1");
    let right = color.style("━".repeat(suffix_width + 2), "1");
    writeln!(out, "{edge}{left}{right}{edge}")?;
    for lang in Lang::all() {
        let mut suffixes = if lang.extensions.is_empty() {
            "<None>".to_owned()
        } else {
            lang.extensions.join(", ")
        };
        if lang.id == "Emojicode" {
            suffixes.push('\u{200b}');
        }
        writeln!(
            out,
            "{edge}{}{}{edge}",
            cell(lang.name, lang_width, 0, false),
            cell(&suffixes, suffix_width, 3, false)
        )?;
    }
    writeln!(out, "{row}\n")
}

fn language_cell(text: &str, width: usize, pad: usize) -> String {
    let width = width.saturating_sub(pad);
    let char_width = |c: char| match c {
        '🍇' => 2,
        '\u{200b}' => 0,
        _ => 1,
    };
    let size: usize = text.chars().map(char_width).sum();
    let left = " ".repeat(pad);
    if size <= width {
        format!(" {left}{text}{} ", " ".repeat(width - size))
    } else {
        let ellipsis = if width >= 3 { "..." } else { "." };
        let mut consumed = 0;
        let prefix: String = text
            .chars()
            .take_while(|&c| {
                consumed += char_width(c);
                consumed <= width.saturating_sub(ellipsis.len())
            })
            .collect();
        format!(" {left}{prefix}{ellipsis} ")
    }
}

pub(crate) fn stream_header(out: &mut impl Write) -> io::Result<()> {
    writeln!(
        out,
        "#{:^10} {:^80} {:^12} {:^12} {:^12} {:^12}",
        "language", "path", "lines", "code", "comments", "blanks"
    )?;
    let bar = "#".repeat(12);
    writeln!(out, "########## {} {bar} {bar} {bar} {bar}", "#".repeat(80))
}

pub(crate) fn stream(
    out: &mut impl Write,
    format: Streaming,
    lang: Lang,
    report: &Report,
) -> io::Result<()> {
    match format {
        Streaming::Simple => {
            let counts = report.stats.counts;
            writeln!(
                out,
                "{:>10} {:<80} {:>12} {:>12} {:>12} {:>12}",
                lang.name(),
                report.path.to_string_lossy(),
                counts.lines(),
                counts.code,
                counts.comments,
                counts.blanks
            )
        }
        Streaming::Json => writeln!(out, "{}", crate::wire::stream(lang, report)),
    }
}
