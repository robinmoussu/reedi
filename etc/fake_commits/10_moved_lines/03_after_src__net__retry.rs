use std::time::Duration;

pub fn retry<T>(operation: impl Fn() -> Option<T>) -> Option<T> {
    (0..MAXIMUM_ATTEMPTS).find_map(|_| operation())
}
