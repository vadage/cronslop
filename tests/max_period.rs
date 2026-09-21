//! The maximum gap each kind of schedule produces, grouped so a failure
//! names the shape of schedule that broke.

use cronslop::max_period_seconds;

const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;

#[track_caller]
fn assert_periods(cases: &[(&str, u64)]) {
    for &(expr, expected) in cases {
        assert_eq!(max_period_seconds(expr), expected, "{expr}");
    }
}

#[test]
fn sub_hourly_schedules() {
    assert_periods(&[
        ("* * * * *", MINUTE),
        ("*/1 * * * *", MINUTE),
        ("*/2 * * * *", 2 * MINUTE),
        ("*/5 * * * *", 5 * MINUTE),
        ("*/10 * * * *", 10 * MINUTE),
        ("*/15 * * * *", 15 * MINUTE),
        ("*/30 * * * *", 30 * MINUTE),
        ("0,30 * * * *", 30 * MINUTE),
        ("0,15,30,45 * * * *", 15 * MINUTE),
        ("5,20,35,50 * * * *", 15 * MINUTE),
    ]);
}

#[test]
fn hourly_schedules() {
    assert_periods(&[
        ("0 * * * *", HOUR),
        ("0 */2 * * *", 2 * HOUR),
        ("0 */6 * * *", 6 * HOUR),
        ("0 */12 * * *", 12 * HOUR),
        ("0 8,12,16,20 * * *", 12 * HOUR),
        ("15 */4 * * *", 4 * HOUR),
        // Fires at 6,9,12,15,18 — the wait is from 18:30 to 06:30.
        ("30 6-18/3 * * *", 12 * HOUR),
    ]);
}

#[test]
fn daily_schedules() {
    assert_periods(&[
        ("0 0 * * *", DAY),
        ("0 12 * * *", DAY),
        ("30 8 * * *", DAY),
        ("0 9 * * *", DAY),
        ("30 9 * * *", DAY),
        ("0 18 * * *", DAY),
        ("45 23 * * *", DAY),
        // Restricted, but to every possible value.
        ("0 0 * * 0-6", DAY),
        ("0 0 1-31 * *", DAY),
        ("0 0 * 1-12 *", DAY),
    ]);
}

#[test]
fn day_of_week_schedules() {
    assert_periods(&[
        ("0 0 * * 0", 7 * DAY),
        ("0 0 * * 1", 7 * DAY),
        ("0 0 * * 5", 7 * DAY),
        ("0 0 * * 6", 7 * DAY),
        // Weekdays only: the wait is the weekend, Friday to Monday.
        ("0 9 * * 1-5", 3 * DAY),
        ("0 18 * * 1-5", 3 * DAY),
        ("30 8 * * 1-5", 3 * DAY),
        ("0 9 * * 1,3,5", 3 * DAY),
        ("0 9 * * 2,4", 5 * DAY),
        ("0 9 * * 0,6", 6 * DAY),
        // Daytime windows: the wait spans the night plus the weekend.
        ("0 9-17 * * 1-5", 3 * DAY - 8 * HOUR),
        ("*/10 9-17 * * 1-5", 3 * DAY - 8 * HOUR - 50 * MINUTE),
        ("0 */2 * * 1-5", 2 * DAY + 2 * HOUR),
        ("*/5 8-17 * * 1-5", 3 * DAY - 9 * HOUR - 55 * MINUTE),
    ]);
}

#[test]
fn day_of_month_and_month_schedules() {
    assert_periods(&[
        // Longest month-to-month wait is Jan 1 -> Feb 1 in a leap year.
        ("0 0 1 * *", 31 * DAY),
        ("0 0 15 * *", 31 * DAY),
        ("0 0 1,15 * *", 17 * DAY),
        ("0 0 1-7 * *", 25 * DAY),
        ("0 0 1 */3 *", 92 * DAY),
        ("0 0 1 */6 *", 184 * DAY),
        ("0 0 1 1 *", 366 * DAY),
        ("0 0 1 1,7 *", 184 * DAY),
        ("0 9 1-5 * *", 27 * DAY),
        ("0 9 1,15 * *", 17 * DAY),
        ("0 0 1 1-12 *", 31 * DAY),
        ("0 0 1-7 1,4,7,10 *", 86 * DAY),
    ]);
}

#[test]
fn leap_day_only_schedule_waits_for_a_real_leap_year() {
    // Feb 29 exists only in leap years, and the Gregorian rule skips
    // century years, so the worst case is an 8-year wait (e.g. 2096 to
    // 2104, spanning the non-leap year 2100).
    assert_eq!(max_period_seconds("0 0 29 2 *"), 2_921 * DAY);
}

#[test]
fn day_of_month_and_day_of_week_are_ored() {
    // "the 1st, or any Monday" — bounded by the weekly term.
    assert_eq!(max_period_seconds("0 9 1 * 1"), 7 * DAY);
}

#[test]
fn question_mark_behaves_as_a_wildcard() {
    assert_periods(&[
        ("0 0 * * ?", DAY),
        ("0 0 ? * 1-5", 3 * DAY),
        ("0 0 1 * ?", 31 * DAY),
        ("0 0 ? * 0", 7 * DAY),
        ("*/15 * * * ?", 15 * MINUTE),
        ("0 */6 * * ?", 6 * HOUR),
        ("0 9 ? * 1-5", 3 * DAY),
        ("0 0 1 ? *", 31 * DAY),
    ]);
}

#[test]
fn descriptors_match_the_expressions_they_stand_for() {
    let cases = [
        ("@yearly", "0 0 1 1 *"),
        ("@annually", "0 0 1 1 *"),
        ("@monthly", "0 0 1 * *"),
        ("@weekly", "0 0 * * 0"),
        ("@daily", "0 0 * * *"),
        ("@midnight", "0 0 * * *"),
        ("@hourly", "0 * * * *"),
    ];
    for (descriptor, equivalent) in cases {
        assert_eq!(max_period_seconds(descriptor), max_period_seconds(equivalent), "{descriptor}");
    }
}
