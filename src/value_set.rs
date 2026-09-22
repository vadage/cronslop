//! The set of values a cron field matches.

/// A set of small non-negative integers, held as a 64-bit mask.
///
/// Every cron field's values fit in `0..=59` (minutes are the widest), so
/// a `u64` holds any field's value set outright: no allocation, membership
/// is a bit test, and iteration walks set bits. [`ValueSet::CAPACITY`] is
/// checked against each field's bounds at compile time, in
/// `FieldSpec::new`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ValueSet(u64);

impl ValueSet {
    /// Values from `0` to `CAPACITY - 1` can be held. Anything else is
    /// outside every cron field's bounds and is rejected before it gets
    /// here.
    pub(crate) const CAPACITY: u32 = u64::BITS;

    /// The empty set.
    pub(crate) const EMPTY: Self = Self(0);

    /// Adds `value`. Values at or above [`Self::CAPACITY`] are ignored,
    /// which cannot happen for a parsed field.
    pub(crate) const fn insert(&mut self, value: u32) {
        self.0 |= bit(value);
    }

    /// Whether `value` is in the set.
    pub(crate) const fn contains(self, value: u32) -> bool {
        self.0 & bit(value) != 0
    }

    /// Every value, i.e. `0..CAPACITY`.
    pub(crate) const ALL: Self = Self(u64::MAX);

    /// The union of two sets.
    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The values at least `floor`.
    pub(crate) const fn at_least(self, floor: u32) -> Self {
        match u64::MAX.checked_shl(floor) {
            Some(keep) => Self(self.0 & keep),
            None => Self::EMPTY,
        }
    }

    /// Every value from `first` to `last`, inclusive.
    ///
    /// A whole range in two bit operations, rather than one insert per
    /// value — and cron fields are mostly ranges.
    pub(crate) const fn from_range(first: u32, last: u32) -> Self {
        Self::ALL.up_to(last).at_least(first)
    }

    /// `first`, then every seventh value after it.
    ///
    /// A weekday recurs every seven days, so one of these covers a whole
    /// month's worth of, say, Mondays without walking the month.
    pub(crate) const fn every_seventh_from(first: u32) -> Self {
        match EVERY_SEVENTH.checked_shl(first) {
            Some(bits) => Self(bits),
            None => Self::EMPTY,
        }
    }

    /// The values at most `limit`, e.g. the days of the month that a
    /// 28-day February actually has.
    pub(crate) const fn up_to(self, limit: u32) -> Self {
        // Shifting an all-ones mask right by the complement of `limit`
        // leaves exactly bits `0..=limit` set.
        match u64::MAX.checked_shr(Self::CAPACITY.saturating_sub(1).saturating_sub(limit)) {
            Some(keep) => Self(self.0 & keep),
            None => self,
        }
    }

    /// The values in the set, ascending.
    pub(crate) fn iter(self) -> impl Iterator<Item = u32> {
        let mut remaining = self.0;
        std::iter::from_fn(move || {
            if remaining == 0 {
                return None;
            }
            let value = remaining.trailing_zeros();
            // Clear the lowest set bit. `remaining` is non-zero here, so
            // the wrapping subtraction is an ordinary one.
            remaining &= remaining.wrapping_sub(1);
            Some(value)
        })
    }
}

/// Bits 0, 7, 14, ... — one per week, so that shifting it into place
/// marks every day of a month sharing one weekday.
const EVERY_SEVENTH: u64 = 0x8102_0408_1020_4081;

/// The bit standing for `value`, or no bit at all if `value` is out of
/// range.
const fn bit(value: u32) -> u64 {
    match 1_u64.checked_shl(value) {
        Some(bit) => bit,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::ValueSet;

    fn set_of(values: &[u32]) -> ValueSet {
        let mut set = ValueSet::EMPTY;
        for &value in values {
            set.insert(value);
        }
        set
    }

    #[test]
    fn holds_the_values_inserted() {
        let set = set_of(&[0, 1, 29, 59]);
        assert_eq!(set.iter().collect::<Vec<_>>(), vec![0, 1, 29, 59]);
        assert!(set.contains(0) && set.contains(59));
        assert!(!set.contains(2) && !set.contains(58));
        assert_eq!(ValueSet::EMPTY.iter().count(), 0);
    }

    #[test]
    fn ignores_values_it_cannot_hold() {
        let mut set = ValueSet::EMPTY;
        set.insert(ValueSet::CAPACITY);
        set.insert(u32::MAX);
        assert_eq!(set.iter().count(), 0, "out-of-range values must not wrap onto real ones");
        assert!(!set.contains(ValueSet::CAPACITY));
        assert!(!set.contains(u32::MAX));
    }

    #[test]
    fn every_seventh_marks_one_weekday_of_a_month() {
        // Mondays of a 31-day month whose 2nd is a Monday.
        let mondays = ValueSet::every_seventh_from(2).up_to(31);
        assert_eq!(mondays.iter().collect::<Vec<_>>(), vec![2, 9, 16, 23, 30]);
        // The stride must reach the top of the range and stop there.
        assert_eq!(ValueSet::every_seventh_from(1).up_to(31).iter().count(), 5);
        assert_eq!(ValueSet::every_seventh_from(0).up_to(63).iter().count(), 10);
        assert_eq!(ValueSet::every_seventh_from(ValueSet::CAPACITY).iter().count(), 0);
    }

    #[test]
    fn from_range_covers_the_whole_range() {
        assert_eq!(ValueSet::from_range(0, 59).iter().count(), 60);
        assert_eq!(ValueSet::from_range(5, 5), set_of(&[5]));
        assert_eq!(ValueSet::from_range(1, 3), set_of(&[1, 2, 3]));
        assert_eq!(ValueSet::from_range(3, 1).iter().count(), 0, "a reversed range is empty");
        assert_eq!(ValueSet::from_range(0, 63).iter().count(), 64);
    }

    #[test]
    fn union_and_at_least_combine_sets() {
        assert_eq!(set_of(&[1, 9]).union(set_of(&[9, 20])), set_of(&[1, 9, 20]));
        assert_eq!(set_of(&[1, 9, 20]).at_least(9), set_of(&[9, 20]));
        assert_eq!(set_of(&[1, 9]).at_least(0), set_of(&[1, 9]));
        assert_eq!(set_of(&[1, 9]).at_least(ValueSet::CAPACITY).iter().count(), 0);
        assert_eq!(ValueSet::ALL.up_to(3).at_least(1), set_of(&[1, 2, 3]));
    }

    #[test]
    fn up_to_keeps_only_values_within_the_limit() {
        let set = set_of(&[1, 28, 29, 30, 31]);
        assert_eq!(set.up_to(28).iter().collect::<Vec<_>>(), vec![1, 28]);
        assert_eq!(set.up_to(29).iter().collect::<Vec<_>>(), vec![1, 28, 29]);
        assert_eq!(set.up_to(0).iter().count(), 0);
        assert_eq!(set.up_to(31), set);
        assert_eq!(set.up_to(u32::MAX), set, "a limit past the end keeps everything");
    }
}
