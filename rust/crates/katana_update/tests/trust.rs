//! The pinned keys and the gate between downloaded bytes and a Manifest (the twin of
//! tests/update/test_trust.cpp). The order of the gate is trust.rs's: the size; the signature line
//! parses; the trust is not a placeholder; the line names a trusted key; the signature verifies
//! over the exact bytes; only then is the text read as a manifest; and then product, target and
//! channel are held to the install. Each case here says which of those it reaches.
//!
//! Expectations come from outside the code under test (CLAUDE.md, section 4): the fixtures signed
//! by the `openssl` command line, the RFC 8032 section 7.1 public keys, and one manifest written
//! below and signed with RFC 8032 TEST 1's secret key by a pure-Python implementation of RFC 8032
//! (checked against the RFC's own vectors, and accepted by `openssl pkeyutl -verify -rawin`). No
//! test key is in any program that ships: the Trust is a PARAMETER and these tests build theirs
//! (tests/test_key, the twin of tests/update/test_key.hpp).
//!
//! Nothing here skips: every case runs against the real manifest reader, so a regression of the
//! reader fails the cases that read a Manifest instead of passing quietly.

pub mod test_key;

use katana_core::path_text::{path_from_utf8, read_file_bytes};
use katana_core::{ErrorCode, Result};
use katana_update::feed::MAX_MANIFEST_BYTES;
use katana_update::install::{Install, InstallKind};
use katana_update::manifest::Manifest;
use katana_update::signature::{
    PublicKey, check_public_key, key_id_of, parse_signature_line, verify_ed25519,
};
use katana_update::trust::{Trust, TrustedKey, production_trust, verify_manifest};
use test_key::test_trust;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tests/update/fixtures"
);

// Through the project's reader, so that a checkout under a folder named in any script opens.
fn read_fixture(relative: &str) -> Vec<u8> {
    read_file_bytes(&path_from_utf8(format!("{FIXTURES}/{relative}")))
        .unwrap_or_else(|error| panic!("{relative}: {}", error.describe()))
}

struct Release {
    json: Vec<u8>,
    sig: Vec<u8>,
}

fn read_release(folder: &str) -> Release {
    read_release_of(folder, "win64")
}

fn read_release_of(folder: &str, target: &str) -> Release {
    let base = format!("release/{folder}/Katana-update-{target}.json");
    Release {
        json: read_fixture(&base),
        sig: read_fixture(&format!("{base}.sig")),
    }
}

fn install_for(target: &str) -> Install {
    Install {
        target: target.to_owned(),
        kind: InstallKind::Archive,
        ..Install::default() // product "katana" by default
    }
}

