//! Colour NAMES, and the colours they mean.
//!
//! A survey code rule, the symbol it gives and a pen inside a linestyle
//! definition all NAME their colour - "red", "dark grey", "sui test water" -
//! and so does every string of an archive file. Two things turn a name into a
//! colour: the standard names, which mean the same in every drawing, and a
//! customisation's own table for the names only it uses.
//!
//! They are in the entity layer because three layers resolve a name and may not
//! all see each other (tools/check_layering.cmake): `cad` codes a survey, the
//! archive reader imports an archive, the window paints a definition. The
//! standard names began as a table private to the archive reader, out of cad's
//! reach, so a front end had to hand cad a function to ask.
//!
//! ONE FOLD. A name is compared after fold_colour_name, and nothing compares a
//! colour name any other way: were the standard names and a customisation's
//! table folded differently ("gray" grey to one and unknown to the other), one
//! name would have two colours.

use std::collections::BTreeMap;

use katana_core::error::{Error, ErrorCode, Status, make_error};
use katana_core::text::is_ascii_space;

use crate::entity::Color;

struct NamedColour {
    name: &'static str, // already folded
    colour: Color,
}

const fn named(name: &'static str, r: u8, g: u8, b: u8) -> NamedColour {
    NamedColour {
        name,
        colour: Color::new(r, g, b, 255),
    }
}

// A customisation and an archive file carry colour NAMES; the RGB behind a
// name is defined by whoever wrote the file and can be redefined there, so no
// table here can be "right". These are the names such files use as standard,
// at the RGB the same names have in the X11 / CSS colour list - the nearest
// thing to a public definition of what "orange" or "brown" means. Two
// departures: X11's DarkGray (169) is LIGHTER than its Gray (128), an accident
// of history that would draw "dark grey" paler than "grey", so dark and light
// grey are set either side of grey instead; and "green" is X11's Lime
// (0, 255, 0), the pure primary every CAD palette means by it, not X11's
// half-bright Green.
static STANDARD_COLOURS: [NamedColour; 27] = [
    named("red", 255, 0, 0),
    named("green", 0, 255, 0),
    named("blue", 0, 0, 255),
    named("yellow", 255, 255, 0),
    named("cyan", 0, 255, 255),
    named("magenta", 255, 0, 255),
    named("white", 255, 255, 255),
    named("black", 0, 0, 0),
    named("grey", 128, 128, 128),
    named("orange", 255, 165, 0),
    named("brown", 165, 42, 42),
    named("purple", 128, 0, 128),
    named("pink", 255, 192, 203),
    named("violet", 238, 130, 238),
    named("dark red", 139, 0, 0),
    named("dark green", 0, 100, 0),
    named("dark blue", 0, 0, 139),
    named("dark cyan", 0, 139, 139),
    named("dark magenta", 139, 0, 139),
    named("dark orange", 255, 140, 0),
    named("dark grey", 64, 64, 64),
    named("light grey", 192, 192, 192),
    named("light blue", 173, 216, 230),
    named("light green", 144, 238, 144),
    named("light cyan", 224, 255, 255),
    named("light yellow", 255, 255, 224),
    named("light pink", 255, 182, 193),
];

/// The standard colour whose name is `folded`, a name already through
/// fold_colour_name.
fn standard_entry(folded: &str) -> Option<&'static NamedColour> {
    STANDARD_COLOURS.iter().find(|entry| entry.name == folded)
}

/// core::trimmed's blank: the six ASCII white-space characters.
fn is_blank(character: char) -> bool {
    u8::try_from(character).is_ok_and(is_ascii_space)
}

/// The form a colour name is compared in: ASCII letters in lower case, `_`
/// and `-` read as a blank, THEN no blanks at either end - so a separator at
/// an end goes as a blank there does, and "red_" is "red" - and "gray" as
/// "grey" where it is the whole name or its last word ("Dark_Gray" is "dark
/// grey"). Blanks inside a name are kept as they are: "dark  red", with two,
/// is not "dark red". Folding a folded name changes nothing, which is what
/// lets it be the key a table is kept by.
pub fn fold_colour_name(name: &str) -> String {
    let separated = name.to_ascii_lowercase().replace(['_', '-'], " ");
    // Trimmed AFTER the separators have become blanks, so that one at an end
    // goes as a blank there does and "red_" is "red". The other order, which
    // this had while it was the archive import's own, left "red " - a folded
    // name with a blank at its end, which folded again was "red": two answers
    // from one fold, and a table could then hold "red_" beside the standard
    // red it is another spelling of.
    let mut key = String::from(separated.trim_matches(is_blank));
    if key == "gray" {
        key = String::from("grey");
    } else if key.ends_with(" gray") {
        let start = key.len() - 4;
        key.replace_range(start.., "grey");
    }
    key
}

