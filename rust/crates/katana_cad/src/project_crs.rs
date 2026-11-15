//! The project's coordinate system: what it is, how it is set, and which
//! systems suit a place (docs/cad.md, "The project's coordinate system";
//! include/katana/cad/project_crs.hpp, src/katana_cad/project_crs.cpp).
//!
//! A project keeps its coordinate system as text in its metadata (storage's
//! ProjectMetadata::coordinate_system): "EPSG:7856", or WKT or a PROJ string
//! for a system with no code, or nothing at all for local coordinates.
//! Everything that meets the outside world needs it - GIS > Online Data has
//! nowhere to put a web map without it, GIS imports reproject into it, the
//! sheets print it in the title block - so it is set here, once, through ONE
//! undoable step (Document::set_coordinate_system), from the File menu, the
//! status bar, the online data dialog or the command line (CRS ...).
//!
//! What is accepted is anything PROJ accepts (geodesy's
//! CoordinateReferenceSystem::from_user_input): an EPSG code with or without
//! its "EPSG:", WKT, a PROJ string. It is stored as "EPSG:<code>" whenever the
//! system has one, so two spellings of one system - "epsg:7856", "7856", its
//! WKT - are one value.
//!
//! Document::set_coordinate_system is kept here with the rest of it, as the
//! sheet set's members are kept with the sheets.

use std::sync::OnceLock;

use katana_commands::CommandPtr;
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_geodesy::{CoordinateReferenceSystem, CrsKind, GeographicExtent};

use crate::document::Document;

/// A system as a list offers it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CrsChoice {
    pub id: String,    // "EPSG:7856"
    pub name: String,  // "GDA2020 / MGA zone 56", as the EPSG register names it
    pub group: String, // "Australia - GDA2020 MGA", for a list's headings
}

/// What a system is, for a label or a reply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CrsDescription {
    pub id: String,    // how the project stores it: "EPSG:7856", or the text given
    pub name: String,  // "GDA2020 / MGA zone 56"
    pub kind: String,  // "projected", "geographic 2D", ...
    pub units: String, // "metre", "degree", ...
    pub projected: bool,
    pub area_of_use: Option<GeographicExtent>,
}

/// The C++'s local trim: blanks, tabs and line ends at either end, and no
/// other control character.
fn trimmed(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\r', '\n'])
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// PROJ reads "7856" as nothing; a person typing a code means EPSG's.
fn as_proj_input(text: &str) -> String {
    let text = trimmed(text);
    if all_digits(text) {
        format!("EPSG:{text}")
    } else {
        String::from(text)
    }
}

fn parse(text: &str) -> Result<CoordinateReferenceSystem> {
    let input = as_proj_input(text);
    if input.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidCrs,
            "no coordinate system was given",
        ));
    }
    match CoordinateReferenceSystem::from_user_input(&input) {
        Ok(crs) => Ok(crs),
        // PROJ's database would not open (geodesy's ProjContext::create): the
        // installation, not the text, is what is wrong, and "give an EPSG
        // code" blamed a valid one. Said as it is, with what was asked.
        Err(error) if error.code == ErrorCode::NotFound => Err(make_error(
            ErrorCode::NotFound,
            error.message,
            format!("{input}; {}", error.context),
        )),
        Err(error) => {
            // The text, then what PROJ made of it: "not a coordinate system"
            // alone left a person to guess whether the text or the program was
            // wrong (a WKT that had lost its quotes looked like any other).
            let mut reason = error.message;
            if !error.context.is_empty() {
                reason.push_str(&format!(" ({})", error.context));
            }
            Err(make_error(
                ErrorCode::InvalidCrs,
                "not a coordinate system: give an EPSG code (EPSG:7856), WKT or a PROJ string",
                format!("{input}; {reason}"),
            ))
        }
    }
}

/// The kind as a reply writes it: plain lower-case words, whatever spelling
/// geodesy's own to_string uses for its logs.
fn kind_words(kind: CrsKind) -> &'static str {
    match kind {
        CrsKind::Geographic2D => "geographic 2D",
        CrsKind::Geographic3D => "geographic 3D",
        CrsKind::Geocentric => "geocentric",
        CrsKind::Projected => "projected",
        CrsKind::Vertical => "vertical",
        CrsKind::Compound => "compound",
        CrsKind::LocalEngineering => "local engineering",
        CrsKind::Other => "other",
    }
}

