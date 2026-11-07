//! Ed25519 (RFC 8032) over the manifest's exact bytes, verified in this process, never by a
//! shell-out. The C++ verifies with OpenSSL's EVP interface and makes the RFC 8032 checks OpenSSL
//! does not on OpenSSL's big numbers; the port verifies with ed25519-dalek's `verify_strict` and
//! makes the same checks with curve25519-dalek (docs/rust.md, "Third-party libraries").
//!
//! The signature file is one line: `ed25519 <keyid> <base64>`, where <keyid> is key_id_of(public
//! key), the first 16 lower-case hex digits of the 32-byte public key, and <base64> is the
//! 64-byte signature (standard alphabet, padded). A trailing LF or CRLF is allowed.
//!
//! What the verifier must refuse beyond what a verification equation says, each checked HERE,
//! before the library is asked, so that the rule does not depend on which library a build links:
//!
//! 1. S below the group order L (RFC 8032, 5.1.7). Without it a valid signature has a second
//!    form, S + L, that verifies too.
//! 2. A public key that is the canonical encoding of a point of the prime-order subgroup, and not
//!    the neutral element. Measured with the openssl command line (3.6.5) while the C++ was
//!    written: the all-zero key, an all-zero signature and about a quarter of the one-byte
//!    messages (9, 14, 17, 19, 26 ...) verify, because that key is a point of order 4 and the
//!    verification equation then holds for some messages whatever the signer knew. The pinned
//!    keys are generated and pass; the rule is for a key a build was handed wrongly. (The
//!    placeholder keys, trust.rs, are chosen to be no point at all.)
//!
//! How the C++'s checks map here, each with its refusal kept (classify_public_key):
//!
//! * y at or above p (RFC 8032, 5.1.3): the 255 bits under the sign bit, compared with p here,
//!   because curve25519-dalek's decompression reduces y modulo p and would take the alias;
//! * no x for this y: curve25519-dalek's decompression answers None, as the C++'s square root
//!   finds none;
//! * x = 0 with the sign bit set ("-0"): decompression takes it as x = 0, so the canonical
//!   encoding of what it decoded is compared with the key, and differs from it in that bit alone
//!   (y is already canonical, and for x != 0 the sign bit is x's own);
//! * the neutral element: is_identity;
//! * L * A not the identity (small or mixed order): is_torsion_free, which is that product;
//! * S below L: the same byte comparison as the C++'s.
//!
//! verify_strict then checks the equation [S]B = R + [k]A without the cofactor, as OpenSSL does,
//! with R compared as bytes; it also refuses an R of small order, which no signer that follows
//! RFC 8032 makes, and which a key and an S that passed the checks above cannot make verify.
//! OpenSSL's own environment (OPENSSL_CONF and the rest) has no twin: nothing in this verifier
//! reads a file or loads code, so hardening only removes the variables, for the programs this one
//! starts.

use crate::feed::MAX_SIGNATURE_BYTES;
use curve25519_dalek::edwards::CompressedEdwardsY;
use curve25519_dalek::traits::IsIdentity;
use ed25519_dalek::{Signature, VerifyingKey};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};

pub type PublicKey = [u8; 32];
pub type SignatureBytes = [u8; 64];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureLine {
    /// "ed25519".
    pub algorithm: String,
    /// 16 lower-case hex digits.
    pub key_id: String,
    pub signature: SignatureBytes,
}

impl Default for SignatureLine {
    fn default() -> Self {
        Self {
            algorithm: String::new(),
            key_id: String::new(),
            signature: [0; 64],
        }
    }
}

const ALGORITHM: &[u8] = b"ed25519";
const KEY_ID_DIGITS: usize = 16;
// 64 bytes make 88 base64 characters: 21 groups of three bytes, then one byte and two pads.
const SIGNATURE_BASE64_LENGTH: usize = 88;

fn is_lower_hex_digit(c: u8) -> bool {
    matches!(c, b'0'..=b'9' | b'a'..=b'f')
}

fn hex_value(c: u8) -> u8 {
    if c <= b'9' { c - b'0' } else { c - b'a' + 10 }
}

fn is_all_lower_hex(text: &[u8]) -> bool {
    text.iter().all(|&c| is_lower_hex_digit(c))
}

// ---- base64, strict ----------------------------------------------------------------------------

fn base64_value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

