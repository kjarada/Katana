//! Small pieces the survey code manager's tabs share (customisation/code_manager_support.hpp): how a
//! linestyle or symbol name a rule gives stands against the loaded library, a colour swatch, and
//! the colour-name field. The last section is shared with the symbol library too, since both
//! managers read and write Katana customisation files, through CUSTOMISE lines: the filter their
//! file dialogs offer, the name a customisation written from a session goes under, and a file as a
//! word of such a line.
//!
//! None of these needs the code manager itself (the plan's section 2: the C++'s include of
//! code_manager.hpp here was the cycle's only edge, and nothing of it is used): each is a free
//! function over the Document, so the code manager (CU2, wave 2) calls them and nothing here calls
//! it. The Qt combo-box helpers (makeColourField, refreshColourField, setColourField) are
//! `ColourField`, a named editable choice.

use std::path::Path;

use egui::{Response, Ui};
use katana_cad::Document;
use katana_cad::colour_lookup::resolve_colour;
use katana_cad::survey_coding::is_plain_linestyle;
use katana_core::error::make_error;
use katana_core::path_text::{generic_path_utf8, path_to_utf8};
use katana_core::text::lowered;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::Color;
use katana_entity::colour_names::{standard_colour, standard_colour_names};
use katana_entity::customisation::validate_customisation_name;
use katana_entity::tables::is_built_in_symbol_name;

use crate::canvas::{Color as CanvasColor, Image};
use crate::command_word::command_word;
use crate::drive::{Choice, choice};
use crate::script_runner::from_native_separators;

/// What a name in a rule draws as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefinitionState {
    /// A linestyle that is the plain continuous line: "0", "1", "continuous".
    Plain,
    /// The library defines it (as the right kind, for a linestyle).
    Defined,
    /// A symbol no library defines that Katana draws itself.
    BuiltIn,
    /// A linestyle naming a definition drawn at vertices: draws solid (D2).
    WrongKind,
    /// Nothing defines it.
    Undefined,
}

/// `document` may be None (the drawing has closed): then nothing is defined.
pub fn linestyle_state(document: Option<&Document>, name: &str) -> DefinitionState {
    if is_plain_linestyle(name) {
        return DefinitionState::Plain;
    }
    match document.and_then(|document| document.definition_for(name)) {
        None => DefinitionState::Undefined,
        Some(definition) if definition.at_vertices => DefinitionState::WrongKind,
        Some(_) => DefinitionState::Defined,
    }
}

pub fn symbol_state(document: Option<&Document>, name: &str) -> DefinitionState {
    if document.is_some_and(|document| document.definition_for(name).is_some()) {
        return DefinitionState::Defined;
    }
    if is_built_in_symbol_name(name) {
        DefinitionState::BuiltIn
    } else {
        DefinitionState::Undefined
    }
}

/// "TEST Water Main", "0 (plain line)", "Old Kerb (not defined)".
pub fn definition_label(name: &str, state: DefinitionState) -> String {
    match state {
        DefinitionState::Plain => format!("{name} (plain line)"),
        DefinitionState::Defined => String::from(name),
        DefinitionState::BuiltIn => format!("{name} (built in)"),
        DefinitionState::WrongKind => format!("{name} (an `at vertices` symbol)"),
        DefinitionState::Undefined => format!("{name} (not defined)"),
    }
}

/// The colour `name` means in `document`: its customisation's own names, then the standard ones
/// (cad's resolve_colour). With no document (the drawing has closed) the standard names, which need
/// none. None: neither knows it.
pub fn colour_of(document: Option<&Document>, name: &str) -> Option<Color> {
    match document {
        Some(document) => resolve_colour(document, name),
        None => standard_colour(name),
    }
}

/// The swatch's grey border: so white and black read on either ground.
const SWATCH_BORDER: CanvasColor = CanvasColor::rgb(128, 128, 128);

/// A filled square of `colour`, `size` pixels a side, with a thin border so white and black read on
/// either ground. QPainter's fillRect(1, 1, size - 2, size - 2) and a one-pixel drawRect(1, 1,
/// size - 3, size - 3), whose outline covers the pixels from 1 to size - 2 on each side, on a
/// transparent square.
pub fn colour_swatch(colour: &Color, size: u32) -> Image {
    let mut swatch = Image::new(size, size);
    swatch.fill(CanvasColor::TRANSPARENT);
    if size < 3 {
        return swatch;
    }
    let fill = CanvasColor::rgb(colour.r, colour.g, colour.b);
    let last = size - 2;
    for y in 1..=last {
        for x in 1..=last {
            let edge = x == 1 || y == 1 || x == last || y == last;
            swatch.set_pixel(x, y, if edge { SWATCH_BORDER } else { fill });
        }
    }
    swatch
}

/// The names a colour field offers: none, then the customisation's own - the names only this
/// session knows, which is why they come first - then the standard ones. Both from the tables that
/// resolve them, so the field offers exactly what is drawn and keeps no copy of either. A person may
/// still type any other name: a rule's "sui test water" is kept as written whether or not this
/// session defines it.
pub fn colour_names(document: Option<&Document>) -> Vec<String> {
    let mut names = vec![String::new()];
    if let Some(document) = document {
        for entry in document.customisation_state().colours.entries() {
            names.push(entry.name);
        }
    }
    names.extend(standard_colour_names());
    names
}