fn key_from_hex(hex: &str) -> PublicKey {
    let digit = |c: u8| if c <= b'9' { c - b'0' } else { c - b'a' + 10 };
    let digits = hex.as_bytes();
    let mut key: PublicKey = [0; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = digit(digits[2 * i]) * 16 + digit(digits[2 * i + 1]);
    }
    key
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn describe(verified: &Result<Manifest>) -> String {
    match verified {
        Ok(_) => String::from("accepted"),
        Err(error) => error.describe(),
    }
}

// The value of `set(<name> <value>)` inside the block of cmake/KatanaUpdateKeys.cmake that
// tools/update_keytool.py rewrites (between the ">>> keys" and "<<< keys" lines), without its
// quotes; empty when the block or the line is not there.
fn keys_cmake_value(file: &str, name: &str) -> String {
    let (Some(begin), Some(end)) = (file.find("# >>> keys"), file.find("# <<< keys")) else {
        return String::new();
    };
    if end < begin {
        return String::new();
    }
    let block = &file[begin..end];
    let opening = format!("set({name} ");
    let Some(at) = block.find(&opening) else {
        return String::new();
    };
    let from = at + opening.len();
    let Some(close) = block[from..].find(')') else {
        return String::new();
    };
    let value = &block[from..from + close];
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_owned()
}

// True when `key` passes the key check of verify_ed25519 (a prime-order point). Held to the
// signature layer's own answer too: a signature that is wrong for a key the check accepts is a
// MISMATCH, and for one it does not accept, "public key is not accepted".
fn key_is_accepted_as_a_key(key: &PublicKey) -> bool {
    let checked = check_public_key(key).is_ok();
    let error = verify_ed25519(key, b"x", &[0u8; 64]).expect_err("a zero signature verified");
    assert_eq!(
        checked,
        error.message.contains("does not match"),
        "check_public_key and verify_ed25519 disagree about the key"
    );
    checked
}

// The macOS notify-only manifest of the fixtures with its channel changed to "beta" (and compacted
// to one line), signed with TEST 1's secret key by the Python of the module's comment. A beta
// manifest is validly signed: only the channel rule can refuse it.
const BETA_MANIFEST: &str = r#"{"applier":1,"channel":"beta","expires":"2027-04-16T08:40:00Z","files":[],"notes_url":"https://github.com/kjarada/Katana/releases/tag/v0.10.0","notify_only":true,"product":"katana","published":"2026-10-18T08:40:00Z","schema":1,"sequence":1792312800,"tag":"v0.10.0","target":"macos-arm64","version":"0.10.0"}"#;
const BETA_SIGNATURE_LINE: &str = "ed25519 d75a980182b10ab7 H8tVyMWKZx9U2xHSecYJ/WDTV5rlUxfDb+gKHSJPwXIxsSYPOOmmYsM4I4JO1ZgLrqo7MHSj2NF48WrFff0dDA==\n";

// A text that is not a manifest, signed with TEST 1's secret key by the same Python (and accepted
// by `openssl pkeyutl -verify -rawin`): the signature is good, so only the parser can refuse it,
// and it is only asked after the signature.
const SIGNED_GARBAGE: &str = "This text is signed, but it is not a manifest.\n";
const SIGNED_GARBAGE_LINE: &str = "ed25519 d75a980182b10ab7 B5eWfJ4fKMnDEO0NgevDBR824jSWQi6h5cE3S36DSHkmFOJdo8TGUb/vbwaOpsgUHkch+viB0GrrQO2gbifgBw==\n";

// "PLACEHOLDER-..." as cmake/KatanaUpdateKeys.cmake spells them (the ASCII of the words).
const PLACEHOLDER_ACTIVE_HEX: &str =
    "504c414345484f4c4445522d4143544956452d4e4f542d412d4b45592d30302d";
const PLACEHOLDER_SPARE_HEX: &str =
    "504c414345484f4c4445522d53504152452d4e4f542d412d4b45592d2d30302d";

const PUBLISHED_KEYS: [&str; 3] = [
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
    "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
    "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
];

fn trust_of(hex_keys: &[&str], placeholder: bool) -> Trust {
    Trust {
        keys: hex_keys
            .iter()
            .map(|hex| TrustedKey {
                id: hex[..16].to_owned(),
                key: key_from_hex(hex),
            })
            .collect(),
        placeholder,
    }
}

// Keys with TEST 1's first 8 bytes (so the same key id) that are no public key at all: after those
// bytes only byte 8 is set (to 1, or to 2), so y is a number of about 2^71 for which no x exists
// (RFC 8032, 5.1.3). Found, and the two checked, with a pure-Python decoder written from the RFC
// (which decodes the three RFC keys and does not decode y = 2 or the placeholders).
fn unusable_key_with_the_id_of_test_one(ninth: u8) -> TrustedKey {
    let mut key = test_trust().keys[0].key;
    key[8..].fill(0);
    key[8] = ninth;
    TrustedKey {
        id: key_id_of(&key),
        key,
    }
}

mod trust {
    use super::*;

    // ---- the keys this build pins ------------------------------------------------------------

    #[test]
    fn the_build_pins_two_distinct_keys_and_none_of_them_is_a_published_test_key() {
        let trust = production_trust();
        assert_eq!(trust.keys.len(), 2); // active, then spare
        assert_ne!(trust.keys[0].key, trust.keys[1].key);
        if !trust.placeholder {
            // Real keys differ in their first 8 bytes with probability 1 - 2^-64. (The two
            // placeholders both begin "PLACEHOLD", so they share an id; they verify nothing.)
            assert_ne!(trust.keys[0].id, trust.keys[1].id);
        }
        for key in &trust.keys {
            assert_eq!(key.id, key_id_of(&key.key));
            assert_eq!(key.id.len(), 16);
        }
        // The three public keys of RFC 8032 section 7.1: anyone holds their private halves, and
        // cmake/KatanaUpdateKeys.cmake refuses to pin them. A trust that held one would be one
        // that anybody could sign for.
        for published in PUBLISHED_KEYS {
            assert_ne!(trust.keys[0].key, key_from_hex(published));
            assert_ne!(trust.keys[1].key, key_from_hex(published));
        }
        // The one object, whoever asks and whenever.
        assert!(std::ptr::eq(production_trust(), trust));
    }

    #[test]
    fn the_pinned_keys_are_the_ones_in_katana_update_keys_cmake_active_then_spare() {
        // What trust.rs pins reaches it from cmake/KatanaUpdateKeys.cmake through build.rs. This
        // reads the same file as TEXT, the other way round, so that keys swapped between the two
        // slots, a value that went stale, or a flag that disagrees with the file are all caught:
        // the file is the oracle, "active" being the one CI signs with and "spare" the offline
        // one. (The file is reached through the fixtures folder: tests/update/fixtures -> the
        // source root.)
        let file = String::from_utf8(read_fixture("../../../cmake/KatanaUpdateKeys.cmake"))
            .expect("the keys file is text");
        let active = keys_cmake_value(&file, "KATANA_UPDATE_KEY_ACTIVE");
        let spare = keys_cmake_value(&file, "KATANA_UPDATE_KEY_SPARE");
        let flag = keys_cmake_value(&file, "KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS");
        assert_eq!(
            active.len(),
            64,
            "KATANA_UPDATE_KEY_ACTIVE not found in the keys block"
        );
        assert_eq!(
            spare.len(),
            64,
            "KATANA_UPDATE_KEY_SPARE not found in the keys block"
        );
        assert!(
            flag == "ON" || flag == "OFF",
            "KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS is '{flag}'"
        );

        let trust = production_trust();
        assert_eq!(trust.keys.len(), 2);
        assert_eq!(
            trust.keys[0].key,
            key_from_hex(&active),
            "slot 0 must be the active key"
        );
        assert_eq!(
            trust.keys[1].key,
            key_from_hex(&spare),
            "slot 1 must be the spare key"
        );
        // The key id is the first 16 digits of the key as the file spells it
        // (tools/update_keytool.py).
        assert_eq!(trust.keys[0].id, &active[..16]);
        assert_eq!(trust.keys[1].id, &spare[..16]);
        assert_eq!(trust.placeholder, flag == "ON");

        if trust.placeholder {
            // While the owner has not pinned real keys: the ASCII the file's comment says they
            // spell.
            assert!(trust.keys[0].key.starts_with(b"PLACEHOLDER-ACTIVE"));
            assert!(trust.keys[1].key.starts_with(b"PLACEHOLDER-SPARE"));
        }
    }

    #[test]
    fn the_key_check_helper_tells_a_key_the_check_accepts_from_one_it_does_not() {
        // The helper the next case holds the pinned keys to, itself held to keys whose status is
        // known from outside: the three published RFC 8032 keys are prime-order points; the
        // all-zero key is a point of order 4, the two placeholders are no point at all.
        for published in PUBLISHED_KEYS {
            assert!(
                key_is_accepted_as_a_key(&key_from_hex(published)),
                "{published}"
            );
        }
        assert!(!key_is_accepted_as_a_key(&[0; 32]));
        assert!(!key_is_accepted_as_a_key(&key_from_hex(
            PLACEHOLDER_ACTIVE_HEX
        )));
        assert!(!key_is_accepted_as_a_key(&key_from_hex(
            PLACEHOLDER_SPARE_HEX
        )));
    }

    #[test]
    fn a_real_pinned_key_passes_the_key_check_and_a_placeholder_does_not() {
        // Once the owner pins real keys (docs/update.md), a key that was mistyped, or that is not
        // a prime-order point, would refuse every update on every install with "public key is not
        // accepted", and nothing else in the build would say so. So when the build pins real
        // keys, both must pass the check; a placeholder build's must not (they verify nothing).
        let trust = production_trust();
        for key in &trust.keys {
            assert_eq!(
                key_is_accepted_as_a_key(&key.key),
                !trust.placeholder,
                "{}",
                key.id
            );
        }
    }

    #[test]
    fn the_build_says_its_keys_are_the_placeholders_exactly_when_they_spell_them_and_then_verifies_nothing()
     {
        // The rule of cmake/KatanaUpdateKeys.cmake, which fails the configure (and build.rs) when
        // the flag and the bytes disagree: "PLACEHOLDER" in ASCII is the first 11 bytes of a
        // placeholder key.
        let trust = production_trust();
        assert_eq!(trust.keys.len(), 2);
        let word = b"PLACEHOLDER";
        let spelled = trust.keys[0].key.starts_with(word) || trust.keys[1].key.starts_with(word);
        assert_eq!(trust.placeholder, spelled);

        // A genuinely signed release, from a trusted-by-the-tests key, gets nothing from this
        // build whichever it is: with placeholders the trust is refused outright; with real keys
        // the test key is simply not among them.
        let release = read_release("v2");
        let verified = verify_manifest(&release.json, &release.sig, trust, &install_for("win64"));
        let error =
            verified.expect_err("a test-signed manifest verified under the production keys");
        assert_eq!(error.code, ErrorCode::Unsupported, "{}", error.describe());

        if trust.placeholder {
            // Every verification FAILS: not one byte of any signature is checked against them.
            for key in &trust.keys {
                let error = verify_ed25519(&key.key, b"x", &[0u8; 64]).expect_err("verified");
                assert!(
                    error.message.contains("public key is not accepted"),
                    "{}",
                    error.describe()
                );
            }
        }
    }

    // ---- placeholders ------------------------------------------------------------------------

    #[test]
    fn the_placeholder_keys_are_no_points_so_no_signature_verifies_under_them() {
        // The two strings in cmake/KatanaUpdateKeys.cmake as they were, as keys: for each, a
        // genuine signature (TEST 1's, over the v2 manifest) and an all-zero one are refused for
        // the key.
        let release = read_release("v2");
        let line = parse_signature_line(&release.sig).expect("the line parses");
        for hex in [PLACEHOLDER_ACTIVE_HEX, PLACEHOLDER_SPARE_HEX] {
            for signature in [line.signature, [0u8; 64]] {
                let error = verify_ed25519(&key_from_hex(hex), &release.json, &signature)
                    .expect_err("verified");
                assert_eq!(error.code, ErrorCode::InvalidArgument);
                assert!(
                    error.message.contains("public key is not accepted"),
                    "{}",
                    error.describe()
                );
            }
        }
    }

    #[test]
    fn a_trust_flagged_as_placeholder_refuses_even_a_signature_that_would_verify() {
        // The test keys are real keys, so under the flag alone nothing but the flag stops the good
        // v2 release: step 2 of the gate, asked whether or not the line names a key in the trust.
        let mut trust = test_trust().clone();
        trust.placeholder = true;
        let release = read_release("v2");
        let error = verify_manifest(&release.json, &release.sig, &trust, &install_for("win64"))
            .expect_err("verified");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(
            error.message.contains("placeholder update keys"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn a_placeholder_trust_says_the_build_carries_placeholders_to_a_real_signer_too() {
        // A development build is asked about a real release: signed by a key that is not one of
        // the placeholders. Unsupported with the header's text (the reason, not "does not
        // trust") because the placeholder question comes before the lookup of the signer.
        let placeholders = trust_of(&[PLACEHOLDER_ACTIVE_HEX, PLACEHOLDER_SPARE_HEX], true);
        let release = read_release("v2");
        let error = verify_manifest(
            &release.json,
            &release.sig,
            &placeholders,
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(
            error.message,
            "this build carries placeholder update keys",
            "{}",
            error.describe()
        );
        assert!(
            error.context.contains("d75a980182b10ab7"),
            "{}",
            error.describe()
        );

        // The same for a trust that lists nothing at all (what production_trust() falls back to).
        let nothing = Trust {
            keys: Vec::new(),
            placeholder: true,
        };
        let error = verify_manifest(&release.json, &release.sig, &nothing, &install_for("win64"))
            .expect_err("verified");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(
            error.message,
            "this build carries placeholder update keys",
            "{}",
            error.describe()
        );
    }

    #[test]
    fn a_placeholder_trust_still_reads_the_size_and_the_signature_line_before_it_refuses() {
        // The order: size, then the line, then the placeholder. A line that is not one is the
        // line's fault whatever the build carries, and so is an empty manifest.
        let placeholders = trust_of(&[PLACEHOLDER_ACTIVE_HEX, PLACEHOLDER_SPARE_HEX], true);
        let release = read_release("v2");
        let garbage = verify_manifest(
            &release.json,
            "not a signature line",
            &placeholders,
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(
            garbage.code,
            ErrorCode::ParseFailure,
            "{}",
            garbage.describe()
        );
        let empty = verify_manifest("", &release.sig, &placeholders, &install_for("win64"))
            .expect_err("verified");
        assert_eq!(
            empty.code,
            ErrorCode::InvalidArgument,
            "{}",
            empty.describe()
        );
    }

    #[test]
    fn a_placeholder_key_named_by_the_signature_line_still_verifies_nothing_even_with_the_flag_off()
    {
        // A trust whose keys are the placeholders but whose flag is wrongly off (the configure
        // step refuses that pairing, but the engine does not rely on it): a line naming a
        // placeholder's id finds the key, and the key is refused as no public key, not accepted.
        // The code is Internal ("could not check", trust.rs), not a forged signature's
        // InvalidArgument: a key of the trust that is no key says nothing about the release it
        // was asked about.
        let trust = trust_of(&[PLACEHOLDER_ACTIVE_HEX, PLACEHOLDER_SPARE_HEX], false);
        let release = read_release("v2");
        let sig = cat(&[b"ed25519 504c414345484f4c ", &release.sig[25..]]);
        let error = verify_manifest(&release.json, &sig, &trust, &install_for("win64"))
            .expect_err("verified");
        assert_eq!(error.code, ErrorCode::Internal, "{}", error.describe());
        assert!(
            error.message.contains("public key is not accepted"),
            "{}",
            error.describe()
        );
    }

    // ---- the gate, in the header's order, up to the parse ------------------------------------

    #[test]
    fn an_empty_manifest_is_refused_before_anything_else_is_looked() {
        // Not even the signature line is read: a garbage one still gives the manifest's reason.
        let error = verify_manifest(
            "",
            "not a signature line",
            test_trust(),
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("manifest is empty"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn a_manifest_above_the_cap_is_refused_and_one_at_the_cap_is_not() {
        let over = vec![b'{'; MAX_MANIFEST_BYTES as usize + 1];
        let too_big = verify_manifest(
            &over,
            "not a signature line",
            test_trust(),
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(too_big.code, ErrorCode::InvalidArgument);
        assert!(
            too_big.message.contains("larger than"),
            "{}",
            too_big.describe()
        );

        // Exactly the cap passes the size gate and meets the next one: the signature line.
        let at_cap = vec![b'{'; MAX_MANIFEST_BYTES as usize];
        let at_the_limit = verify_manifest(
            &at_cap,
            "not a signature line",
            test_trust(),
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(
            at_the_limit.code,
            ErrorCode::ParseFailure,
            "{}",
            at_the_limit.describe()
        );
    }

    #[test]
    fn a_malformed_signature_line_is_a_parse_failure_before_any_key_is_looked_up() {
        let release = read_release("v2");
        for sig in [
            Vec::new(),
            b"\n".to_vec(),
            b"ed25519 d75a980182b10ab7\n".to_vec(),
            cat(&[&release.sig, &release.sig]),
            cat(&[b"ED25519", &release.sig[7..]]),
        ] {
            let verified =
                verify_manifest(&release.json, &sig, test_trust(), &install_for("win64"));
            let Err(error) = verified else {
                panic!("accepted: '{}'", String::from_utf8_lossy(&sig));
            };
            assert_eq!(error.code, ErrorCode::ParseFailure, "{}", error.describe());
        }
    }

    #[test]
    fn a_signer_this_build_does_not_trust_is_unsupported_whatever_the_signature() {
        let release = read_release("bad/wrong-key"); // signed by TEST 3, named as TEST 3
        let error = verify_manifest(
            &release.json,
            &release.sig,
            test_trust(),
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(
            error
                .message
                .contains("signed by a key this build does not trust"),
            "{}",
            error.describe()
        );
        assert!(
            error.context.contains("fc51cd8e6218a1a3"),
            "{}",
            error.describe()
        );

        // Even though that signature is perfectly good: pinning the signer's key makes it verify
        // at the signature layer, so it was the trust that refused it, not the arithmetic.
        let mut with_the_other_key = test_trust().clone();
        with_the_other_key.keys.push(TrustedKey {
            id: String::from("fc51cd8e6218a1a3"),
            key: key_from_hex(PUBLISHED_KEYS[2]),
        });
        let line = parse_signature_line(&release.sig).expect("the line parses");
        assert!(
            verify_ed25519(
                &with_the_other_key.keys.last().expect("a key").key,
                &release.json,
                &line.signature
            )
            .is_ok()
        );
    }

    #[test]
    fn a_trust_with_no_keys_trusts_no_signer() {
        let release = read_release("v2");
        let error = verify_manifest(
            &release.json,
            &release.sig,
            &Trust::default(),
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(error.code, ErrorCode::Unsupported);
    }

    #[test]
    fn a_listed_key_whose_id_is_not_the_id_of_its_bytes_is_not_a_trusted_signer() {
        // A Trust built wrongly: TEST 1's bytes under an id that is not theirs. A line naming that
        // id finds the entry, and the entry is ignored rather than believed.
        let trust = Trust {
            keys: vec![TrustedKey {
                id: String::from("0000000000000000"),
                key: test_trust().keys[0].key,
            }],
            placeholder: false,
        };
        let release = read_release("v2");
        let sig = cat(&[b"ed25519 0000000000000000 ", &release.sig[25..]]);
        let error = verify_manifest(&release.json, &sig, &trust, &install_for("win64"))
            .expect_err("verified");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(
            error.message.contains("does not trust"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn a_flipped_bit_is_refused_by_the_signature_before_the_text_is_parsed() {
        // The answer is the signature's, InvalidArgument "does not match", not the parser's. Held
        // by the code of the error: the parser answers ParseFailure (or another code) for a
        // manifest it dislikes, so a verifier that parsed first could not give this one.
        let release = read_release("bad/flipped-manifest-bit");
        let error = verify_manifest(
            &release.json,
            &release.sig,
            test_trust(),
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("does not match"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn a_text_that_is_not_a_manifest_and_is_not_signed_is_refused_for_its_signature_not_its_shape()
    {
        // The order again, from the other side: with a parser that rejects this text, a gate that
        // parsed first would answer ParseFailure here. The answer is the signature's.
        let release = read_release("v2");
        let error = verify_manifest(
            "this is not json",
            &release.sig,
            test_trust(),
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(
            error.code,
            ErrorCode::InvalidArgument,
            "{}",
            error.describe()
        );
        assert!(
            error.message.contains("does not match"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn a_signed_text_that_is_not_a_manifest_passes_the_signature_and_is_refused_by_the_parser() {
        // The signature verifies (checked at the signature layer first), so the refusal is the
        // parse.
        let line = parse_signature_line(SIGNED_GARBAGE_LINE).expect("the line parses");
        assert!(verify_ed25519(&test_trust().keys[0].key, SIGNED_GARBAGE, &line.signature).is_ok());

        let error = verify_manifest(
            SIGNED_GARBAGE,
            SIGNED_GARBAGE_LINE,
            test_trust(),
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(error.code, ErrorCode::ParseFailure, "{}", error.describe());
        assert!(
            !error.message.contains("does not match"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn a_signature_copied_from_another_release_is_refused() {
        // v2's manifest with v1's genuine signature (same key, same file name): the signature is
        // for the other manifest's bytes.
        let swapped = read_release("bad/swapped-sig");
        let error = verify_manifest(
            &swapped.json,
            &swapped.sig,
            test_trust(),
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("does not match"),
            "{}",
            error.describe()
        );

        // And the two real releases' signatures are not interchangeable: v1's manifest under v2's
        // line.
        let v1 = read_release("v1");
        let v2 = read_release("v2");
        let crossed = verify_manifest(&v1.json, &v2.sig, test_trust(), &install_for("win64"))
            .expect_err("verified");
        assert_eq!(crossed.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_signature_by_another_key_under_the_name_of_a_pinned_one_is_refused() {
        // TEST 3 signed it and the line says "d75a..." (the active key's id): the id selects the
        // key to verify with, and under that key the signature is wrong.
        let liar = read_release("bad/wrong-key-claims-active");
        let error = verify_manifest(&liar.json, &liar.sig, test_trust(), &install_for("win64"))
            .expect_err("verified");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("does not match"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn the_active_keys_signature_is_not_accepted_as_the_spare_s_or_the_other_way_around() {
        // The id picks the key. v2 (active) with its line's id changed to the spare's, and the
        // spare-signed release with the active's: both signatures are good, for the wrong key.
        let active = read_release("v2");
        let as_spare = cat(&[b"ed25519 3d4017c3e843895a ", &active.sig[25..]]);
        let first = verify_manifest(&active.json, &as_spare, test_trust(), &install_for("win64"))
            .expect_err("verified");
        assert_eq!(first.code, ErrorCode::InvalidArgument);

        let spare = read_release("v2-spare-signed");
        let as_active = cat(&[b"ed25519 d75a980182b10ab7 ", &spare.sig[25..]]);
        let second = verify_manifest(&spare.json, &as_active, test_trust(), &install_for("win64"))
            .expect_err("verified");
        assert_eq!(second.code, ErrorCode::InvalidArgument);
    }

    // ---- the accept paths and the keys the trust lists ---------------------------------------

    #[test]
    fn the_active_and_the_spare_keys_both_pass_the_whole_gate() {
        let trust = test_trust().clone();
        let active = read_release("v2");
        assert!(verify_manifest(&active.json, &active.sig, &trust, &install_for("win64")).is_ok());
        let spare = read_release("v2-spare-signed");
        assert!(verify_manifest(&spare.json, &spare.sig, &trust, &install_for("win64")).is_ok());

        // A CRLF line end is part of the line's grammar, not of the signed bytes.
        let crlf = cat(&[&active.sig[..active.sig.len() - 1], b"\r\n"]);
        assert!(verify_manifest(&active.json, &crlf, &trust, &install_for("win64")).is_ok());

        // Dropping the active key from the trust (a rotation) leaves the spare's release verifying
        // and the active's not trusted.
        let spare_only = Trust {
            keys: vec![trust.keys[1].clone()],
            placeholder: false,
        };
        assert!(
            verify_manifest(&spare.json, &spare.sig, &spare_only, &install_for("win64")).is_ok()
        );
        let refused = verify_manifest(
            &active.json,
            &active.sig,
            &spare_only,
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(
            refused.code,
            ErrorCode::Unsupported,
            "{}",
            refused.describe()
        );

        // The control: a release the signature layer refuses does not pass.
        let flipped = read_release("bad/flipped-manifest-bit");
        assert!(
            verify_manifest(&flipped.json, &flipped.sig, &trust, &install_for("win64")).is_err()
        );
    }

    #[test]
    fn every_listed_key_with_the_named_id_is_tried_not_just_the_first() {
        // Two entries with one id (real keys collide with probability 2^-64; a Trust is a
        // parameter and need not come from the generator). The one that is no key must not hide
        // the signer's, whichever is listed first.
        let real = test_trust().keys[0].clone();
        let unusable = unusable_key_with_the_id_of_test_one(1);
        assert_eq!(unusable.id, real.id);
        assert_ne!(unusable.key, real.key);

        let release = read_release("v2");
        for order in [
            vec![unusable.clone(), real.clone()],
            vec![real.clone(), unusable.clone()],
        ] {
            let trust = Trust {
                keys: order,
                placeholder: false,
            };
            let verified =
                verify_manifest(&release.json, &release.sig, &trust, &install_for("win64"));
            assert!(verified.is_ok(), "{}", describe(&verified));
        }
    }

    #[test]
    fn a_listed_key_that_is_no_public_key_is_a_could_not_check_and_never_a_forgery() {
        // A mistyped pin: the key the line names is not a point of the prime-order subgroup.
        // Every release of every install would be refused for it, and the code that says so must
        // not be the one of a forged signature (InvalidArgument), which callers hold to be
        // unreachable for a genuine release: it is Internal, "could not check", with the reason.
        let real = test_trust().keys[0].clone();
        let unusable = unusable_key_with_the_id_of_test_one(1);
        assert_eq!(unusable.id, real.id);
        assert!(check_public_key(&unusable.key).is_err()); // the premise, from the signature layer
        let release = read_release("v2");

        let only_unusable = Trust {
            keys: vec![unusable.clone()],
            placeholder: false,
        };
        let alone = verify_manifest(
            &release.json,
            &release.sig,
            &only_unusable,
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(alone.code, ErrorCode::Internal, "{}", alone.describe());
        assert!(
            alone.message.contains("cannot be used"),
            "{}",
            alone.describe()
        );
        assert!(
            alone.message.contains("public key is not accepted"),
            "{}",
            alone.describe()
        );
        assert!(alone.context.contains(&real.id), "{}", alone.describe());

        // The control: the genuine key with a signature that is wrong IS the forgery's code.
        let flipped = read_release("bad/flipped-manifest-bit");
        let only_real = Trust {
            keys: vec![real.clone()],
            placeholder: false,
        };
        let forged = verify_manifest(
            &flipped.json,
            &flipped.sig,
            &only_real,
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(
            forged.code,
            ErrorCode::InvalidArgument,
            "{}",
            forged.describe()
        );

        // Both listed, the signature wrong for the real one: a failure to check outranks a
        // refusal, whichever order they are listed in, so that the one key that could not be
        // looked at is not hidden behind the other's "does not match".
        for order in [
            vec![real.clone(), unusable.clone()],
            vec![unusable.clone(), real.clone()],
        ] {
            let both = Trust {
                keys: order,
                placeholder: false,
            };
            let error = verify_manifest(&flipped.json, &flipped.sig, &both, &install_for("win64"))
                .expect_err("verified");
            assert_eq!(error.code, ErrorCode::Internal, "{}", error.describe());
            assert!(
                error.message.contains("cannot be used"),
                "{}",
                error.describe()
            );
        }

        // Two of them, neither usable: still a failure to check, not an unknown signer.
        let another = unusable_key_with_the_id_of_test_one(2);
        assert_eq!(another.id, real.id);
        assert_ne!(another.key, unusable.key);
        assert!(check_public_key(&another.key).is_err());
        let neither = Trust {
            keys: vec![unusable, another],
            placeholder: false,
        };
        let refused = verify_manifest(&release.json, &release.sig, &neither, &install_for("win64"))
            .expect_err("verified");
        assert_eq!(refused.code, ErrorCode::Internal, "{}", refused.describe());
    }

    // ---- the whole gate: the result carries the manifest -------------------------------------

    #[test]
    fn a_real_release_signed_by_the_active_key_verifies_and_carries_the_bytes_that_were_signed() {
        let release = read_release("v2");
        let manifest = verify_manifest(
            &release.json,
            &release.sig,
            test_trust(),
            &install_for("win64"),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        // The producer's own v2 (tools/update_make_fixtures.py): Katana 0.10.0 for win64, 10
        // files.
        assert_eq!(manifest.product, "katana");
        assert_eq!(manifest.target, "win64");
        assert_eq!(manifest.channel, "stable");
        assert_eq!(manifest.tag, "v0.10.0");
        assert_eq!(manifest.files.len(), 10);
        // The record the helper keeps for later removals is the bytes as downloaded, so that it
        // can be verified again from the file.
        assert_eq!(manifest.signed_json, release.json);
        assert_eq!(manifest.signature_line, release.sig);
    }

    #[test]
    fn the_spare_key_verifies_too() {
        let release = read_release("v2-spare-signed");
        let manifest = verify_manifest(
            &release.json,
            &release.sig,
            test_trust(),
            &install_for("win64"),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(manifest.signed_json, release.json);
        assert_eq!(manifest.signature_line, release.sig);
    }

    #[test]
    fn a_spare_only_trust_verifies_the_spare_and_refuses_the_active() {
        let spare_only = Trust {
            keys: vec![test_trust().keys[1].clone()],
            placeholder: false,
        };
        let spare = read_release("v2-spare-signed");
        assert!(
            verify_manifest(&spare.json, &spare.sig, &spare_only, &install_for("win64")).is_ok()
        );
        let active = read_release("v2");
        let refused = verify_manifest(
            &active.json,
            &active.sig,
            &spare_only,
            &install_for("win64"),
        )
        .expect_err("verified");
        assert_eq!(refused.code, ErrorCode::Unsupported); // the active key was dropped
    }

    #[test]
    fn a_line_end_of_crlf_is_accepted_and_kept_as_received() {
        let release = read_release("v2");
        let crlf = cat(&[&release.sig[..release.sig.len() - 1], b"\r\n"]);
        let manifest = verify_manifest(&release.json, &crlf, test_trust(), &install_for("win64"))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(manifest.signature_line, crlf);
    }

    #[test]
    fn a_notify_only_manifest_verifies_for_its_own_target() {
        let release = read_release_of("v2-macos-notify", "macos-arm64");
        let manifest = verify_manifest(
            &release.json,
            &release.sig,
            test_trust(),
            &install_for("macos-arm64"),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(manifest.notify_only);
        assert!(manifest.files.is_empty());
    }

    #[test]
    fn a_validly_signed_manifest_of_another_target_is_not_an_update_for_this_install() {
        let release = read_release("v2");
        for target in [
            "win-arm64",
            "linux-x86_64",
            "linux-aarch64",
            "macos-arm64",
            "",
            "WIN64",
            "win64 ",
        ] {
            let error = verify_manifest(
                &release.json,
                &release.sig,
                test_trust(),
                &install_for(target),
            )
            .expect_err(target);
            assert_eq!(
                error.code,
                ErrorCode::InvalidArgument,
                "{}",
                error.describe()
            );
            assert!(
                error.message.contains("another target"),
                "{}",
                error.describe()
            );
        }
    }

    #[test]
    fn a_validly_signed_manifest_of_another_product_is_not_an_update_for_this_install() {
        let release = read_release("v2");
        let install = Install {
            product: String::from("other"),
            ..install_for("win64")
        };
        let error = verify_manifest(&release.json, &release.sig, test_trust(), &install)
            .expect_err("verified");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("another product"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn a_validly_signed_manifest_of_another_channel_is_refused() {
        // The control first: the same install takes the stable notify-only manifest of the
        // fixtures, so the refusal below is about the channel and not the shape of this one.
        let stable = read_release_of("v2-macos-notify", "macos-arm64");
        assert!(
            verify_manifest(
                &stable.json,
                &stable.sig,
                test_trust(),
                &install_for("macos-arm64")
            )
            .is_ok()
        );

        let error = verify_manifest(
            BETA_MANIFEST,
            BETA_SIGNATURE_LINE,
            test_trust(),
            &install_for("macos-arm64"),
        )
        .expect_err("verified");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("another channel"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn the_beta_manifest_is_genuinely_signed_so_only_its_channel_can_refuse_it() {
        // Not parser-dependent: the literal above really is signed by TEST 1 (so the channel case
        // refuses a good signature, not a bad one), and altering one byte of it is refused.
        let line = parse_signature_line(BETA_SIGNATURE_LINE).expect("the line parses");
        let active = test_trust().keys[0].key;
        assert!(verify_ed25519(&active, BETA_MANIFEST, &line.signature).is_ok());
        let mut altered = BETA_MANIFEST.as_bytes().to_vec();
        let at = BETA_MANIFEST.find("beta").expect("the channel");
        altered[at] = b'B';
        assert!(verify_ed25519(&active, &altered, &line.signature).is_err());
    }
}
