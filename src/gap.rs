//! Computing the longest gap between consecutive runs of a schedule.
//!
//! Every answer here is exact. Three strategies are used, picked by what
//! the schedule restricts, from cheapest to most general:
//!
//!   - **A 7-day cycle**, when only day-of-week is restricted. Weekday
//!     spacing does not depend on the calendar, so this is exact.
//!   - **A one-year cycle**, when only the calendar is restricted. A leap
//!     year is enough: February is the only month whose length varies, so
//!     a gap spanning it is at its longest in a leap year.
//!   - **A full Gregorian cycle**, otherwise — see [`cycle_gap_seconds`].
//!
//! The third case is needed whenever weekdays are combined with a
//! restricted calendar (cron's OR rule: "the 13th, or any Friday, in
//! March"), or whenever a schedule can fire on February the 29th. Neither
//! repeats on a short cycle: the weekday a given date falls on drifts
//! year to year, and the 29th exists only in leap years. Walking 400
//! years settles both, because the Gregorian calendar repeats exactly
//! then.
//!
//! An earlier version approximated the OR case as the smaller of the two
//! bounds taken separately. That was wrong, not merely imprecise: the
//! day-of-week bound ignores the month field, so "any Friday in March"
//! was answered with 7 days against a true 343. Differential testing
//! against `robfig/cron` and an independent calendar walk found it; see
//! `tools/differential/`.

use crate::schedule::Schedule;
use crate::value_set::ValueSet;

const SECONDS_PER_MINUTE: u32 = 60;
const SECONDS_PER_HOUR: u32 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * 3_600;
const DAYS_IN_WEEK: u64 = 7;
const DAYS_IN_LEAP_YEAR: u32 = 366;
const DAYS_IN_COMMON_YEAR: u32 = 365;

/// The maximum gap, in seconds, between consecutive runs of `schedule`.
pub(crate) fn max_gap_seconds(schedule: &Schedule) -> u64 {
    let Some(times) = FiringTimes::of(schedule) else {
        return 0; // a field matched nothing, so the schedule never fires
    };

    if !schedule.day_of_week.is_restricted() {
        if includes_leap_day(schedule) {
            // February the 29th exists only in leap years, which a
            // single-year calendar cannot express.
            cycle_gap_seconds(schedule, times)
        } else {
            // The calendar alone decides, and one leap year is exact.
            calendar_gap_seconds(schedule, times)
        }
    } else if !schedule.day_of_month.is_restricted() && !schedule.month.is_restricted() {
        // The weekday alone decides, and a 7-day cycle is exact.
        weekday_gap_seconds(schedule, times)
    } else {
        // Weekdays combined with a restricted calendar: no short cycle
        // expresses it, so walk a real one.
        cycle_gap_seconds(schedule, times)
    }
}

/// Whether February the 29th is one of the days this schedule fires on.
/// Only a restricted day-of-month can single it out; an unrestricted one
/// fires every day of February either way.
const fn includes_leap_day(schedule: &Schedule) -> bool {
    schedule.day_of_month.is_restricted()
        && schedule.month.values.contains(FEBRUARY)
        && schedule.day_of_month.values.contains(LEAP_DAY)
}

const FEBRUARY: u32 = 2;
const LEAP_DAY: u32 = 29;

/// The widest run of days between consecutive firings, accumulated as
/// firings are discovered in ascending order.
///
/// Nothing is collected: each strategy streams its firing days through
/// one of these, so no path allocates a day list.
#[derive(Debug, Clone, Copy, Default)]
struct DaySpans {
    first: Option<u64>,
    last: Option<u64>,
    widest: u64,
}

impl DaySpans {
    /// Records a run of firings that begins at `first`, ends at `last`,
    /// and waits at most `widest_inside` days somewhere in between. Runs
    /// must arrive in ascending order and must not overlap.
    fn record_run(&mut self, first: u64, last: u64, widest_inside: u64) {
        match self.last {
            Some(previous) => self.widest = self.widest.max(first.saturating_sub(previous)),
            None => self.first = Some(first),
        }
        self.widest = self.widest.max(widest_inside);
        self.last = Some(last);
    }

