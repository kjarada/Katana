//! THE TEST KEYS (the twin of tests/update/test_key.hpp and test_key.cpp). Two Ed25519 keys that
//! are not secret: RFC 8032, section 7.1, TEST 1 (the "active" key) and TEST 2 (the "spare").
//! Anyone can sign with them, which is the point: a test key must never be one that a release
//! could be signed with.
//!
//! They exist only in test programs, none of which is installed. The production programs pass
//! production_trust() (trust.rs), the keys pinned at build time; nothing in the environment, a
//! file or an option moves that.
//!
//! The Python side signs with the same keys: tests/update/fixtures builds the PKCS#8 private key
//! from the 32-byte seed with the 16-byte prefix 302e020100300506032b657004220420 (a PEM of it is
//! NOT committed: *.pem is git-ignored, by design), and `openssl pkeyutl -sign -rawin` signs a
//! manifest with it. The seeds and public keys below were checked against OpenSSL: the public key
//! it derives from each seed is the one the RFC prints.
//!
//! Deliberately does not call the library's own hex reader or key_id_of: the tests of those
//! compare against THIS, so it must not be built from them.

use katana_update::trust::{Trust, TrustedKey};
use std::sync::OnceLock;

pub const TEST_ACTIVE_SEED_HEX: &str =
    "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
pub const TEST_ACTIVE_PUBLIC_HEX: &str =
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
pub const TEST_SPARE_SEED_HEX: &str =
    "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb";
pub const TEST_SPARE_PUBLIC_HEX: &str =
    "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";

fn nibble(c: u8) -> u8 {
    if c.is_ascii_digit() {
        c - b'0'
    } else {
        c - b'a' + 10 // the constants above are lower-case hexadecimal
    }
}

fn key_from_hex(hex: &str) -> TrustedKey {
    let digits = hex.as_bytes();
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = nibble(digits[2 * i]) * 16 + nibble(digits[2 * i + 1]);
    }
    TrustedKey {
        id: hex[..16].to_owned(),
        key,
    }
}

/// The active key then the spare, ids as key_id_of gives them (the first 16 hex digits),
/// placeholder false. The one definition of the test trust root.
pub fn test_trust() -> &'static Trust {
    static TRUST: OnceLock<Trust> = OnceLock::new();
    TRUST.get_or_init(|| Trust {
        keys: vec![
            key_from_hex(TEST_ACTIVE_PUBLIC_HEX),
            key_from_hex(TEST_SPARE_PUBLIC_HEX),
        ],
        placeholder: false,
    })
}
