//! The Document's surface members (document.rs; docs/terrain.md, "Surfaces in
//! the drawing"; src/katana_cad/terrain/surface_document.cpp): the drawing's
//! one store of named TIN surfaces and their display styles, each change ONE
//! undoable step, saved with the project. Kept here, beside the rest of the
//! surface code, as the sheets are kept with the sheets: document.rs only
//! calls in at new, open and save.

use std::sync::Arc;

use katana_commands::CommandPtr;
use katana_core::error::make_error;
use katana_core::text::equals_ignoring_case;
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{ErrorCode, Result, Status};
use katana_storage::{ProjectContents, ProjectMetadata, SurfaceRecord};
use katana_terrain::{NamedSurface, SurfaceStore, decode_tin, encode_tin};

use crate::document::{Document, DocumentChange};
use crate::terrain::surface_document_io::surface_styles_metadata_value;
use crate::terrain::surface_style::{
    SurfaceStyleSet, built_in_surface_styles, check_surface_style_set, surface_styles_from_json,
};

/// The metadata key the styles are kept under, as versioned JSON - the
/// sheets' arrangement: the storage layer carries a key it does not read, so
/// the styles need no schema change. Written by every save, empty for the
/// built-in set, so a set put back to the built-ins overwrites the row a
/// custom one left (the metadata table is never emptied: ProjectStore::save).
pub(crate) const SURFACE_STYLES_KEY: &str = "surface_styles";

/// The surfaces and their styles as one value: what open() reads out of a
/// project and checks before it installs anything, and what new_document()
/// installs empty (the C++ `Document::SurfaceState`).
pub(crate) struct SurfaceState {
    pub(crate) surfaces: SurfaceStore,
    pub(crate) styles: SurfaceStyleSet,
    pub(crate) styles_status: Status,
    /// What the read repaired, one sentence each (a surface name an earlier
    /// build wrote that this one's door refuses): logged by open().
    pub(crate) notes: Vec<String>,
}

impl Default for SurfaceState {
    fn default() -> Self {
        Self {
            surfaces: SurfaceStore::default(),
            styles: built_in_surface_styles(),
            styles_status: Ok(()),
            notes: Vec::new(),
        }
    }
}

fn same_surfaces(a: &SurfaceStore, b: &SurfaceStore) -> bool {
    // By what each holds - a surface by its pointer, since a surface is
    // immutable and an edit makes a new one - never by the revision, which
    // counts how a store came to be, not what it is.
    a.all() == b.all()
}

/// A surface names a style the styles do not have. Refused only for a name
/// NEW to the drawing: a surface that already named a style since lost (a
/// project whose styles a newer Katana wrote) is drawn in the default
/// (style_of) and must not block every later change of the surfaces.
fn check_style_names(next: &SurfaceStore, now: &SurfaceStore, styles: &SurfaceStyleSet) -> Status {
    for surface in next.all() {
        if surface.name.is_empty() || surface.surface.is_none() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a surface needs a name and a surface",
                surface.name.as_str(),
            ));
        }
        if surface.style.is_empty() || styles.find(&surface.style).is_some() {
            continue;
        }
        let named = now
            .all()
            .iter()
            .any(|before| equals_ignoring_case(before.style.as_bytes(), surface.style.as_bytes()));
        if !named {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "no surface style has that name; SURFACESTYLE LIST names them",
                format!("surface={} style={}", surface.name, surface.style),
            ));
        }
    }
    Ok(())
}

/// A style a surface uses, which `next` no longer has though `now` had it.
fn check_styles_in_use(
    next: &SurfaceStyleSet,
    now: &SurfaceStyleSet,
    surfaces: &SurfaceStore,
) -> Status {
    for surface in surfaces.all() {
        if surface.style.is_empty()
            || next.find(&surface.style).is_some()
            || now.find(&surface.style).is_none()
        {
            continue;
        }
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "surface {} is drawn in style {}; give it another style first",
                surface.name, surface.style
            ),
            format!("style={} surface={}", surface.style, surface.name),
        ));
    }
    Ok(())
}

