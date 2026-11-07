//! The named TIN surfaces of a drawing. The Document holds the one store, so a
//! verb that reads or makes a surface - the geoprocessing bindings' SURFACE
//! <name> (docs/geoprocessing.md), the surface edits, the volumes, a leader
//! reading a level off a surface - finds it by the same name on every front
//! end. A change is made on a copy and committed as one undo step, and the
//! store is saved with the project (docs/terrain.md, "Surfaces in the drawing").
//!
//! A surface is shared, immutable, so a background job can read one while the
//! window draws it, and a copy of the store - what an undo step keeps - costs
//! a pointer per surface. A change to the store is a new revision, which is
//! how a view knows to look again without being told what changed.

use std::sync::Arc;

use katana_core::text::equals_ignoring_case;
use katana_core::text_encoding::{is_valid_utf8, utf8_or_windows1252};
use katana_core::{Error, ErrorCode, Status};

use crate::tin_surface::TinSurface;

#[derive(Clone, Debug)]
pub struct NamedSurface {
    pub name: String,
    pub surface: Option<Arc<TinSurface>>,
    /// Where it came from, for a person: "point cloud site.laz", "raster
    /// dem.tif", "drawing (12 entities)".
    pub source: String,
    /// The display style it is drawn in, by name; empty for the drawing's
    /// default style.
    pub style: String,
    /// Whether the views draw it. A hidden surface is still sampled, measured
    /// and exported: hiding is a matter of the picture, not of the data.
    pub visible: bool,
}

impl Default for NamedSurface {
    fn default() -> Self {
        Self {
            name: String::new(),
            surface: None,
            source: String::new(),
            style: String::new(),
            visible: true,
        }
    }
}

/// As the C++'s defaulted comparison: the surface by identity (a shared
/// pointer compares its address), the rest by value.
impl PartialEq for NamedSurface {
    fn eq(&self, other: &Self) -> bool {
        let same_surface = match (&self.surface, &other.surface) {
            (Some(first), Some(second)) => Arc::ptr_eq(first, second),
            (None, None) => true,
            _ => false,
        };
        self.name == other.name
            && same_surface
            && self.source == other.source
            && self.style == other.style
            && self.visible == other.visible
    }
}

/// Free text a surface carries - its source and the style it names - is
/// refused when it is not UTF-8, for the reason the name is (check_name). The
/// text is not echoed: it is the bytes that are wrong, and a message holding
/// them reaches the same JSON writers.
fn check_text(field: &str, text: &[u8]) -> Status {
    if is_valid_utf8(text) {
        return Ok(());
    }
    Err(Error::new(
        ErrorCode::InvalidArgument,
        format!("a surface's {field} is not valid UTF-8"),
    )
    .with_context(format!("field={field}")))
}

