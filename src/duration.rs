//! Parsing the Go-style duration string that `@every` takes.
//!
//! The semantics are `time.ParseDuration` followed by `cron.Every`, which
//! is the pair `robfig/cron` uses, so the accepted syntax and the
//! resulting period match what Kubernetes admits:
//!
//!   - one optional leading sign, then `<number><unit>` repeated
//!   - units `ns`, `us`/`µs`, `ms`, `s`, `m`, `h`
//!   - a bare `0` is the one number allowed without a unit
//!   - the total must fit in an `i64` count of nanoseconds
//!   - anything under a second — including zero and any negative
//!     duration — becomes one second, so no `@every` period is ever zero
//!
//! Arithmetic is exact integer arithmetic throughout: a fractional part
//! like the `.5` of `1.5h` is scaled by the unit and divided down, never
//! routed through a float. Anything that would not fit is reported as an
//! error rather than silently wrapping or saturating.

use crate::error::CronError;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// `cron.Every` clamps anything shorter than a second up to one second,
/// so an `@every` schedule never has a period of zero.
const MINIMUM_SECONDS: u64 = 1;

/// `i64::MAX`, the largest value a Go `time.Duration` holds — it counts
/// nanoseconds in an `i64`. `time.ParseDuration` errors above this, so
/// matching the bound keeps the accept/reject boundary identical.
/// Checked against `i64::MAX` in [`tests::max_nanos_is_i64_max`].
const MAX_NANOS: u128 = 9_223_372_036_854_775_807;

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

/// Parses the text after `@every ` into the schedule's period in seconds.
///
/// Whole seconds, rounding down, with a floor of one second. `text` is
/// taken exactly as written: leading or trailing spaces are part of it and
/// make it invalid, as they do in Go.
///
/// # Errors
///
/// Returns [`CronError::InvalidDuration`] if the text is empty,
/// malformed, or too large for an `i64` of nanoseconds.
pub(crate) fn parse_seconds(text: &str) -> Result<u64, CronError> {
    if text.is_empty() {
        return Err(invalid("missing duration after @every"));
    }

    // One leading sign, as Go allows. A negative duration parses happily
    // there and is then clamped up to the one-second minimum, so the sign
    // cannot change the result — but the rest still has to be valid.
    let negative = text.starts_with('-');
    let rest = text.strip_prefix(['-', '+']).unwrap_or(text);

    // Go's one special case: a bare `0` needs no unit.
    if rest == "0" {
        return Ok(MINIMUM_SECONDS);
    }

    let nanos = total_nanos(rest, text)?;
    let seconds =
        u64::try_from(nanos.checked_div(NANOS_PER_SECOND).ok_or_else(|| too_large(text))?)
            .map_err(|_| too_large(text))?;

    if negative {
        return Ok(MINIMUM_SECONDS);
    }
    Ok(seconds.max(MINIMUM_SECONDS))
}

/// Sums the `<number><unit>` terms of an unsigned duration body.
/// `full` is the whole duration, for error messages only.
fn total_nanos(body: &str, full: &str) -> Result<u128, CronError> {
    let mut total: u128 = 0;
    let mut rest = body;
    while !rest.is_empty() {
        let (amount, after_amount) = take_number(rest, full)?;
        let (unit_nanos, after_unit) = take_unit(after_amount, full)?;

        total = amount
            .nanos(unit_nanos)
            .and_then(|nanos| total.checked_add(nanos))
            .filter(|nanos| *nanos <= MAX_NANOS)
            .ok_or_else(|| too_large(full))?;
        rest = after_unit;
    }
    Ok(total)
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

#[cfg(test)]
mod tests {
    use super::MAX_NANOS;

    #[test]
    fn max_nanos_is_i64_max() {
        // `unsigned_abs` of a positive value is infallible, so this needs
        // no unwrap to state the equivalence.
        assert_eq!(MAX_NANOS, u128::from(i64::MAX.unsigned_abs()));
    }
}
