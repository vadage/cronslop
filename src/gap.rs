//! Computing the longest gap between consecutive runs of a schedule,
//! by calendar arithmetic rather than by simulating time forward.
//!
//! The idea is that a schedule's firing days repeat on a fixed cycle — a
//! 7-day cycle for a day-of-week restriction, a one-year cycle for a
//! day-of-month/month restriction — so the worst-case gap is the largest
//! step between consecutive active days in one cycle (including the wrap
//! into the next), adjusted by the times of day the schedule fires at.
//!
//! ## The one approximation
//!
//! When day-of-month/month AND day-of-week are both restricted at once
//! (cron's OR rule: "the 13th, or any Friday"), the exact answer can
//! depend on which weekday a given year's day-of-month lands on — a real
//! multi-year calendar dependency. Rather than simulate arbitrarily far
//! forward, this returns the smaller of the two bounds taken separately,
//! which is always safe: OR'ing in a second condition can only ever create
//! more matching days, never fewer, so the true answer is never larger
//! than either bound alone. That is exact whenever month is unrestricted
//! (verified against real calendar simulation), and conservative in the
//! rarer case where month is restricted too.

use crate::schedule::Schedule;
use crate::value_set::ValueSet;

const SECONDS_PER_MINUTE: u32 = 60;
const SECONDS_PER_HOUR: u32 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * 3_600;
const DAYS_IN_WEEK: u64 = 7;
const DAYS_IN_LEAP_YEAR: u64 = 366;

/// The true maximum gap, in days, between consecutive February 29ths —
/// what a schedule restricted to the leap day alone actually waits for.
/// Normally 4 years (1461 days), but 8 years around a century year the
/// Gregorian rule skips, such as 1900 or 2100. Verified against a scan of
/// real leap years in [`tests::leap_day_gap_matches_the_real_calendar`].
const LEAP_DAY_MAX_GAP_DAYS: u64 = 2_921;

/// The maximum gap, in seconds, between consecutive runs of `schedule`.
pub(crate) fn max_gap_seconds(schedule: &Schedule) -> u64 {
    let Some(times) = FiringTimes::of(schedule) else {
        return 0; // a field matched nothing, so the schedule never fires
    };

    if !schedule.day_of_week.is_restricted() {
        // The calendar alone decides which days fire.
        calendar_gap_seconds(schedule, times)
    } else if !schedule.day_of_month.is_restricted() && !schedule.month.is_restricted() {
        // The weekday alone decides, and a 7-day cycle is exact.
        weekday_gap_seconds(schedule, times)
    } else {
        // Both restricted: cron ORs them. See the approximation note above.
        weekday_gap_seconds(schedule, times).min(calendar_gap_seconds(schedule, times))
    }
}

/// What the gap arithmetic needs to know about the times of day a
/// schedule fires at.
///
/// Only three facts matter: the first and last firing of a day, and the
/// longest wait between two firings within one day. All three follow from
/// the hour and minute sets directly, so the full cross product — up to
/// 1440 times for `* * * * *` — is never built.
#[derive(Debug, Clone, Copy)]
struct FiringTimes {
    /// Seconds from midnight to the day's first firing.
    first: u32,
    /// Seconds from midnight to the day's last firing.
    last: u32,
    /// The longest wait between two firings on the same day, in seconds.
    /// Zero when the schedule fires once a day.
    max_gap_within_day: u64,
}

impl FiringTimes {
    /// `None` if the schedule fires at no time at all, which a parsed
    /// schedule never does.
    fn of(schedule: &Schedule) -> Option<Self> {
        let hours = Spread::of(schedule.hour.values)?;
        let minutes = Spread::of(schedule.minute.values)?;

        // Sorted, the firing times run minute by minute within an hour and
        // then jump to the next hour, so a within-day wait is one of two
        // things: the widest step between two minutes, or the widest step
        // between two hours, less the minutes already covered in between.
        let within_hour = minutes.max_step.saturating_mul(SECONDS_PER_MINUTE);
        let across_hours = hours
            .max_step
            .saturating_mul(SECONDS_PER_HOUR)
            .saturating_sub(minutes.span().saturating_mul(SECONDS_PER_MINUTE));

        Some(Self {
            first: seconds_since_midnight(hours.first, minutes.first),
            last: seconds_since_midnight(hours.last, minutes.last),
            max_gap_within_day: u64::from(within_hour.max(across_hours)),
        })
    }

