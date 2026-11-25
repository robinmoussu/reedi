use std::time::Duration;

pub fn retry<T>(operation: impl Fn() -> Option<T>) -> Option<T> {
    (0..MAXIMUM_ATTEMPTS).find_map(|_| operation())
}

/// The delay before the attempt `attempt_index`, doubled after each attempt.
pub fn backoff_delay(attempt_index: u32) -> Duration {
    INITIAL_DELAY * 2u32.pow(attempt_index)
}
