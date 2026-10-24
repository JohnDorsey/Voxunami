/// A closed range of real numbers, used to bound a function over a region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    lower: f64,
    upper: f64,
}

impl Interval {
    /// Panics if `lower > upper` or either is NaN.
    pub fn new(lower: f64, upper: f64) -> Self {
        assert!(lower <= upper, "invalid interval [{lower}, {upper}]");
        Self { lower, upper }
    }

    pub fn point(value: f64) -> Self {
        Self::new(value, value)
    }

    pub fn lower(self) -> f64 {
        self.lower
    }

    pub fn upper(self) -> f64 {
        self.upper
    }

    pub fn contains(self, value: f64) -> bool {
        self.lower <= value && value <= self.upper
    }

    pub fn plus(self, other: Self) -> Self {
        Self::new(self.lower + other.lower, self.upper + other.upper)
    }

    pub fn minus(self, other: Self) -> Self {
        Self::new(self.lower - other.upper, self.upper - other.lower)
    }

    pub fn shifted(self, offset: f64) -> Self {
        Self::new(self.lower + offset, self.upper + offset)
    }

    /// Multiplies every value by `factor`, which may be negative.
    pub fn scaled(self, factor: f64) -> Self {
        let (a, b) = (self.lower * factor, self.upper * factor);
        Self::new(a.min(b), a.max(b))
    }

    /// The range of `|v|` for `v` in this interval.
    pub fn absolute(self) -> Self {
        if self.lower >= 0.0 {
            self
        } else if self.upper <= 0.0 {
            Self::new(-self.upper, -self.lower)
        } else {
            Self::new(0.0, (-self.lower).max(self.upper))
        }
    }

    /// The smallest interval that holds both.
    pub fn hull(self, other: Self) -> Self {
        Self::new(self.lower.min(other.lower), self.upper.max(other.upper))
    }

    /// The range of `min(a, b)` for `a` in `self` and `b` in `other`.
    pub fn minimum(self, other: Self) -> Self {
        Self::new(self.lower.min(other.lower), self.upper.min(other.upper))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interval(lower: f64, upper: f64) -> Interval {
        Interval::new(lower, upper)
    }

    #[test]
    fn plus_minus_and_shift_combine_the_extreme_cases() {
        assert_eq!(
            interval(1.0, 2.0).plus(interval(10.0, 20.0)),
            interval(11.0, 22.0)
        );
        assert_eq!(
            interval(1.0, 2.0).minus(interval(10.0, 20.0)),
            interval(-19.0, -8.0)
        );
        assert_eq!(interval(-1.0, 3.0).shifted(0.5), interval(-0.5, 3.5));
    }

    #[test]
    fn scaling_by_a_negative_factor_swaps_the_ends() {
        assert_eq!(interval(1.0, 3.0).scaled(2.0), interval(2.0, 6.0));
        assert_eq!(interval(1.0, 3.0).scaled(-2.0), interval(-6.0, -2.0));
        assert_eq!(interval(-1.0, 3.0).scaled(0.0), interval(0.0, 0.0));
    }

    #[test]
    fn absolute_value_covers_positive_negative_and_straddling_ranges() {
        assert_eq!(interval(2.0, 5.0).absolute(), interval(2.0, 5.0));
        assert_eq!(interval(-5.0, -2.0).absolute(), interval(2.0, 5.0));
        assert_eq!(interval(-3.0, 2.0).absolute(), interval(0.0, 3.0));
        assert_eq!(interval(-2.0, 7.0).absolute(), interval(0.0, 7.0));
    }

    #[test]
    fn minimum_takes_the_lower_of_each_end() {
        assert_eq!(
            interval(0.0, 10.0).minimum(interval(3.0, 4.0)),
            interval(0.0, 4.0)
        );
    }

    #[test]
    fn contains_includes_both_ends() {
        let range = interval(-1.0, 1.0);
        assert!(range.contains(-1.0) && range.contains(1.0) && range.contains(0.0));
        assert!(!range.contains(1.0000001) && !range.contains(f64::NAN));
    }

    #[test]
    #[should_panic(expected = "invalid interval")]
    fn inverted_ranges_are_rejected() {
        Interval::new(1.0, 0.0);
    }
}
