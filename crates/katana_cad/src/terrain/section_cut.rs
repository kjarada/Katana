//! Cutting a section of the DRAWING: its shown surfaces along an alignment,
//! with a design profile beside them when there is one (docs/terrain.md,
//! "Hidden is a matter of the picture"; docs/cad.md, "One section routine for
//! both callers"; include/katana/cad/terrain/section_cut.hpp).
//!
//! A section is a picture of the surfaces that were shown when it was cut.
//! This is the one place that decides which those are and at what interval
//! they are sampled, so Terrain > Cut Section and the re-cut of an open
//! section view, when the drawing's surfaces change, cannot differ in either.
//! Headless, so that the recipe a view keeps - the section's own alignment and
//! its design profile - is tested without a window.

use katana_core::{Error, ErrorCode, Result};
use katana_geometry::Polyline2;

use crate::document::Document;
use crate::section::{
    Section, SectionDesign, SectionOptions, SectionSurfaceInput, append_design_profile,
    extract_section,
};

/// Stations across a section's alignment at the least, and the finest
/// interval between them: two thousand across, floored at 100 mm. Fine enough
/// to read off, and the surface breaks are added on top so the interval never
/// decides the accuracy of a ridge or a toe. The floor is the section's own: a
/// finer interval over a short line costs time and changes nothing a person
/// can read.
pub const SECTION_STATIONS: f64 = 2000.0;
pub const LEAST_SECTION_INTERVAL: f64 = 0.1;

/// The interval a section along `length` of alignment is cut at:
/// max(length / SECTION_STATIONS, LEAST_SECTION_INTERVAL).
pub fn drawing_section_interval(length: f64) -> f64 {
    katana_math::cpp::max(length / SECTION_STATIONS, LEAST_SECTION_INTERVAL)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawingSection {
    pub section: Section,
    /// The interval the section was cut at, for the log line that reports it.
    pub interval: f64,
}

/// The drawing's shown surfaces cut along `alignment`, with `design` as one
/// more series when given. Hidden surfaces are left out: a section is a
/// picture, and hiding a surface takes it out of every picture.
///
/// Fails with NotFound when the drawing has no surface, InvalidState when
/// every surface of the drawing is hidden, and with extract_section's and
/// append_design_profile's own errors (InvalidArgument for an alignment of
/// fewer than two distinct vertices, an empty design name ...).
pub fn cut_drawing_section(
    document: &Document,
    alignment: &Polyline2,
    design: Option<&SectionDesign>,
) -> Result<DrawingSection> {
    let mut inputs = Vec::new();
    for named in document.surfaces().all() {
        if named.visible
            && let Some(surface) = &named.surface
        {
            inputs.push(SectionSurfaceInput {
                name: &named.name,
                surface: Some(surface),
            });
        }
    }
    if inputs.is_empty() {
        return Err(if document.surfaces().empty() {
            Error::new(
                ErrorCode::NotFound,
                "the drawing has no surface to cut a section of",
            )
        } else {
            Error::new(
                ErrorCode::InvalidState,
                "every surface of the drawing is hidden",
            )
        });
    }
    let options = SectionOptions {
        interval: drawing_section_interval(alignment.length()),
        ..SectionOptions::default()
    };
    let mut section = extract_section(alignment, &inputs, Some(document.model()), &options)?;
    if let Some(design) = design {
        append_design_profile(&mut section, &design.profile, design.name.clone())?;
    }
    Ok(DrawingSection {
        section,
        interval: options.interval,
    })
}
