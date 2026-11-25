use std::time::Duration;

/// The delay before the attempt `attempt_index`, doubled after each attempt.
pub fn backoff_delay(attempt_index: u32) -> Duration {
    INITIAL_DELAY * 2u32.pow(attempt_index)
}
