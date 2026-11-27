use std::collections::BTreeMap;
use std::ops::AddAssign;
use std::path::PathBuf;

use crate::language::Lang;

#[derive(Clone, Copy, Default)]
pub(crate) struct Counts {
    pub(crate) blanks: usize,
    pub(crate) code: usize,
    pub(crate) comments: usize,
}

impl Counts {
    pub(crate) fn lines(self) -> usize {
        self.blanks + self.code + self.comments
    }
}

impl AddAssign for Counts {
    fn add_assign(&mut self, other: Self) {
        self.blanks += other.blanks;
        self.code += other.code;
        self.comments += other.comments;
    }
}

#[derive(Clone, Default)]
pub(crate) struct CodeStats {
    pub(crate) counts: Counts,
    pub(crate) blobs: BTreeMap<Lang, Self>,
}

impl CodeStats {
    pub(crate) fn merge(&mut self, other: Self) {
        self.counts += other.counts;
        for (lang, stats) in other.blobs {
            self.blobs.entry(lang).or_default().merge(stats);
        }
    }

    pub(crate) fn summary(&self) -> Counts {
        let mut counts = self.counts;
        for stats in self.blobs.values() {
            counts += stats.summary();
        }
        counts
    }
}

#[derive(Clone)]
pub(crate) struct Report {
    pub(crate) path: PathBuf,
    pub(crate) stats: CodeStats,
}

#[derive(Default)]
pub(crate) struct Language {
    pub(crate) counts: Counts,
    pub(crate) reports: Vec<Report>,
    pub(crate) children: BTreeMap<Lang, Vec<Report>>,
    pub(crate) inaccurate: bool,
}

impl Language {
    pub(crate) fn add(&mut self, report: Report) {
        self.counts += report.stats.counts;
        for (&lang, stats) in &report.stats.blobs {
            self.children.entry(lang).or_default().push(Report {
                path: report.path.clone(),
                stats: stats.clone(),
            });
        }
        self.reports.push(report);
    }

    pub(crate) fn summary(&self) -> Counts {
        let mut counts = self.counts;
        for report in self.children.values().flatten() {
            counts += report.stats.summary();
        }
        counts
    }
}

pub(crate) type Languages = BTreeMap<Lang, Language>;
