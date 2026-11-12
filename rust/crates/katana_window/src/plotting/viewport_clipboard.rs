//! Viewports on the clipboard (plotting/viewport_clipboard.cpp; docs/plotting.md, "Editing on the
//! canvas"). A copy is a sheet set's JSON (katana_cad's sheet_json) holding one sheet of the copied
//! viewports: the form a project stores its sheets in, so a paste reads back exactly what was
//! copied - in the same project, in another project, or in another Katana running beside this one.
//!
//! THROUGH THE HOST'S CLIPBOARD, AS TEXT. The C++ put the JSON on the system clipboard under a
//! MIME type of its own (VIEWPORT_MIME_TYPE) and took "is there a copy" from the type being there.
//! The window's clipboard is the host's (host::Clipboard: text and an image), and the desktop's
//! (arboard) carries no type of an application's own, so the JSON travels as the clipboard's text
//! and "is there a copy" is whether the text is copied viewports: text that is not a sheet set is
//! someone else's, nothing to paste (NotFound, as the C++ said of a clipboard without the type).
//! A sheet set holding no viewports is ParseFailure, as the C++'s bytes were.

use katana_cad::plotting::Viewport;
use katana_cad::plotting::sheet_json::{sheet_set_from_json, sheet_set_to_json};
use katana_cad::plotting::{Sheet, SheetSet};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};

use crate::host::Clipboard;

/// The MIME type copied viewports travelled under in the C++ (kViewportMimeType): kept as the
/// name of the form, for a clipboard that one day carries types of its own.
pub const VIEWPORT_MIME_TYPE: &str = "application/x-katana-viewports";

/// `viewports` as the clipboard carries them (viewportsToClipboardBytes). InvalidArgument when
/// there are none, or for what JSON cannot hold (sheet_set_to_json).
pub fn viewports_to_clipboard_bytes(viewports: &[Viewport]) -> Result<Vec<u8>> {
    if viewports.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "no viewports to copy",
            String::new(),
        ));
    }
    let holder = SheetSet {
        sheets: vec![Sheet {
            id: String::from("clipboard"),
            viewports: viewports.to_vec(),
            ..Sheet::default()
        }],
        ..SheetSet::default()
    };
    sheet_set_to_json(&holder).map(String::into_bytes)
}

/// And back (viewportsFromClipboardBytes). ParseFailure for bytes that are not copied viewports.
pub fn viewports_from_clipboard_bytes(bytes: &[u8]) -> Result<Vec<Viewport>> {
    // Bytes that are not UTF-8 are no JSON, as nlohmann refuses them.
    let text = std::str::from_utf8(bytes).map_err(|_| {
        make_error(
            ErrorCode::ParseFailure,
            "the sheet set is not in the expected format",
            "the bytes are not UTF-8",
        )
    })?;
    let mut set = sheet_set_from_json(text)?;
    if set.sheets.len() != 1 || set.sheets[0].viewports.is_empty() {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "the clipboard holds no viewports",
            String::new(),
        ));
    }
    Ok(std::mem::take(&mut set.sheets[0].viewports))
}

/// Puts `viewports` on `clipboard` (copyViewportsToClipboard).
pub fn copy_viewports_to_clipboard(
    clipboard: &mut dyn Clipboard,
    viewports: &[Viewport],
) -> Status {
    let bytes = viewports_to_clipboard_bytes(viewports)?;
    let text = String::from_utf8(bytes).map_err(|_| {
        make_error(
            ErrorCode::Internal,
            "the sheet set's JSON is not UTF-8",
            String::new(),
        )
    })?;
    clipboard.set_text(&text)
}

/// Whether `clipboard` holds copied viewports: what enables Paste (SheetEditor::updateEditActions
/// asked whether the clipboard had the type).
pub fn clipboard_holds_viewports(clipboard: &mut dyn Clipboard) -> bool {
    clipboard
        .text()
        .is_some_and(|text| sheet_set_from_json(&text).is_ok())
}

/// The viewports on `clipboard` (viewportsOnClipboard). NotFound when it holds none - no text, or
/// text that is not a sheet set; ParseFailure for a sheet set that is not copied viewports.
pub fn viewports_on_clipboard(clipboard: &mut dyn Clipboard) -> Result<Vec<Viewport>> {
    let nothing = || {
        make_error(
            ErrorCode::NotFound,
            "nothing to paste: copy a view first",
            String::new(),
        )
    };
    let text = clipboard.text().ok_or_else(nothing)?;
    if sheet_set_from_json(&text).is_err() {
        return Err(nothing());
    }
    viewports_from_clipboard_bytes(text.as_bytes())
}