// The standard alphabet, padded, one string, and CANONICAL: the bits an incomplete last group does
// not use are zero, so there is exactly one text per byte string and a signature file cannot be
// altered without the bytes changing. No whitespace, no URL-safe alphabet.
fn decode_base64_canonical(text: &[u8]) -> Option<Vec<u8>> {
    if text.is_empty() || !text.len().is_multiple_of(4) {
        return None;
    }
    let pads = match text {
        [.., b'=', b'='] => 2,
        [.., b'='] => 1,
        _ => 0,
    };
    let groups = text.as_chunks::<4>().0;
    let mut bytes = Vec::with_capacity(groups.len() * 3);
    for (index, group) in groups.iter().enumerate() {
        let last = index + 1 == groups.len();
        let pads_here = if last { pads } else { 0 };
        let mut bits = 0u32;
        for (i, &c) in group.iter().enumerate() {
            let is_pad = i >= 4 - pads_here;
            // Not in the alphabet, or '=' before the end, or a pad too short.
            let value = if is_pad {
                (c == b'=').then_some(0)
            } else {
                base64_value(c)
            }?;
            bits = (bits << 6) | value;
        }
        let unused_bits = match pads_here {
            2 => 0xffff,
            1 => 0xff,
            _ => 0,
        };
        if bits & unused_bits != 0 {
            return None; // non-canonical
        }
        bytes.push((bits >> 16) as u8);
        if pads_here < 2 {
            bytes.push((bits >> 8) as u8);
        }
        if pads_here < 1 {
            bytes.push(bits as u8);
        }
    }
    Some(bytes)
}

fn bad_line(reason: &str) -> Error {
    make_error(
        ErrorCode::ParseFailure,
        format!("the signature line {reason}"),
        "expected `ed25519 <16 hex digits> <88 base64 characters>` and one line end",
    )
}

// ---- the public key: a point of the prime-order subgroup ---------------------------------------

// L = 2^252 + 27742317777372353535851937790883648493, the order of the base point's subgroup (RFC
// 8032, section 5.1), as bytes, little-endian, the order the signature stores S in.
const GROUP_ORDER_LITTLE_ENDIAN: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyVerdict {
    Accepted,
    /// y >= p, or x = 0 with the sign bit set (RFC 8032, 5.1.3).
    NonCanonical,
    /// No x for this y.
    NotOnTheCurve,
    /// The identity: (0, 1).
    NeutralElement,
    /// A point of small order, or of mixed order (L * A is not the identity).
    NotPrimeOrder,
}

// Whether the 255 bits of y, under the sign bit, are below p = 2^255 - 19 (RFC 7748, section
// 4.1). Little-endian, p is ed, then thirty ff, then 7f: y is at or above it exactly when its top
// byte is 7f, the thirty between are ff and its lowest byte is ed or more.
fn y_is_canonical(key: &PublicKey) -> bool {
    let top = key[31] & 0x7f;
    let at_or_above_prime =
        top == 0x7f && key[1..31].iter().all(|&byte| byte == 0xff) && key[0] >= 0xed;
    !at_or_above_prime
}

// Decodes the key as RFC 8032, 5.1.3 does (its refusals of a y at or above p, of a y with no x
// and of "-0" included) and then requires L * A = the identity. Every point of the curve is a
// point of the prime-order subgroup plus a point of the 8-element torsion subgroup, and L is odd
// and prime to 8, so L * A is the identity exactly when the torsion part is, i.e. when A is in the
// prime-order subgroup (or is the identity, refused before).
fn classify_public_key(key: &PublicKey) -> KeyVerdict {
    if !y_is_canonical(key) {
        return KeyVerdict::NonCanonical;
    }
    let Some(point) = CompressedEdwardsY(*key).decompress() else {
        return KeyVerdict::NotOnTheCurve;
    };
    // "-0" has no encoding: decompression read it as x = 0, whose encoding has the sign bit clear.
    if point.compress().as_bytes() != key {
        return KeyVerdict::NonCanonical;
    }
    if point.is_identity() {
        return KeyVerdict::NeutralElement;
    }
    // The sign bit picks x or -x. Whether the point lies in the prime-order subgroup is the same
    // for both (-A = A reflected is in it exactly when A is).
    if !point.is_torsion_free() {
        return KeyVerdict::NotPrimeOrder;
    }
    KeyVerdict::Accepted
}