    /// Records a single firing on day `index`.
    fn record(&mut self, index: u64) {
        self.record_run(index, index, 0);
    }

    /// The widest span once the cycle wraps round, or `None` if nothing
    /// ever fired.
    fn widest_span(self, cycle_len_days: u64) -> Option<u64> {
        let (first, last) = (self.first?, self.last?);
        let wrap = cycle_len_days.saturating_sub(last).saturating_add(first);
        Some(self.widest.max(wrap))
    }
}

/// Turns accumulated day spans into the schedule's maximum gap.
fn spans_to_seconds(spans: DaySpans, cycle_len_days: u64, times: FiringTimes) -> u64 {
    // `gap_across_days` grows with the span, so the widest span gives the
    // widest gap; the only other candidate is a wait within one day. A
    // schedule that never fires has no span at all.
    spans
        .widest_span(cycle_len_days)
        .map_or(0, |span| gap_across_days(span, times).max(times.max_gap_within_day))
}

/// The maximum gap from day-of-month and month restrictions alone,
/// ignoring day-of-week.
///
/// One pass over a leap-year calendar, which never under-reports:
/// February is the only month whose length varies, and its 29 leap days
/// are always >= the 28 common-year ones, so any gap spanning February is
/// at its longest in a leap year.
///
/// Schedules that can fire on February the 29th are routed to
/// [`cycle_gap_seconds`] instead, since no single year holds both a
/// February with 29 days and one without.
fn calendar_gap_seconds(schedule: &Schedule, times: FiringTimes) -> u64 {
    let mut spans = DaySpans::default();
    record_calendar_days(schedule, &LEAP_YEAR, 0, &mut spans);
    spans_to_seconds(spans, u64::from(DAYS_IN_LEAP_YEAR), times)
}

/// The maximum gap from a day-of-week restriction alone, ignoring
/// day-of-month and month. Exact whenever month is unrestricted: the
/// spacing between weekdays in a repeating 7-day cycle does not depend on
/// which real dates they fall on.
fn weekday_gap_seconds(schedule: &Schedule, times: FiringTimes) -> u64 {
    let mut spans = DaySpans::default();
    for weekday in schedule.day_of_week.values.iter() {
        spans.record(u64::from(weekday));
    }
    spans_to_seconds(spans, DAYS_IN_WEEK, times)
}

/// Streams the day-of-year offsets that day-of-month and month make
/// active into `spans`, offset by `year_start`. Day-of-week is ignored.
fn record_calendar_days(schedule: &Schedule, year: &Year, year_start: u64, spans: &mut DaySpans) {
    let day_of_month = &schedule.day_of_month;
    for month in year.iter().filter(|month| schedule.month.values.contains(month.number)) {
        let start = year_start.saturating_add(u64::from(month.first_day_of_year));
        if day_of_month.is_restricted() {
            // `up_to` drops the days this month does not have, such as
            // the 30th in February.
            for day in day_of_month.values.up_to(month.len).iter() {
                spans.record(year_start.saturating_add(u64::from(month.day_of_year(day))));
            }
        } else {
            // Every day of the month: one run, rather than 31 records.
            let last = start.saturating_add(u64::from(month.len)).saturating_sub(1);
            spans.record_run(start, last, 1);
        }
    }
}

/// The Gregorian calendar repeats exactly every 400 years: 146,097 days,
/// which is 20,871 whole weeks. One cycle therefore contains every
/// possible alignment of weekday, day-of-month and month — including the
/// century years that skip a leap day — so walking it gives the exact
/// answer for any schedule, with no cycle assumption to be wrong about.
const CYCLE_DAYS: u64 = 146_097;