/// The styles' state as a step keeps it: the set, whether the project's could
/// be read, and the key's text when it could not (kept in the metadata, to be
/// written back as it was).
struct StylesState {
    styles: SurfaceStyleSet,
    status: Status,
    kept: Option<String>,
}

impl Document {
    // ---- surfaces (docs/terrain.md, "Surfaces in the drawing") ------------------------
    //
    // The drawing's TIN surfaces, by name: made by SURFACE FROM, an import or
    // a corridor, edited by the surface edits, read by the volumes, the views
    // and a leader's surface values, and saved with the project. A change is
    // made on a COPY of the store - a copy costs a pointer per surface, every
    // surface being shared and immutable - and committed here as ONE undoable
    // step named `step_name`; undo puts the exact store before it back.
    // Committing a store equal to the current one records nothing. Cleared by
    // new_document(), replaced by open().

    pub fn surfaces(&self) -> &SurfaceStore {
        &self.state.surfaces
    }

    /// Counts every change to the surfaces, undo and redo included, and never
    /// goes back: a view compares it with the generation it last drew.
    pub fn surfaces_generation(&self) -> u64 {
        self.state.surfaces_generation
    }

    /// The display styles (terrain/surface_style.rs): the project's, or
    /// built_in_surface_styles() when it saved none. Replaced as ONE undoable
    /// step; check_surface_style refuses a style that cannot be drawn, and a
    /// style a surface uses cannot be removed (InvalidArgument naming it).
    pub fn surface_styles(&self) -> &SurfaceStyleSet {
        &self.state.surface_styles
    }

    /// Counts every change to the styles, undo and redo included, and never
    /// goes back: what a view's cache of a style's drawing keys on.
    pub fn surface_styles_generation(&self) -> u64 {
        self.state.surface_styles_generation
    }

    /// Why the project's styles could not be read when it was opened - a newer
    /// Katana's (Unsupported), or damaged (ParseFailure) - while the built-ins
    /// stand in for them; success otherwise. Styles a newer Katana wrote are
    /// kept as they were and written back by a save, and set_surface_styles
    /// refuses (CommandRejected) to overwrite them; damaged ones are replaced
    /// by the first set given.
    pub fn surface_styles_status(&self) -> Status {
        self.state.surface_styles_status.clone()
    }

    /// The same change as set_surfaces, as a command, for a step that does
    /// more besides - an IMPORT that brings entities and surfaces is ONE undo
    /// step. None when `surfaces` equals the current store. It builds the
    /// command and changes nothing until the command is executed through
    /// execute(), so a tool holding `&Document` can make one.
    pub fn surfaces_command(
        &self,
        surfaces: SurfaceStore,
        step_name: impl Into<String>,
    ) -> Result<Option<CommandPtr>> {
        check_style_names(&surfaces, &self.state.surfaces, &self.state.surface_styles)?;
        if same_surfaces(&surfaces, &self.state.surfaces) {
            return Ok(None);
        }
        // Both sides held by the step, so undo and redo swap them back and
        // forth without copying a surface.
        let states = Arc::new((self.state.surfaces.clone(), surfaces));
        Ok(Some(self.state_command(step_name, move |state, after| {
            state.surfaces = if after {
                states.1.clone()
            } else {
                states.0.clone()
            };
            state.surfaces_generation += 1;
        })))
    }

    /// InvalidArgument for a surface whose style is not in surface_styles().
    pub fn set_surfaces(&mut self, surfaces: SurfaceStore, step_name: impl Into<String>) -> Status {
        match self.surfaces_command(surfaces, step_name)? {
            None => Ok(()), // nothing changed, and nothing to undo
            Some(command) => self.execute(command),
        }
    }

