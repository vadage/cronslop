//! Parse and validation errors, mirroring the failures Kubernetes' own
//! schedule validation (via `robfig/cron`) reports.

use std::fmt;

/// Why a schedule string could not be interpreted.
///
/// The `field` carried by most variants is the human-readable name of the
/// offending cron field: `"minute"`, `"hour"`, `"day-of-month"`,
/// `"month"` or `"day-of-week"`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CronError {
    /// A standard expression needs exactly five space-separated fields.
    WrongFieldCount {
        /// How many fields a standard expression has: always 5.
        expected: usize,
        /// How many fields the input actually had.
        found: usize,
    },
    /// Not a number, and not one of the field's accepted names.
    UnknownName {
        /// The field the offending text appeared in.
        field: &'static str,
        /// The text that could not be interpreted.
        text: String,
    },
    /// A step (`*/N`) that is not a number.
    BadNumber {
        /// The field the offending text appeared in.
        field: &'static str,
        /// The text that could not be interpreted.
        text: String,
    },
    /// More than one `-` in a single term, e.g. `1-2-3`.
    TooManyHyphens {
        /// The field the offending term appeared in.
        field: &'static str,
        /// The whole term, as written.
        text: String,
    },
    /// More than one `/` in a single term, e.g. `1//2`.
    TooManySlashes {
        /// The field the offending term appeared in.
        field: &'static str,
        /// The whole term, as written.
        text: String,
    },
    /// A value below the field's lower bound, e.g. day-of-month `0`.
    BelowMin {
        /// The field the value appeared in.
        field: &'static str,
        /// The offending value.
        value: u32,
        /// The lowest value the field accepts.
        min: u32,
    },
    /// A value above the field's upper bound, e.g. day-of-week `7`.
    AboveMax {
        /// The field the value appeared in.
        field: &'static str,
        /// The offending value.
        value: u32,
        /// The highest value the field accepts.
        max: u32,
    },
    /// A range written backwards, e.g. `5-3`.
    ReversedRange {
        /// The field the range appeared in.
        field: &'static str,
        /// The range's start, which is past its end.
        start: u32,
        /// The range's end.
        end: u32,
    },
    /// A step of `0`, which would never advance.
    ZeroStep {
        /// The field the step appeared in.
        field: &'static str,
    },
    /// A malformed `@every` duration, described by the wrapped message.
    InvalidDuration(String),
}

impl fmt::Display for CronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::WrongFieldCount { expected, found } => {
                write!(f, "expected {expected} space-separated fields, found {found}")
            }
            Self::UnknownName { field, ref text } => {
                write!(f, "'{text}' is not a valid value or name in the {field} field")
            }
            Self::BadNumber { field, ref text } => {
                write!(f, "'{text}' in the {field} field is not a valid number")
            }
            Self::TooManyHyphens { field, ref text } => {
                write!(f, "too many hyphens in {field} field: '{text}'")
            }
            Self::TooManySlashes { field, ref text } => {
                write!(f, "too many slashes in {field} field: '{text}'")
            }
            Self::BelowMin { field, value, min } => {
                write!(f, "beginning of range ({value}) below minimum ({min}) in {field} field")
            }
            Self::AboveMax { field, value, max } => {
                write!(f, "end of range ({value}) above maximum ({max}) in {field} field")
            }
            Self::ReversedRange { field, start, end } => {
                write!(
                    f,
                    "beginning of range ({start}) beyond end of range ({end}) in {field} field"
                )
            }
            Self::ZeroStep { field } => {
                write!(f, "step of range should be a positive number in {field} field")
            }
            Self::InvalidDuration(ref message) => write!(f, "invalid @every duration: {message}"),
        }
    }
}

impl std::error::Error for CronError {}