fn refused_key(verdict: KeyVerdict, key: &PublicKey) -> Error {
    let why = match verdict {
        KeyVerdict::NonCanonical => "not a canonical encoding (RFC 8032, 5.1.3)",
        KeyVerdict::NotOnTheCurve => "not the encoding of a point of the curve",
        KeyVerdict::NeutralElement => "the neutral element",
        KeyVerdict::NotPrimeOrder => {
            "a point of small or mixed order, not of the prime-order subgroup"
        }
        KeyVerdict::Accepted => "",
    };
    make_error(
        ErrorCode::InvalidArgument,
        format!("the public key is not accepted: {why}"),
        format!("key {}", key_id_of(key)),
    )
}

// S, the last 32 bytes of the signature as a little-endian number, compared with L from the top.
fn scalar_is_below_group_order(signature: &SignatureBytes) -> bool {
    for (&s, &order) in signature[32..].iter().zip(&GROUP_ORDER_LITTLE_ENDIAN).rev() {
        if s != order {
            return s < order;
        }
    }
    false // S == L
}

/// The first 16 lower-case hexadecimal digits of the key's 32 bytes.
pub fn key_id_of(key: &PublicKey) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut id = String::with_capacity(KEY_ID_DIGITS);
    for &byte in &key[..KEY_ID_DIGITS / 2] {
        id.push(char::from(DIGITS[usize::from(byte >> 4)]));
        id.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    id
}

/// 64 lower-case hexadecimal digits: the key. ParseFailure otherwise.
pub fn parse_public_key_hex(hex: &str) -> Result<PublicKey> {
    let digits = hex.as_bytes();
    let mut key: PublicKey = [0; 32];
    if digits.len() != 2 * key.len() || !is_all_lower_hex(digits) {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "a public key is 64 lower-case hexadecimal digits",
            format!("got {} characters", digits.len()),
        ));
    }
    for (byte, pair) in key.iter_mut().zip(digits.as_chunks::<2>().0) {
        *byte = hex_value(pair[0]) * 16 + hex_value(pair[1]);
    }
    Ok(key)
}

/// `ed25519 <keyid> <base64>`: its parts. ParseFailure for another algorithm, a malformed key id,
/// base64 that is not 64 bytes, or more than one line.
pub fn parse_signature_line(text: impl AsRef<[u8]>) -> Result<SignatureLine> {
    let text = text.as_ref();
    if text.len() as u64 > MAX_SIGNATURE_BYTES {
        return Err(bad_line(&format!(
            "is longer than the {MAX_SIGNATURE_BYTES} bytes allowed"
        )));
    }
    // One line, ended by at most one LF or CRLF. A stray CR or a second LF is a second line.
    let line = text
        .strip_suffix(b"\r\n")
        .or_else(|| text.strip_suffix(b"\n"))
        .unwrap_or(text);
    if line.is_empty() {
        return Err(bad_line("is empty"));
    }
    if line.iter().any(|&c| c == b'\r' || c == b'\n') {
        return Err(bad_line("is more than one line"));
    }

    // Exactly two single spaces. Anything else (a tab, a doubled or trailing space, a missing
    // field) leaves a token that fails its own test below or too few tokens.
    let fields: Vec<&[u8]> = line.split(|&c| c == b' ').collect();
    let [algorithm, key_id, encoded] = fields[..] else {
        return Err(bad_line(
            "does not have exactly three fields separated by single spaces",
        ));
    };

    if algorithm != ALGORITHM {
        return Err(bad_line("names an algorithm other than ed25519"));
    }
    if key_id.len() != KEY_ID_DIGITS || !is_all_lower_hex(key_id) {
        return Err(bad_line(
            "has a key id that is not 16 lower-case hexadecimal digits",
        ));
    }
    if encoded.len() != SIGNATURE_BASE64_LENGTH {
        return Err(bad_line(
            "has a signature that is not 88 base64 characters (64 bytes)",
        ));
    }
    let Some(bytes) = decode_base64_canonical(encoded) else {
        return Err(bad_line(
            "has a signature that is not canonical padded base64",
        ));
    };
    // 88 characters are 64 bytes only with two pads; with one pad or none they are 65 or 66.
    let Ok(signature) = SignatureBytes::try_from(bytes.as_slice()) else {
        return Err(bad_line(&format!(
            "has a signature of {} bytes, not 64",
            bytes.len()
        )));
    };

    Ok(SignatureLine {
        algorithm: String::from_utf8_lossy(algorithm).into_owned(),
        key_id: String::from_utf8_lossy(key_id).into_owned(),
        signature,
    })
}