    /// Seconds from the day's first firing to its last.
    const fn span(self) -> u32 {
        self.last.saturating_sub(self.first)
    }
}

/// The shape of a set of values: its ends, and the widest step between two
/// consecutive members.
#[derive(Debug, Clone, Copy)]
struct Spread {
    first: u32,
    last: u32,
    /// Zero when the set holds a single value.
    max_step: u32,
}

impl Spread {
    /// `None` for an empty set.
    fn of(values: ValueSet) -> Option<Self> {
        let mut remaining = values.iter();
        let first = remaining.next()?;
        let mut last = first;
        let mut max_step = 0;
        for value in remaining {
            max_step = max_step.max(value.saturating_sub(last));
            last = value;
        }
        Some(Self { first, last, max_step })
    }

    /// The distance from the set's first value to its last.
    const fn span(self) -> u32 {
        self.last.saturating_sub(self.first)
    }
}

/// Seconds from midnight to `hour:minute`. Both values come from parsed
/// fields (0-23 and 0-59), so saturation is unreachable.
const fn seconds_since_midnight(hour: u32, minute: u32) -> u32 {
    hour.saturating_mul(SECONDS_PER_HOUR).saturating_add(minute.saturating_mul(SECONDS_PER_MINUTE))
}

/// Seconds from the last firing of one active day to the first firing
/// `day_span` days later — the schedule's overnight wait.
fn gap_across_days(day_span: u64, times: FiringTimes) -> u64 {
    // The wait is shorter than the whole span by however much of the day
    // the firing times already cover, which is always under 24 hours.
    day_span.saturating_mul(SECONDS_PER_DAY).saturating_sub(u64::from(times.span()))
}

/// The largest gap in seconds given the active day offsets within one
/// repeating cycle (e.g. day-of-year 0..365, or day-of-week 0..6), the
/// cycle's length in days, and the times of day that fire on each active
/// day. Includes the wrap from the last active day of one cycle to the
/// first of the next.
fn max_gap_from_days(days: &[u64], cycle_len_days: u64, times: FiringTimes) -> u64 {
    let (Some(&first_day), Some(&last_day)) = (days.first(), days.last()) else {
        return 0; // schedule never fires
    };

    let wrap_span = cycle_len_days.saturating_sub(last_day).saturating_add(first_day);
    pairs(days)
        .map(|(day, next)| next.saturating_sub(day))
        .chain(std::iter::once(wrap_span))
        .map(|day_span| gap_across_days(day_span, times))
        .chain(std::iter::once(times.max_gap_within_day))
        .max()
        .unwrap_or(0)
}

/// Each consecutive pair of `values`, by value: `[a, b, c]` -> `(a, b)`,
/// `(b, c)`. Empty for slices shorter than two.
fn pairs<T: Copy>(values: &[T]) -> impl Iterator<Item = (T, T)> + '_ {
    values.iter().zip(values.iter().skip(1)).map(|(&value, &next)| (value, next))
}

