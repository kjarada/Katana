//! Editing a project's sheets (docs/plotting.md;
//! include/katana/cad/plotting/sheet_commands.hpp). Every function here is ONE
//! undoable step: it builds the new sheet set from the document's and hands it
//! to Document::set_sheet_set, which records the set before and after. So undo
//! and redo are exact, a failed edit changes nothing, and there is one door
//! in - the same arrangement as every other edit of the drawing.
//!
//! The C++'s default step names are the constants below.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use katana_core::civil_date::YearMonthDay;
use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result, Status};

use crate::document::Document;
use crate::plotting::generators::prepare_for_append;
use crate::plotting::sheet_set::{FieldContext, Sheet, SheetSet, Viewport};

/// The step names the C++ gives by default.
pub const ADD_SHEETS: &str = "ADD_SHEETS";
pub const EDIT_SHEET: &str = "EDIT_SHEET";
pub const EDIT_VIEWPORT: &str = "EDIT_VIEWPORT";
pub const EDIT_SHEETS: &str = "EDIT_SHEETS";

/// The largest logo file accepted: a logo is drawn 53 x 12 mm, which at 600 dpi
/// is 1250 x 283 pixels - a few hundred kilobytes at most. A bigger file is a
/// photograph, and would ride along in every PDF of the set.
pub const MAXIMUM_LOGO_BYTES: u64 = 4 * 1024 * 1024;

fn out_of_range(index: usize, count: usize) -> Error {
    make_error(
        ErrorCode::NotFound,
        "no sheet at that position",
        format!("{index} of {count}"),
    )
}

// A path as the C++'s `path::string()` writes it into a message.
fn path_text(path: &Path) -> String {
    utf8_or_windows1252(path_to_utf8(path))
}

// The C++'s `std::error_code::message()` for a failed file operation: the
// system's own sentence, which Rust's io::Error shows followed by
// " (os error N)" (as storage's project_store.rs reads it).
fn system_message(error: &std::io::Error) -> String {
    let text = format!("{error}");
    if let Some(code) = error.raw_os_error()
        && let Some(sentence) = text.strip_suffix(&format!(" (os error {code})"))
    {
        return sentence.to_owned();
    }
    text
}

// The image formats a logo may be, told by their first bytes rather than by
// the file's extension, and the extension it is stored under.
fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(".png");
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Some(".jpg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(".gif");
    }
    if bytes.starts_with(b"BM") {
        return Some(".bmp");
    }
    None
}

// The source file's name reduced to letters, digits, '-' and '_', so the
// stored name is safe on every file system a project travels to. Byte by
// byte, as the C++ reads its narrow string.
fn safe_stem(image: &Path) -> String {
    let stem: String = image
        .file_stem()
        .map(|stem| path_to_utf8(Path::new(stem)))
        .unwrap_or_default()
        .iter()
        .map(|&byte| {
            if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
                char::from(byte)
            } else {
                '_'
            }
        })
        .collect();
    if stem.is_empty() {
        String::from("logo")
    } else {
        stem
    }
}

fn read_all(path: &Path) -> Result<Vec<u8>> {
    let Ok(mut file) = File::open(path) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "the image cannot be opened",
            path_text(path),
        ));
    };
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the image cannot be read",
            path_text(path),
        ));
    }
    Ok(bytes)
}

// A copy of the document's sheet set, to edit and hand back.
fn sheet_set_of(document: &Document) -> SheetSet {
    SheetSet::clone(&document.sheet_set())
}

/// Appends `sheets` - typically a generator's output - renumbering their ids
/// so they join the set without clashing (prepare_for_append).
pub fn add_sheets(document: &mut Document, mut sheets: Vec<Sheet>, step_name: &str) -> Status {
    let mut set = sheet_set_of(document);
    prepare_for_append(&set, &mut sheets);
    set.sheets.extend(sheets);
    document.set_sheet_set(&set, step_name)
}

/// Inserts one sheet before `at` (at the end when not given).
pub fn add_sheet(document: &mut Document, sheet: Sheet, at: Option<usize>) -> Status {
    let mut set = sheet_set_of(document);
    let position = at.unwrap_or(set.sheets.len());
    if position > set.sheets.len() {
        return Err(out_of_range(position, set.sheets.len()));
    }
    let mut one = vec![sheet];
    prepare_for_append(&set, &mut one);
    set.sheets.insert(position, one.remove(0));
    document.set_sheet_set(&set, "ADD_SHEET")
}

