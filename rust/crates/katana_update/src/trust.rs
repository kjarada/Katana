//! Who this program trusts to sign an update, and the one function that turns bytes from the
//! network into a Manifest.
//!
//! Two Ed25519 public keys are pinned at build time (cmake/KatanaUpdateKeys.cmake): `active`,
//! whose private half CI signs releases with, and an offline `spare`. Either verifies. Rotation:
//! ship a build that pins a new spare; if the active key leaks, sign a release with the spare that
//! drops it. Lose both and an installed copy cannot update until it is installed again by hand.
//!
//! THE TRUST ROOT IS A PARAMETER, never a global the environment can move. [`production_trust`]
//! is what katana, katana_cli, katana_mcp and katana_updater pass; only the test executables,
//! never installed, pass a test key. So no test key exists in a shipped binary, and no variable,
//! file or flag at run time changes which signatures count.
//!
//! The pinned keys reach this crate as the C++ build passes them to trust.cpp: build.rs reads the
//! block of cmake/KatanaUpdateKeys.cmake that tools/update_keytool.py rewrites, holds it to the
//! same rules the configure does, and hands it over as KATANA_UPDATE_KEY_ACTIVE,
//! KATANA_UPDATE_KEY_SPARE (64 lower-case hexadecimal digits each) and
//! KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS (1 while the file holds the placeholders, else 0). This
//! module alone reads them.
//!
//! PLACEHOLDERS. Until the owner ran the key steps in docs/update.md the pinned keys were
//! placeholders: 32 bytes that spell "PLACEHOLDER-..." in ASCII and that are not the encoding of
//! any curve point, so no private key can match them. A Trust built from them has `placeholder ==
//! true`, and verify_manifest REFUSES to verify against it (Unsupported, "this build carries
//! placeholder update keys"), so a development build can neither be updated nor forged for.

use crate::feed::{MAX_MANIFEST_BYTES, MAX_SIGNATURE_BYTES};
use crate::install::Install;
use crate::manifest::{Manifest, parse_manifest};
use crate::signature::{
    PublicKey, SignatureBytes, check_public_key, key_id_of, parse_public_key_hex,
    parse_signature_line, verify_ed25519,
};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};
use std::cmp::Ordering;
use std::sync::OnceLock;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrustedKey {
    /// key_id_of(key).
    pub id: String,
    pub key: PublicKey,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trust {
    /// Active, spare.
    pub keys: Vec<TrustedKey>,
    /// True: the pinned keys are the placeholders (see above).
    pub placeholder: bool,
}

const PINNED_ACTIVE: &str = env!("KATANA_UPDATE_KEY_ACTIVE");
const PINNED_SPARE: &str = env!("KATANA_UPDATE_KEY_SPARE");
const KEYS_ARE_PLACEHOLDERS: &str = env!("KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS");
// The only channel a build follows; the signed channel member is held to it.
const CHANNEL: &str = "stable";

const fn is_lower_hex(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !matches!(bytes[i], b'0'..=b'9' | b'a'..=b'f') {
            return false;
        }
        i += 1;
    }
    true
}

