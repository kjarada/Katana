//! An archive <-> Katana's domain model.
//!
//! IMPORT: WHERE EACH ELEMENT GOES.
//!
//! ```text
//! model                  a layer. Archive model names are already
//!                        `/`-separated tree names, which is exactly what a
//!                        Katana layer path is, so "Stage 1/Water/Drainage"
//!                        arrives as that nested layer.
//! string super (and      one vertex: a Point. Otherwise a Polyline, with
//! 2d 3d 4d pipe          arcs chorded to `curve_tolerance` - Polyline2 has
//! polyline face          no arc segments. Transitions are chorded too.
//! interface)
//! string arc             an Arc; `string circle` and `string feature` a
//!                        Circle.
//! string text, and the   Text entities.
//! vertex text of a 4d
//! or super string
//! string plot_frame      the sheet rectangle, as a closed Polyline.
//! string drainage        the line as a Polyline, each pit as a Point
//!                        carrying its name, type and size; pipes as
//!                        properties of the line.
//! string super_alignment a named Alignment when the geometry can be
//! alignment, pipeline    expressed by PIs (see below); always ALSO a
//!                        Polyline of the solved centreline, so nothing is
//!                        invisible when it cannot.
//! tin, full_tin          a katana_terrain::TinSurface of the visible
//!                        triangles.
//! super_tin              reported; the member tins are what is imported.
//! primitive_3d/trimesh   a mesh in the session (ImportedMesh), beside the
//!                        surfaces: not an entity.
//! string las_cloud_data  a list of points for the caller to make a
//!                        reference layer from (this layer cannot see the
//!                        point cloud types; interop can).
//! ```
//!
//! HEIGHTS. Entities are 2D. A string whose vertices share one height carries
//! it as the `elevation` property, which is what Surface From Drawing reads.
//! One whose heights differ carries them all in `elevations`, space
//! separated, in vertex order - a text property because the property model
//! has no list type, and written with enough digits to read back exactly.
//! Null heights are the word `null` in that list.
//!
//! STYLES. A library linestyle name becomes a Katana Style of that name,
//! which the entity carries as its style - the same field a drawn entity's
//! style lives in, so the style panel, the STYLE verb and the plotter all see
//! it. The importer lists the styles it needs (`styles_needed`) and the caller
//! creates the missing ones in the same transaction as the layers. A .12da
//! says nothing about what a linestyle looks like, so a new one is continuous
//! at the default weight, described as a library linestyle, for the user to
//! finish in the style manager.
//!
//! ATTRIBUTES become entity properties, typed as they were (integer, real,
//! text); a group flattens into its members' names, `Group/Sub/Name`. What the
//! archive knows about a string that Katana has no field for - its name,
//! colour NAME, chainage, breakline, point ids - goes in `metadata` under the
//! format's `12d.*` keys, and export reads it back from there, so a string
//! that is imported and exported unchanged is the string it was.
//!
//! ALIGNMENTS. Katana defines an alignment by its PIs and derives the
//! elements; an archive's super alignment stores the solved elements and,
//! separately, however the designer constructed them. Where the construction
//! is the IP method the PIs are taken directly. Otherwise they are
//! RECONSTRUCTED from the solved elements - each run of spiral/arc/spiral
//! between two straights is one PI at the intersection of those straights -
//! and the reconstruction is then CHECKED: the alignment is solved by Katana
//! and every vertex the archive recorded must lie within
//! `alignment_tolerance` of it. A compound or reverse curve, an alignment that
//! starts on an arc, or a transition Katana does not have (it has the
//! clothoid) fails that check or never reaches it, and the alignment is then
//! imported as its polyline only, with a warning that says why.

use katana_entity::{Alignment, Color, ColourTable, Entity, EntityId, Layer, PropertyMap, Style};
use katana_geometry::{Box2, TriangleMesh, Vec2};
use katana_terrain::TinSurface;

use crate::archive::{Archive, SuperTin};

// The two elevation keys are not this format's: they are conventions the
// whole application shares - the surface builder reads them and the survey
// import writes them - so they live in the entity layer and are used from
// there. The `12d.*` keys below are this format's own and stay.
pub use katana_entity::entity::{ELEVATION_PROPERTY, ELEVATIONS_PROPERTY};