/// The maximum gap from day-of-month and month restrictions alone,
/// ignoring day-of-week.
///
/// The ordinary case is resolved with a single pass over a leap-year
/// calendar, which never under-reports: February is the only month whose
/// length varies, and its 29 leap days are always >= the 28 common-year
/// ones, so any gap spanning February can only be equal or longer under
/// the leap model.
///
/// The one thing that model cannot express is a schedule that fires only
/// in leap years (day 29 restricted to February). That shows up as a
/// schedule matching under the leap calendar but never under a common
/// one, and needs the multi-century [`LEAP_DAY_MAX_GAP_DAYS`].
fn calendar_gap_seconds(schedule: &Schedule, times: FiringTimes) -> u64 {
    let leap_days = active_days_of_year(schedule, &LEAP_YEAR);

    if !leap_days.is_empty() && !fires_in_common_year(schedule) {
        return gap_across_days(LEAP_DAY_MAX_GAP_DAYS, times);
    }
    max_gap_from_days(&leap_days, DAYS_IN_LEAP_YEAR, times)
}

/// The maximum gap from a day-of-week restriction alone, ignoring
/// day-of-month and month. Exact whenever month is unrestricted: the
/// spacing between weekdays in a repeating 7-day cycle does not depend on
/// which real dates they fall on.
fn weekday_gap_seconds(schedule: &Schedule, times: FiringTimes) -> u64 {
    let days: Vec<u64> = schedule.day_of_week.values.iter().map(u64::from).collect();
    max_gap_from_days(&days, DAYS_IN_WEEK, times)
}

/// The 0-based day-of-year offsets that day-of-month and month make
/// active, ascending. Day-of-week is ignored.
fn active_days_of_year(schedule: &Schedule, year: &Year) -> Vec<u64> {
    let day_of_month = &schedule.day_of_month;
    // At most one entry per allowed day in each month.
    let mut days = Vec::with_capacity(year.len().saturating_mul(day_of_month.values.count()));
    for month in months_of(schedule, year) {
        if day_of_month.is_restricted() {
            // `up_to` drops the days this month does not have, such as
            // the 30th in February.
            let matching = day_of_month.values.up_to(month.len);
            days.extend(matching.iter().map(|day| u64::from(month.day_of_year(day))));
        } else {
            days.extend(month.days_of_year().map(u64::from));
        }
    }
    days
}

/// The months of `year` that the schedule's month field allows.
fn months_of<'a>(schedule: &'a Schedule, year: &'a Year) -> impl Iterator<Item = &'a Month> {
    year.iter().filter(|month| schedule.month.values.contains(month.number))
}

/// Whether the schedule matches any day at all in a 365-day year. Answers
/// the same question as `active_days_of_year(.., &COMMON_YEAR).is_empty()`
/// without building the day list.
fn fires_in_common_year(schedule: &Schedule) -> bool {
    let day_of_month = &schedule.day_of_month;
    months_of(schedule, &COMMON_YEAR).any(|month| {
        !day_of_month.is_restricted() || !day_of_month.values.up_to(month.len).is_empty()
    })
}

/// One month of a year of fixed length.
#[derive(Debug, Clone, Copy)]
struct Month {
    /// 1-12.
    number: u32,
    /// How many days the month has.
    len: u32,
    /// 0-based day-of-year offset of the month's first day.
    first_day_of_year: u32,
}

impl Month {
    /// The 0-based day-of-year offset of this month's `day` (1-based).
    /// `day` comes from a day-of-month field, so it is at least 1 and
    /// saturation is unreachable.
    const fn day_of_year(&self, day: u32) -> u32 {
        self.first_day_of_year.saturating_add(day).saturating_sub(1)
    }

    /// The 0-based day-of-year offsets of every day in this month.
    fn days_of_year(&self) -> impl Iterator<Item = u32> {
        (1..=self.len).zip(self.first_day_of_year..).map(|(_, day_of_year)| day_of_year)
    }
}

/// The twelve months of a year, with their day-of-year offsets resolved.
type Year = [Month; 12];

const LEAP_YEAR: Year = year_of([31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]);
const COMMON_YEAR: Year = year_of([31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]);