const fn same_text(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// The C++'s static_asserts on the same definitions (trust.cpp).
const _: () = assert!(
    PINNED_ACTIVE.len() == 64,
    "KATANA_UPDATE_KEY_ACTIVE is 64 hexadecimal digits"
);
const _: () = assert!(
    PINNED_SPARE.len() == 64,
    "KATANA_UPDATE_KEY_SPARE is 64 hexadecimal digits"
);
const _: () = assert!(
    is_lower_hex(PINNED_ACTIVE),
    "KATANA_UPDATE_KEY_ACTIVE is lower-case hexadecimal"
);
const _: () = assert!(
    is_lower_hex(PINNED_SPARE),
    "KATANA_UPDATE_KEY_SPARE is lower-case hexadecimal"
);
const _: () = assert!(
    !same_text(PINNED_ACTIVE, PINNED_SPARE),
    "the active and the spare key are two keys"
);
const _: () = assert!(
    same_text(KEYS_ARE_PLACEHOLDERS, "0") || same_text(KEYS_ARE_PLACEHOLDERS, "1"),
    "KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS is 0 or 1"
);

fn build_production_trust() -> Trust {
    let mut trust = Trust {
        keys: Vec::with_capacity(2),
        placeholder: same_text(KEYS_ARE_PLACEHOLDERS, "1"),
    };
    for hex in [PINNED_ACTIVE, PINNED_SPARE] {
        let Ok(key) = parse_public_key_hex(hex) else {
            // Not reachable: the assertions above hold both strings to the form the parser
            // takes. But if it were, the answer is a trust that verifies nothing, never a
            // partial one.
            return Trust {
                keys: Vec::new(),
                placeholder: true,
            };
        };
        trust.keys.push(TrustedKey {
            id: key_id_of(&key),
            key,
        });
    }
    trust
}

// verify_ed25519 under a key the TRUST lists. A key that is no usable public key is not a bad
// signature: it is a trust that was built wrongly (a mistyped pin), and it would refuse every
// release of every install. verify_ed25519 answers that with InvalidArgument, the code of a forged
// signature, which the callers hold to be unreachable for a genuine release; so the key is checked
// first and its failure is reported as Internal, "could not check", with its reason.
fn verify_with_pinned_key(
    key: &PublicKey,
    signed_bytes: &[u8],
    signature: &SignatureBytes,
) -> Status {
    if let Err(why) = check_public_key(key) {
        return Err(make_error(
            ErrorCode::Internal,
            format!("a key of the update trust cannot be used: {}", why.message),
            why.context,
        ));
    }
    verify_ed25519(key, signed_bytes, signature)
}

/// The keys this build pinned: `active` then `spare`, from KATANA_UPDATE_KEY_ACTIVE and
/// KATANA_UPDATE_KEY_SPARE. One object, whoever asks and whenever.
pub fn production_trust() -> &'static Trust {
    static TRUST: OnceLock<Trust> = OnceLock::new();
    TRUST.get_or_init(build_production_trust)
}

