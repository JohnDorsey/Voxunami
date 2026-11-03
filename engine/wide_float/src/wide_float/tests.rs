//! What the public tests cannot see: how many widths of π a thread keeps, and which.

use super::{PI_OF_RECENT_WIDTHS, PI_WIDTHS_KEPT, WideFloat};
use crate::Precision;

fn kept_widths() -> Vec<u64> {
    PI_OF_RECENT_WIDTHS.with_borrow(|kept| kept.iter().map(|pi| pi.precision.bits()).collect())
}

#[test]
fn a_thread_keeps_the_widths_it_asked_for_lately_and_no_more() {
    // (A thread of its own: the memory is by thread, and the tests of a binary share none.)
    std::thread::spawn(|| {
        assert!(kept_widths().is_empty());
        let asked: Vec<u64> = (0..3 * PI_WIDTHS_KEPT as u64)
            .map(|index| 128 + index)
            .collect();
        for &bits in &asked {
            WideFloat::pi(Precision::new(bits).unwrap());
            assert!(kept_widths().len() <= PI_WIDTHS_KEPT);
        }
        // The last ones asked, the oldest of them first; and asking for one of them again changes nothing.
        let newest = &asked[asked.len() - PI_WIDTHS_KEPT..];
        assert_eq!(kept_widths(), newest);
        WideFloat::pi(Precision::new(newest[3]).unwrap());
        assert_eq!(kept_widths(), newest);
    })
    .join()
    .unwrap();
}
