//! What the C++ build compiles into the update library, read from the same files and with the same
//! defaults, so that one build of the checkout gives both programs the same values:
//!
//! * KATANA_VERSION: project(Katana VERSION ...) of the checkout's CMakeLists.txt, which the C++
//!   build passes to the library (src/katana_update/CMakeLists.txt), so the version is still
//!   written in one place.
//! * KATANA_UPDATE_REPO and KATANA_UPDATE_FEED: the cache defaults that
//!   src/katana_update/CMakeLists.txt declares ("kjarada/Katana" and ""). A configure that is given
//!   another value (`-DKATANA_UPDATE_REPO=...`) changes the C++ alone; the same value reaches the
//!   Rust when the variable of that name is in cargo's environment, which nothing sets yet
//!   (docs/rust.md, "Not done").
//! * KATANA_UPDATE_KEY_ACTIVE, KATANA_UPDATE_KEY_SPARE and KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS: the
//!   block of cmake/KatanaUpdateKeys.cmake that tools/update_keytool.py rewrites, held to the rules
//!   that file holds a configure to. A `-D` cannot move them in the C++ either: the file sets them
//!   as plain variables, which hide a cache value of the same name. So no environment variable is
//!   read for them here: whose signature counts is the file's alone. KATANA_REQUIRE_UPDATE_KEYS,
//!   which fails a configure that still holds the placeholders, is read from the environment as
//!   the feed's two are.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    );
    let checkout = manifest.join("../../..");

    let cmake = checkout.join("CMakeLists.txt");
    let text = read(&cmake);
    let version = project_version(&text)
        .unwrap_or_else(|| panic!("{} has no project(Katana VERSION ...)", cmake.display()));
    println!("cargo:rustc-env=KATANA_VERSION={version}");

    let library = checkout.join("src/katana_update/CMakeLists.txt");
    let text = read(&library);
    for name in ["KATANA_UPDATE_REPO", "KATANA_UPDATE_FEED"] {
        let value = configured(name).unwrap_or_else(|| {
            cache_default(&text, name)
                .unwrap_or_else(|| panic!("{} declares no {name}", library.display()))
        });
        println!("cargo:rustc-env={name}={value}");
    }

    let keys = checkout.join("cmake/KatanaUpdateKeys.cmake");
    let text = read(&keys);
    let pinned = PinnedKeys::read(&text).unwrap_or_else(|why| panic!("{}: {why}", keys.display()));
    println!("cargo:rustc-env=KATANA_UPDATE_KEY_ACTIVE={}", pinned.active);
    println!("cargo:rustc-env=KATANA_UPDATE_KEY_SPARE={}", pinned.spare);
    println!(
        "cargo:rustc-env=KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS={}",
        u8::from(pinned.placeholders)
    );
}

fn read(path: &Path) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

// The value a build was configured with, from cargo's environment; None when it names none.
fn configured(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={name}");
    std::env::var(name).ok()
}

// The word after VERSION inside project(Katana ...). It is not checked here: program_version is
// the check, and a broken one is its Internal error, as in the C++.
fn project_version(text: &str) -> Option<&str> {
    const OPENING: &str = "project(Katana";
    let start = text.find(OPENING)? + OPENING.len();
    let rest = &text[start..];
    let arguments = &rest[..rest.find(')')?];
    let mut words = arguments.split_ascii_whitespace();
    while let Some(word) = words.next() {
        if word == "VERSION" {
            return words.next();
        }
    }
    None
}

// The quoted default of `set(<name> "<value>" CACHE ...)`.
fn cache_default(text: &str, name: &str) -> Option<String> {
    let opening = format!("set({name} ");
    let start = text.find(&opening)? + opening.len();
    let rest = text[start..].trim_start().strip_prefix('"')?;
    let value = &rest[..rest.find('"')?];
    let after = rest[value.len() + 1..].trim_start();
    after.starts_with("CACHE").then(|| value.to_owned())
}

// The value of `set(<name> <value>)` inside the block that tools/update_keytool.py rewrites
// (between the ">>> keys" and "<<< keys" lines), without its quotes.
fn keys_value<'a>(block: &'a str, name: &str) -> Option<&'a str> {
    let opening = format!("set({name} ");
    let start = block.find(&opening)? + opening.len();
    let rest = &block[start..];
    let value = &rest[..rest.find(')')?];
    Some(
        value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(value),
    )
}

