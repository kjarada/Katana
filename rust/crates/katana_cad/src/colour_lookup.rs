//! What a colour NAME means in a drawing
//! (include/katana/cad/colour_lookup.hpp;
//! src/katana_cad/customisation/colour_lookup.cpp).
//!
//! A survey code rule, the symbol it gives and a pen inside a definition all
//! name their colour. ONE resolver answers for a Document: its customisation's
//! own table, then the standard names (entity's resolve_colour). It is the
//! Document's own and not a function a front end hands over, because a front
//! end's would not know a customisation's names ("sui test water"), which then
//! resolve nowhere at all.

use katana_entity::Color;

use crate::document::Document;

/// The colour `name` means in `document`; None for a name neither its
/// customisation nor the standard names know, and the caller then leaves the
/// colour alone.
pub fn resolve_colour(document: &Document, name: &str) -> Option<Color> {
    katana_entity::colour_names::resolve_colour(&document.customisation_state().colours, name)
}

/// resolve_colour as a function, in the shape a survey coding's colour_of and
/// a code table's colour lookup take. It asks `document` at each call, so it
/// follows a colour table installed after it was made - and it borrows the
/// Document, which it cannot outlive.
///
/// A caller that passes NO lookup still gets no colours: this is offered, not
/// applied behind anyone's back.
pub fn colour_lookup(document: &Document) -> impl Fn(&str) -> Option<Color> + '_ {
    move |name: &str| resolve_colour(document, name)
}
