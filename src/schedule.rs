//! Parsing a standard 5-field cron expression into the set of values each
//! field matches.
//!
//! The accepted syntax is `robfig/cron`'s `ParseStandard`, which is the
//! parser Kubernetes' `CronJob` controller validates schedules with, rather
//! than cron folklore in general:
//!
//!   - `*` and `?` as wildcards
//!   - lists (`1,3,5`), ranges (`1-5`), and steps (`*/4`, `1-20/2`)
//!   - the `N/step` form, meaning "N through the field's max, stepped"
//!     (`5/15` on minutes is 5,20,35,50 — NOT just 5)
//!   - 3-letter English names for months (`JAN`..`DEC`) and weekdays
//!     (`SUN`..`SAT`), case-insensitively
//!   - the `@yearly`/`@annually`, `@monthly`, `@weekly`, `@daily`/
//!     `@midnight` and `@hourly` descriptors
//!
//! Note that day-of-week is bounded at 0-6 strictly. `7` as an alias for
//! Sunday is valid under the older POSIX convention but is rejected here,
//! as Kubernetes' parser rejects it.

use crate::error::CronError;
use crate::value_set::ValueSet;

/// The number of fields a standard cron expression has.
const FIELD_COUNT: usize = 5;

/// A field's name, bounds, and the names it accepts, kept together so a
/// parse site cannot pair one field's bounds with another's name.
#[derive(Debug)]
struct FieldSpec {
    name: &'static str,
    min: u32,
    max: u32,
    /// Accepted names, lowercase; empty for fields that take numbers only.
    names: &'static [(&'static str, u32)],
}

impl FieldSpec {
    /// Panics at compile time, when the constants below are evaluated, if
    /// a field's values would not fit in a [`ValueSet`].
    const fn new(
        name: &'static str,
        min: u32,
        max: u32,
        names: &'static [(&'static str, u32)],
    ) -> Self {
        assert!(max < ValueSet::CAPACITY, "a field's values must fit in a ValueSet");
        assert!(min <= max, "a field's bounds must not be reversed");
        Self { name, min, max, names }
    }
}

const MINUTE: FieldSpec = FieldSpec::new("minute", 0, 59, &[]);
const HOUR: FieldSpec = FieldSpec::new("hour", 0, 23, &[]);
const DAY_OF_MONTH: FieldSpec = FieldSpec::new("day-of-month", 1, 31, &[]);
const MONTH: FieldSpec = FieldSpec::new("month", 1, 12, &MONTH_NAMES);
const DAY_OF_WEEK: FieldSpec = FieldSpec::new("day-of-week", 0, 6, &DOW_NAMES);

const MONTH_NAMES: [(&str, u32); 12] = [
    ("jan", 1),
    ("feb", 2),
    ("mar", 3),
    ("apr", 4),
    ("may", 5),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("sep", 9),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
];

const DOW_NAMES: [(&str, u32); 7] =
    [("sun", 0), ("mon", 1), ("tue", 2), ("wed", 3), ("thu", 4), ("fri", 5), ("sat", 6)];

/// One parsed field: the values it matches, plus whether it was written as
/// a wildcard.
///
/// The wildcard flag is not recoverable from the values alone — `*` and
/// `0-6` expand to the same day-of-week set, but only the explicit form
/// counts as a restriction under cron's day-of-month/day-of-week OR rule.
#[derive(Debug)]
pub(crate) struct Field {
    /// The values this field matches. Never empty: a field that would
    /// match nothing is rejected at parse time.
    pub(crate) values: ValueSet,
    is_wildcard: bool,
}

impl Field {
    /// Whether this field narrows the schedule at all, i.e. was written as
    /// something other than `*` or `?`.
    pub(crate) const fn is_restricted(&self) -> bool {
        !self.is_wildcard
    }

    fn parse(raw: &str, spec: &FieldSpec) -> Result<Self, CronError> {
        let raw = raw.trim();
        let mut values = ValueSet::EMPTY;
        for term in raw.split(',') {
            parse_term_into(term, spec, &mut values)?;
        }
        Ok(Self { values, is_wildcard: raw == "*" || raw == "?" })
    }
}

/// A parsed schedule: which minutes, hours and days it matches.
#[derive(Debug)]
pub(crate) struct Schedule {
    pub(crate) minute: Field,
    pub(crate) hour: Field,
    pub(crate) day_of_month: Field,
    pub(crate) month: Field,
    /// 0-6, Sunday = 0.
    pub(crate) day_of_week: Field,
}

