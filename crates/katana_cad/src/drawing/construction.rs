//! Construction lines and rays (docs/drawing.md, "Draw tools";
//! include/katana/cad/drawing/construction.hpp): drawing aids that run on for
//! ever, which the model holds as lines KILOMETRES long on the construction
//! layer, since it has no infinite line. The layer is the marker: an entity on
//! "construction", or on a layer beneath it, is drawn on screen and snapped
//! to, but never plotted and never counted in the drawing's extents - Zoom
//! Extents frames the drawing, not the 100 km reach of an aid.

pub const CONSTRUCTION_LAYER: &str = "construction";

/// How far a construction line or ray reaches from its base point, in model
/// units: past any drawing's extent, short of where coordinates lose their
/// millimetres (1e5 m at double precision still resolves 1e-11 m).
pub const CONSTRUCTION_REACH: f64 = 1.0e5;

/// True for the construction layer and every layer beneath it.
pub fn is_construction_layer(layer: &str) -> bool {
    layer == CONSTRUCTION_LAYER
        || (layer.len() > CONSTRUCTION_LAYER.len()
            && layer.starts_with(CONSTRUCTION_LAYER)
            && layer.as_bytes()[CONSTRUCTION_LAYER.len()] == b'/')
}