// The standard colour names, under the names this module's import and export
// and the front ends have always called them by. They are the entity layer's
// (katana_entity::colour_names, which says what each returns): a
// customisation's own colours are resolved beside them there, by cad, which
// may not see this crate. An archive carries only the NAME of a colour, so a
// name that is not one of the 27 ("pen 025", "vis concrete") is left ByLayer
// rather than guessed at. The functions THEMSELVES, not copies.
pub use katana_entity::colour_names::{
    nearest_standard_colour, standard_colour, standard_colour_names,
};

// The heights import writes are read back with katana_entity::entity::heights_of,
// the one reader of them.

pub use crate::domain_common::{chorded_plan, layer_path_for_model, symbol_for_linestyle};
pub use crate::domain_export::{from_domain, split_list};
pub use crate::domain_import::to_domain;

// Property and metadata keys, in one place so import and export cannot
// disagree about them.
pub const META_NAME: &str = "12d.name";
/// Which vertex of its string a point came from, one-based. Set only where a
/// `breakline point` string of several vertices was split into one point per
/// vertex (see the import), so that they can be told apart and put back in
/// the order the string had them.
pub const META_VERTEX: &str = "12d.vertex";
pub const META_COLOUR: &str = "12d.colour";
pub const META_COLOUR_RGB: &str = "12d.colour_rgb";
pub const META_CHAINAGE: &str = "12d.chainage";
pub const META_BREAKLINE: &str = "12d.breakline";
pub const META_ELEMENT: &str = "12d.element";
pub const META_POINT_IDS: &str = "12d.point_ids";
pub const META_SOURCE: &str = "source";
/// Every scalar a string carried that Katana has no field for, as
/// `12d.x.<key>`; export writes them back as they were.
pub const META_EXTRA_PREFIX: &str = "12d.x.";
/// A vertex symbol (manual 1.5.8.4.10) is a linestyle drawn at a vertex, with
/// a colour, a size, a rotation, an offset and a raise. On a POINT it is the
/// point's whole appearance, so the point takes the symbol's linestyle as its
/// style (the Style carries the shape and the size) and the symbol's colour
/// as its colour; what remains of the block, and only where it is not the
/// default, is `12d.symbol.<key>`. The string's own linestyle, which the
/// format writes on a point beside the symbol's, is kept as
/// META_STRING_STYLE when the two differ so that export can put it back. On
/// a LINE the string's one symbol block works the same way: the line takes the
/// symbol's linestyle as its style, and the symbol is drawn at EVERY vertex,
/// as the format draws it (decision D8, cad::resolveLinePattern and
/// cad::symbolVertices); the style's linetype, being the symbol's own name,
/// draws a plain line under it. Only a string carrying a DIFFERENT symbol per
/// vertex, which the format allows and its program does not write, keeps
/// every block as `12d.symbol.<key>` holding one value per block, the way
/// segment colours are kept; that one is written back but not drawn, and the
/// import says so.
pub const META_SYMBOL_PREFIX: &str = "12d.symbol.";
/// What a text annotation says that Katana's TextGeometry has no field for,
/// as `12d.text.<key>`: the slant and width factor (Katana text has neither),
/// the justification and the sizing mode it was placed by, and an offset in
/// paper millimetres, which has no model-unit meaning without a plot scale.
/// Kept so that a text imported and exported unchanged is the text it was.
pub const META_TEXT_PREFIX: &str = "12d.text.";
pub const META_LEADER_PREFIX: &str = "12d.leader.";
pub const META_STRING_STYLE: &str = "12d.string_style";