/// A colour-name field (makeColourField): editable, listing the names `colour_names` gives, and
/// taking any typed name as it is - never folded to a listed one's case, as a combo's own lookup
/// at the end of an edit would.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColourField {
    pub names: Vec<String>,
    pub chosen: Choice,
}

impl ColourField {
    pub fn new(document: Option<&Document>) -> Self {
        Self {
            names: colour_names(document),
            chosen: Choice::editable(""),
        }
    }

    /// refreshColourField: the names listed again - the customisation's colours have changed -
    /// keeping what the field shows: the rule in the form is as it was.
    pub fn refresh(&mut self, document: Option<&Document>) {
        let shown = self.text();
        self.names = colour_names(document);
        self.set(&shown);
    }

    /// setColourField: `name` shown, listed or not, without changing its case.
    pub fn set(&mut self, name: &str) {
        match self.names.iter().position(|listed| listed == name) {
            Some(row) => self.chosen.choose(row, &self.names),
            None => {
                self.chosen.index = None;
                self.chosen.text = String::from(name);
            }
        }
    }

    /// The name the field shows.
    pub fn text(&self) -> String {
        self.chosen.current_text(&self.names)
    }

    /// The field drawn as `name`.
    pub fn ui(&mut self, ui: &mut Ui, name: &str) -> Response {
        choice(ui, name, &self.names, &mut self.chosen)
    }
}

// ---- Katana customisation files (both managers) ------------------------------------------------

/// What a file dialog offers for one.
pub fn customisation_file_filter() -> &'static str {
    "Katana customisation (*.customisation.json *.json)"
}

/// The name a customisation written from this session goes under: the session's own. A session
/// nothing was loaded into has none, and a file must have one (validate_customisation_name), so it
/// is then the file's own name without `.customisation.json` or `.json` - "kerbs" for
/// kerbs.customisation.json - and "customisation" where even that is no name.
pub fn exported_customisation_name(document: Option<&Document>, file: &Path) -> String {
    if let Some(document) = document
        && !document.customisation_state().name.is_empty()
    {
        return document.customisation_state().name.clone();
    }
    let file_name = file.file_name().map(Path::new).unwrap_or(Path::new(""));
    let mut name = String::from_utf8_lossy(&path_to_utf8(file_name)).into_owned();
    for suffix in [".json", ".customisation"] {
        if name.len() > suffix.len() && lowered(name.as_bytes()).ends_with(suffix.as_bytes()) {
            name.truncate(name.len() - suffix.len());
        }
    }
    if validate_customisation_name(&name).is_ok() {
        name
    } else {
        String::from("customisation")
    }
}

/// `path` as one word of a CUSTOMISE line, in quotes and with '/'. A name with no directory is
/// given one ("./keep"): the family reads a line's whole FIRST word as a keyword, quotes or none -
/// the tokenizer takes them off - so a file called `keep` would run KEEP, and EXPORT refuses a file
/// called as one of its own words. Every bare name is, so that no list of the family's words is
/// kept in a dialog to go stale. What no line can carry - a double quote, a line break - is refused
/// by the one rule every dialog writes a word by (command_word).
pub fn customisation_file_word(path: &str) -> Result<String> {
    command_word(path, "the file's path")?;
    let bare = !path.contains('/') && !path.contains(':');
    Ok(format!("\"{}{path}\"", if bare { "./" } else { "" }))
}

/// `customisation_file_word` of a path, written generically (with '/').
pub fn customisation_file_word_of(path: &Path) -> Result<String> {
    customisation_file_word(&String::from_utf8_lossy(&generic_path_utf8(path)))
}

/// A path as a person typed it into a dialog's file box: with '/', as every dialog here writes one,
/// and without the blanks round it. ONE pair of double quotes round the WHOLE of it is taken off
/// too, with the blanks just inside them. That is how a file manager hands a path over (Explorer's
/// "Copy as path": "C:\data\site codes.json", quotes and all), and the quotes are no part of the
/// file's name; left on, the path was refused for holding a double quote. A quote anywhere else is
/// still customisation_file_word's to refuse.
pub fn typed_file_path(typed: &str) -> String {
    let mut path = typed.trim();
    if path.chars().count() >= 2 && path.starts_with('"') && path.ends_with('"') {
        path = path[1..path.len() - 1].trim();
    }
    from_native_separators(path)
}

/// The file a person typed, as one word of a CUSTOMISE line: as typed_file_path reads it, then
/// customisation_file_word. InvalidArgument when nothing is typed. Settings and the start screen
/// name a file by it.
pub fn typed_customisation_file_word(typed: &str) -> Result<String> {
    let path = typed_file_path(typed);
    if path.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "no file is named: type its path, or choose it with Browse",
            String::new(),
        ));
    }
    customisation_file_word(&path)
}

/// `bytes` as the whole of the file at `path`, for a file that is NOT a customisation (the code
/// list's CSV): a customisation file is written by CUSTOMISE EXPORT, which writes beside the file
/// and renames, so that a write that fails part way leaves what was there. FileExportFailure, with
/// the path beside it, when it cannot be opened or written.
pub fn write_file_bytes(path: &Path, bytes: &[u8]) -> Status {
    use std::io::Write;
    let shown = String::from_utf8_lossy(&path_to_utf8(path)).into_owned();
    let Ok(mut file) = std::fs::File::create(path) else {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "cannot open the file for writing",
            shown,
        ));
    };
    if file.write_all(bytes).and_then(|()| file.flush()).is_err() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file could not be written",
            shown,
        ));
    }
    Ok(())
}
