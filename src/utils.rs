use itertools::{EitherOrBoth::*, Itertools as _};
use std::cmp::Ordering;

pub fn cmp_ignore_case_ascii(a: &str, b: &str) -> bool {
    a.bytes()
        .zip_longest(b.bytes())
        .map(|ab| match ab {
            Left(_) => Ordering::Greater,
            Right(_) => Ordering::Less,
            Both(a, b) => {
                if a == b' ' && b == b' ' {
                    Ordering::Equal
                } else {
                    a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
                }
            }
        })
        .find(|&ordering| ordering != Ordering::Equal)
        .is_none()
}
