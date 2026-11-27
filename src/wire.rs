use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read};

use crate::json::{self, Value, quote};
use crate::language::Lang;
use crate::stats::{CodeStats, Counts, Languages, Report};

pub(crate) fn output(languages: &Languages) -> String {
    let mut out = String::from("{");
    let mut total = Counts::default();
    let mut inaccurate = false;
    for (&lang, language) in languages {
        quote(&mut out, lang.name());
        out.push_str(":{");
        counts(&mut out, language.counts);
        out.push_str(",\"reports\":");
        reports(&mut out, &language.reports);
        out.push_str(",\"children\":{");
        report_map(
            &mut out,
            language
                .children
                .iter()
                .map(|(&lang, reports)| (lang, reports.as_slice())),
        );
        write!(out, "}},\"inaccurate\":{}}},", language.inaccurate).unwrap();
        total += language.summary();
        inaccurate |= language.inaccurate;
    }
    out.push_str("\"Total\":{");
    counts(&mut out, total);
    out.push_str(",\"reports\":[],\"children\":{");
    report_map(
        &mut out,
        languages
            .iter()
            .map(|(&lang, l)| (lang, l.reports.as_slice())),
    );
    write!(out, "}},\"inaccurate\":{inaccurate}}}}}").unwrap();
    out
}

pub(crate) fn stream(lang: Lang, report: &Report) -> String {
    let mut out = String::from("{\"language\":");
    quote(&mut out, lang.name());
    out.push_str(",\"stats\":{\"name\":");
    quote(&mut out, &report.path.to_string_lossy());
    out.push_str(",\"stats\":");
    stats(&mut out, &report.stats, true);
    out.push_str("}}");
    out
}

fn counts(out: &mut String, c: Counts) {
    write!(
        out,
        "\"blanks\":{},\"code\":{},\"comments\":{}",
        c.blanks, c.code, c.comments
    )
    .unwrap();
}

fn report_map<'a>(out: &mut String, iter: impl Iterator<Item = (Lang, &'a [Report])>) {
    for (index, (lang, list)) in iter.enumerate() {
        if index != 0 {
            out.push(',');
        }
        quote(out, lang.name());
        out.push(':');
        reports(out, list);
    }
}

fn reports(out: &mut String, reports: &[Report]) {
    out.push('[');
    for (index, report) in reports.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str("{\"stats\":");
        stats(out, &report.stats, false);
        out.push_str(",\"name\":");
        quote(out, &report.path.to_string_lossy());
        out.push('}');
    }
    out.push(']');
}

fn stats(out: &mut String, stats: &CodeStats, streaming: bool) {
    let c = stats.counts;
    out.push('{');
    if streaming {
        write!(out, "\"blanks\":{},\"blobs\":{{", c.blanks).unwrap();
        // The streaming object is emitted with JSON's key order at every level.
        let blobs: BTreeMap<_, _> = stats
            .blobs
            .iter()
            .map(|(lang, stats)| (lang.name(), stats))
            .collect();
        blob_map(out, blobs.into_iter(), true);
        write!(out, "}},\"code\":{},\"comments\":{}", c.code, c.comments).unwrap();
    } else {
        counts(out, c);
        out.push_str(",\"blobs\":{");
        blob_map(
            out,
            stats.blobs.iter().map(|(lang, stats)| (lang.name(), stats)),
            false,
        );
        out.push('}');
    }
    out.push('}');
}

fn blob_map<'a>(
    out: &mut String,
    entries: impl Iterator<Item = (&'a str, &'a CodeStats)>,
    streaming: bool,
) {
    for (index, (lang, blob)) in entries.enumerate() {
        if index != 0 {
            out.push(',');
        }
        quote(out, lang);
        out.push(':');
        stats(out, blob, streaming);
    }
}

pub(crate) fn valid_input(input: &str) -> bool {
    let text = match fs::read_to_string(input) {
        Ok(text) => text,
        Err(_) if input == "stdin" => {
            let mut text = String::new();
            if io::stdin().read_to_string(&mut text).is_err() {
                return false;
            }
            text
        }
        Err(_) => input.to_owned(),
    };
    let Some(parsed) = json::parse(&text) else {
        return false;
    };
    let Some(object) = parsed.object() else {
        return false;
    };
    // Validate previous input against the rlc output schema before scanning.
    object.contains_key("Total")
        && object.iter().all(|(name, value)| {
            (name == "Total" || Lang::named(name).is_some()) && valid_language(value)
        })
}

fn valid_language(value: &Value) -> bool {
    valid_counts(value)
        && valid_reports(value.get("reports"))
        && matches!(value.get("inaccurate"), Some(Value::Bool))
        && value
            .get("children")
            .and_then(Value::object)
            .is_some_and(|children| {
                children.iter().all(|(name, reports)| {
                    Lang::named(name).is_some() && valid_reports(Some(reports))
                })
            })
}

fn valid_reports(value: Option<&Value>) -> bool {
    value.and_then(Value::array).is_some_and(|reports| {
        reports.iter().all(|report| {
            report.get("name").and_then(Value::string).is_some()
                && report.get("stats").is_some_and(valid_stats)
        })
    })
}

fn valid_stats(stats: &Value) -> bool {
    valid_counts(stats)
        && stats
            .get("blobs")
            .and_then(Value::object)
            .is_some_and(|blobs| {
                blobs
                    .iter()
                    .all(|(name, stats)| Lang::named(name).is_some() && valid_stats(stats))
            })
}

fn valid_counts(value: &Value) -> bool {
    ["blanks", "code", "comments"]
        .iter()
        .all(|key| value.get(key).and_then(Value::unsigned).is_some())
}

pub(crate) fn input_error(input: &str) {
    eprintln!("Error:\n Failed to parse input file: {input}\n\nrlc accepts JSON reports.");
}