/// Builds a [`Year`] from its month lengths, accumulating the day-of-year
/// offsets once, at compile time.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "const evaluation: an out-of-range index or an overflow here is a compile error, \
              not a runtime panic, and the loop is bounded by the array's own length"
)]
const fn year_of(lengths: [u32; 12]) -> Year {
    let mut months = [Month { number: 0, len: 0, first_day_of_year: 0 }; 12];
    let mut index = 0;
    let mut number = 1;
    let mut first_day_of_year = 0;
    while index < months.len() {
        months[index] = Month { number, len: lengths[index], first_day_of_year };
        first_day_of_year += lengths[index];
        number += 1;
        index += 1;
    }
    months
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests are allowed to panic, and are the place the constants above get checked"
)]
mod tests {
    use super::{
        COMMON_YEAR, DAYS_IN_LEAP_YEAR, FiringTimes, LEAP_DAY_MAX_GAP_DAYS, LEAP_YEAR, Schedule,
        Year,
    };

    const fn is_leap_year(year: i64) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    fn assert_offsets_are_cumulative(year: &Year, total_days: u32) {
        let mut expected_offset = 0;
        for (index, month) in year.iter().enumerate() {
            let number = u32::try_from(index).unwrap() + 1;
            assert_eq!(month.number, number, "month number");
            assert_eq!(
                month.first_day_of_year, expected_offset,
                "offset of month {}",
                month.number
            );
            expected_offset += month.len;
        }
        assert_eq!(expected_offset, total_days, "days in year");
    }

    #[test]
    fn calendar_tables_are_consistent() {
        assert_offsets_are_cumulative(&LEAP_YEAR, u32::try_from(DAYS_IN_LEAP_YEAR).unwrap());
        assert_offsets_are_cumulative(&COMMON_YEAR, 365);
        assert_eq!(LEAP_YEAR[1].len, 29, "February, leap year");
        assert_eq!(COMMON_YEAR[1].len, 28, "February, common year");
    }

    /// The summary in [`FiringTimes`] replaces building every firing time
    /// of the day. It must agree with doing exactly that.
    #[test]
    fn firing_times_match_the_full_cross_product() {
        const MINUTE_FIELDS: [&str; 10] =
            ["0", "30", "59", "0,30", "0,1", "5,20,35,50", "*/15", "*/5", "0-58/29", "*"];
        const HOUR_FIELDS: [&str; 10] =
            ["0", "9", "23", "0,23", "9-17", "*/6", "*/2", "1,2,3", "0,12", "*"];

        for minute_field in MINUTE_FIELDS {
            for hour_field in HOUR_FIELDS {
                let expr = format!("{minute_field} {hour_field} * * *");
                let schedule = Schedule::parse(&expr).unwrap();
                let times = FiringTimes::of(&schedule).unwrap();

                // The reference: every firing time of the day, in order.
                let mut all = Vec::new();
                for hour in schedule.hour.values.iter() {
                    for minute in schedule.minute.values.iter() {
                        all.push(super::seconds_since_midnight(hour, minute));
                    }
                }
                all.sort_unstable();

                assert_eq!(times.first, all[0], "first firing of '{expr}'");
                assert_eq!(times.last, *all.last().unwrap(), "last firing of '{expr}'");
                let widest =
                    all.windows(2).map(|pair| u64::from(pair[1] - pair[0])).max().unwrap_or(0);
                assert_eq!(times.max_gap_within_day, widest, "widest wait within '{expr}'");
            }
        }
    }

    #[test]
    fn leap_day_gap_matches_the_real_calendar() {
        // The longest run of days between two February 29ths, found by
        // walking real Gregorian leap years across three millennia.
        let leap_years: Vec<i64> = (1..3000).filter(|&year| is_leap_year(year)).collect();
        let longest = leap_years
            .windows(2)
            .map(|pair| {
                (pair[0]..pair[1])
                    .map(|year| if is_leap_year(year) { 366 } else { 365 })
                    .sum::<u64>()
            })
            .max()
            .unwrap();

        assert_eq!(longest, LEAP_DAY_MAX_GAP_DAYS);
        assert_eq!(longest, 8 * 365 + 1, "eight years spanning a skipped century leap day");
    }
}
