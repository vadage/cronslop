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

    /// Whether the set holds nothing.
    pub(crate) const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// How many values the set holds, as a `usize` for sizing buffers.
    /// Never fails in practice: the count is at most [`Self::CAPACITY`].
    pub(crate) fn count(self) -> usize {
        usize::try_from(self.0.count_ones()).unwrap_or(0)
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
        assert_eq!(set.count(), 4);
        assert!(set.contains(0) && set.contains(59));
        assert!(!set.contains(2) && !set.contains(58));
        assert!(!set.is_empty());
        assert!(ValueSet::EMPTY.is_empty());
        assert_eq!(ValueSet::EMPTY.iter().count(), 0);
    }

    #[test]
    fn ignores_values_it_cannot_hold() {
        let mut set = ValueSet::EMPTY;
        set.insert(ValueSet::CAPACITY);
        set.insert(u32::MAX);
        assert!(set.is_empty(), "out-of-range values must not wrap onto real ones");
        assert!(!set.contains(ValueSet::CAPACITY));
        assert!(!set.contains(u32::MAX));
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