/// Removes a sheet. The last one can go too: an empty set is a valid set.
pub fn remove_sheet(document: &mut Document, index: usize) -> Status {
    let mut set = sheet_set_of(document);
    if index >= set.sheets.len() {
        return Err(out_of_range(index, set.sheets.len()));
    }
    set.sheets.remove(index);
    document.set_sheet_set(&set, "REMOVE_SHEET")
}

/// Moves the sheet at `from` so that it ends up at `to`.
pub fn move_sheet(document: &mut Document, from: usize, to: usize) -> Status {
    let mut set = sheet_set_of(document);
    if from >= set.sheets.len() {
        return Err(out_of_range(from, set.sheets.len()));
    }
    if to >= set.sheets.len() {
        return Err(out_of_range(to, set.sheets.len()));
    }
    let moving = set.sheets.remove(from);
    set.sheets.insert(to, moving);
    document.set_sheet_set(&set, "MOVE_SHEET")
}

/// A deep copy placed after the original, named "<name> (copy)", with new
/// sheet and viewport ids and none of the original's per-sheet sheet number.
pub fn duplicate_sheet(document: &mut Document, index: usize) -> Status {
    let mut set = sheet_set_of(document);
    if index >= set.sheets.len() {
        return Err(out_of_range(index, set.sheets.len()));
    }
    let mut copy = set.sheets[index].clone();
    copy.name.push_str(" (copy)");
    // A typed sheet number belongs to the sheet it was typed on; two sheets
    // saying the same number is exactly what automatic numbering prevents.
    copy.fields.remove("sheet_number");
    copy.id.clear();
    for viewport in &mut copy.viewports {
        viewport.id.clear();
    }
    let mut one = vec![copy];
    prepare_for_append(&set, &mut one);
    set.sheets.insert(index + 1, one.remove(0));
    document.set_sheet_set(&set, "DUPLICATE_SHEET")
}

/// Applies `edit` to a copy of sheet `index` and stores the result as one
/// step. The edit may refuse by returning an error; nothing changes then.
pub fn edit_sheet(
    document: &mut Document,
    index: usize,
    edit: impl FnOnce(&mut Sheet) -> Status,
    step_name: &str,
) -> Status {
    let mut set = sheet_set_of(document);
    let count = set.sheets.len();
    let Some(sheet) = set.sheets.get_mut(index) else {
        return Err(out_of_range(index, count));
    };
    edit(sheet)?;
    document.set_sheet_set(&set, step_name)
}

/// The same for the viewport with id `viewport_id`, wherever it is.
pub fn edit_viewport(
    document: &mut Document,
    viewport_id: &str,
    edit: impl FnOnce(&mut Viewport) -> Status,
    step_name: &str,
) -> Status {
    let mut set = sheet_set_of(document);
    let found = set
        .sheets
        .iter_mut()
        .flat_map(|sheet| sheet.viewports.iter_mut())
        .find(|viewport| viewport.id == viewport_id);
    let Some(viewport) = found else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no viewport with that id",
            viewport_id,
        ));
    };
    edit(viewport)?;
    document.set_sheet_set(&set, step_name)
}

/// The same for the whole set: the shared title-block values, the numbering,
/// the revisions.
pub fn edit_sheet_set(
    document: &mut Document,
    edit: impl FnOnce(&mut SheetSet) -> Status,
    step_name: &str,
) -> Status {
    let mut set = sheet_set_of(document);
    edit(&mut set)?;
    document.set_sheet_set(&set, step_name)
}

/// Copies `image` (PNG, JPEG, GIF or BMP, recognised by its content, at most
/// MAXIMUM_LOGO_BYTES) into the project's assets/ directory and makes it the
/// set's logo, as one undoable step; returns the name it is stored under. The
/// file stays in assets/ after an undo - it is the name that is undone - so a
/// redo finds it. InvalidState for a drawing not saved to a project yet (the
/// logo is kept with the project); Unsupported for another kind of file.
pub fn import_logo(document: &mut Document, image: &Path) -> Result<String> {
    let name = import_image_asset(document, image, MAXIMUM_LOGO_BYTES, "logo")?;
    edit_sheet_set(
        document,
        |set| {
            set.defaults.logo_asset = name.clone();
            Ok(())
        },
        "SET_LOGO",
    )?;
    Ok(name)
}