/// A year the cycle can start from: divisible by 400, so it is a leap
/// year, and its 1 January fell on a Saturday. Checked in
/// [`tests::the_gregorian_cycle_closes`].
const CYCLE_START_YEAR: i64 = 2000;
const CYCLE_START_WEEKDAY: u32 = 6;
const CYCLE_END_YEAR: i64 = CYCLE_START_YEAR + 400;

/// The maximum gap over one full Gregorian cycle. This is the fallback
/// for every schedule whose firing days do not repeat on a short cycle:
/// a restricted day-of-week combined with a restricted calendar, or any
/// schedule that can fire on February the 29th.
///
/// The cycle is not walked a day at a time. A year's firing pattern
/// depends only on its length and the weekday its 1 January falls on, so
/// there are fourteen possible years in all; those are derived once and
/// the 400 years then stitched together from them.
fn cycle_gap_seconds(schedule: &Schedule, times: FiringTimes) -> u64 {
    let shapes = YearShapes::of(schedule);

    let mut spans = DaySpans::default();
    let mut year_start: u64 = 0;
    let mut weekday = CYCLE_START_WEEKDAY;
    for year in CYCLE_START_YEAR..CYCLE_END_YEAR {
        let is_leap = is_leap_year(year);
        let shape = shapes.of_year(is_leap, weekday);
        if let Some(first) = shape.first {
            spans.record_run(
                year_start.saturating_add(first),
                year_start.saturating_add(shape.last),
                shape.widest_inside,
            );
        }
        let year_len = if is_leap { DAYS_IN_LEAP_YEAR } else { DAYS_IN_COMMON_YEAR };
        year_start = year_start.saturating_add(u64::from(year_len));
        weekday = weekday_after(weekday, year_len);
    }
    debug_assert_eq!(year_start, CYCLE_DAYS, "a Gregorian cycle is 146,097 days");

    spans_to_seconds(spans, CYCLE_DAYS, times)
}

/// Where a year's firings start and end, and the widest wait inside it.
/// Day numbers are offsets from 1 January of that year.
#[derive(Debug, Clone, Copy, Default)]
struct YearShape {
    first: Option<u64>,
    last: u64,
    widest_inside: u64,
}

/// The fourteen shapes a year of this schedule can take: two year
/// lengths, each starting on one of seven weekdays.
struct YearShapes {
    leap: [YearShape; DAYS_IN_WEEK_USIZE],
    common: [YearShape; DAYS_IN_WEEK_USIZE],
}

const DAYS_IN_WEEK_USIZE: usize = 7;

impl YearShapes {
    fn of(schedule: &Schedule) -> Self {
        let mut shapes =
            Self { leap: [YearShape::default(); 7], common: [YearShape::default(); 7] };
        let mut weekday = 0;
        for (leap, common) in shapes.leap.iter_mut().zip(shapes.common.iter_mut()) {
            *leap = year_shape(schedule, &LEAP_YEAR, weekday);
            *common = year_shape(schedule, &COMMON_YEAR, weekday);
            weekday = next_weekday(weekday);
        }
        shapes
    }

    /// The shape of a year of this length starting on this weekday.
    /// `first_weekday` is always 0-6, so the lookup always finds one.
    fn of_year(&self, is_leap: bool, first_weekday: u32) -> YearShape {
        let row = if is_leap { &self.leap } else { &self.common };
        let shape = usize::try_from(first_weekday).ok().and_then(|index| row.get(index)).copied();
        debug_assert!(shape.is_some(), "weekday {first_weekday} is out of range");
        shape.unwrap_or_default()
    }
}

/// Derives one year's shape, a month at a time.
fn year_shape(schedule: &Schedule, year: &Year, first_weekday: u32) -> YearShape {
    let mut spans = DaySpans::default();
    let mut weekday = first_weekday;
    for month in year {
        if schedule.month.values.contains(month.number) {
            for day in firing_days_of(schedule, month, weekday).iter() {
                spans.record(u64::from(month.day_of_year(day)));
            }
        }
        weekday = weekday_after(weekday, month.len);
    }
    YearShape {
        first: spans.first,
        last: spans.last.unwrap_or_default(),
        widest_inside: spans.widest,
    }
}