    /// The surfaces and the styles as ONE undoable step, checked together:
    /// what renaming a style a surface uses needs, the style renamed in the
    /// set and in every surface that names it at once.
    pub fn set_surfaces_and_styles(
        &mut self,
        surfaces: SurfaceStore,
        styles: SurfaceStyleSet,
        step_name: impl Into<String>,
    ) -> Status {
        if let Err(error) = &self.state.surface_styles_status
            && error.code == ErrorCode::Unsupported
        {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "the surface styles were written by a newer version of Katana; they are kept as \
                 they are",
                error.describe(),
            ));
        }
        check_surface_style_set(&styles)?;
        // The surfaces are checked against the NEW styles: a rename gives a
        // surface a style name only the new set has.
        check_style_names(&surfaces, &self.state.surfaces, &styles)?;
        check_styles_in_use(&styles, &self.state.surface_styles, &surfaces)?;
        let surfaces_move = !same_surfaces(&surfaces, &self.state.surfaces);
        let styles_move =
            !(styles == self.state.surface_styles) || self.state.surface_styles_status.is_err();
        if !surfaces_move && !styles_move {
            return Ok(());
        }
        let before = StylesState {
            styles: self.state.surface_styles.clone(),
            status: self.state.surface_styles_status.clone(),
            kept: self
                .state
                .metadata
                .unknown_keys
                .get(SURFACE_STYLES_KEY)
                .cloned(),
        };
        let after = StylesState {
            styles,
            status: Ok(()),
            kept: None,
        };
        let surface_states = (self.state.surfaces.clone(), surfaces);
        let command = self.state_command(step_name, move |state, to_after| {
            if surfaces_move {
                state.surfaces = if to_after {
                    surface_states.1.clone()
                } else {
                    surface_states.0.clone()
                };
                state.surfaces_generation += 1;
            }
            if styles_move {
                let chosen = if to_after { &after } else { &before };
                state.surface_styles = chosen.styles.clone();
                state.surface_styles_status = chosen.status.clone();
                match &chosen.kept {
                    Some(text) => {
                        state
                            .metadata
                            .unknown_keys
                            .insert(String::from(SURFACE_STYLES_KEY), text.clone());
                    }
                    None => {
                        state.metadata.unknown_keys.remove(SURFACE_STYLES_KEY);
                    }
                }
                state.surface_styles_generation += 1;
            }
        });
        self.execute(command)
    }

    pub fn set_surface_styles(
        &mut self,
        styles: SurfaceStyleSet,
        step_name: impl Into<String>,
    ) -> Status {
        let surfaces = self.state.surfaces.clone();
        self.set_surfaces_and_styles(surfaces, styles, step_name)
    }

    // ---- new, open and save -----------------------------------------------------------

    /// Decodes the project's surface records and reads its styles' metadata
    /// key, taking the key out of `metadata` when it was read (it is the
    /// Document's to write) and leaving it when it could not be (written back
    /// as it was). Fails, and the open with it, on a surface it cannot decode.
    pub(crate) fn read_surface_state(
        records: &mut [SurfaceRecord],
        metadata: &mut ProjectMetadata,
    ) -> Result<SurfaceState> {
        let mut state = SurfaceState::default();
        for record in records.iter_mut() {
            let surface = decode_tin(&record.data).map_err(|error| {
                make_error(
                    error.code,
                    format!(
                        "surface {} in the project cannot be read: {}",
                        String::from_utf8_lossy(&record.name),
                        error.message
                    ),
                    error.context,
                )
            })?;
            // The bytes are done with once decoded: a project's surfaces can
            // be tens of megabytes, held twice for no reason otherwise.
            record.data = Vec::new();
            let raw_name = std::mem::take(&mut record.name);
            // A name this build's door (SurfaceStore::check_name) would refuse
            // but an earlier build let through - the bytes of a katana_cli on
            // Windows typed in the ANSI code page, or a double quote from an
            // importer - is repaired rather than refusing the open: the person
            // could not otherwise reach the project, nor rename the surface,
            // because a line cannot name it. The repair is logged, and visible
            // in SURFACE LIST.
            let name = if SurfaceStore::check_name(&raw_name).is_ok() {
                // check_name accepts UTF-8 alone.
                String::from_utf8(raw_name)
                    .unwrap_or_else(|refused| utf8_or_windows1252(refused.into_bytes()))
            } else {
                let repaired = state.surfaces.unique_name(&raw_name);
                state
                    .notes
                    .push(format!("surface name repaired on open: \"{repaired}\""));
                repaired
            };
            let named = NamedSurface {
                name,
                surface: Some(Arc::new(surface)),
                // Free text, not a key: read as the Windows-1252 it was when
                // it is not UTF-8, so the project's own JSON replies and its
                // next save cannot fail on it.
                source: utf8_or_windows1252(&record.source),
                style: utf8_or_windows1252(&record.style),
                visible: record.visible,
            };
            state.surfaces.add(named).map_err(|error| {
                make_error(
                    error.code,
                    format!("the project's surfaces cannot be read: {}", error.message),
                    error.context,
                )
            })?;
        }
        let Some(text) = metadata.unknown_keys.get(SURFACE_STYLES_KEY) else {
            return Ok(state); // the built-ins: a project saved before there were styles
        };
        if text.is_empty() {
            metadata.unknown_keys.remove(SURFACE_STYLES_KEY); // the built-ins, said so by a save
            return Ok(state);
        }
        match surface_styles_from_json(text) {
            // The built-ins stand in, and the key stays to be written back as
            // it was: a newer Katana's styles are not this build's to lose,
            // and a damaged set is not repaired by guessing.
            Err(error) => state.styles_status = Err(error),
            Ok(styles) => {
                state.styles = styles;
                metadata.unknown_keys.remove(SURFACE_STYLES_KEY);
            }
        }
        Ok(state)
    }

    pub(crate) fn install_surface_state(&mut self, state: SurfaceState) {
        self.state.surfaces = state.surfaces;
        self.state.surfaces_generation += 1;
        self.state.surface_styles = state.styles;
        self.state.surface_styles_status = state.styles_status;
        self.state.surface_styles_generation += 1;
    }

    /// The surfaces and the styles into what a save writes. A Status, for the
    /// save that carries it: the style set's JSON must never be what ends it.
    pub(crate) fn write_surface_state(&self, contents: &mut ProjectContents) -> Status {
        contents.surfaces.reserve(self.state.surfaces.all().len());
        for named in self.state.surfaces.all() {
            // The store refuses a surface without one (SurfaceStore::add,
            // replace), so none is passed over here.
            let Some(surface) = &named.surface else {
                continue;
            };
            contents.surfaces.push(SurfaceRecord {
                name: named.name.clone().into_bytes(),
                source: named.source.clone().into_bytes(),
                style: named.style.clone().into_bytes(),
                visible: named.visible,
                data: encode_tin(surface),
            });
        }
        // Styles that could not be read are in the metadata still, and go
        // back as they came. Read ones are the Document's, written every time.
        if self.state.surface_styles_status.is_ok() {
            let value = surface_styles_metadata_value(&self.state.surface_styles)?;
            contents
                .metadata
                .unknown_keys
                .insert(String::from(SURFACE_STYLES_KEY), value);
        }
        Ok(())
    }

    /// DocumentChange::SURFACES and SURFACE_STYLES for what moved since the
    /// last call; a step's notification adds them.
    pub(crate) fn surface_parts_changed(&mut self) -> u32 {
        let state = &mut self.state;
        let mut parts = 0;
        if state.surfaces_generation != state.surfaces_notified {
            parts |= DocumentChange::SURFACES;
            state.surfaces_notified = state.surfaces_generation;
        }
        if state.surface_styles_generation != state.surface_styles_notified {
            parts |= DocumentChange::SURFACE_STYLES;
            state.surface_styles_notified = state.surface_styles_generation;
        }
        parts
    }
}
