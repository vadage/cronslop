//! Computes the maximum possible gap, in seconds, between consecutive runs
//! of a cron schedule.
//!
//! That is the worst-case "how long could this job go without firing",
//! derived by calendar arithmetic rather than by simulating time forward.
//!
//! ```
//! use cronslop::try_max_period_seconds;
//!
//! assert_eq!(try_max_period_seconds("0 * * * *").unwrap(), 3_600);
//! assert_eq!(try_max_period_seconds("0 9 * * MON-FRI").unwrap(), 259_200);
//! assert_eq!(try_max_period_seconds("@every 1h30m").unwrap(), 5_400);
//! ```
//!
//! The accepted syntax, and the input rejected as malformed, match
//! `robfig/cron`'s `ParseStandard` — the parser Kubernetes' `CronJob`
//! controller validates schedules with. The `schedule` module documents
//! the syntax in detail, and the `gap` module documents how the gap is
//! derived. Every answer is exact; there is no approximation.
//!
//! ## Deliberately out of scope: timezones and DST
//!
//! Kubernetes `CronJob`s pin a schedule to a timezone via the separate
//! `spec.timeZone` field, not the schedule string (the older `CRON_TZ=`/
//! `TZ=` prefix, while understood by the underlying library, is explicitly
//! rejected by Kubernetes' own validation). This crate therefore treats a
//! schedule as pure UTC calendar arithmetic with no DST, by design: if
//! every `CronJob` being analyzed runs in UTC, there is no daylight-saving
//! transition to account for, and modeling one would add real complexity
//! for zero benefit. If that assumption ever stops holding, the per-day
//! calculations in `gap` would need to account for the 23- and 25-hour
//! days around a transition in the target timezone.
//!
//! ## Panics
//!
//! [`try_max_period_seconds`] never panics on any input: it reports every
//! rejection as a [`CronError`]. [`max_period_seconds`] is the one
//! exception, and panics by documented contract.

/// The README's Rust examples, compiled and run as doctests so they cannot
/// drift from the real API. Not compiled outside `cargo test --doc`.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeExamples;

mod duration;
mod error;
mod gap;
mod schedule;
mod value_set;

pub use crate::error::CronError;

/// Returns the maximum gap, in seconds, between consecutive runs of a
/// Kubernetes-valid cron schedule.
///
/// Accepts standard 5-field cron, its `@yearly`/`@monthly`/`@weekly`/
/// `@daily`/`@midnight`/`@hourly` descriptors, and `@every <duration>`,
/// where the duration itself is the period, floored at one second.
///
/// The expression is not trimmed: as in `robfig/cron`, padding around a
/// 5-field expression is fine, but a descriptor must match exactly.
///
/// # Errors
///
/// Returns the [`CronError`] describing the first problem found, for
/// anything Kubernetes' own schedule validation would also reject.
pub fn try_max_period_seconds(expr: &str) -> Result<u64, CronError> {
    match expr.strip_prefix(EVERY_PREFIX) {
        Some(duration) => duration::parse_seconds(duration),
        None => Ok(gap::max_gap_seconds(&schedule::Schedule::parse(expr)?)),
    }
}

/// The `@every` descriptor, including the space that must follow it.
///
/// `robfig/cron` matches this prefix literally, so `@every1h` is not an
/// `@every` schedule at all and `@every  1h` has a duration that starts
/// with a space. Both are rejected, and `expr` is never trimmed, so that
/// a schedule Kubernetes would refuse is refused here too.
const EVERY_PREFIX: &str = "@every ";

/// Convenience wrapper over [`try_max_period_seconds`] for callers who know
/// their input is already valid and would rather panic on a bug than thread
/// a `Result` through.
///
/// Prefer [`try_max_period_seconds`] for schedules that were not already
/// validated elsewhere (e.g. by the Kubernetes API server itself).
///
/// # Panics
///
/// If `expr` is not a valid schedule. The panic message is the
/// [`CronError`] that [`try_max_period_seconds`] would have returned.
#[must_use = "the computed period is the only result of this call"]
#[expect(
    clippy::panic,
    reason = "panicking on invalid input is this function's documented contract; \
              `try_max_period_seconds` is the non-panicking form"
)]
pub fn max_period_seconds(expr: &str) -> u64 {
    try_max_period_seconds(expr).unwrap_or_else(|error| panic!("{error}"))
}