/// The days of `month` this schedule fires on, given the weekday its
/// first day falls on.
///
/// Whole months are resolved with bit masks rather than a day-by-day
/// test, under cron's rule that a restricted day-of-month and
/// day-of-week are OR'd together.
fn firing_days_of(schedule: &Schedule, month: &Month, first_weekday: u32) -> ValueSet {
    let (day_of_month, day_of_week) = (&schedule.day_of_month, &schedule.day_of_week);
    let by_date = day_of_month.values.up_to(month.len);
    match (day_of_month.is_restricted(), day_of_week.is_restricted()) {
        (true, true) => by_date.union(weekday_days(day_of_week.values, month.len, first_weekday)),
        (true, false) => by_date,
        (false, true) => weekday_days(day_of_week.values, month.len, first_weekday),
        (false, false) => ValueSet::ALL.up_to(month.len).at_least(1),
    }
}

/// The days of a month falling on any of `weekdays`, given the weekday
/// its first day falls on.
fn weekday_days(weekdays: ValueSet, month_len: u32, first_weekday: u32) -> ValueSet {
    let mut days = ValueSet::EMPTY;
    for weekday in weekdays.iter() {
        // The first day of the month with this weekday, 1-based.
        let offset = weekday.saturating_add(DAYS_IN_WEEK_U32).saturating_sub(first_weekday) % 7;
        days = days.union(ValueSet::every_seventh_from(offset.saturating_add(1)));
    }
    days.up_to(month_len)
}

const DAYS_IN_WEEK_U32: u32 = 7;

/// The day after `weekday`, wrapping Saturday back to Sunday.
const fn next_weekday(weekday: u32) -> u32 {
    if weekday >= 6 { 0 } else { weekday.saturating_add(1) }
}

/// The weekday `days` days after `weekday`.
const fn weekday_after(weekday: u32, days: u32) -> u32 {
    weekday.saturating_add(days % 7) % 7
}

const fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
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
    use super::{COMMON_YEAR, DAYS_IN_LEAP_YEAR, FiringTimes, LEAP_YEAR, Schedule, Year};

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
        assert_offsets_are_cumulative(&LEAP_YEAR, DAYS_IN_LEAP_YEAR);
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
        let leap_years: Vec<i64> = (1..3000).filter(|&year| super::is_leap_year(year)).collect();
        let longest = leap_years
            .windows(2)
            .map(|pair| {
                (pair[0]..pair[1])
                    .map(|year| if super::is_leap_year(year) { 366 } else { 365 })
                    .sum::<u64>()
            })
            .max()
            .unwrap();

        assert_eq!(longest, 8 * 365 + 1, "eight years spanning a skipped century leap day");

        // The cycle walk has to find the same wait without being told.
        let schedule = Schedule::parse("0 0 29 2 *").unwrap();
        let times = FiringTimes::of(&schedule).unwrap();
        assert_eq!(super::cycle_gap_seconds(&schedule, times), longest * 86_400);
    }

    #[test]
    fn the_gregorian_cycle_closes() {
        // 400 years is 146,097 days and 20,871 whole weeks, which is what
        // makes one cycle enough to see every weekday alignment.
        let days: u64 = (super::CYCLE_START_YEAR..super::CYCLE_END_YEAR)
            .map(|year| if super::is_leap_year(year) { 366 } else { 365 })
            .sum();
        assert_eq!(days, super::CYCLE_DAYS);
        assert_eq!(days % 7, 0, "the cycle must be a whole number of weeks");

        // Walking the cycle returns the weekday to where it started.
        let mut weekday = super::CYCLE_START_WEEKDAY;
        for _ in 0..days {
            weekday = super::next_weekday(weekday);
        }
        assert_eq!(weekday, super::CYCLE_START_WEEKDAY);
    }
}