fn choice(code: i32, name: String, group: &str) -> CrsChoice {
    CrsChoice {
        id: format!("EPSG:{code}"),
        name,
        group: String::from(group),
    }
}

/// The zone a longitude falls in: 6 degrees each, zone 1 from 180 W. The
/// longitude is a checked place, so the floor is a small whole number and the
/// cast is exact.
fn utm_zone(longitude: f64) -> i32 {
    let zone = ((longitude + 180.0) / 6.0).floor() as i32 + 1;
    zone.clamp(1, 60)
}

/// `text` read as a coordinate system and returned as the project stores it:
/// "EPSG:<code>" when it has one, else the text itself, trimmed. Empty text is
/// empty: local coordinates. InvalidCRS for text that names no system.
pub fn normalise_coordinate_system(text: &str) -> Result<String> {
    let input = trimmed(text);
    if input.is_empty() {
        return Ok(String::new());
    }
    let crs = parse(input)?;
    if let Some(code) = crs.epsg_code() {
        return Ok(format!("EPSG:{code}"));
    }
    Ok(String::from(input))
}

/// What `text` is. InvalidCRS for empty text or text that names no system.
pub fn describe_coordinate_system(text: &str) -> Result<CrsDescription> {
    let crs = parse(text)?;
    let id = normalise_coordinate_system(text)?;
    Ok(CrsDescription {
        id,
        name: String::from(crs.name()),
        kind: String::from(kind_words(crs.kind())),
        units: String::from(crs.horizontal_unit_name()),
        projected: crs.is_projected(),
        area_of_use: crs.area_of_use(),
    })
}

/// The systems a survey or civil project is usually in, grouped: the MGA
/// zones of GDA2020 and GDA94 (49 to 56, the mainland), the national Albers
/// and the geographic systems of both, WGS 84 and its UTM zones, Web
/// Mercator, and New Zealand's and Great Britain's national grids. Any other
/// is typed. Built once.
pub fn common_coordinate_systems() -> &'static [CrsChoice] {
    static LIST: OnceLock<Vec<CrsChoice>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut out = Vec::new();
        let gda2020 = "Australia - GDA2020 MGA";
        for zone in 49..=56 {
            out.push(choice(
                7800 + zone,
                format!("GDA2020 / MGA zone {zone}"),
                gda2020,
            ));
        }
        let gda94 = "Australia - GDA94 MGA";
        for zone in 49..=56 {
            out.push(choice(
                28300 + zone,
                format!("GDA94 / MGA zone {zone}"),
                gda94,
            ));
        }
        let national = "Australia - national";
        out.push(choice(
            9473,
            String::from("GDA2020 / Australian Albers"),
            national,
        ));
        out.push(choice(
            3577,
            String::from("GDA94 / Australian Albers"),
            national,
        ));
        out.push(choice(7844, String::from("GDA2020"), national));
        out.push(choice(4283, String::from("GDA94"), national));
        let world = "World";
        out.push(choice(4326, String::from("WGS 84"), world));
        out.push(choice(
            3857,
            String::from("WGS 84 / Pseudo-Mercator"),
            world,
        ));
        out.push(choice(
            2193,
            String::from("NZGD2000 / New Zealand Transverse Mercator 2000"),
            "New Zealand",
        ));
        out.push(choice(
            27700,
            String::from("OSGB36 / British National Grid"),
            "Great Britain",
        ));
        for zone in 1..=60 {
            out.push(choice(
                32600 + zone,
                format!("WGS 84 / UTM zone {zone}N"),
                "World - WGS 84 UTM north",
            ));
        }
        for zone in 1..=60 {
            out.push(choice(
                32700 + zone,
                format!("WGS 84 / UTM zone {zone}S"),
                "World - WGS 84 UTM south",
            ));
        }
        out
    })
}

