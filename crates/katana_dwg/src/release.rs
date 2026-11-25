//! The DWG releases: which are read, which are written, and the words that name them
//! (docs/dwg.md, "Releases").

use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};

/// A release a DWG is written as: R2000 to R2018, the six CADCraft's bridge writes (docs/dwg.md,
/// "Releases"). Oldest first, so `<` is "older than".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DwgRelease {
    R2000,
    R2004,
    R2007,
    R2010,
    R2013,
    /// The default: the newest release, written by every AutoCAD since 2018, acadrust's own
    /// default and CADCraft's.
    #[default]
    R2018,
}

impl DwgRelease {
    pub const DEFAULT: DwgRelease = DwgRelease::R2018;

    /// Every release written, oldest first.
    pub const ALL: [DwgRelease; 6] = [
        DwgRelease::R2000,
        DwgRelease::R2004,
        DwgRelease::R2007,
        DwgRelease::R2010,
        DwgRelease::R2013,
        DwgRelease::R2018,
    ];

    /// "R2018": the word a line gives it, and what a reply calls it.
    pub fn name(self) -> &'static str {
        match self {
            DwgRelease::R2000 => "R2000",
            DwgRelease::R2004 => "R2004",
            DwgRelease::R2007 => "R2007",
            DwgRelease::R2010 => "R2010",
            DwgRelease::R2013 => "R2013",
            DwgRelease::R2018 => "R2018",
        }
    }

    /// "AC1032": the version string the file begins with, the DXF's $ACADVER.
    pub fn code(self) -> &'static str {
        match self {
            DwgRelease::R2000 => "AC1015",
            DwgRelease::R2004 => "AC1018",
            DwgRelease::R2007 => "AC1021",
            DwgRelease::R2010 => "AC1024",
            DwgRelease::R2013 => "AC1027",
            DwgRelease::R2018 => "AC1032",
        }
    }

    /// From R2004 a DWG holds a colour that no index is (true colour); R2000 holds the indices
    /// alone.
    pub fn has_true_colour(self) -> bool {
        self >= DwgRelease::R2004
    }

    /// From R2007 a DWG's strings are UTF-16; before, they are in the drawing's code page and an
    /// extended data string is held to 255 bytes.
    pub fn is_unicode(self) -> bool {
        self >= DwgRelease::R2007
    }

    /// The release a word names: "R2018", "2018" or "AC1032", in any letter case. R12, R13 and
    /// R14 are refused by name (Unsupported: read, never written); any other word is
    /// InvalidArgument, the releases written listed.
    pub fn parse(word: &str) -> Result<DwgRelease> {
        let folded = word.trim().to_ascii_uppercase();
        for release in DwgRelease::ALL {
            let year = &release.name()[1..];
            if folded == release.name() || folded == year || folded == release.code() {
                return Ok(release);
            }
        }
        for (code, name) in OLDER_THAN_WRITTEN {
            if folded == code || folded == name {
                return Err(make_error(
                    ErrorCode::Unsupported,
                    format!(
                        "{name} DWG ({code}) is not written: Katana reads it, and writes R2000 \
                         (AC1015) to R2018 (AC1032)"
                    ),
                    word,
                ));
            }
        }
        let names: Vec<&str> = DwgRelease::ALL.iter().map(|release| release.name()).collect();
        Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "release= is a DWG release Katana writes: {} (or its year, or its version \
                 string such as AC1032)",
                names.join(", ")
            ),
            word,
        ))
    }
}

/// The releases read and not written, by version string and name.
const OLDER_THAN_WRITTEN: [(&str, &str); 3] =
    [("AC1009", "R12"), ("AC1012", "R13"), ("AC1014", "R14")];

/// Every release read, by the version string the file begins with: each one acadrust reads
/// (its io/dwg/dwg_version.rs, `DwgVersion::from_version_string`) - R12, which is R11's format
/// too, and R13 to R2018. Their names are katana_dxf's (`katana_dxf::release_name`), the same
/// version strings a DXF's $ACADVER holds.
pub(crate) const READ: [&str; 9] = [
    "AC1009", "AC1012", "AC1014", "AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032",
];
