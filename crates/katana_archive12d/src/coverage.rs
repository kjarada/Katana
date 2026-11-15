//! Every element of the archive format, and what Katana does with it.
//!
//! "Make sure all elements are accounted for" is only a checkable claim if the
//! list of elements is written down somewhere a test can reach. This is that
//! list: one row per element the V15 manual defines, in the manual's order,
//! with its section. The coverage tests write a minimal instance of each row,
//! read it, and fail if the reader reports the block as unrecognised - so a
//! row cannot be added here without the reader learning the element, and the
//! table in docs/interop.md is generated from the same rows rather than
//! maintained beside them.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handling {
    /// Read, imported into the domain model, and written on export.
    Full,
    /// Read and imported; export has no source for it.
    ImportOnly,
    /// Read into the Archive; the domain model has nowhere to put it.
    ReadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElementCoverage {
    /// As it appears in a file: "string super", "full_tin".
    pub keyword: &'static str,
    pub manual_section: &'static str,
    pub handling: Handling,
    /// What it is in Katana, or why it is not.
    pub becomes: &'static str,
}

const fn row(
    keyword: &'static str,
    manual_section: &'static str,
    handling: Handling,
    becomes: &'static str,
) -> ElementCoverage {
    ElementCoverage {
        keyword,
        manual_section,
        handling,
        becomes,
    }
}

// In the manual's order. "becomes" is the sentence docs/interop.md prints.
static ROWS: [ElementCoverage; 30] = [
    row(
        "model",
        "1.4.1",
        Handling::Full,
        "a layer; a tree name such as Stage 1/Water arrives as that nested layer",
    ),
    row(
        "colour",
        "1.4.2",
        Handling::Full,
        "the entity's colour where the name is one of the standard colours; the name is always kept",
    ),
    row(
        "style",
        "1.4.3",
        Handling::Full,
        "kept on the entity as 12d.style and written back",
    ),
    row(
        "breakline",
        "1.4.4",
        Handling::Full,
        "kept on the entity as 12d.breakline and written back",
    ),
    row(
        "null",
        "1.4.5",
        Handling::Full,
        "a height equal to the null value, or the null keyword, is no height at all",
    ),
    row(
        "attributes",
        "1.3",
        Handling::Full,
        "typed entity properties; a group flattens into Group/Name and is rebuilt on export",
    ),
    row(
        "project_attributes",
        "-",
        Handling::ReadOnly,
        "read into the archive; a Katana project has no attributes of its own",
    ),
    row(
        "drafting_element/leader",
        "-",
        Handling::ImportOnly,
        "a native leader with its note, arrow, landing and explicit paper sizes",
    ),
    row(
        "tin",
        "1.4.7.2",
        Handling::Full,
        "a surface (terrain::TinSurface)",
    ),
    row(
        "full_tin",
        "1.4.7.1",
        Handling::ImportOnly,
        "a surface of its visible, non-construction triangles; written back as a tin",
    ),
    row(
        "super_tin",
        "1.4.8",
        Handling::ReadOnly,
        "reported; its member tins are what is imported",
    ),
    row(
        "primitive_3d",
        "1.4.9",
        Handling::Full,
        "a mesh in the session (geometry::TriangleMesh), drawn in 3D and as its footprint in plan; written back with its per-face colours",
    ),
    row(
        "string arc",
        "1.5.1",
        Handling::ImportOnly,
        "an Arc; exported arcs are written as two-vertex super strings",
    ),
    row("string circle", "1.5.2", Handling::Full, "a Circle"),
    row(
        "string drainage",
        "1.5.3",
        Handling::ImportOnly,
        "the line as a Polyline carrying its pipes, each pit and house connection as a Point",
    ),
    row(
        "string face",
        "1.5.4",
        Handling::ImportOnly,
        "a closed or open Polyline",
    ),
    row("string feature", "1.5.5", Handling::ImportOnly, "a Circle"),
    row(
        "string interface",
        "1.5.6",
        Handling::ImportOnly,
        "a Polyline",
    ),
    row(
        "string plot_frame",
        "1.5.7",
        Handling::ImportOnly,
        "the sheet rectangle, paper size times plot scale, as a closed Polyline",
    ),
    row(
        "string super",
        "1.5.8",
        Handling::Full,
        "a Point, Line or Polyline; arcs and transitions chorded to a stated tolerance",
    ),
    row(
        "string super_alignment",
        "1.5.9",
        Handling::Full,
        "a named Alignment where its geometry is PI-definable and verifies, and always a Polyline of the centreline",
    ),
    row("string text", "1.5.10", Handling::Full, "Text"),
    row(
        "string 2d",
        "1.5.11",
        Handling::ImportOnly,
        "as string super",
    ),
    row(
        "string 3d",
        "1.5.12",
        Handling::ImportOnly,
        "as string super",
    ),
    row(
        "string 4d",
        "1.5.13",
        Handling::ImportOnly,
        "as string super, with its vertex text as Text",
    ),
    row(
        "string pipe",
        "1.5.14",
        Handling::ImportOnly,
        "as string super, with its diameter",
    ),
    row(
        "string polyline",
        "1.5.15",
        Handling::ImportOnly,
        "as string super",
    ),
    row(
        "string alignment",
        "1.5.16",
        Handling::ImportOnly,
        "as string super_alignment",
    ),
    row(
        "string pipeline",
        "1.5.17",
        Handling::ImportOnly,
        "as string super_alignment",
    ),
    row(
        "string las_cloud_data",
        "1.5.18",
        Handling::ImportOnly,
        "a point cloud reference layer; all eleven point record formats, tagged and compact",
    ),
];

pub fn element_coverage() -> &'static [ElementCoverage] {
    &ROWS
}

pub fn to_string(handling: Handling) -> &'static str {
    match handling {
        Handling::Full => "import and export",
        Handling::ImportOnly => "import",
        Handling::ReadOnly => "read",
    }
}