/// OK when `signature` is a valid Ed25519 signature of `message` under `key`. InvalidArgument
/// when it is not (the message says which: key not accepted, S out of range, mismatch), never a
/// panic on any input, and never OK for a message that was altered by one bit. Internal, and only
/// then, when the check itself cannot run, which says nothing about the signature; the C++'s
/// libcrypto could fail so, and the port's verifier cannot, but the code is kept for the callers
/// that tell the two apart.
pub fn verify_ed25519(
    key: &PublicKey,
    message: impl AsRef<[u8]>,
    signature: &SignatureBytes,
) -> Status {
    let context = format!("key {}", key_id_of(key));
    // S first, which needs nothing, and the key; and only then the verdict on the signature.
    if !scalar_is_below_group_order(signature) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the signature is refused: S is not below the group order L (RFC 8032, 5.1.7)",
            context,
        ));
    }
    let verdict = classify_public_key(key);
    if verdict != KeyVerdict::Accepted {
        return Err(refused_key(verdict, key));
    }
    // Not reachable for a key the check accepted (it decodes the same way), and said as the C++
    // says a verification it could not set up.
    let Ok(verifying) = VerifyingKey::from_bytes(key) else {
        return Err(make_error(
            ErrorCode::Internal,
            "the verification could not be set up",
            context,
        ));
    };
    // An empty message is a message like any other (RFC 8032's TEST 1 signs exactly that).
    match verifying.verify_strict(message.as_ref(), &Signature::from_bytes(signature)) {
        Ok(()) => Ok(()),
        Err(_) => Err(make_error(
            ErrorCode::InvalidArgument,
            "the signature does not match the message under this key",
            context,
        )),
    }
}

/// OK when `key` passes the key check that verify_ed25519 makes: it is the canonical encoding of a
/// point of the prime-order subgroup, and not the neutral element. InvalidArgument (the message
/// says "the public key is not accepted" and why) when it is not. For a caller that holds a key of
/// its own (the trust a build pins, trust.rs) and must tell "this key is unusable" from "this
/// signature is wrong": a refusal of the key says nothing about any signature.
pub fn check_public_key(key: &PublicKey) -> Status {
    let verdict = classify_public_key(key);
    if verdict != KeyVerdict::Accepted {
        return Err(refused_key(verdict, key));
    }
    Ok(())
}

/// The first thing the helper's main() does: removes OPENSSL_CONF, OPENSSL_MODULES and
/// OPENSSL_ENGINES from this process's environment. The C++ also starts libcrypto without reading
/// a configuration file; the port's verifier reads none, so the variables matter only to the
/// programs this one starts, which inherit none of them. Not called by katana, katana_cli or
/// katana_mcp: they share a process with an HTTP library and a toolkit that read those variables
/// for their own reasons. Idempotent. Internal if one of the three variables is still set
/// afterwards (the other two are removed all the same). The helper that relaunches katana reads
/// them BEFORE this call if it means to hand the user's own values on, and builds the child's
/// environment from that copy.
///
/// # Safety
///
/// No other thread may read or write the environment while this runs (the C++'s "not thread-safe
/// against other threads reading the environment, so call it before starting any"): it changes
/// the environment, which std::env::remove_var requires of its caller.
#[allow(
    unsafe_code,
    reason = "Edition 2024 marks changing the environment unsafe; the caller's promise is this function's"
)]
pub unsafe fn harden_open_ssl_environment() -> Status {
    // The three variables that make OpenSSL read a file or load code from a place the environment
    // names (a configuration, a provider module, an engine). The same list as
    // tools/update_make_manifest.py gives the openssl it runs.
    const VARIABLES: [&str; 3] = ["OPENSSL_CONF", "OPENSSL_MODULES", "OPENSSL_ENGINES"];
    let mut still_set: Option<&str> = None;
    for name in VARIABLES {
        // SAFETY: the caller promises that no other thread reads or writes the environment while
        // this runs (the function's safety contract). On Windows the name is matched without
        // regard to case, as Windows matches it.
        unsafe { std::env::remove_var(name) };
        // Trusted on its own word as little as the C++ trusts the return value: the variable is
        // looked for, and a survivor is reported after the others have had their turn.
        if std::env::var_os(name).is_some() && still_set.is_none() {
            still_set = Some(name);
        }
    }
    if let Some(name) = still_set {
        return Err(make_error(
            ErrorCode::Internal,
            "an OpenSSL environment variable could not be removed",
            name,
        ));
    }
    Ok(())
}