/// The colour of one of the 27 standard names, compared by fold_colour_name.
/// None for any other name: a colour outside the standard ones is defined by
/// a customisation (ColourTable below) or by nothing Katana has, and such a
/// name ("pen 025") leaves a colour alone rather than being guessed at.
pub fn standard_colour(name: &str) -> Option<Color> {
    standard_entry(&fold_colour_name(name)).map(|entry| entry.colour)
}

/// The 27 names, lower case, in the table's order: the plain names first, then
/// the dark and the light shades. For a colour field to list, so that it
/// offers nothing standard_colour would not draw and keeps no copy of the
/// table.
pub fn standard_colour_names() -> Vec<String> {
    STANDARD_COLOURS
        .iter()
        .map(|entry| String::from(entry.name))
        .collect()
}

/// The standard name nearest to `colour` by distance in RGB - what an archive
/// export writes for a colour that never had a name.
pub fn nearest_standard_colour(colour: Color) -> String {
    let mut best = STANDARD_COLOURS[0].name;
    let mut best_distance: i64 = -1;
    for entry in &STANDARD_COLOURS {
        let dr = i64::from(entry.colour.r) - i64::from(colour.r);
        let dg = i64::from(entry.colour.g) - i64::from(colour.g);
        let db = i64::from(entry.colour.b) - i64::from(colour.b);
        let distance = dr * dr + dg * dg + db * db;
        if best_distance < 0 || distance < best_distance {
            best_distance = distance;
            best = entry.name;
        }
    }
    String::from(best)
}

/// One name of a ColourTable (the C++ ColourTable::Entry).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColourTableEntry {
    pub name: String, // as written; it is found by its fold
    pub colour: Color,
}

/// A customisation's own colour names: "sui test power" is #FF7F00. A value
/// type, compared whole.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColourTable {
    // Keyed by the fold, which is the identity; the name as written is kept so
    // that it can be shown and written back as its author spelled it.
    entries: BTreeMap<String, ColourTableEntry>,
}

impl ColourTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fails with InvalidArgument for a name that is empty or nothing but
    /// blanks and separators; for a name whose fold another entry already
    /// has ("SUI Gas" beside "sui_gas": a lookup could not say which was
    /// meant); and for a name whose fold is a standard name. That last one
    /// keeps a standard name stable with or without a customisation,
    /// including a headless archive import with an empty palette. (The C++
    /// also refused a name that is not UTF-8; a String cannot be one.)
    pub fn add(&mut self, name: impl Into<String>, colour: Color) -> Status {
        let name = name.into();
        let folded = fold_colour_name(&name);
        // Blanks and separators fold away at the ends: a name made of nothing
        // else names nothing.
        if folded.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a colour name is empty",
            ));
        }
        if standard_entry(&folded).is_some() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "a standard colour name cannot be given another colour: \"{folded}\" means the same in every drawing"
                ),
                name,
            ));
        }
        if let Some(existing) = self.entries.get(&folded) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "two colour names read alike: names are compared without regard to case, and with '_' and '-' as a blank",
                format!("\"{}\" and \"{name}\"", existing.name),
            ));
        }
        self.entries
            .insert(folded, ColourTableEntry { name, colour });
        Ok(())
    }

    /// The colour of `name`, compared by fold_colour_name; None when the table
    /// does not have it.
    pub fn find(&self, name: &str) -> Option<Color> {
        self.entries
            .get(&fold_colour_name(name))
            .map(|entry| entry.colour)
    }

    /// Every entry, in the order of the folded names - the order a file lists
    /// them in, so that writing a table twice gives the same text.
    pub fn entries(&self) -> Vec<ColourTableEntry> {
        self.entries.values().cloned().collect()
    }

    pub fn size(&self) -> usize {
        self.entries.len()
    }

    pub fn empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// THE resolver of a colour name: the customisation's table, then the
/// standard names. None when neither knows the name, and the caller then
/// leaves the colour alone. (The table cannot hold a standard name, so the
/// order decides nothing today; it is stated so that it stays decided.)
pub fn resolve_colour(table: &ColourTable, name: &str) -> Option<Color> {
    if let Some(own) = table.find(name) {
        return Some(own);
    }
    standard_colour(name)
}
