//! The one piece of a project's surface state that the door cannot reach with
//! the input it guards against (src/katana_cad/terrain/surface_document_io.hpp;
//! docs/terrain.md, "Surfaces in the drawing"). Private to katana_cad and its
//! tests.
//!
//! Document::set_surface_styles refuses a style set the JSON writer cannot
//! carry on the way in, and a set read on open passed the same check, so the
//! set a save meets always passes it: the guard in the save is defence for the
//! day a path forgets, and no test could hand it the set it refuses through
//! the Document. It is a function of plain data so that one can.

use katana_core::Result;

use crate::terrain::surface_style::{
    SurfaceStyleSet, built_in_surface_styles, check_surface_style_set, surface_styles_to_json,
};

/// The value a project's metadata keeps the surface styles under (the
/// Document's write_surface_state): empty for the built-in set, else the set's
/// JSON. A set check_surface_style_set refuses is that refusal, with the style
/// or field named, and the save is a Status, never a panic and never a
/// rewritten name.
pub fn surface_styles_metadata_value(styles: &SurfaceStyleSet) -> Result<String> {
    // The set is one check_surface_style_set accepted on the way in, so this
    // is a guard and not an expected refusal. It is here because a save
    // promises a Status and the JSON writer must never be what ends it.
    check_surface_style_set(styles)?;
    Ok(if *styles == built_in_surface_styles() {
        String::new()
    } else {
        surface_styles_to_json(styles)
    })
}
