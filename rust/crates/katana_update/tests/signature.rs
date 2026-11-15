//! Ed25519 verification and the signature line (the twin of tests/update/test_signature.cpp).
//!
//! Every expectation here comes from OUTSIDE the code under test (CLAUDE.md, section 4):
//!
//! * RFC 8032, section 7.1, TEST 1 to 3: the public key, message and signature the RFC prints.
//!   The same values were re-derived from the RFC's secret keys by a pure-Python implementation
//!   written from RFC 8032 sections 5.1 and 6 (not by this library), and TEST 2 is also accepted
//!   by `openssl pkeyutl -verify -rawin`.
//! * The base64 of those signatures, the group order L and the eight points of small order with
//!   their non-canonical aliases, a mixed-order point and an off-curve y: the same Python, which
//!   decodes points as RFC 8032, 5.1.3 does and finds each small-order point as L * Q for a point
//!   Q of the curve. S + L of TEST 2 is a signature the verification equation holds for (shown in
//!   that Python) and that RFC 8032, 5.1.7 forbids.
//! * tests/update/fixtures/release/**: manifests signed by the `openssl` command line
//!   (tools/update_make_fixtures.py), the independent signer of the whole system.
//! * The all-zero public key with an all-zero signature, which `openssl pkeyutl -verify` (3.6.5)
//!   ACCEPTS for the one-byte messages 9, 14, 17, 19, 26, 27, 39, 47 ... (measured over all 256).
//!
//! What each case would catch is named in its comment: removing the explicit S check, the key
//! check or the strict base64 makes the case after it fail.
//!
//! To regenerate the Python-derived literals: tests/update/tool_tests/rfc8032_oracle.py (run it
//! with `python -I`; about 100 lines from RFC 8032 sections 5.1 and 6 and nothing else, and it
//! checks itself against TEST 1 to 3 first). An order-8 point is L * Q for a curve point Q found
//! by decoding small y values, and its other-sign and other-y forms are the same points with x or
//! y negated; a non-canonical alias is y + p written in 255 bits (p = 2^255 - 19); the mixed-order
//! key is TEST 1's A plus one order-8 point, encoded; "S + L" is TEST 2's S plus L, little-endian,
//! in the signature's second half (it still satisfies 8*S*B = 8*R + 8*h*A, which is why only the
//! explicit S < L test refuses it).

#![allow(
    unsafe_code,
    reason = "Edition 2024 marks changing the environment unsafe, and the hardening cases set and restore the variables they test."
)]

use katana_core::path_text::{path_from_utf8, read_file_bytes};
use katana_core::{ErrorCode, Status};
use katana_update::feed::MAX_SIGNATURE_BYTES;
use katana_update::signature::{
    PublicKey, SignatureBytes, check_public_key, harden_open_ssl_environment, key_id_of,
    parse_public_key_hex, parse_signature_line, verify_ed25519,
};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tests/update/fixtures"
);

// ---- bytes the tests build for themselves, without the library under test ---------------------

fn nibble(c: u8) -> u8 {
    if c <= b'9' { c - b'0' } else { c - b'a' + 10 } // the literals below are lower-case hexadecimal
}

fn bytes_from_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| nibble(pair[0]) * 16 + nibble(pair[1]))
        .collect()
}

fn array_from_hex<const N: usize>(hex: &str) -> [u8; N] {
    let bytes = bytes_from_hex(hex);
    assert_eq!(bytes.len(), N, "{hex}");
    let mut out = [0u8; N];
    let count = bytes.len().min(N);
    out[..count].copy_from_slice(&bytes[..count]);
    out
}

fn key_from_hex(hex: &str) -> PublicKey {
    array_from_hex::<32>(hex)
}

fn signature_from_hex(hex: &str) -> SignatureBytes {
    array_from_hex::<64>(hex)
}

fn encode_base64(bytes: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = Vec::new();
    for group in bytes.chunks(3) {
        let left = group.len();
        let b0 = u32::from(group[0]);
        let b1 = if left > 1 { u32::from(group[1]) } else { 0 };
        let b2 = if left > 2 { u32::from(group[2]) } else { 0 };
        let bits = (b0 << 16) | (b1 << 8) | b2;
        text.push(ALPHABET[((bits >> 18) & 63) as usize]);
        text.push(ALPHABET[((bits >> 12) & 63) as usize]);
        text.push(if left > 1 {
            ALPHABET[((bits >> 6) & 63) as usize]
        } else {
            b'='
        });
        text.push(if left > 2 {
            ALPHABET[(bits & 63) as usize]
        } else {
            b'='
        });
    }
    text
}