#[derive(Clone, Debug, PartialEq)]
pub struct ImportOptions {
    /// Largest distance a chord may stand off the arc or transition it
    /// replaces, in model units. Stated rather than hidden: it is a lossy
    /// step.
    pub curve_tolerance: f64,
    /// How far a vertex the archive recorded may lie from the alignment Katana
    /// solves from the reconstructed PIs before the reconstruction is
    /// rejected. 10 mm: the format's default "clothoid" is a series
    /// approximation of the spiral Katana computes exactly, and the two differ
    /// by a few millimetres on a tight curve - while a wrongly reconstructed
    /// PI is out by metres.
    pub alignment_tolerance: f64,
    /// Prefixed to every layer, as a parent path: "survey" puts model "Kerb"
    /// on layer "survey/Kerb". Empty: models become top-level layers.
    pub layer_prefix: String,
    /// Subtracted from every plan coordinate. See interop's
    /// VectorImportOptions.
    pub origin_shift: Option<Vec2>,
    pub attributes_as_properties: bool,
    /// Written into every entity's metadata as `source`.
    pub source_name: String,
    /// The active customisation's palette; an empty table still resolves
    /// standard names.
    pub colours: ColourTable,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            curve_tolerance: 0.001,
            alignment_tolerance: 0.010,
            layer_prefix: String::new(),
            origin_shift: None,
            attributes_as_properties: true,
            source_name: String::new(),
            colours: ColourTable::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ImportedSurface {
    pub name: String,
    pub colour: String,
    pub surface: TinSurface,
    pub triangles_in_file: usize,
    /// Nulled, construction, or null-height.
    pub triangles_nulled: usize,
}

/// A `primitive_3d` (manual 1.4.9): a mesh of triangles that is NOT a
/// surface. It may be closed, may overhang, and the format's program writes
/// pipes, pits and structures as one. Held beside the surfaces in the session, for
/// the same reason and with the same consequence: it is not an entity and is
/// not undoable.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportedMesh {
    pub name: String,
    /// The archive model, as a layer path.
    pub layer: String,
    /// The archive's name for it, kept whatever Katana makes of it.
    pub colour_name: String,
    pub color: Option<Color>,
    pub mesh: TriangleMesh,
    /// One per face where the file colours its faces (`face_infos` +
    /// `face_flags`), otherwise empty. The name is kept beside the RGB so a
    /// colour Katana has no RGB for still goes back out as it came in.
    pub face_colour_names: Vec<String>,
    pub face_colors: Vec<Option<Color>>,
    pub properties: PropertyMap,
    /// Read, reported, and not modelled: an edge list is a drawing decision
    /// the format's program makes about ITS viewer, and per-vertex and
    /// per-edge infos likewise.
    pub edges_in_file: usize,
    pub vertex_infos_in_file: usize,
    pub edge_infos_in_file: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ImportedCloudPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub intensity: u16,
    pub classification: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportedCloud {
    pub name: String,
    pub points: Vec<ImportedCloudPoint>,
    /// A ref_data cloud: the LAS file it names.
    pub reference_file: String,
}

/// How many of each element the archive held, and what became of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ElementTally {
    /// "string super", "full_tin" ...
    pub keyword: String,
    pub read: usize,
    pub imported: usize,
}

#[derive(Clone, Debug, Default)]
pub struct DomainImport {
    pub entities: Vec<Entity>,
    /// Layers the entities reference, in first-use order, with the colour of
    /// the first string seen on each where that colour is one Katana knows.
    pub layers_needed: Vec<Layer>,
    /// Styles the entities reference, in first-use order: one per distinct
    /// library linestyle name. The caller creates those the drawing lacks.
    pub styles_needed: Vec<Style>,
    pub alignments: Vec<Alignment>,
    pub surfaces: Vec<ImportedSurface>,
    pub meshes: Vec<ImportedMesh>,
    pub clouds: Vec<ImportedCloud>,
    /// Super tins, as (name, member names): nothing to import, but a caller
    /// may want to say they were there.
    pub super_tins: Vec<SuperTin>,

    pub bounds: Box2,
    pub tally: Vec<ElementTally>,
    pub warnings: Vec<String>,
}

// ---- export ----------------------------------------------------------------

/// A surface to write back as a `tin`; None writes nothing.
#[derive(Clone, Debug, Default)]
pub struct ExportSurface<'a> {
    pub name: String,
    pub surface: Option<&'a TinSurface>,
}

/// A mesh to write back as a `primitive_3d`. The face colours, where given,
/// become the `face_infos` table and the one-based `face_flags` that index
/// it, which is how the format carries them.
#[derive(Clone, Debug, Default)]
pub struct ExportMesh<'a> {
    pub name: String,
    pub layer: String,
    pub colour_name: String,
    pub mesh: Option<&'a TriangleMesh>,
    pub face_colour_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportOptions {
    /// Empty exports the whole model; otherwise only these entities.
    pub entities: Vec<EntityId>,
    /// Added back to every plan coordinate, undoing an import's origin_shift.
    pub origin_shift: Option<Vec2>,
    pub properties_as_attributes: bool,
    pub include_alignments: bool,
    /// Chord tolerance for the solved centreline written with each alignment.
    pub curve_tolerance: f64,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            origin_shift: None,
            properties_as_attributes: true,
            include_alignments: true,
            curve_tolerance: 0.001,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DomainExport {
    pub archive: Archive,
    pub entities_written: usize,
    pub entities_skipped: usize,
    pub alignments_written: usize,
    pub surfaces_written: usize,
    pub meshes_written: usize,
    pub warnings: Vec<String>,
}
