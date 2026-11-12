//! `reediif` computes the diffs displayed by `reedi`: the hunks of each modified file, and the
//! pairing of their removed and added lines.

pub mod diff;
pub mod model;
pub mod pairing;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
