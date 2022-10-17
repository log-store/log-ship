use std::ffi::OsStr;
use std::num::ParseIntError;
use std::ops::{Bound, RangeBounds};
use std::ops::Bound::{Excluded, Included, Unbounded};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use crate::LogValue;

/// Maps a range from one type to another
/// Note: Bound::map is coming: https://github.com/rust-lang/rust/issues/86026
pub fn map_range<T: Clone, U: Clone, R: RangeBounds<T>, F: Fn(T) -> U>(range: R, f: F) -> impl RangeBounds<U> + Clone {
    let start = match range.start_bound() {
        Bound::Included(i) => { Included(f(i.clone())) }
        Bound::Excluded(e) => { Excluded(f(e.clone())) }
        Bound::Unbounded => { Unbounded }
    };

    let end = match range.end_bound() {
        Included(i) => { Included(f(i.clone())) }
        Excluded(e) => { Excluded(f(e.clone())) }
        Unbounded => { Unbounded }
    };

    (start, end)
}

/// Checks to see if any part of the two ranges overlap
pub fn overlaps<T: PartialOrd, R1: RangeBounds<T>, R2: RangeBounds<T>>(range1: &R1, range2: &R2) -> bool {
    let (r1s, r1e) = (range1.start_bound(), range1.end_bound());

    // r1s is contained
    if let Bound::Included(s) | Bound::Excluded(s) = r1s {
        if range2.contains(s) {
            return true;
        }
    }

    // r1e is contained
    if let Bound::Included(e) | Bound::Excluded(e) = r1e {
        if range2.contains(e) {
            return true;
        }
    }

    let (r2s, r2e) = (range2.start_bound(), range2.end_bound());

    // r2s is contained
    if let Bound::Included(s) | Bound::Excluded(s) = r2s {
        if range1.contains(s) {
            return true;
        }
    }

    // r2e is contained
    if let Bound::Included(e) | Bound::Excluded(e) = r2e {
        if range1.contains(e) {
            return true;
        }
    }

    false
}

/// Rounds a duration _down_ to the nearest minute
#[inline]
pub fn round_duration(duration: Duration) -> Duration {
    let rounded_secs = (duration.as_secs() / 60) * 60;

    Duration::from_secs(rounded_secs)
}

/// Rounds a timestamp _down_ to the nearest minute
pub fn round_timestamp(value: LogValue) -> LogValue {
    match value {
        LogValue::TimeStamp(ts) => {
            LogValue::TimeStamp(round_duration(ts))
        }
        _ => {
            panic!("Attempting to round a non-timestamp!");
        }
    }
}

/// Fetches the current epoch as a String
#[inline(always)]
pub fn epoch_string() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis().to_string()
}

/// Fetches the current epoch as a number of ms
#[inline(always)]
pub fn epoch() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

/// Converts a string to an epoch
#[inline(always)]
pub fn string2epoch(epoch_str: &OsStr) -> Result<u128, ParseIntError> {
    epoch_str.to_string_lossy().parse::<u128>()
}


#[cfg(test)]
mod std_utils_tests {
    use std::ops::{Bound};
    use crate::overlaps;

    #[test]
    fn overlaps_tests() {
        let full_range = ..;

        // unbounded
        assert!(overlaps(&full_range, &(0..10)));
        assert!(overlaps(&(0..10), &full_range));

        // overlapping
        assert!(overlaps(&(1..2), &(0..10)));
        assert!(overlaps(&(0..10), &(1..2)));
        assert!(overlaps(&(1..12), &(0..10)));
        assert!(overlaps(&(0..10), &(1..12)));
        assert!(overlaps(&(1..12), &(8..10)));
        assert!(overlaps(&(8..10), &(1..12)));

        // non-overlapping
        assert!(!overlaps(&(1..2), &(3..10)));
        assert!(!overlaps(&(3..10), &(1..2)));
        assert!(!overlaps(&(Bound::Unbounded, Bound::Included(2)), &(3..10)));
        assert!(!overlaps(&(3..10), &(Bound::Included(11), Bound::Unbounded)));
    }
}
