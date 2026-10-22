//! Digest of the release archives.

use sha2::Digest;
use sha2::Sha256;

/// The SHA-256 hash of `data`, in hexadecimal.
pub fn sha256(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}
