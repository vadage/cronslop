//! Compatibility with `robfig/cron`'s `ParseStandard`, the parser
//! Kubernetes validates `CronJob` schedules with: named values, the
//! `N/step` rule, `@every`, and the errors Kubernetes' own parser raises.
#![expect(clippy::unwrap_used, clippy::panic, reason = "a test that cannot panic cannot fail")]

use cronslop::{CronError, try_max_period_seconds};

/// Asserts that two spellings of the same schedule agree.
#[track_caller]
fn assert_same_period(left: &str, right: &str) {
    assert_eq!(
        try_max_period_seconds(left).unwrap(),
        try_max_period_seconds(right).unwrap(),
        "{left} vs {right}"
    );
}

#[test]
fn named_weekdays_match_numeric_equivalents() {
    assert_same_period("0 9 * * MON-FRI", "0 9 * * 1-5");
    assert_same_period("0 9 * * mon,wed,fri", "0 9 * * 1,3,5");
    assert_same_period("0 0 * * Sun", "0 0 * * 0");
}

#[test]
fn named_months_match_numeric_equivalents() {
    assert_same_period("0 0 1 JAN *", "0 0 1 1 *");
    assert_same_period("0 0 1 jan,jul *", "0 0 1 1,7 *");
}

#[test]
fn bare_value_with_step_runs_to_field_max() {
    // "5/15" on minutes means 5,20,35,50 (5 through 59 by 15) — NOT just
    // "5". Internal gap is 15 min, and the wrap from :50 back to :05 next
    // hour is also 15 min, so the max is 900s either way; the real
    // assertion is that it behaves like the equivalent list, not like a
    // lone value (which would give 3600s).
    assert_same_period("5/15 * * * *", "5,20,35,50 * * * *");
    assert_ne!(try_max_period_seconds("5/15 * * * *").unwrap(), 3_600);
}

#[test]
fn every_duration_is_the_period_itself() {
    assert_eq!(try_max_period_seconds("@every 1h30m").unwrap(), 5_400);
    assert_eq!(try_max_period_seconds("@every 90s").unwrap(), 90);
    assert_eq!(try_max_period_seconds("@every 1h").unwrap(), 3_600);
    assert_eq!(try_max_period_seconds("@every 1h30m10s").unwrap(), 5_410);
    // Truncated to whole seconds, never rounded up.
    assert_eq!(try_max_period_seconds("@every 1.999s").unwrap(), 1);
}

#[test]
fn every_never_yields_a_period_below_one_second() {
    // `cron.Every` clamps anything under a second up to one second, so a
    // sub-second, zero or negative duration is a one-second period — not
    // zero, and not an error.
    for expr in ["@every 500ms", "@every 1ns", "@every 0", "@every 0s", "@every -1h"] {
        assert_eq!(try_max_period_seconds(expr).unwrap(), 1, "{expr}");
    }
}

#[test]
fn every_requires_exactly_one_space_before_the_duration() {
    // `robfig/cron` matches the literal prefix "@every ", so neither of
    // these is an `@every` schedule and Kubernetes rejects both.
    for expr in ["@every1h", "@every  1h", "@everything 1h", "@every"] {
        assert!(try_max_period_seconds(expr).is_err(), "expected '{expr}' to be rejected");
    }
}

#[test]
fn every_duration_is_bounded_by_go_s_i64_of_nanoseconds() {
    // `time.Duration` counts nanoseconds in an i64, and `ParseDuration`
    // errors past that, so the accept/reject boundary sits here.
    assert_eq!(try_max_period_seconds("@every 9223372036854775807ns").unwrap(), 9_223_372_036);
    assert!(try_max_period_seconds("@every 9223372036854775808ns").is_err());
}

#[test]
fn schedules_are_not_trimmed() {
    // `strings.Fields` tolerates padding around a 5-field expression, but
    // a descriptor is matched against the raw string, so a leading space
    // stops it being one.
    assert_eq!(try_max_period_seconds(" 0 0 * * * ").unwrap(), 86_400);
    assert!(try_max_period_seconds(" @hourly").is_err());
    assert!(try_max_period_seconds("@hourly ").is_err());
}

/// Asserts that `expr` is rejected, and names the error it must produce.
#[track_caller]
fn assert_rejected(expr: &str, expected: &CronError) {
    match try_max_period_seconds(expr) {
        Err(actual) => assert_eq!(&actual, expected, "{expr}"),
        Ok(seconds) => panic!("expected '{expr}' to be rejected, got {seconds}s"),
    }
}

#[test]
fn day_of_week_7_is_rejected_not_folded() {
    // Kubernetes' parser bounds day-of-week at 0-6 strictly; unlike the
    // older POSIX convention, 7 is not accepted as an alias for Sunday and
    // must error, not silently become 0.
    assert_rejected("0 0 * * 7", &CronError::AboveMax { field: "day-of-week", value: 7, max: 6 });
}

#[test]
fn reversed_range_is_rejected() {
    assert_rejected(
        "0 0 * * 5-3",
        &CronError::ReversedRange { field: "day-of-week", start: 5, end: 3 },
    );
}

#[test]
fn zero_step_is_rejected() {
    assert_rejected("*/0 * * * *", &CronError::ZeroStep { field: "minute" });
}

#[test]
fn wrong_field_count_is_rejected() {
    assert_rejected("0 0 * * * *", &CronError::WrongFieldCount { expected: 5, found: 6 });
    assert_rejected("0 0 * *", &CronError::WrongFieldCount { expected: 5, found: 4 });
}

#[test]
fn unknown_name_is_rejected() {
    assert_rejected(
        "0 0 * * MONDAY",
        &CronError::UnknownName { field: "day-of-week", text: "MONDAY".to_owned() },
    );
}

#[test]
fn out_of_bounds_values_are_rejected() {
    assert_rejected("60 0 * * *", &CronError::AboveMax { field: "minute", value: 60, max: 59 });
    assert_rejected("0 24 * * *", &CronError::AboveMax { field: "hour", value: 24, max: 23 });
    assert_rejected("0 0 0 * *", &CronError::BelowMin { field: "day-of-month", value: 0, min: 1 });
    assert_rejected("0 0 1 13 *", &CronError::AboveMax { field: "month", value: 13, max: 12 });
}

#[test]
fn malformed_terms_are_rejected() {
    assert_rejected(
        "0 0 1//2 * *",
        &CronError::TooManySlashes { field: "day-of-month", text: "1//2".to_owned() },
    );
    assert_rejected(
        "0 0 1-2-3 * *",
        &CronError::TooManyHyphens { field: "day-of-month", text: "1-2-3".to_owned() },
    );
    assert_rejected(
        "0 0 */a * *",
        &CronError::BadNumber { field: "day-of-month", text: "a".to_owned() },
    );
}

#[test]
fn malformed_every_durations_are_rejected() {
    for expr in ["@every ", "@every 5x", "@every h", "@every 1.2.3s", "@every 1h 30m"] {
        assert!(
            matches!(try_max_period_seconds(expr), Err(CronError::InvalidDuration(_))),
            "expected '{expr}' to be rejected"
        );
    }
}
