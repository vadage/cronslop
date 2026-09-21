//! Parsing the Go-style duration string that `@every` takes.
//!
//! Arithmetic is exact integer arithmetic throughout: a duration is a
//! count of nanoseconds, and a fractional part like the `.5` of `1.5h` is
//! scaled by the unit and divided down, never routed through a float.
//! Anything that would not fit is reported as an error rather than
//! silently wrapping or saturating.

use crate::error::CronError;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Fraction digits kept. The result is whole seconds, so digits past this
/// point cannot affect it; keeping the count bounded also keeps the
/// power-of-ten scale factor well inside `u128`.
const MAX_FRACTION_DIGITS: usize = 18;

/// Units Go's `time.ParseDuration` accepts, with their length in
/// nanoseconds. Ordered longest-first so that the first prefix match is
/// also the longest one (`ms` must win over `m`).
const UNITS: [(&str, u128); 7] = [
    ("µs", 1_000),
    ("ns", 1),
    ("us", 1_000),
    ("ms", 1_000_000),
    ("s", NANOS_PER_SECOND),
    ("m", 60 * NANOS_PER_SECOND),
    ("h", 3_600 * NANOS_PER_SECOND),
];

/// Parses a duration such as `1h30m10s`, `90s` or `500ms` into whole
/// seconds, rounding down. Sub-second durations therefore come back as 0.
///
/// # Errors
///
/// Returns [`CronError::InvalidDuration`] if the text is empty, negative,
/// malformed, or too large to represent.
pub(crate) fn parse_seconds(text: &str) -> Result<u64, CronError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(invalid("missing duration after @every"));
    }
    if text == "0" {
        return Ok(0);
    }
    if text.starts_with('-') {
        return Err(invalid(format!("negative duration not supported: {text}")));
    }

    let mut total_nanos: u128 = 0;
    let mut rest = text.strip_prefix('+').unwrap_or(text);
    while !rest.is_empty() {
        let (amount, after_amount) = take_number(rest, text)?;
        let (unit_nanos, after_unit) = take_unit(after_amount, text)?;

        total_nanos = amount
            .nanos(unit_nanos)
            .and_then(|nanos| total_nanos.checked_add(nanos))
            .ok_or_else(|| too_large(text))?;
        rest = after_unit;
    }

    let seconds = total_nanos.checked_div(NANOS_PER_SECOND).ok_or_else(|| too_large(text))?;
    u64::try_from(seconds).map_err(|_| too_large(text))
}

/// A non-negative decimal number, kept exactly: `1.5` is whole 1, fraction
/// 5, one fraction digit.
#[derive(Debug, Clone, Copy)]
struct Decimal {
    whole: u128,
    fraction: u128,
    fraction_digits: u32,
}

impl Decimal {
    /// A whole number, with no fractional part.
    const fn whole(whole: u128) -> Self {
        Self { whole, fraction: 0, fraction_digits: 0 }
    }

    /// Parses `12`, `12.5`, `.5` or `12.`, or `None` if `text` is not one
    /// of those (`1.2.3`, `.`, an empty string, or a number too large to
    /// represent).
    fn parse(text: &str) -> Option<Self> {
        let Some((whole_text, fraction_text)) = text.split_once('.') else {
            // The common case: no decimal point at all.
            return Some(Self::whole(parse_digits(text)?));
        };
        if fraction_text.contains('.') || (whole_text.is_empty() && fraction_text.is_empty()) {
            return None;
        }

        // Digits past `MAX_FRACTION_DIGITS` cannot change a whole-second
        // result, so they are dropped rather than overflowing the scale.
        let kept = fraction_text.get(..MAX_FRACTION_DIGITS).unwrap_or(fraction_text);
        Some(Self {
            whole: parse_digits(whole_text)?,
            fraction: parse_digits(kept)?,
            fraction_digits: u32::try_from(kept.len()).ok()?,
        })
    }

    /// This number of `unit_nanos`-sized units, in nanoseconds, or `None`
    /// on overflow.
    fn nanos(self, unit_nanos: u128) -> Option<u128> {
        let whole = self.whole.checked_mul(unit_nanos)?;
        if self.fraction == 0 {
            return Some(whole);
        }
        let scale = 10_u128.checked_pow(self.fraction_digits)?;
        let fraction = self.fraction.checked_mul(unit_nanos)?.checked_div(scale)?;
        whole.checked_add(fraction)
    }
}

/// Parses a run of ASCII digits, treating an empty run as zero.
fn parse_digits(digits: &str) -> Option<u128> {
    if digits.is_empty() { Some(0) } else { digits.parse().ok() }
}

/// Splits off the leading decimal number, e.g. `1.5h` -> (1.5, `"h"`).
/// `full` is the whole duration, for error messages only.
fn take_number<'a>(rest: &'a str, full: &str) -> Result<(Decimal, &'a str), CronError> {
    let digits = rest.bytes().take_while(|byte| byte.is_ascii_digit() || *byte == b'.').count();
    let Some((number, after)) = rest.split_at_checked(digits) else {
        return Err(invalid(format!("expected a number at '{rest}' in duration '{full}'")));
    };
    if number.is_empty() {
        return Err(invalid(format!("expected a number at '{rest}' in duration '{full}'")));
    }
    let amount = Decimal::parse(number)
        .ok_or_else(|| invalid(format!("bad number '{number}' in duration '{full}'")))?;
    Ok((amount, after))
}

/// Splits off the leading unit, returning its length in nanoseconds.
/// `full` is the whole duration, for error messages only.
fn take_unit<'a>(rest: &'a str, full: &str) -> Result<(u128, &'a str), CronError> {
    UNITS
        .iter()
        .find_map(|&(unit, nanos)| Some((nanos, rest.strip_prefix(unit)?)))
        .ok_or_else(|| invalid(format!("unknown unit at '{rest}' in duration '{full}'")))
}

fn invalid(message: impl Into<String>) -> CronError {
    CronError::InvalidDuration(message.into())
}

fn too_large(full: &str) -> CronError {
    invalid(format!("duration '{full}' is too large"))
}