/// The copy import_logo makes, for any image a sheet shows (an Image view's
/// file): `image` checked as above against `maximum_bytes` and copied into the
/// project's assets/ directory under a name of its own, the same image found
/// and reused; returns that name. `what` names the image in the errors ("a
/// logo must be..."). Not an edit: nothing refers to the file until the caller
/// stores its name, as one undoable step. The C++ defaults the last two to
/// MAXIMUM_LOGO_BYTES and "logo".
pub fn import_image_asset(
    document: &Document,
    image: &Path,
    maximum_bytes: u64,
    what: &str,
) -> Result<String> {
    let Some(project) = document.project_directory() else {
        return Err(Error::new(
            ErrorCode::InvalidState,
            format!(
                "save the drawing as a project first: the {what} is kept in the project's \
                 assets folder"
            ),
        ));
    };
    // "a logo", "an image".
    let article = if what.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    };
    // file_size: an error for a path that is not there or not a regular file.
    let size = match std::fs::metadata(image) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        _ => {
            return Err(make_error(
                ErrorCode::NotFound,
                "the image cannot be found",
                path_text(image),
            ));
        }
    };
    if size > maximum_bytes {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the image is too large for {article} {what} (at most {} MB)",
                maximum_bytes / (1024 * 1024)
            ),
            format!("{}: {size} bytes", path_text(image)),
        ));
    }
    let bytes = read_all(image)?;
    let Some(extension) = image_extension(&bytes) else {
        return Err(make_error(
            ErrorCode::Unsupported,
            format!("{article} {what} must be a PNG, JPEG, GIF or BMP image"),
            path_text(image),
        ));
    };
    let assets = project.join("assets");
    if let Err(error) = std::fs::create_dir_all(&assets) {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the project's assets folder cannot be made",
            format!("{}: {}", path_text(&assets), system_message(&error)),
        ));
    }
    // A name of its own, never overwriting another file: an undo may bring
    // back the logo a later import would otherwise have replaced. The same
    // image imported twice is found and reused.
    let stem = safe_stem(image);
    let mut attempt = 1;
    loop {
        let name = if attempt == 1 {
            format!("{stem}{extension}")
        } else {
            format!("{stem}-{attempt}{extension}")
        };
        let target = assets.join(&name);
        if !target.exists() {
            if std::fs::write(&target, &bytes).is_err() {
                return Err(make_error(
                    ErrorCode::FileExportFailure,
                    format!("the {what} cannot be written"),
                    path_text(&target),
                ));
            }
            return Ok(name);
        }
        if let Ok(existing) = read_all(&target)
            && existing == bytes
        {
            return Ok(name);
        }
        attempt += 1;
    }
}

/// Where the set's logo is on disk, when it has one and the project is saved.
pub fn logo_path(document: &Document) -> Option<PathBuf> {
    let set = document.sheet_set();
    let name = &set.defaults.logo_asset;
    let project = document.project_directory()?;
    if name.is_empty() {
        return None;
    }
    Some(project.join("assets").join(name))
}

/// The project's contribution to the title-block fields: its name,
/// description, coordinate system and file name, with `plot_date` as given.
pub fn field_context_for(document: &Document, plot_date: String) -> FieldContext {
    let metadata = document.metadata();
    let file_name = match document.project_directory() {
        Some(project) => project
            .file_name()
            .map(|name| path_text(Path::new(name)))
            .unwrap_or_default(),
        None => metadata.name.clone(),
    };
    FieldContext {
        project_name: metadata.name.clone(),
        project_description: metadata.description.clone(),
        coordinate_system: metadata.coordinate_system.clone(),
        file_name,
        plot_date,
    }
}

/// A date as the frame prints one: dd/mm/yy.
pub fn frame_date(date: YearMonthDay) -> String {
    format!("{:02}/{:02}/{:02}", date.day, date.month, date.year % 100)
}