impl Schedule {
    /// Parses a 5-field expression, or an `@`-descriptor standing for one.
    ///
    /// # Errors
    ///
    /// Returns the [`CronError`] describing the first problem found.
    pub(crate) fn parse(expr: &str) -> Result<Self, CronError> {
        // Filled in place rather than collected: a schedule parses
        // without touching the heap.
        let mut parts = expand_descriptor(expr).split_whitespace();
        let mut fields = [""; FIELD_COUNT];
        let mut found: usize = 0;
        for field in &mut fields {
            let Some(part) = parts.next() else { break };
            *field = part;
            found = found.saturating_add(1);
        }
        let found = found.saturating_add(parts.count());
        if found != FIELD_COUNT {
            return Err(CronError::WrongFieldCount { expected: FIELD_COUNT, found });
        }
        let [minute, hour, day_of_month, month, day_of_week] = fields;

        Ok(Self {
            minute: Field::parse(minute, &MINUTE)?,
            hour: Field::parse(hour, &HOUR)?,
            day_of_month: Field::parse(day_of_month, &DAY_OF_MONTH)?,
            month: Field::parse(month, &MONTH)?,
            day_of_week: Field::parse(day_of_week, &DAY_OF_WEEK)?,
        })
    }
}

/// Rewrites an `@`-descriptor as the 5-field expression it stands for.
/// Anything else is passed through untouched.
fn expand_descriptor(expr: &str) -> &str {
    match expr {
        "@yearly" | "@annually" => "0 0 1 1 *",
        "@monthly" => "0 0 1 * *",
        "@weekly" => "0 0 * * 0",
        "@daily" | "@midnight" => "0 0 * * *",
        "@hourly" => "0 * * * *",
        other => other,
    }
}

/// Expands one comma-separated term of a field (`*`, `5`, `1-5`, `*/4`,
/// `1-20/2`, `5/15`, `mon-fri`, ...) into `values`.
fn parse_term_into(term: &str, spec: &FieldSpec, values: &mut ValueSet) -> Result<(), CronError> {
    let (range_text, step_text) = split_in_two(term, '/')
        .ok_or_else(|| CronError::TooManySlashes { field: spec.name, text: term.to_owned() })?;
    let (start_text, end_text) = split_in_two(range_text, '-')
        .ok_or_else(|| CronError::TooManyHyphens { field: spec.name, text: term.to_owned() })?;

    let (start, mut end) = if range_text == "*" || range_text == "?" {
        (spec.min, spec.max)
    } else {
        let start = parse_value(start_text, spec)?;
        let end = match end_text {
            Some(text) => parse_value(text, spec)?,
            None => start,
        };
        (start, end)
    };

    let step = match step_text {
        Some(text) => text
            .parse::<u32>()
            .map_err(|_| CronError::BadNumber { field: spec.name, text: text.to_owned() })?,
        None => 1,
    };
    // A step applied to a bare value means "that value through the field's
    // max" — `5/15` on minutes is 5,20,35,50, not just 5.
    if step_text.is_some() && end_text.is_none() {
        end = spec.max;
    }

    if start < spec.min {
        return Err(CronError::BelowMin { field: spec.name, value: start, min: spec.min });
    }
    if end > spec.max {
        return Err(CronError::AboveMax { field: spec.name, value: end, max: spec.max });
    }
    if start > end {
        return Err(CronError::ReversedRange { field: spec.name, start, end });
    }
    if step == 0 {
        return Err(CronError::ZeroStep { field: spec.name });
    }

    if step == 1 {
        // The common case — `*`, `1-5`, a bare value — is a contiguous
        // range, which the value set fills without a loop.
        *values = values.union(ValueSet::from_range(start, end));
        return Ok(());
    }

    // `checked_add` rather than `for .. .step_by()`: it needs no cast, and
    // a step large enough to overflow simply ends the range, which is the
    // same as stepping past `end`.
    let mut value = start;
    while value <= end {
        values.insert(value);
        let Some(next) = value.checked_add(step) else { break };
        value = next;
    }
    Ok(())
}

/// Parses a single number, or one of the field's names (case-insensitively).
fn parse_value(text: &str, spec: &FieldSpec) -> Result<u32, CronError> {
    if let Ok(value) = text.parse::<u32>() {
        return Ok(value);
    }
    spec.names
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(text))
        .map(|&(_, value)| value)
        .ok_or_else(|| CronError::UnknownName { field: spec.name, text: text.to_owned() })
}

/// Splits on `sep` into a head and an optional tail, or `None` if `sep`
/// occurs more than once.
fn split_in_two(text: &str, sep: char) -> Option<(&str, Option<&str>)> {
    let mut pieces = text.splitn(3, sep);
    let head = pieces.next()?;
    let tail = pieces.next();
    pieces.next().is_none().then_some((head, tail))
}
