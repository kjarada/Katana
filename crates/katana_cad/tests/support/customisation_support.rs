//! What the customisation tests share (wave H2's; the targets tests/customisation and
//! tests/customisation_env include it with `#[path]`): a scratch folder of one test's own, the
//! digest of a file's bytes worked out independently, and a built-in made from a text as a host
//! holds one.

#![allow(
    dead_code,
    reason = "Each test target that includes this module uses only some of its helpers."
)]

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use katana_cad::customisation_host::{BuiltInCustomisation, BuiltInEntry};
use katana_entity::Customisation;
use katana_entity::customisation::{customisation_digest, customisation_from_json};

/// A fresh folder for one test's files, removed when the test ends. Named by the test and the
/// process, so that no two tests - in this process, which runs them on parallel threads, or in
/// another running the same binary - share one (the C++ named the process's folder and removed it
/// whole, which here would remove the folders of tests still running).
pub struct Scratch {
    pub root: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "katana-cad-tests-customisation-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the scratch folder is made");
        Self { root }
    }

    pub fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, text).expect("the scratch file is written");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// FNV-1a, 64-bit, as 16 lower-case hexadecimal digits: the digest of a file's bytes, worked out
/// here from the algorithm's published offset basis and prime rather than by the function under
/// test.
pub fn fnv1a(bytes: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// `text` as a host holds a built-in: read, with the digest of its bytes.
pub fn built_in_from(text: &str) -> BuiltInCustomisation {
    match customisation_from_json(text.as_bytes()) {
        Ok(customisation) => BuiltInCustomisation {
            customisation: Some(Arc::new(customisation)),
            digest: fnv1a(text),
            problem: String::new(),
        },
        Err(error) => panic!("{}", error.describe()),
    }
}

/// `text` as an entry of the list a host offers, its digest the reader's own.
pub fn entry_from(text: &str, source: &str) -> BuiltInEntry {
    let customisation = customisation_from_json(text.as_bytes())
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    BuiltInEntry {
        name: customisation.name.clone(),
        description: customisation.description.clone(),
        source: String::from(source),
        built_in: BuiltInCustomisation {
            customisation: Some(Arc::new(customisation)),
            digest: customisation_digest(text.as_bytes()),
            problem: String::new(),
        },
    }
}

/// The small built-in of test_customisation_host.cpp. Counted by hand: 2 definitions, 1 of them
/// listed as a symbol; 2 rules; 1 colour; start spelled "S".
pub const SMALL: &str = r##"{
  "format": "katana-customisation", "version": 1,
  "name": "Small Built In",
  "description": "Two definitions and two rules, for tests",
  "notice": ["Written for these tests."],
  "colours": {"sui gas": "#FF7F00"},
  "linework": {"start": "S", "end": "END", "close": "CL", "arcStart": "BC",
               "arcEnd": "EC", "join": "JPN", "rectangle": "RECT"},
  "linestyles": [
    {"name": "TEST Fence", "strokes": [["move", 0, 0], ["draw", 2, 0]]}
  ],
  "symbols": [
    {"name": "TEST Peg", "atVertices": true, "strokes": [["move", 0, 0], ["circle", 0.5]]}
  ],
  "codes": [
    {"key": "FE*", "sets": "feature", "layer": "FENCES", "colour": "sui gas",
     "linestyle": "TEST Fence"},
    {"key": "PG*", "sets": "symbol", "symbol": {"name": "TEST Peg", "size": 1.5}}
  ]
}"##;

/// The first stand-in of test_builtin_library.cpp. By hand: 2 definitions (ALPHA Line, ALPHA
/// Dot), 3 rules over 2 keys (AA* twice, AB* once).
pub const ALPHA: &str = r#"{
  "format": "katana-customisation", "version": 1, "name": "Alpha",
  "description": "The first stand-in",
  "linestyles": [{"name": "ALPHA Line", "strokes": [["move", 0, 0], ["draw", 1, 0]]}],
  "symbols": [{"name": "ALPHA Dot", "atVertices": true, "strokes": [["circle", 0.5]]}],
  "codes": [
    {"key": "AA*", "sets": "feature", "layer": "ALPHA A", "linestyle": "ALPHA Line"},
    {"key": "AA*", "sets": "symbol", "symbol": {"name": "ALPHA Dot"}},
    {"key": "AB*", "sets": "feature", "layer": "ALPHA B"}
  ]
}"#;

/// The second stand-in: 1 definition (BETA Line), 1 rule over 1 key (BB*).
pub const BETA: &str = r#"{
  "format": "katana-customisation", "version": 1, "name": "Beta Two",
  "description": "The second stand-in",
  "linestyles": [{"name": "BETA Line", "strokes": [["move", 0, 0], ["draw", 2, 0]]}],
  "codes": [{"key": "BB*", "sets": "feature", "layer": "BETA", "linestyle": "BETA Line"}]
}"#;

/// The two stand-ins as the entries of a list a host offers.
pub fn two_entries() -> Vec<BuiltInEntry> {
    vec![
        entry_from(ALPHA, "alpha.customisation.json"),
        entry_from(BETA, "beta.customisation.json"),
    ]
}

/// A built-in standing in for the compiled-in one. This build may have none, and the rule of the
/// seam is about whatever a build has: with a stand-in, the tests of that rule are the same tests,
/// and can fail, on every machine.
pub fn stand_in() -> BuiltInCustomisation {
    built_in_from(
        r#"{"format": "katana-customisation", "version": 1, "name": "Stand In",
 "codes": [{"key": "SI*", "sets": "feature", "layer": "STAND IN"}]}"#,
    )
}

/// The text of a path, as core's path_to_utf8 gives it and a message carries it.
pub fn path_text(path: &std::path::Path) -> String {
    katana_core::text_encoding::utf8_or_windows1252(katana_core::path_text::path_to_utf8(path))
}

/// Whether two built-ins hold the very same customisation, shared (the C++'s comparison of two
/// shared_ptr): both none, or one object.
pub fn same(left: &Option<Arc<Customisation>>, right: &Option<Arc<Customisation>>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        _ => false,
    }
}