fn same_name(first: &str, second: &str) -> bool {
    equals_ignoring_case(first.as_bytes(), second.as_bytes())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceStore {
    surfaces: Vec<NamedSurface>,
    revision: u64,
}

impl SurfaceStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `name` can be a surface's name - InvalidArgument, with a sentence
    /// that does not echo the name, when it cannot:
    ///   - empty;
    ///   - not UTF-8. Project storage and the JSON replies write the name, so
    ///     the name is refused where it enters, as a layer's or an alignment's is;
    ///   - holding a double quote or a line break. A command line names a
    ///     surface in quotes and cannot carry either (the tokenizer has no
    ///     escape), so such a surface could be listed and never addressed again.
    ///
    /// add and rename apply it, so every verb and dialog that names a surface
    /// does, with no check of its own.
    pub fn check_name(name: &[u8]) -> Status {
        if name.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a surface needs a name",
            ));
        }
        check_text("name", name)?;
        if name
            .iter()
            .any(|&byte| matches!(byte, b'"' | b'\r' | b'\n'))
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a surface's name holds a double quote or a line break, which a command line \
                 cannot name",
            )
            .with_context("field=name"));
        }
        Ok(())
    }

    /// `name` made into one check_name accepts, for a name that is not the
    /// person's to choose - read out of a file, or out of a project an earlier
    /// build wrote: bytes that are not UTF-8 are read as Windows-1252, a double
    /// quote becomes an apostrophe and a line break a blank. Text that needs
    /// none of it comes back unchanged. unique_name does it for the importers.
    pub fn nameable(name: &[u8]) -> String {
        utf8_or_windows1252(name)
            .chars()
            .map(|character| match character {
                '"' => '\'',
                '\r' | '\n' => ' ',
                other => other,
            })
            .collect()
    }

    /// AlreadyExists for a name in use, compared case-insensitively, since a
    /// person types SURFACE Ground as readily as SURFACE ground;
    /// InvalidArgument for a name check_name refuses, or no surface.
    pub fn add(&mut self, surface: NamedSurface) -> Status {
        Self::check_name(surface.name.as_bytes())?;
        check_text("source", surface.source.as_bytes())?;
        check_text("style", surface.style.as_bytes())?;
        if surface.surface.is_none() {
            return Err(Error::new(ErrorCode::InvalidArgument, "no surface to keep")
                .with_context(surface.name));
        }
        if self.find(&surface.name).is_some() {
            return Err(
                Error::new(ErrorCode::AlreadyExists, "a surface already has that name")
                    .with_context(surface.name),
            );
        }
        self.surfaces.push(surface);
        self.revision += 1;
        Ok(())
    }

    /// Case-insensitive; None when there is none of that name.
    pub fn find(&self, name: &str) -> Option<&NamedSurface> {
        self.surfaces
            .iter()
            .find(|surface| same_name(&surface.name, name))
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.surfaces
            .iter()
            .position(|surface| same_name(&surface.name, name))
    }

    /// False when there was none of that name.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.surfaces.len();
        self.surfaces
            .retain(|surface| !same_name(&surface.name, name));
        if self.surfaces.len() == before {
            return false;
        }
        self.revision += 1;
        true
    }

    /// The surface of `name` replaced by `surface`, keeping its place, name,
    /// source, style and visibility - what an edit of a surface commits.
    /// NotFound for no surface of that name; InvalidArgument for no surface.
    pub fn replace(&mut self, name: &str, surface: Option<Arc<TinSurface>>) -> Status {
        let Some(surface) = surface else {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "no surface to keep").with_context(name)
            );
        };
        let Some(at) = self.position(name) else {
            return Err(
                Error::new(ErrorCode::NotFound, "no surface of that name").with_context(name)
            );
        };
        self.surfaces[at].surface = Some(surface);
        self.revision += 1;
        Ok(())
    }

    /// NotFound for no surface of `name`; AlreadyExists when `new_name` is
    /// another surface's (a change of case alone is allowed); InvalidArgument
    /// for a new name check_name refuses.
    pub fn rename(&mut self, name: &str, new_name: &str) -> Status {
        Self::check_name(new_name.as_bytes())?;
        let Some(at) = self.position(name) else {
            return Err(
                Error::new(ErrorCode::NotFound, "no surface of that name").with_context(name)
            );
        };
        if self.position(new_name).is_some_and(|other| other != at) {
            return Err(
                Error::new(ErrorCode::AlreadyExists, "a surface already has that name")
                    .with_context(new_name),
            );
        }
        self.surfaces[at].name = String::from(new_name);
        self.revision += 1;
        Ok(())
    }

    /// NotFound for no surface of that name. Whether the style exists is the
    /// Document's to check: the store does not know the styles.
    pub fn set_style(&mut self, name: &str, style: &str) -> Status {
        check_text("style", style.as_bytes())?;
        let Some(at) = self.position(name) else {
            return Err(
                Error::new(ErrorCode::NotFound, "no surface of that name").with_context(name)
            );
        };
        self.surfaces[at].style = String::from(style);
        self.revision += 1;
        Ok(())
    }

    pub fn set_visible(&mut self, name: &str, visible: bool) -> Status {
        let Some(at) = self.position(name) else {
            return Err(
                Error::new(ErrorCode::NotFound, "no surface of that name").with_context(name)
            );
        };
        self.surfaces[at].visible = visible;
        self.revision += 1;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.surfaces.clear();
        self.revision += 1;
    }

    /// In the order they were added.
    pub fn all(&self) -> &Vec<NamedSurface> {
        &self.surfaces
    }
    pub fn empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    /// Bumped by every change, including a clear of an empty store: what tells
    /// a staging copy it was changed. A view keys on the Document's surfaces
    /// generation instead - an undo puts back a store with the revision it had
    /// then.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// `base` (made nameable), or "base (2)", "base (3)" ...: the first no
    /// surface has. What a front end names an import whose name is taken, as the
    /// session names a second alignment of the same name.
    pub fn unique_name(&self, base: &[u8]) -> String {
        // Made nameable first: the name an importer reads out of a file is not the
        // person's to choose, and refusing the whole import for a quote in a TIN's
        // name would be the wrong answer to it.
        let stem = if base.is_empty() {
            String::from("surface")
        } else {
            Self::nameable(base)
        };
        let mut name = stem.clone();
        let mut copy = 2;
        while self.find(&name).is_some() {
            name = format!("{stem} ({copy})");
            copy += 1;
        }
        name
    }
}
