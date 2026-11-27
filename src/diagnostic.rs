use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static VERBOSITY: AtomicU8 = AtomicU8::new(0);

pub(crate) fn configure(verbosity: u8) {
    VERBOSITY.store(
        if verbosity <= 3 { verbosity } else { 0 },
        Ordering::Relaxed,
    );
}

pub(crate) fn error(target: &str, message: fmt::Arguments<'_>) {
    log("ERROR", target, message);
}

pub(crate) fn warn(target: &str, message: fmt::Arguments<'_>) {
    if VERBOSITY.load(Ordering::Relaxed) >= 1 {
        log("WARN", target, message);
    }
}

pub(crate) fn debug(target: &str, message: fmt::Arguments<'_>) {
    if VERBOSITY.load(Ordering::Relaxed) >= 2 {
        log("DEBUG", target, message);
    }
}

pub(crate) fn tracing() -> bool {
    VERBOSITY.load(Ordering::Relaxed) >= 3
}

pub(crate) fn trace(target: &str, message: fmt::Arguments<'_>) {
    log("TRACE", target, message);
}

fn log(level: &str, target: &str, message: fmt::Arguments<'_>) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (year, month, day) = date(now / 86400);
    let hour = now / 3600 % 24;
    let minute = now / 60 % 60;
    let second = now % 60;
    eprintln!(
        "[{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z {level:<5} {target}] {message}"
    );
}

fn date(days_since_epoch: u64) -> (u64, usize, u64) {
    // A Gregorian 400-year cycle contains 146,097 days.
    let mut year = 1970 + 400 * (days_since_epoch / 146097);
    let mut day = days_since_epoch % 146097;
    let leap = |year: u64| {
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
    };
    while day >= 365 + u64::from(leap(year)) {
        day -= 365 + u64::from(leap(year));
        year += 1;
    }
    let months = [
        31,
        28 + u64::from(leap(year)),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while day >= months[month] {
        day -= months[month];
        month += 1;
    }
    (year, month + 1, day + 1)
}