/// The whole gate. `json` and `sig` are the two files of a release as downloaded. In this order,
/// stopping at the first failure:
///
/// 0. `json` is not empty and not above MAX_MANIFEST_BYTES (InvalidArgument), whatever `sig`
///    holds;
/// 1. the signature line parses (ParseFailure);
/// 2. trust is not a placeholder (Unsupported, "this build carries placeholder update keys"):
///    asked before the key is looked up, so that a development build says why it refuses every
///    release and not that it does not know the signer;
/// 3. the line's key id names a key in `trust` (otherwise Unsupported "signed by a key this build
///    does not trust");
/// 4. verify_ed25519 over the EXACT bytes of `json`;
/// 5. only now parse_manifest;
/// 6. the signed product equals install.product, target equals install.target, channel is
///    "stable";
///
/// and the result carries signed_json and signature_line. It does NOT decide "newer than
/// installed" (check() does, with the installed version and sequence), because the helper repeats
/// that test itself from its own copy. Failures: InvalidArgument (an empty or oversize manifest, a
/// signature that does not verify, a manifest for another product, target or channel),
/// ParseFailure (a signature line or a manifest that does not parse), Unsupported (a key this
/// build does not trust, or placeholder keys), and Internal when the check could not be made: the
/// key the line names is no usable public key (a Trust built wrongly: a mistyped pin, say). That
/// says nothing about the manifest, so a caller must treat it as "could not check", never as
/// "forged". When several listed keys share the named id, a key that accepts wins, then an
/// Internal, then the first refusal. Callers that tell these apart do it on the code, not on the
/// message text.
pub fn verify_manifest(
    json: impl AsRef<[u8]>,
    sig: impl AsRef<[u8]>,
    trust: &Trust,
    install: &Install,
) -> Result<Manifest> {
    let (json, sig) = (json.as_ref(), sig.as_ref());
    // Before any parsing or arithmetic: nothing is signed by an empty file, and what the transport
    // was allowed to hand over (MAX_MANIFEST_BYTES) is also the most that is verified.
    if json.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the manifest is empty",
            "",
        ));
    }
    if json.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the manifest is larger than the {MAX_MANIFEST_BYTES} bytes this program accepts"
            ),
            format!("{} bytes", json.len()),
        ));
    }

    // The bytes this call works on, taken once: the signature is checked over these, the manifest
    // is read from these, and these are what the result carries, so "the bytes that were verified
    // are the bytes that are kept" holds by construction. The line is copied up to one byte past
    // its cap, which is all parse_signature_line needs to refuse an oversize one.
    let signed_json = json.to_vec();
    let line_cap = (MAX_SIGNATURE_BYTES as usize + 1).min(sig.len());
    let signature_line = sig[..line_cap].to_vec();

    // 1. The signature line parses.
    let line = parse_signature_line(&signature_line)?;

    // 2. A trust flagged as placeholders verifies nothing, whatever it lists, and for such a build
    //    that is the reason an update is refused: it is said here, before the lookup that would
    //    otherwise answer "signed by a key this build does not trust" to every real release.
    if trust.placeholder {
        return Err(make_error(
            ErrorCode::Unsupported,
            "this build carries placeholder update keys",
            format!("key {}", line.key_id),
        ));
    }

    // 3. The line names a key this build trusts. A listed key whose id is not the id of its bytes
    //    is a Trust somebody built wrongly, and counts as absent.
    // 4. The signature over the exact bytes. Every listed key with the named id is tried, in the
    //    order listed (active, then spare): two keys share an id with probability 2^-64, but a
    //    Trust is a parameter and need not come from the generator. What is reported: a key that
    //    accepts wins; else a failure to check (Internal) over a refusal, so that "could not look"
    //    is never hidden behind "forged"; else the first refusal.
    let mut outcome: Option<Status> = None;
    for candidate in &trust.keys {
        if candidate.id != line.key_id || key_id_of(&candidate.key) != candidate.id {
            continue;
        }
        let attempt = verify_with_pinned_key(&candidate.key, &signed_json, &line.signature);
        let accepted = attempt.is_ok();
        let outranks = match (&attempt, &outcome) {
            (Err(now), Some(Err(before))) => {
                now.code == ErrorCode::Internal && before.code != ErrorCode::Internal
            }
            _ => false,
        };
        if accepted || outcome.is_none() || outranks {
            outcome = Some(attempt);
        }
        if accepted {
            break;
        }
    }
    match outcome {
        None => {
            return Err(make_error(
                ErrorCode::Unsupported,
                "signed by a key this build does not trust",
                format!("key {}", line.key_id),
            ));
        }
        Some(Err(error)) => return Err(error),
        Some(Ok(())) => {}
    }

    // 5. Only now is the text read as a manifest.
    let mut manifest = parse_manifest(&signed_json)?;

    // 6. Signed, and for this program: a validly signed manifest of another product, another
    //    platform or another channel is still not an update for this install.
    if manifest.product != install.product {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the manifest is signed for another product",
            format!(
                "manifest '{}', this install '{}'",
                manifest.product, install.product
            ),
        ));
    }
    if manifest.target != install.target {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the manifest is signed for another target",
            format!(
                "manifest '{}', this install '{}'",
                manifest.target, install.target
            ),
        ));
    }
    if manifest.channel != CHANNEL {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the manifest is signed for another channel than stable",
            format!("manifest '{}'", manifest.channel),
        ));
    }

    manifest.signed_json = signed_json;
    manifest.signature_line = signature_line;
    Ok(manifest)
}

/// True when the manifest is above what is installed: a higher version; or the same version with
/// a higher sequence, but only when the install has the helper's record of its build
/// (Install::has_installed_record). A fresh install has none (its sequence is the placeholder 0,
/// whatever its package names, install.rs), so the same version is NOT newer: otherwise a fresh
/// install would be offered the very release it was made from. A lower version is never newer.
/// The one place the order is written (check() and the helper both call it). In the C++ it is one
/// of the finished functions of contract.cpp, with its tests in tests/update/test_contract.cpp.
pub fn is_newer_than(manifest: &Manifest, install: &Install) -> bool {
    match manifest.version.cmp(&install.version) {
        Ordering::Equal => {
            // The same version. Only a record the helper wrote says which build is installed; a
            // fresh install has none and counts as the release it was made from.
            install.has_installed_record && manifest.sequence > install.sequence
        }
        order => order == Ordering::Greater,
    }
}