// CMake's truth: ON, YES, TRUE, Y and a number other than 0 (its documentation of if(<constant>)).
fn cmake_true(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    matches!(upper.as_str(), "ON" | "YES" | "TRUE" | "Y")
        || value.parse::<f64>().is_ok_and(|number| number != 0.0)
}

struct PinnedKeys {
    active: String,
    spare: String,
    placeholders: bool,
}

impl PinnedKeys {
    // The block, and the refusals cmake/KatanaUpdateKeys.cmake makes of it at configure, in its
    // order and with its words.
    fn read(text: &str) -> Result<Self, String> {
        let begin = text.find("# >>> keys").ok_or("no \"# >>> keys\" line")?;
        let end = text.find("# <<< keys").ok_or("no \"# <<< keys\" line")?;
        if end < begin {
            return Err(String::from("the keys block ends before it begins"));
        }
        let block = &text[begin..end];
        let value = |name: &str| {
            keys_value(block, name)
                .map(str::to_owned)
                .ok_or_else(|| format!("the keys block sets no {name}"))
        };
        let flag = value("KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS")?;
        let active = value("KATANA_UPDATE_KEY_ACTIVE")?;
        let spare = value("KATANA_UPDATE_KEY_SPARE")?;

        for (which, key) in [("ACTIVE", &active), ("SPARE", &spare)] {
            let hex = key.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'));
            if !hex || key.len() != 64 {
                return Err(format!(
                    "KATANA_UPDATE_KEY_{which} must be 64 lower-case hexadecimal digits (an \
                     Ed25519 public key), not '{key}'. Do not edit cmake/KatanaUpdateKeys.cmake by \
                     hand: python tools/update_keytool.py pub writes it."
                ));
            }
        }
        // The public halves of RFC 8032 section 7.1, TEST 1, 2 and 3: published, so anyone holds
        // the private halves and can sign an update that a build pinning one would accept.
        const PUBLISHED_TEST_KEYS: [&str; 3] = [
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
            "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        ];
        for (which, key) in [("ACTIVE", &active), ("SPARE", &spare)] {
            if PUBLISHED_TEST_KEYS.contains(&key.as_str()) {
                return Err(format!(
                    "KATANA_UPDATE_KEY_{which} is a published RFC 8032 test key. Anyone holds its \
                     private half and can sign an update that this build would accept. Generate \
                     real keys (docs/update.md, the owner's steps) and run python \
                     tools/update_keytool.py pub."
                ));
            }
        }
        if active == spare {
            return Err(String::from(
                "the spare key must be a different key from the active key: a spare that is the \
                 active key is no way to recover from losing it",
            ));
        }

        // The flag and the bytes must agree, in both directions: "PLACEHOLDER" in ASCII.
        const PLACEHOLDER_PREFIX: &str = "504c414345484f4c444552";
        let placeholders = cmake_true(&flag);
        let spells_placeholder =
            active.starts_with(PLACEHOLDER_PREFIX) || spare.starts_with(PLACEHOLDER_PREFIX);
        if placeholders && !spells_placeholder {
            return Err(String::from(
                "KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS says the keys are placeholders, but they do \
                 not spell PLACEHOLDER. Run python tools/update_keytool.py pub, which sets both \
                 together.",
            ));
        }
        if !placeholders && spells_placeholder {
            return Err(String::from(
                "KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS says the keys are real, but they spell \
                 PLACEHOLDER. A key that spells that is no key anyone holds.",
            ));
        }
        let required =
            configured("KATANA_REQUIRE_UPDATE_KEYS").is_some_and(|value| cmake_true(&value));
        if placeholders && required {
            return Err(String::from(
                "KATANA_REQUIRE_UPDATE_KEYS is ON, but cmake/KatanaUpdateKeys.cmake still holds the \
                 PLACEHOLDER update keys. A release built with them could never be updated and \
                 would carry keys nobody holds. Generate the keys and run tools/update_keytool.py \
                 pub (docs/update.md, the owner's steps), then commit the file.",
            ));
        }
        Ok(Self {
            active,
            spare,
            placeholders,
        })
    }
}