// Through the project's reader, so that a checkout under a folder named in any script opens.
fn read_fixture(relative: &str) -> Vec<u8> {
    read_file_bytes(&path_from_utf8(format!("{FIXTURES}/{relative}")))
        .unwrap_or_else(|error| panic!("{relative}: {}", error.describe()))
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

// ---- RFC 8032, section 7.1 ---------------------------------------------------------------------

struct RfcVector {
    name: &'static str,
    public_key: &'static str,
    message: &'static str, // hex
    signature: &'static str,
    signature_base64: &'static str, // from the Python, base64.b64encode
}

static RFC_VECTORS: [RfcVector; 3] = [
    RfcVector {
        name: "TEST 1",
        public_key: "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        message: "",
        signature: "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        signature_base64: "5VZDAMNgrHKQhuLMgG6CioSHfx645dl02HPgZSJJAVVfuIIVkKM7rMYeOXAc+bRr0lv18FlbviRlUUFDjnoQCw==",
    },
    RfcVector {
        name: "TEST 2",
        public_key: "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        message: "72",
        signature: "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
        signature_base64: "kqAJqfDUyrhyDoILX2QlQKKye1QWUD+Ps3YiI+vbadoIWsHkPhWZbkWPNhPQ8R2MOHsurrQwKu6wDSkWErsMAA==",
    },
    RfcVector {
        name: "TEST 3",
        public_key: "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        message: "af82",
        signature: "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
        signature_base64: "YpHWV97sJAJIJ+acOr4BowzlSKKEdDpEXjaA19taw6wY/5tTjRbykK5n92CYTcZZSnwV6XFu0o3AJ77O6h7ECg==",
    },
];

const TEST1_PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const TEST2_PUBLIC: &str = "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";
const TEST3_PUBLIC: &str = "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025";

// A refusal of the signature or the key: InvalidArgument, naming `why` (the module: "the message
// says which").
fn expect_refused(status: &Status, why: &str) {
    let Err(error) = status else {
        panic!("accepted, but it must be refused: {why}");
    };
    assert_eq!(
        error.code,
        ErrorCode::InvalidArgument,
        "{}",
        error.describe()
    );
    assert!(error.message.contains(why), "{}", error.describe());
}

const NOT_BELOW_ORDER: &str = "S is not below the group order";
const KEY_NOT_ACCEPTED: &str = "public key is not accepted";
const DOES_NOT_MATCH: &str = "does not match";

struct Generated {
    public_key: &'static str,
    message: &'static str,
    signature: &'static str,
}

// The 24 keys of twenty_four_keys_generated_and_signed_by_openssl_are_accepted_and_verify, copied
// from the C++ case.
static GENERATED: [Generated; 24] = [
    Generated {
        public_key: "8f4b3d1e44e9e2ad69f61922143dd1d601c2b6a482576bb8a3b2aab6c23c36d0",
        message: "katana pinned-key acceptance 00",
        signature: "71f67902866966899e9bd2e5146d0c71cbfb8fd7f47062da3f11640becd460a29143e60bbc4aa828cdf6cff6f90792fb9fb1e4edf83ecf91b1df18a6ac7c2901",
    },
    Generated {
        public_key: "90f81fa3df0c6ee3240d22bc1d98bf02e57de454037185969b6770b731cf8d8d",
        message: "katana pinned-key acceptance 01",
        signature: "bc10c225b36b54f7da3c31d8ed2b9abcdc9d9adddb54848dada9319c26df4cc1e80b9adb0d5027e0ec403fc5efa85ac10336a5c1567c64c211ca58fbcc133305",
    },
    Generated {
        public_key: "327505024baf2b72195b33ca5279b3a8ee4a14230d3e8173d9f548b732908102",
        message: "katana pinned-key acceptance 02",
        signature: "2c004fa0f34a4a87c1326762e546b1d42566b1ca5d0a4f127c76ecfeadbaf6a7d1588e0278bc6d829f2abf782eb11493609a9de0f874ca0789bac2abab026508",
    },
    Generated {
        public_key: "4a76c73dc2e9723de4513e4e4941bbfcdcfa44a8b217a101a98af8a244b40b04",
        message: "katana pinned-key acceptance 03",
        signature: "67eb5d05f383c72d01c36d1654085a8933753686efaa87a6b85d704e8ac047cc9bd7c517009ceb9a22e3f0cb35f255b9427e6b12cc858a49e37a65b1d1b00e0e",
    },
    Generated {
        public_key: "6848403760627b5ee74dd4a814be716041e39b88f9d8f40bf877fa6ee42bd531",
        message: "katana pinned-key acceptance 04",
        signature: "adb9715e4968a2bb0d7037c525d788b9c1e650bc2e939d3162eef1ca405ee41e666caf9975492f6dc57d0e5f523b760ce89eb2316b36e9b41f55d59cc9afe504",
    },
    Generated {
        public_key: "1dde638a3b8de773720464e9fe89422143f547d92a0385233e8d7ad01a8bf794",
        message: "katana pinned-key acceptance 05",
        signature: "8e1b5d8829eab19899ed548f01d5d858f629bcdd689615ab9a006aeed462b1784e5165d45006a2a9e4a2d0503a575a16e7a79faaae964176ed2166c592ee110f",
    },
    Generated {
        public_key: "d167bf3b67fd2b35a966e9d92faca7e99b6a93374aa9e78714729b959460ebf1",
        message: "katana pinned-key acceptance 06",
        signature: "470349a0c91d2991606a3c1b46d4607214beee50b4b00a9aacedea31b190a6e0c429b7052f3c4eb42834521e949841f043cebce5e54b0e5cec3fec0909db8b05",
    },
    Generated {
        public_key: "db80f56b5f28b86b1ed8f603905439084f6091176c54581b78f061f74cf62ff1",
        message: "katana pinned-key acceptance 07",
        signature: "da7ebe7845a191807386a0913c0538d55539b332c97076c137c71fef3045815a45682c657b52c00dd5e8b9afef536c2b7ab8e2dc10f40218103b382e8def3f0f",
    },
    Generated {
        public_key: "f9c5a68f13722d22e401d1a35468bdf46ed29c3bd2d3009e4977ef210d8b3845",
        message: "katana pinned-key acceptance 08",
        signature: "84256eb77ed1270ac38f853f67763b87719486ade4ad30e58cfc239c049e1f4d84ed423df6f63e0a0ed4ac6f9224bd5006a75ebf2c6e69ed2ca2b9d4dfb41c02",
    },
    Generated {
        public_key: "c873020c2511c531458ed9f70b9cd951ec5eb6e132b8398fedfd89d05a5f5696",
        message: "katana pinned-key acceptance 09",
        signature: "53113e3617c3e655493591c2a4fbe125e219bb571216e2aef15f5f0a39c224c02ac766a9d2766e558b1fffc6f3e2ebacf310e0723fa6e0f308494412ee796705",
    },
    Generated {
        public_key: "bc5ab8b95569f114b4518d24803deafdcab4eb4192cae0f7248dc59c4371284d",
        message: "katana pinned-key acceptance 10",
        signature: "c4fb49b3ee9d7acb18e5a0294e0e772dfde95e84d923ddb72c4aad501a7f94476270ff5549ad61fc98895932d24fd6c554fd6e0213c1bc0d2248236b2b311407",
    },
    Generated {
        public_key: "a6666db5dde6e1cfb23e0ac88b7846af6b6fe2b756afaa4eed4489107e518eed",
        message: "katana pinned-key acceptance 11",
        signature: "e1bfc8fcd5bcd9a02020ac7677bfa2b683e85ca8b83503c21edecbe70b99cbd8eb52c8d882cdfacc6a486d9428b213d3e148ccc0c116774d47351ea128233c00",
    },
    Generated {
        public_key: "180cb369001c685ddeb100d9d2a91e06e5b84cf82e62480c943169becd5f06cd",
        message: "katana pinned-key acceptance 12",
        signature: "00e42c762ac8521700d74702fea4d0e0dd4d15eae12a899f6e0b21d91afe7cd3cdc8607c0394b4a1aea5c7aeff93bd1fc0c82c9456970842c0eea9b2f4934704",
    },
    Generated {
        public_key: "9f098d312c327f70a7fab43cb06566521770634490cfd2ddcbbd2579f7b8f25f",
        message: "katana pinned-key acceptance 13",
        signature: "56e73ce09321869d232752df1665cd796f81db99aa8802d8d50d6d49db2209d4c60d987ad88b89b12e990b4ac370118e871f5e6bf39a27262b5dd6c338097800",
    },
    Generated {
        public_key: "515f1ab9696e39c0382ecc45fc2115b2a2120440b6bda4e82126dd6021793307",
        message: "katana pinned-key acceptance 14",
        signature: "89fd9650cfa9f56b5c601b37bdf6758b988c319ad85afc89a550b348c17045d0e2d29978046ff0199908d11fe72f9c8acc2c8493c1aaa45a23c6c8369a4c830e",
    },
    Generated {
        public_key: "013b7e24fa650062ad46a338c498d2e6d68e4af4349df1508277bdbc3faa7875",
        message: "katana pinned-key acceptance 15",
        signature: "6ade83b3e43ef779fcdbedf3a920f9b5a43c647e7f49f26dc05ecb89322d85f93d283e8eaffc9b8159abc87b51ff6d6eccaf31b2d295af845fc9a6d6dc4fe60a",
    },
    Generated {
        public_key: "e67f6d8f6963d780d71a83e62e931f3afaa3a7e806f15d3fad8a5a0aa418eec3",
        message: "katana pinned-key acceptance 16",
        signature: "f6a3ab1b06009338b05486a547a9f1a6f30d17a183fdea8de8c3cb53b60a1f3386c1f433082f882f92e24e01ed971f7669d8c970626d34a33535f48c046f810e",
    },
    Generated {
        public_key: "d0bd9a1528b97f1afae87e081d898db8a26b6f9615176a004d3734d77da4b862",
        message: "katana pinned-key acceptance 17",
        signature: "91278ba50a82e7af34310c22f09d821a9676293ed4dc7277db862b8127a7dd253a75195b138b45cf5c8eb0065472744619443863bda5a2a5814e80dacb5d7001",
    },
    Generated {
        public_key: "cfbff8e9016695eb3a486c05a31267df8f149a84d735fc9ba68a130f7791fff9",
        message: "katana pinned-key acceptance 18",
        signature: "d1c86294658844bd0a7a2dd1bdbab07f69ed4bee37d370548a1f71eaff9ab654692aa6d4eee57919dfb37fa6df98f815f7bc98f3492089116319c1c3241f1d00",
    },
    Generated {
        public_key: "85f717e404472727b03a826d6e877ad49a8240a351c6abf6c6d961a997252c33",
        message: "katana pinned-key acceptance 19",
        signature: "d605b3ca4c2e08164b74ddd62e3d1cfd10fef287f6d66425f0ea2dde3b6baaee17459e1a8419abd58e754b5adc947c21d68ea03134ea6b997df186a65ce9c00c",
    },
    Generated {
        public_key: "08615244369c8e9ea481bc746941b2e22e7ff9f530ed55ac6299571000409559",
        message: "katana pinned-key acceptance 20",
        signature: "bf907c35577d5d94197933709d3bde53dc30cb8323d251b6af74d46a90077c3791b1c8565124abc0745b9b0c7124f2884394680fdc8337b09681def93bac710b",
    },
    Generated {
        public_key: "e4fae266737fde6b9300329bb2c3b3727a60e4298414d829aef96dbeba088ea3",
        message: "katana pinned-key acceptance 21",
        signature: "027f4b16abbc02945cc6edc72d2595e866edcbc53221aad48a850eb7ddac50dcea442599b02f9d8ca0981c04522f39f3ca1dd8694c2d509f7122626589d5eb0b",
    },
    Generated {
        public_key: "44f87831819905fd429073ec69cccb029084aeda11c18fd0e596906bd5b62add",
        message: "katana pinned-key acceptance 22",
        signature: "ef242155b147f2d85027358e7a083da6834e51694d47a3da3bfadf604fd673f49572a5d8754a266d7549594cdd77d41992f0f6962367b4c985a6bc92f9d5440f",
    },
    Generated {
        public_key: "81eb584872bc2e1bc9a490ceebd233204aaa86924402ee14b5fa3b6bbba9e346",
        message: "katana pinned-key acceptance 23",
        signature: "f26a8919ae496c2ed956db0509901ba7e069727950abd57b94697aa9e7d8dc5969878fa8e4b2e7025c5881e5a12e0355872a335900ef81523b96463c727f5202",
    },
];

// ---- the environment ---------------------------------------------------------------------------

const OPEN_SSL_VARIABLES: [&str; 3] = ["OPENSSL_CONF", "OPENSSL_MODULES", "OPENSSL_ENGINES"];
// The one that marks the program started again by the configuration case.
const HOSTILE_CHILD_MARKER: &str = "KATANA_T5_HOSTILE_CHILD";
// The same for the program started by the inheritance case.
const INHERITANCE_CHILD_MARKER: &str = "KATANA_T5_INHERITANCE_CHILD";
// Every variable a case of this file sets, for the guard to put back whatever way the case ends.
const VARIABLES_THE_TESTS_SET: [&str; 7] = [
    "OPENSSL_CONF",
    "OPENSSL_MODULES",
    "OPENSSL_ENGINES",
    "OPENSSL_CONF_INCLUDE",
    "KATANA_T5_UNRELATED",
    HOSTILE_CHILD_MARKER,
    INHERITANCE_CHILD_MARKER,
];

fn set_variable(name: &str, value: &str) {
    // SAFETY: only the cases that hold the environment's lock (RestoreOpenSslVariables) change the
    // environment, and no other case of this program reads it.
    unsafe { std::env::set_var(name, value) };
}

fn remove_variable(name: &str) {
    // SAFETY: as set_variable.
    unsafe { std::env::remove_var(name) };
}

fn variable(name: &str) -> Option<String> {
    std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
}

// Puts the variables back as they were, and holds the environment for one case at a time: the
// cases of one program run on threads of one process here, where the C++ ran each in its own.
struct RestoreOpenSslVariables {
    saved: Vec<Option<std::ffi::OsString>>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

static ENVIRONMENT: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl RestoreOpenSslVariables {
    fn new() -> Self {
        let lock = ENVIRONMENT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self {
            saved: VARIABLES_THE_TESTS_SET
                .iter()
                .map(std::env::var_os)
                .collect(),
            _lock: lock,
        }
    }
}

impl Drop for RestoreOpenSslVariables {
    fn drop(&mut self) {
        for (name, saved) in VARIABLES_THE_TESTS_SET.iter().zip(&self.saved) {
            match saved {
                // SAFETY: the lock is still held (it is dropped after this body).
                Some(value) => unsafe { std::env::set_var(name, value) },
                None => remove_variable(name),
            }
        }
    }
}

fn harden() -> Status {
    // SAFETY: called only by cases that hold the environment's lock.
    unsafe { harden_open_ssl_environment() }
}

// A folder of the case's own (its name and the process's), removed when the case ends.
struct ScratchFolder {
    path: std::path::PathBuf,
}

impl ScratchFolder {
    fn new(case: &str) -> Self {
        let path = std::env::temp_dir().join(format!("katana_t5_{case}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch folder can be made");
        Self { path }
    }
}

impl Drop for ScratchFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// Runs this test program on the one case `filter` names, with `folder` as its working directory;
// its exit code (None when it could not be started or was ended by a signal) and both of its
// streams. The child inherits this process's environment as it is now, which is what the cases
// rely on.
fn run_program_on_one_case(filter: &str, folder: &std::path::Path) -> (Option<i32>, String) {
    let program = std::env::current_exe().expect("this program's own path");
    match std::process::Command::new(&program)
        .args([filter, "--exact", "--test-threads=1", "--nocapture"])
        .current_dir(folder)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            (output.status.code(), text)
        }
        Err(error) => (None, format!("(not started: {error})")),
    }
}

// What the test harness prints when the one case it ran passed.
const ONE_PASSED: &str = "1 passed; 0 failed; 0 ignored";

mod signature {
    use super::*;

    // ---- key ids and public keys -------------------------------------------------------------

    #[test]
    fn the_key_id_is_the_first_sixteen_lower_case_hex_digits_of_the_key() {
        // The digits are the RFC's own: the public key printed in section 7.1, cut after 8 bytes.
        assert_eq!(key_id_of(&key_from_hex(TEST1_PUBLIC)), "d75a980182b10ab7");
        assert_eq!(key_id_of(&key_from_hex(TEST2_PUBLIC)), "3d4017c3e843895a");
        assert_eq!(key_id_of(&key_from_hex(TEST3_PUBLIC)), "fc51cd8e6218a1a3");

        // A leading zero byte keeps both of its digits, and the digits are lower case.
        let mut zeros: PublicKey = [0; 32];
        zeros[1] = 0x0a;
        zeros[2] = 0xff;
        zeros[7] = 0xbc;
        assert_eq!(key_id_of(&zeros), "000aff00000000bc");
        assert_eq!(
            key_id_of(&key_from_hex(&"f".repeat(64))),
            "ffffffffffffffff"
        );
    }

    #[test]
    fn a_public_key_parses_from_exactly_sixty_four_lower_case_hex_digits() {
        for vector in &RFC_VECTORS {
            let parsed = parse_public_key_hex(vector.public_key)
                .unwrap_or_else(|error| panic!("{}: {}", vector.name, error.describe()));
            assert_eq!(parsed, key_from_hex(vector.public_key), "{}", vector.name);
        }
        let good = TEST1_PUBLIC;
        let refused = [
            String::new(),
            good[..63].to_owned(),
            format!("{good}0"),
            format!("{}g", "a".repeat(good.len() - 1)),
            String::from("D75A980182B10AB7D54BFED3C964073A0EE172F3DAA62325AF021A68F707511A"), // upper case
            format!("{} ", &good[..63]),
            format!(" {}", &good[1..]),
            format!("0x{}", &good[2..]),
            format!("{}\0{}", &good[..10], &good[11..]),
        ];
        for text in &refused {
            let parsed = parse_public_key_hex(text);
            let Err(error) = parsed else {
                panic!("accepted: '{text}'");
            };
            assert_eq!(error.code, ErrorCode::ParseFailure);
        }
    }

    // ---- the signature line ------------------------------------------------------------------

    #[test]
    fn a_signature_line_of_the_rfc_vectors_parses_to_the_rfc_bytes() {
        // The base64 is the Python's, the bytes are the RFC's: a decoder that is off by one bit
        // anywhere in 64 bytes cannot agree with both.
        for vector in &RFC_VECTORS {
            let line = format!(
                "ed25519 {} {}\n",
                key_id_of(&key_from_hex(vector.public_key)),
                vector.signature_base64
            );
            let parsed = parse_signature_line(&line)
                .unwrap_or_else(|error| panic!("{}: {}", vector.name, error.describe()));
            assert_eq!(parsed.algorithm, "ed25519");
            assert_eq!(parsed.key_id, &vector.public_key[..16]);
            assert_eq!(
                parsed.signature,
                signature_from_hex(vector.signature),
                "{}",
                vector.name
            );
        }
    }

    #[test]
    fn the_signature_line_of_an_openssl_signed_fixture_parses_to_the_bytes_python_decodes() {
        // v2's line, decoded by Python's base64 module.
        let parsed = parse_signature_line(read_fixture("release/v2/Katana-update-win64.json.sig"))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(parsed.key_id, "d75a980182b10ab7");
        assert_eq!(
            parsed.signature,
            signature_from_hex(
                "b26c00f6aed5840972037d63273b2c3ab93fc427d89ff40d3e4cc8418ec35bb173c232a2ecfd835f38eaefb951f17d0807a6b1bed24420ac72d7accdf8ddd709"
            )
        );
    }

    #[test]
    fn allows_one_line_end_of_lf_or_crlf_or_none() {
        let line = read_fixture("release/v2/Katana-update-win64.json.sig");
        assert_eq!(line.last(), Some(&b'\n')); // the producer writes the LF
        let bare = &line[..line.len() - 1];
        let reference =
            parse_signature_line(bare).unwrap_or_else(|error| panic!("{}", error.describe()));
        for text in [line.clone(), bare.to_vec(), cat(&[bare, b"\r\n"])] {
            let parsed =
                parse_signature_line(&text).unwrap_or_else(|error| panic!("{}", error.describe()));
            assert_eq!(parsed.signature, reference.signature);
            assert_eq!(parsed.key_id, reference.key_id);
        }
    }

    #[test]
    fn a_malformed_signature_line_is_refused() {
        let fixture = read_fixture("release/v2/Katana-update-win64.json.sig");
        assert!(fixture.len() >= 114);
        let algorithm = &fixture[0..7];
        let key_id = &fixture[8..24];
        let encoded = &fixture[25..113];
        assert_eq!(algorithm, b"ed25519");
        assert_eq!(fixture[7], b' ');
        assert_eq!(fixture[24], b' ');
        assert_eq!(&encoded[86..], b"==");
        let assemble = |a: &[u8], k: &[u8], e: &[u8]| cat(&[a, b" ", k, b" ", e]);
        let good = assemble(algorithm, key_id, encoded);
        assert!(
            parse_signature_line(&good).is_ok(),
            "the control line must parse"
        );

        let sixty_four = bytes_from_hex(
            "b26c00f6aed5840972037d63273b2c3ab93fc427d89ff40d3e4cc8418ec35bb173c232a2ecfd835f38eaefb951f17d0807a6b1bed24420ac72d7accdf8ddd709",
        );
        assert_eq!(sixty_four.len(), 64);
        assert_eq!(
            encode_base64(&sixty_four),
            encoded,
            "the test's own encoder agrees with the fixture"
        );

        // The last data character carries two bits and four that must be zero ('Q' is 010000);
        // 'R' is 010001, which a lenient decoder reads as the same 64 bytes.
        let mut non_canonical = encoded.to_vec();
        assert_eq!(non_canonical[85], b'Q');
        non_canonical[85] = b'R';
        // URL-safe spellings of '+' and '/' (this signature has both).
        assert!(encoded.contains(&b'+'));
        assert!(encoded.contains(&b'/'));
        let url_safe: Vec<u8> = encoded
            .iter()
            .map(|&c| match c {
                b'+' => b'-',
                b'/' => b'_',
                other => other,
            })
            .collect();
        let mut pad_in_the_middle = encoded.to_vec();
        pad_in_the_middle[10] = b'=';

        let with = |extra: &[u8]| cat(&[&sixty_four, extra]);
        let refused: Vec<(&str, Vec<u8>)> = vec![
            // the algorithm
            ("another algorithm", assemble(b"ed448", key_id, encoded)),
            (
                "upper-case algorithm",
                assemble(b"ED25519", key_id, encoded),
            ),
            (
                "a longer algorithm word",
                assemble(b"ed25519x", key_id, encoded),
            ),
            ("no algorithm", assemble(b"", key_id, encoded)),
            // the key id
            (
                "a key id one digit short",
                assemble(algorithm, &key_id[..15], encoded),
            ),
            (
                "a key id one digit long",
                assemble(algorithm, &cat(&[key_id, b"a"]), encoded),
            ),
            (
                "an upper-case key id",
                assemble(algorithm, b"D75A980182B10AB7", encoded),
            ),
            (
                "a key id that is not hexadecimal",
                assemble(algorithm, b"d75a980182b10abg", encoded),
            ),
            ("no key id", assemble(algorithm, b"", encoded)),
            // the signature text
            (
                "63 bytes (84 characters)",
                assemble(algorithm, key_id, &encode_base64(&sixty_four[..63])),
            ),
            // 88 characters, so the length test alone passes them: 65 bytes end in one pad and 66
            // in none
            (
                "65 bytes (88 characters, one pad)",
                assemble(algorithm, key_id, &encode_base64(&with(b"x"))),
            ),
            (
                "66 bytes (88 characters, no pad)",
                assemble(algorithm, key_id, &encode_base64(&with(b"xy"))),
            ),
            (
                "67 bytes (92 characters)",
                assemble(algorithm, key_id, &encode_base64(&with(b"xyz"))),
            ),
            (
                "32 bytes",
                assemble(algorithm, key_id, &encode_base64(&sixty_four[..32])),
            ),
            ("no signature", assemble(algorithm, key_id, b"")),
            (
                "the padding cut off",
                assemble(algorithm, key_id, &encoded[..86]),
            ),
            (
                "one pad where two belong",
                assemble(algorithm, key_id, &encoded[..87]),
            ),
            (
                "three pads",
                assemble(algorithm, key_id, &cat(&[&encoded[..85], b"==="])),
            ),
            (
                "a pad in the middle",
                assemble(algorithm, key_id, &pad_in_the_middle),
            ),
            (
                "non-canonical last character",
                assemble(algorithm, key_id, &non_canonical),
            ),
            (
                "the URL-safe alphabet",
                assemble(algorithm, key_id, &url_safe),
            ),
            (
                "a character outside the alphabet",
                assemble(
                    algorithm,
                    key_id,
                    &cat(&[&encoded[..40], b"*", &encoded[41..]]),
                ),
            ),
            (
                "a non-ASCII byte",
                assemble(
                    algorithm,
                    key_id,
                    &cat(&[&encoded[..40], b"\xC3", &encoded[41..]]),
                ),
            ),
            // the separators
            (
                "two spaces after the algorithm",
                cat(&[algorithm, b"  ", key_id, b" ", encoded]),
            ),
            (
                "two spaces after the key id",
                cat(&[algorithm, b" ", key_id, b"  ", encoded]),
            ),
            (
                "a tab for a space",
                cat(&[algorithm, b"\t", key_id, b" ", encoded]),
            ),
            (
                "a space inside the signature",
                assemble(
                    algorithm,
                    key_id,
                    &cat(&[&encoded[..44], b" ", &encoded[44..]]),
                ),
            ),
            ("a leading space", cat(&[b" ", &good])),
            ("a trailing space", cat(&[&good, b" "])),
            ("a trailing space and LF", cat(&[&good, b" \n"])),
            ("a trailing tab", cat(&[&good, b"\t"])),
            ("two fields", cat(&[algorithm, b" ", encoded])),
            ("four fields", cat(&[&good, b" extra"])),
            // the lines
            ("nothing", Vec::new()),
            ("an empty line", b"\n".to_vec()),
            ("only CRLF", b"\r\n".to_vec()),
            ("two line ends", cat(&[&good, b"\n\n"])),
            ("two CRLF", cat(&[&good, b"\r\n\r\n"])),
            ("a bare CR", cat(&[&good, b"\r"])),
            ("a CR before the LF twice", cat(&[&good, b"\r\r\n"])),
            ("a leading empty line", cat(&[b"\n", &good])),
            ("the line twice", cat(&[&good, b"\n", &good, b"\n"])),
            ("a second line of anything", cat(&[&good, b"\nnote\n"])),
            ("a NUL after the line", cat(&[&good, b"\0"])),
            (
                "a NUL inside the key id",
                cat(&[
                    algorithm,
                    b" ",
                    &key_id[..4],
                    b"\0",
                    &key_id[5..],
                    b" ",
                    encoded,
                ]),
            ),
            (
                "longer than the cap",
                vec![b'a'; MAX_SIGNATURE_BYTES as usize + 1],
            ),
            (
                "a valid line padded to the cap",
                cat(&[
                    &good,
                    &vec![b'\n'; MAX_SIGNATURE_BYTES as usize - good.len() + 1],
                ]),
            ),
        ];
        for (what, text) in &refused {
            let Err(error) = parse_signature_line(text) else {
                panic!("accepted: {what}");
            };
            assert_eq!(error.code, ErrorCode::ParseFailure, "{what}");
        }
    }

    #[test]
    fn an_oversize_or_two_line_signature_file_is_refused_for_that_reason() {
        // The other checks would refuse these as well (the text would not be base64), so the
        // reason is what shows the cheap checks ran first: the cap is on bytes before any
        // scanning, and a second line is named as one.
        let good = read_fixture("release/v2/Katana-update-win64.json.sig");
        let oversize =
            parse_signature_line(cat(&[&good, &vec![b'x'; MAX_SIGNATURE_BYTES as usize]]))
                .expect_err("oversize");
        assert_eq!(oversize.code, ErrorCode::ParseFailure);
        assert!(
            oversize.message.contains("longer than"),
            "{}",
            oversize.describe()
        );

        for text in [
            cat(&[&good, &good]),
            cat(&[&good, b"\n"]),
            cat(&[&good, b"note\n"]),
            cat(&[b"x\r", &good]),
            cat(&[&good[..40], b"\r", &good[40..]]),
        ] {
            let error = parse_signature_line(&text).expect_err("two lines");
            assert_eq!(error.code, ErrorCode::ParseFailure);
            assert!(
                error.message.contains("more than one line"),
                "{}",
                error.describe()
            );
        }
    }

    #[test]
    fn every_signature_file_of_the_fixtures_parses_and_names_the_key_its_generator_said() {
        // tools/update_make_fixtures.py: what signed each, by its key's first 16 digits.
        let expected = [
            ("v1", "Katana-update-win64.json", "d75a980182b10ab7"),
            ("v2", "Katana-update-win64.json", "d75a980182b10ab7"),
            (
                "v2-spare-signed",
                "Katana-update-win64.json",
                "3d4017c3e843895a",
            ),
            (
                "v2-macos-notify",
                "Katana-update-macos-arm64.json",
                "d75a980182b10ab7",
            ),
            (
                "bad/wrong-key",
                "Katana-update-win64.json",
                "fc51cd8e6218a1a3",
            ),
            (
                "bad/wrong-key-claims-active",
                "Katana-update-win64.json",
                "d75a980182b10ab7",
            ), // the lie
            (
                "bad/flipped-manifest-bit",
                "Katana-update-win64.json",
                "d75a980182b10ab7",
            ),
            (
                "bad/swapped-sig",
                "Katana-update-win64.json",
                "d75a980182b10ab7",
            ),
            (
                "bad/tampered-member",
                "Katana-update-win64.json",
                "d75a980182b10ab7",
            ),
            (
                "bad/path-traversal",
                "Katana-update-win64.json",
                "d75a980182b10ab7",
            ),
            (
                "bad/downgrade-sequence",
                "Katana-update-win64.json",
                "d75a980182b10ab7",
            ),
            (
                "bad/downgrade-version",
                "Katana-update-win64.json",
                "d75a980182b10ab7",
            ),
        ];
        for (folder, manifest, key_id) in expected {
            let parsed =
                parse_signature_line(read_fixture(&format!("release/{folder}/{manifest}.sig")))
                    .unwrap_or_else(|error| panic!("{folder}: {}", error.describe()));
            assert_eq!(parsed.key_id, key_id, "{folder}");
            assert_eq!(parsed.algorithm, "ed25519");
        }
    }

    // ---- RFC 8032 known answers --------------------------------------------------------------

    #[test]
    fn rfc8032_section71_tests1_to3_verify() {
        for vector in &RFC_VECTORS {
            let status = verify_ed25519(
                &key_from_hex(vector.public_key),
                bytes_from_hex(vector.message),
                &signature_from_hex(vector.signature),
            );
            assert!(
                status.is_ok(),
                "{}: {}",
                vector.name,
                status.map_or_else(|error| error.describe(), |()| String::new())
            );
        }
    }

    #[test]
    fn a_message_with_no_data_pointer_is_the_empty_message() {
        // TEST 1 signs the empty message. The C++ hands over a string_view whose data() is null;
        // a Rust slice always has a pointer, so the twin is the empty slice that owns nothing,
        // and what it holds the verifier to is the same: it is the empty message and nothing else.
        let nothing: &[u8] = &[];
        assert!(
            nothing.is_empty(),
            "the premise: there is nothing to hand over"
        );
        let one = &RFC_VECTORS[0];
        let status = verify_ed25519(
            &key_from_hex(one.public_key),
            nothing,
            &signature_from_hex(one.signature),
        );
        assert!(
            status.is_ok(),
            "{}",
            status.map_or_else(|error| error.describe(), |()| String::new())
        );
        // And it is the empty message and nothing else: another key's signature of another
        // message does not verify for it, and TEST 1's signature does not verify for a message
        // of one byte.
        expect_refused(
            &verify_ed25519(
                &key_from_hex(RFC_VECTORS[1].public_key),
                nothing,
                &signature_from_hex(RFC_VECTORS[1].signature),
            ),
            DOES_NOT_MATCH,
        );
        expect_refused(
            &verify_ed25519(
                &key_from_hex(one.public_key),
                b"\0",
                &signature_from_hex(one.signature),
            ),
            DOES_NOT_MATCH,
        );
    }

    #[test]
    fn each_rfc_vector_fails_when_any_bit_of_the_message_is_flipped() {
        // TEST 2 and 3 sign 8 and 16 bits. TEST 1 signs the empty message, which has no bit to
        // flip; that one is held by a_signature_is_for_its_own_message_and_key_only, which
        // lengthens it.
        for vector in &RFC_VECTORS {
            let message = bytes_from_hex(vector.message);
            for bit in 0..message.len() * 8 {
                let mut altered = message.clone();
                altered[bit / 8] ^= 1 << (bit % 8);
                expect_refused(
                    &verify_ed25519(
                        &key_from_hex(vector.public_key),
                        &altered,
                        &signature_from_hex(vector.signature),
                    ),
                    DOES_NOT_MATCH,
                );
            }
        }
    }

    #[test]
    fn each_rfc_vector_fails_when_any_bit_of_the_signature_or_the_key_is_flipped() {
        // All 512 bits of the signature and all 256 of the key, for all three vectors: 2304
        // refusals. Which reason differs (a flip in S can leave S above L; a flip in the key can
        // leave no point, or a point of the wrong order), but no flip may verify.
        for vector in &RFC_VECTORS {
            let key = key_from_hex(vector.public_key);
            let signature = signature_from_hex(vector.signature);
            let message = bytes_from_hex(vector.message);
            assert!(
                verify_ed25519(&key, &message, &signature).is_ok(),
                "{}: the control",
                vector.name
            );
            for bit in 0..signature.len() * 8 {
                let mut altered = signature;
                altered[bit / 8] ^= 1 << (bit % 8);
                let error = verify_ed25519(&key, &message, &altered).expect_err(&format!(
                    "{}: signature bit {bit} flipped, still accepted",
                    vector.name
                ));
                assert_eq!(error.code, ErrorCode::InvalidArgument);
            }
            for bit in 0..key.len() * 8 {
                let mut altered = key;
                altered[bit / 8] ^= 1 << (bit % 8);
                let error = verify_ed25519(&altered, &message, &signature).expect_err(&format!(
                    "{}: key bit {bit} flipped, still accepted",
                    vector.name
                ));
                assert_eq!(error.code, ErrorCode::InvalidArgument);
            }
        }
    }

    #[test]
    fn a_signature_is_for_its_own_message_and_key_only() {
        let one = &RFC_VECTORS[0];
        let two = &RFC_VECTORS[1];
        let three = &RFC_VECTORS[2];

        // Another vector's key, message or signature.
        expect_refused(
            &verify_ed25519(
                &key_from_hex(two.public_key),
                bytes_from_hex(one.message),
                &signature_from_hex(one.signature),
            ),
            DOES_NOT_MATCH,
        );
        expect_refused(
            &verify_ed25519(
                &key_from_hex(one.public_key),
                bytes_from_hex(two.message),
                &signature_from_hex(two.signature),
            ),
            DOES_NOT_MATCH,
        );
        expect_refused(
            &verify_ed25519(
                &key_from_hex(three.public_key),
                bytes_from_hex(two.message),
                &signature_from_hex(two.signature),
            ),
            DOES_NOT_MATCH,
        );
        expect_refused(
            &verify_ed25519(
                &key_from_hex(two.public_key),
                bytes_from_hex(three.message),
                &signature_from_hex(two.signature),
            ),
            DOES_NOT_MATCH,
        );

        // The empty message is not a one-byte one, and a message is not its own extension.
        expect_refused(
            &verify_ed25519(
                &key_from_hex(two.public_key),
                b"",
                &signature_from_hex(two.signature),
            ),
            DOES_NOT_MATCH,
        );
        expect_refused(
            &verify_ed25519(
                &key_from_hex(two.public_key),
                b"\x72\x00",
                &signature_from_hex(two.signature),
            ),
            DOES_NOT_MATCH,
        );
        expect_refused(
            &verify_ed25519(
                &key_from_hex(one.public_key),
                b"\0",
                &signature_from_hex(one.signature),
            ),
            DOES_NOT_MATCH,
        );
        expect_refused(
            &verify_ed25519(
                &key_from_hex(one.public_key),
                vec![b'x'; 1 << 20],
                &signature_from_hex(one.signature),
            ),
            DOES_NOT_MATCH,
        );
    }

    // ---- S below L ---------------------------------------------------------------------------

    #[test]
    fn a_signature_whose_s_is_not_below_the_group_order_is_refused_for_that() {
        let vector = &RFC_VECTORS[1];
        let key = key_from_hex(vector.public_key);
        let message = bytes_from_hex(vector.message);
        let r = bytes_from_hex(vector.signature)[..32].to_vec();
        let signature_with = |s: &[u8]| -> SignatureBytes {
            let mut out = [0u8; 64];
            out[..32].copy_from_slice(&r);
            out[32..].copy_from_slice(&s[..32]);
            out
        };

        // The control: TEST 2's own S verifies.
        assert!(verify_ed25519(&key, &message, &signature_from_hex(vector.signature)).is_ok());

        // S + L: the verification equation still holds for it (the Python checked), so a
        // verifier that only runs the equation accepts a second signature for the same message.
        // The reason below (not "does not match") is what shows the comparison here ran.
        let s_plus_l =
            bytes_from_hex("f52db7415978abc61b2c2eb6aeebfca0387b2eaeb4302aeeb00d291612bb0c10");
        expect_refused(
            &verify_ed25519(&key, &message, &signature_with(&s_plus_l)),
            NOT_BELOW_ORDER,
        );
        // S + 2L (the top three bits of the last byte are no longer zero).
        expect_refused(
            &verify_ed25519(
                &key,
                &message,
                &signature_with(&bytes_from_hex(
                    "e201ad9e73dbbd1ef2c825598de5dbb5387b2eaeb4302aeeb00d291612bb0c20",
                )),
            ),
            NOT_BELOW_ORDER,
        );
        // S = L exactly, little-endian: edd3f55c1a631258d69cf7a2def9de14 then zeros, then 0x10.
        let order =
            bytes_from_hex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
        expect_refused(
            &verify_ed25519(&key, &message, &signature_with(&order)),
            NOT_BELOW_ORDER,
        );
        // The largest 256-bit value, and the smallest above L (L + 1).
        expect_refused(
            &verify_ed25519(&key, &message, &signature_with(&[0xff; 32])),
            NOT_BELOW_ORDER,
        );
        let mut order_plus_one = order.clone();
        order_plus_one[0] += 1; // 0xed + 1, no carry
        expect_refused(
            &verify_ed25519(&key, &message, &signature_with(&order_plus_one)),
            NOT_BELOW_ORDER,
        );
        // Only the top byte above: 0x11 where L has 0x10, whatever lies below it.
        let mut top_byte = [0u8; 32];
        top_byte[31] = 0x11;
        expect_refused(
            &verify_ed25519(&key, &message, &signature_with(&top_byte)),
            NOT_BELOW_ORDER,
        );
        // Every byte equal to L's but one that is higher, at each position (a comparison that
        // looks at the wrong end, or stops at the first difference upward, gets one of these
        // wrong).
        for i in 0..32 {
            let mut s = order.clone();
            if s[i] == 0xff {
                continue;
            }
            s[i] += 1;
            // Raising a byte of L raises S above L; lower bytes are L's own.
            expect_refused(
                &verify_ed25519(&key, &message, &signature_with(&s)),
                NOT_BELOW_ORDER,
            );
        }
    }

    #[test]
    fn the_largest_scalar_below_the_group_order_is_refused_as_a_mismatch_not_for_its_size() {
        // L - 1 is a legal S (RFC 8032, 5.1.7 says S < L), so the explicit comparison lets it
        // through to the verifier, which finds it does not belong to this message. A comparison
        // that is off by one the other way (S + 1 against L, or "equal" declared one byte too
        // early) would refuse it for its size instead.
        let vector = &RFC_VECTORS[1];
        let r = bytes_from_hex(vector.signature)[..32].to_vec();
        let s = bytes_from_hex("ecd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
        let mut signature = [0u8; 64];
        signature[..32].copy_from_slice(&r);
        signature[32..].copy_from_slice(&s);
        expect_refused(
            &verify_ed25519(
                &key_from_hex(vector.public_key),
                bytes_from_hex(vector.message),
                &signature,
            ),
            DOES_NOT_MATCH,
        );
        // And the smallest scalars, 0 and 1, are below L as well.
        for low in [0u8, 1] {
            let mut small = [0u8; 64];
            small[..32].copy_from_slice(&r);
            small[32] = low;
            expect_refused(
                &verify_ed25519(
                    &key_from_hex(vector.public_key),
                    bytes_from_hex(vector.message),
                    &small,
                ),
                DOES_NOT_MATCH,
            );
        }
    }

    // ---- the public key: a point of the prime-order subgroup -------------------------------------

    #[test]
    fn a_public_key_that_is_not_a_prime_order_point_is_refused_whatever_the_signature() {
        // Computed in the Python, not by this library: the eight points with 8P = the identity (as
        // L * Q for curve points Q), the aliases RFC 8032, 5.1.3 forbids, a point of mixed order,
        // and a y for which no x exists. Each is paired with TEST 2's genuine signature and
        // message: refused for the key, since the equation has nothing to say about it.
        let keys = [
            // the torsion subgroup
            (
                "order 4, y = 0",
                "0000000000000000000000000000000000000000000000000000000000000000",
            ),
            (
                "order 4, y = 0, other x",
                "0000000000000000000000000000000000000000000000000000000000000080",
            ),
            (
                "order 1, the neutral element",
                "0100000000000000000000000000000000000000000000000000000000000000",
            ),
            (
                "order 8",
                "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
            ),
            (
                "order 8, other x",
                "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85",
            ),
            (
                "order 8, other y",
                "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
            ),
            (
                "order 8, other y and x",
                "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
            ),
            (
                "order 2, y = p - 1",
                "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            ),
            // not canonical
            (
                "y = p",
                "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            ),
            (
                "y = p, sign bit set",
                "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            ),
            (
                "y = p + 1",
                "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            ),
            (
                "x = 0 with the sign bit set (identity)",
                "0100000000000000000000000000000000000000000000000000000000000080",
            ),
            (
                "x = 0 with the sign bit set (y = p - 1)",
                "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            ),
            (
                "all ones",
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            ),
            // a point of the curve of order 8 * L: TEST 1's key plus an order-8 point
            (
                "mixed order",
                "9158312a9a8d6e3b34c891d6d61444f8b8211c5117ebad15bdb0bd68b07e0245",
            ),
            // no point
            (
                "y = 2, no x",
                "0200000000000000000000000000000000000000000000000000000000000000",
            ),
            (
                "y = 7, no x",
                "0700000000000000000000000000000000000000000000000000000000000000",
            ),
            // the placeholders of cmake/KatanaUpdateKeys.cmake: "PLACEHOLDER-ACTIVE-NOT-A-KEY-00-"
            // and "PLACEHOLDER-SPARE-NOT-A-KEY--00-" in ASCII (the Python: neither decodes)
            (
                "the active placeholder",
                "504c414345484f4c4445522d4143544956452d4e4f542d412d4b45592d30302d",
            ),
            (
                "the spare placeholder",
                "504c414345484f4c4445522d53504152452d4e4f542d412d4b45592d2d30302d",
            ),
        ];
        let vector = &RFC_VECTORS[1];
        for (what, hex) in keys {
            let key = key_from_hex(hex);
            let refused = |status: &Status| {
                let Err(error) = status else {
                    panic!("{what}: accepted");
                };
                assert_eq!(error.code, ErrorCode::InvalidArgument, "{what}");
                assert!(
                    error.message.contains(KEY_NOT_ACCEPTED),
                    "{what}: {}",
                    error.describe()
                );
            };
            refused(&verify_ed25519(
                &key,
                bytes_from_hex(vector.message),
                &signature_from_hex(vector.signature),
            ));
            // ... and with a signature of zeros, the shape of a forgery.
            refused(&verify_ed25519(
                &key,
                bytes_from_hex(vector.message),
                &[0u8; 64],
            ));
            // The key check on its own (what the trust gate asks of a pinned key) says the same.
            refused(&check_public_key(&key));
        }
    }

    #[test]
    fn the_all_zero_key_and_signature_that_openssl_accepts_are_refused() {
        // Measured with `openssl pkeyutl -verify -pubin -rawin` (3.6.5): the all-zero public key
        // (a point of order 4) and the all-zero signature verify one-byte messages 9, 14, 17, 19,
        // 26, 27, 39, 47, 49, 51 ... 239 (64 of 256), because the equation 8*S*B = 8*R + 8*h*A has
        // only multiples of the order-4 point in it. This is a forgery for a build that pins the
        // zero key; the key check is what stops it. All 256 messages, so that it does not depend
        // on which of them the equation happens to hold for.
        let zero_key: PublicKey = [0; 32];
        let zero_signature: SignatureBytes = [0; 64];
        for byte in 0..=255u8 {
            expect_refused(
                &verify_ed25519(&zero_key, [byte], &zero_signature),
                KEY_NOT_ACCEPTED,
            );
        }
        // The same signature under a real key is a plain mismatch, so the refusal above is about
        // the key.
        expect_refused(
            &verify_ed25519(&key_from_hex(TEST2_PUBLIC), b"\x09", &zero_signature),
            DOES_NOT_MATCH,
        );
    }

    #[test]
    fn the_rfc_keys_are_accepted_as_prime_order_points() {
        // A key the check refuses would make every pinned key useless: a good signature by a
        // genuine key verified above; here the same key with a wrong signature is a MISMATCH, not
        // a key refusal.
        for vector in &RFC_VECTORS {
            let mut wrong = signature_from_hex(vector.signature);
            wrong[0] ^= 1;
            let error = verify_ed25519(
                &key_from_hex(vector.public_key),
                bytes_from_hex(vector.message),
                &wrong,
            )
            .expect_err("a wrong signature");
            assert!(
                !error.message.contains(KEY_NOT_ACCEPTED),
                "{}: {}",
                vector.name,
                error.describe()
            );
            let checked = check_public_key(&key_from_hex(vector.public_key));
            assert!(
                checked.is_ok(),
                "{}: {}",
                vector.name,
                checked.map_or_else(|error| error.describe(), |()| String::new())
            );
        }
    }

    #[test]
    fn twenty_four_keys_generated_and_signed_by_openssl_are_accepted_and_verify() {
        // The check that a key is a prime-order point is a false refusal away from leaving every
        // installed copy unable to update, with nothing in the build to say so. Three RFC keys are
        // too few samples for that: these are 24 keys made by `openssl genpkey -algorithm
        // ed25519` with a signature each by `openssl pkeyutl -sign -rawin` over the message
        // printed. The keys are throw-away (their secrets were not kept) and each signature was
        // checked back with `pkeyutl -verify`. Such keys are what the pinned ones will be.
        for entry in &GENERATED {
            let key = key_from_hex(entry.public_key);
            let signature = signature_from_hex(entry.signature);
            let good = verify_ed25519(&key, entry.message, &signature);
            assert!(
                good.is_ok(),
                "{}: {}",
                entry.public_key,
                good.map_or_else(|error| error.describe(), |()| String::new())
            );
            assert!(check_public_key(&key).is_ok(), "{}", entry.public_key);

            // One bit of the message: a MISMATCH for this key, so the key was accepted as a key.
            let mut altered = entry.message.as_bytes().to_vec();
            altered[0] ^= 1;
            let bad = verify_ed25519(&key, &altered, &signature).expect_err(entry.public_key);
            assert!(bad.message.contains(DOES_NOT_MATCH), "{}", bad.describe());
            assert!(
                !bad.message.contains(KEY_NOT_ACCEPTED),
                "{}",
                bad.describe()
            );
        }
    }

    // ---- manifests signed by the openssl command line ------------------------------------------

    #[test]
    fn fixture_manifests_signed_by_openssl_verify_under_exactly_the_key_that_signed_them() {
        let active = key_from_hex(TEST1_PUBLIC);
        let spare = key_from_hex(TEST2_PUBLIC);
        let other = key_from_hex(TEST3_PUBLIC);
        let cases = [
            ("v1", "Katana-update-win64.json", true, false, false),
            ("v2", "Katana-update-win64.json", true, false, false),
            (
                "v2-spare-signed",
                "Katana-update-win64.json",
                false,
                true,
                false,
            ),
            (
                "v2-macos-notify",
                "Katana-update-macos-arm64.json",
                true,
                false,
                false,
            ),
            // wrong in the signature
            (
                "bad/flipped-manifest-bit",
                "Katana-update-win64.json",
                false,
                false,
                false,
            ),
            (
                "bad/swapped-sig",
                "Katana-update-win64.json",
                false,
                false,
                false,
            ),
            (
                "bad/wrong-key",
                "Katana-update-win64.json",
                false,
                false,
                true,
            ),
            (
                "bad/wrong-key-claims-active",
                "Katana-update-win64.json",
                false,
                false,
                true,
            ),
            // validly signed, wrong elsewhere (paths, the zip, the order): the signature layer
            // passes them and the layers that own those rules refuse them
            (
                "bad/tampered-member",
                "Katana-update-win64.json",
                true,
                false,
                false,
            ),
            (
                "bad/path-traversal",
                "Katana-update-win64.json",
                true,
                false,
                false,
            ),
            (
                "bad/downgrade-sequence",
                "Katana-update-win64.json",
                true,
                false,
                false,
            ),
            (
                "bad/downgrade-version",
                "Katana-update-win64.json",
                true,
                false,
                false,
            ),
        ];
        for (folder, name, by_active, by_spare, by_other) in cases {
            let manifest = read_fixture(&format!("release/{folder}/{name}"));
            let line = parse_signature_line(read_fixture(&format!("release/{folder}/{name}.sig")))
                .unwrap_or_else(|error| panic!("{folder}: {}", error.describe()));
            let signature = &line.signature;
            assert_eq!(
                verify_ed25519(&active, &manifest, signature).is_ok(),
                by_active,
                "{folder} under the active key"
            );
            assert_eq!(
                verify_ed25519(&spare, &manifest, signature).is_ok(),
                by_spare,
                "{folder} under the spare key"
            );
            assert_eq!(
                verify_ed25519(&other, &manifest, signature).is_ok(),
                by_other,
                "{folder} under the key nobody trusts"
            );
        }
    }

    #[test]
    fn a_flipped_bit_or_a_changed_length_anywhere_in_a_signed_manifest_is_refused() {
        // The exact bytes are signed: no whitespace, line end or length is "equivalent".
        let manifest = read_fixture("release/v2/Katana-update-win64.json");
        let line = parse_signature_line(read_fixture("release/v2/Katana-update-win64.json.sig"))
            .expect("the line parses");
        let key = key_from_hex(TEST1_PUBLIC);
        let signature = &line.signature;
        assert!(
            verify_ed25519(&key, &manifest, signature).is_ok(),
            "the control"
        );
        assert!(manifest.len() > 1500);

        let mut flips = 0usize;
        for offset in (0..manifest.len()).step_by(37) {
            let mut altered = manifest.clone();
            altered[offset] ^= 1 << (offset % 8);
            expect_refused(&verify_ed25519(&key, &altered, signature), DOES_NOT_MATCH);
            flips += 1;
        }
        assert!(flips > 40); // the loop reached many offsets, the first and (at 37 * k) others
        let mut last_byte = manifest.clone();
        *last_byte.last_mut().expect("a manifest") ^= 1;
        expect_refused(&verify_ed25519(&key, &last_byte, signature), DOES_NOT_MATCH);

        expect_refused(
            &verify_ed25519(&key, &manifest[..manifest.len() - 1], signature),
            DOES_NOT_MATCH,
        );
        expect_refused(
            &verify_ed25519(&key, &manifest[1..], signature),
            DOES_NOT_MATCH,
        );
        expect_refused(
            &verify_ed25519(&key, cat(&[&manifest, b"\n"]), signature),
            DOES_NOT_MATCH,
        );
        expect_refused(
            &verify_ed25519(&key, cat(&[&manifest, b" "]), signature),
            DOES_NOT_MATCH,
        );
        expect_refused(&verify_ed25519(&key, b"", signature), DOES_NOT_MATCH);
        let mut crlf = Vec::new();
        for &c in &manifest {
            if c == b'\n' {
                crlf.push(b'\r');
            }
            crlf.push(c);
        }
        expect_refused(&verify_ed25519(&key, &crlf, signature), DOES_NOT_MATCH);
    }

    // ---- thread safety and the environment -----------------------------------------------------

    #[test]
    fn verification_is_safe_from_many_threads_at_once() {
        use std::sync::atomic::{AtomicI32, Ordering};
        let key = key_from_hex(RFC_VECTORS[1].public_key);
        let good = signature_from_hex(RFC_VECTORS[1].signature);
        let mut bad = good;
        bad[5] ^= 0x10;

        let wrong_answers = AtomicI32::new(0);
        let verifications = AtomicI32::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..25 {
                        if verify_ed25519(&key, b"\x72", &good).is_err() {
                            wrong_answers.fetch_add(1, Ordering::Relaxed);
                        }
                        if verify_ed25519(&key, b"\x72", &bad).is_ok() {
                            wrong_answers.fetch_add(1, Ordering::Relaxed);
                        }
                        if verify_ed25519(&key, b"\x73", &good).is_ok() {
                            wrong_answers.fetch_add(1, Ordering::Relaxed);
                        }
                        verifications.fetch_add(3, Ordering::Relaxed);
                    }
                });
            }
        });
        assert_eq!(verifications.load(Ordering::Relaxed), 8 * 25 * 3);
        assert_eq!(wrong_answers.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn hardening_removes_the_three_open_ssl_variables_and_verification_still_works() {
        let _restore = RestoreOpenSslVariables::new();
        for name in OPEN_SSL_VARIABLES {
            set_variable(name, "somewhere-an-attacker-chose");
            assert!(
                variable(name).is_some(),
                "{name}: the test could not set it"
            );
        }
        set_variable("OPENSSL_CONF_INCLUDE", "left alone"); // another variable: not this function's
        set_variable("KATANA_T5_UNRELATED", "left alone");

        let hardened = harden();
        assert!(
            hardened.is_ok(),
            "{}",
            hardened.map_or_else(|error| error.describe(), |()| String::new())
        );
        for name in OPEN_SSL_VARIABLES {
            assert_eq!(variable(name), None, "{name}");
        }
        assert_eq!(
            variable("KATANA_T5_UNRELATED").as_deref(),
            Some("left alone")
        );
        assert!(variable("OPENSSL_CONF_INCLUDE").is_some());
        remove_variable("OPENSSL_CONF_INCLUDE");
        remove_variable("KATANA_T5_UNRELATED");

        // Idempotent, and the library still does its job afterwards.
        assert!(harden().is_ok());
        assert!(
            verify_ed25519(
                &key_from_hex(RFC_VECTORS[1].public_key),
                bytes_from_hex(RFC_VECTORS[1].message),
                &signature_from_hex(RFC_VECTORS[1].signature),
            )
            .is_ok()
        );
    }

    #[test]
    fn hardening_with_nothing_set_changes_nothing() {
        let _restore = RestoreOpenSslVariables::new();
        for name in OPEN_SSL_VARIABLES {
            remove_variable(name);
        }
        assert!(harden().is_ok());
        for name in OPEN_SSL_VARIABLES {
            assert_eq!(variable(name), None, "{name}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn hardening_removes_the_variables_whatever_their_case_on_windows() {
        // Windows matches environment names without regard to case, so a variable spelled
        // "openssl_modules" is the one OpenSSL reads. (tools/update_make_manifest.py tests the
        // same for the openssl it runs.)
        let _restore = RestoreOpenSslVariables::new();
        set_variable("openssl_conf", "x");
        set_variable("OpenSSL_Modules", "y");
        set_variable("OPENSSL_engines", "z");
        assert!(
            variable("OPENSSL_CONF").is_some(),
            "the premise: Windows folds the case"
        );

        assert!(harden().is_ok());
        for name in [
            "OPENSSL_CONF",
            "openssl_conf",
            "OPENSSL_MODULES",
            "openssl_modules",
            "OPENSSL_ENGINES",
            "openssl_engines",
        ] {
            assert_eq!(variable(name), None, "{name}");
        }
    }

    // ---- a configuration file named by the environment must not decide what verifies -----------
    //
    // In the C++, OpenSSL reads the file OPENSSL_CONF names when it first initialises, once per
    // process, and the file decides which providers load; verify_ed25519 initialises libcrypto
    // with OPENSSL_INIT_NO_LOAD_CONFIG, and the case starts the test program again with the
    // variable set and nothing having called libcrypto. The port's verifier reads no file at all;
    // the case is kept, the same way, so that it holds whatever library a later build links: the
    // child, started with the three variables naming a file that loads the base provider alone and
    // two folders with nothing in them, must still verify RFC 8032 TEST 1 to 3. The child does not
    // call harden_open_ssl_environment either, which would clear the variables and hide what is
    // being shown.

    #[test]
    fn a_configuration_file_named_by_the_environment_does_not_change_what_verifies() {
        if std::env::var_os(HOSTILE_CHILD_MARKER).is_some() {
            // The child. RFC 8032 TEST 1 to 3 must verify.
            for vector in &RFC_VECTORS {
                let status = verify_ed25519(
                    &key_from_hex(vector.public_key),
                    bytes_from_hex(vector.message),
                    &signature_from_hex(vector.signature),
                );
                assert!(
                    status.is_ok(),
                    "{}: {}",
                    vector.name,
                    status.map_or_else(|error| error.describe(), |()| String::new())
                );
            }
            return;
        }

        let scratch = ScratchFolder::new("configuration");
        assert!(scratch.path.is_dir());
        std::fs::write(
            scratch.path.join("base_provider_only.cnf"),
            "openssl_conf = katana_t5_init\n\
             [katana_t5_init]\n\
             providers = katana_t5_providers\n\
             [katana_t5_providers]\n\
             base = katana_t5_base\n\
             [katana_t5_base]\n\
             activate = 1\n",
        )
        .expect("the configuration file can be written");

        let _restore = RestoreOpenSslVariables::new();
        // Relative names, resolved against the child's working directory (the scratch folder).
        set_variable("OPENSSL_CONF", "base_provider_only.cnf");
        set_variable("OPENSSL_MODULES", "no_modules_here");
        set_variable("OPENSSL_ENGINES", "no_engines_here");
        set_variable(HOSTILE_CHILD_MARKER, "1");

        let (exit_code, text) = run_program_on_one_case(
            "signature::a_configuration_file_named_by_the_environment_does_not_change_what_verifies",
            &scratch.path,
        );
        assert_eq!(exit_code, Some(0), "{text}");
        // One case ran in the child (a filter that matched nothing would also exit 0) and it
        // passed.
        assert!(text.contains(ONE_PASSED), "{text}");
    }

    // ---- what a program this one starts inherits -------------------------------------------------
    //
    // harden_open_ssl_environment promises that afterwards a program the process starts holds none
    // of the three variables, which is what the helper's relaunch of katana relies on. Reading them
    // back proves only this process's view of its environment; this case starts the program again
    // after the call, and the child reports what IT was handed.

    #[test]
    fn hardening_leaves_none_of_the_three_variables_to_a_program_this_one_starts() {
        if std::env::var_os(INHERITANCE_CHILD_MARKER).is_some() {
            // The child. What it holds is what it inherited.
            for name in OPEN_SSL_VARIABLES {
                assert_eq!(variable(name), None, "{name} reached the child");
            }
            #[cfg(windows)]
            for name in ["openssl_conf", "OpenSSL_Modules", "OPENSSL_engines"] {
                assert_eq!(variable(name), None, "{name} reached the child");
            }
            // The controls: the environment was inherited at all, and a variable of another name
            // survived the call.
            assert_eq!(
                variable("KATANA_T5_UNRELATED").as_deref(),
                Some("inherited"),
                "the child inherited nothing"
            );
            return;
        }

        let scratch = ScratchFolder::new("inheritance");
        assert!(scratch.path.is_dir());

        let _restore = RestoreOpenSslVariables::new();
        for name in OPEN_SSL_VARIABLES {
            set_variable(name, "somewhere-an-attacker-chose");
        }
        // Spelled in another case, which is the same variable on Windows.
        #[cfg(windows)]
        set_variable("openssl_conf", "somewhere-an-attacker-chose");
        set_variable("KATANA_T5_UNRELATED", "inherited");
        set_variable(INHERITANCE_CHILD_MARKER, "1");
        assert!(harden().is_ok());

        let (exit_code, text) = run_program_on_one_case(
            "signature::hardening_leaves_none_of_the_three_variables_to_a_program_this_one_starts",
            &scratch.path,
        );
        assert_eq!(exit_code, Some(0), "{text}");
        assert!(text.contains(ONE_PASSED), "{text}");
    }
}