/// The common systems whose id, name or group holds every word of `words`,
/// case-insensitive, in list order. Every one for no words.
pub fn find_coordinate_systems(words: &str) -> Vec<CrsChoice> {
    let mut wanted: Vec<String> = Vec::new();
    let mut word = String::new();
    for character in words.to_ascii_lowercase().chars() {
        if character == ' ' || character == '\t' || character == ',' {
            if !word.is_empty() {
                wanted.push(std::mem::take(&mut word));
            }
        } else {
            word.push(character);
        }
    }
    if !word.is_empty() {
        wanted.push(word);
    }
    let mut found = Vec::new();
    for entry in common_coordinate_systems() {
        let haystack = format!("{} {} {}", entry.id, entry.name, entry.group).to_ascii_lowercase();
        if wanted
            .iter()
            .all(|needle| haystack.contains(needle.as_str()))
        {
            found.push(entry.clone());
        }
    }
    found
}

/// The systems that suit a place, best first: in and around Australia the
/// place's GDA2020 MGA zone, then its GDA94 MGA zone; everywhere the WGS 84
/// UTM zone (north or south); and last WGS 84 itself. InvalidArgument for a
/// longitude outside -180..180 or a latitude outside -90..90. Near the poles,
/// where no UTM zone is defined (beyond 84 N and 80 S), WGS 84 alone.
pub fn suggest_coordinate_systems(longitude: f64, latitude: f64) -> Result<Vec<CrsChoice>> {
    if !longitude.is_finite()
        || !latitude.is_finite()
        || !(-180.0..=180.0).contains(&longitude)
        || !(-90.0..=90.0).contains(&latitude)
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a place is a longitude from -180 to 180 and a latitude from -90 to 90",
        ));
    }
    let mut out = Vec::new();
    let zone = utm_zone(longitude);
    // Australia and its waters, where MGA is what a survey is in. GDA2020's
    // MGA runs from zone 46 to 59, GDA94's from 48 to 58.
    let australia = (-60.0..=-8.0).contains(&latitude) && longitude >= 90.0;
    if australia && (46..=59).contains(&zone) {
        out.push(choice(
            7800 + zone,
            format!("GDA2020 / MGA zone {zone}"),
            "Australia - GDA2020 MGA",
        ));
    }
    if australia && (48..=58).contains(&zone) {
        out.push(choice(
            28300 + zone,
            format!("GDA94 / MGA zone {zone}"),
            "Australia - GDA94 MGA",
        ));
    }
    // UTM is defined from 80 S to 84 N.
    if (-80.0..=84.0).contains(&latitude) {
        let north = latitude >= 0.0;
        let base = if north { 32600 } else { 32700 };
        out.push(choice(
            base + zone,
            format!("WGS 84 / UTM zone {zone}{}", if north { "N" } else { "S" }),
            if north {
                "World - WGS 84 UTM north"
            } else {
                "World - WGS 84 UTM south"
            },
        ));
    }
    out.push(choice(4326, String::from("WGS 84"), "World"));
    Ok(out)
}

impl Document {
    /// The project's coordinate system, set as ONE undoable step named
    /// `step_name` (the C++ defaults it to Document::SET_CRS): an EPSG code,
    /// WKT or a PROJ string, stored as "EPSG:<code>" when the system has one;
    /// empty text for local coordinates. InvalidCRS for text that names no
    /// system, and then nothing changes; setting the value it already has is
    /// no step at all.
    pub fn set_coordinate_system(&mut self, text: &str, step_name: impl Into<String>) -> Status {
        match self.coordinate_system_command(text, step_name)? {
            None => Ok(()), // nothing changed, and nothing to undo
            Some(command) => self.execute(command),
        }
    }

    /// The same change as a command, for a step that does more besides - an
    /// IMPORT that adopts the file's coordinate system with its entities is
    /// ONE undo step (docs/interop.md, "Import options"). None when the system
    /// is already `text`'s; InvalidCRS as set_coordinate_system. Making it
    /// changes nothing; running it through execute() does.
    pub fn coordinate_system_command(
        &self,
        text: &str,
        step_name: impl Into<String>,
    ) -> Result<Option<CommandPtr>> {
        let stored = normalise_coordinate_system(text)?;
        if stored == self.state.metadata.coordinate_system {
            return Ok(None);
        }
        // The value before and after, so undo and redo are exact: the C++'s
        // ProjectValueCommand.
        let before = self.state.metadata.coordinate_system.clone();
        Ok(Some(self.state_command(
            step_name,
            move |state, to_after| {
                state.metadata.coordinate_system = if to_after {
                    stored.clone()
                } else {
                    before.clone()
                };
            },
        )))
    }
}
