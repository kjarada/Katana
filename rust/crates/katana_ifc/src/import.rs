//! An IFC file (IFC2X3, IFC4, IFC4X3) -> the drawing: entities, layers,
//! alignments and surfaces, with no third-party library.
//!
//! WHAT COMES IN, AND AS WHAT:
//!
//! ```text
//! IfcAlignment          a named Alignment, its PIs and PVIs RECONSTRUCTED
//!                       from the IFC 4.3 business logic - each run of
//!                       [CLOTHOID] CIRCULARARC [CLOTHOID] between two LINEs
//!                       is one PI at the intersection of those lines; each
//!                       PARABOLICARC one PVI at the intersection of its
//!                       grades - and then CHECKED: Katana solves the
//!                       reconstruction and every segment the file states
//!                       must start within ALIGNMENT_TOLERANCE of where it
//!                       does. Where the check fails, or the geometry has no
//!                       PI form (it starts or ends on a curve, compounds
//!                       two arcs, or uses a transition Katana does not have
//!                       - it has the clothoid), the alignment comes in as a
//!                       3D polyline of its exact geometry, and the warning
//!                       says why. Its stations are taken from its
//!                       Pset_Stationing referents.
//! an element or         its drawable shape - the Axis, else the FootPrint,
//! annotation            else the Annotation representation, else the centre
//!                       line of a swept disk body - as Points, Lines, Arcs,
//!                       Circles, Polylines and Texts in project coordinates,
//!                       heights kept (entity::set_heights) and absent where
//!                       the file has none. An element whose shape is only a
//!                       solid Katana cannot draw comes in as a point at its
//!                       placement, so it is not lost, and is counted.
//! a terrain             IfcGeographicElement TERRAIN (or any product whose
//!                       body is a triangulated face set or TIN) becomes a
//!                       surface for the caller to keep.
//! ```
//!
//! Every entity keeps what it was: `ifc.class`, `ifc.globalId`, `ifc.name`,
//! `ifc.predefinedType`, `ifc.tag` and the source file in its metadata; its
//! property sets and quantities as properties named "<set>/<property>" (the
//! way a .12da attribute group flattens), its classifications as
//! "Classification/<system>", the distribution system it serves as "System";
//! its presentation layer as its layer, and its colour. What Katana itself
//! wrote - Katana_Attributes and Katana_Provenance - goes back where it came
//! from, so an export read back is the drawing it was.
//!
//! COORDINATES. Lengths are brought to metres from the file's length unit and
//! angles to radians from its plane angle unit. A file with an IfcMapConversion
//! is read into the coordinates of its projected system - the conversion's
//! eastings, northings, height, rotation and scale applied - so that a
//! georeferenced file lands where it is; IfcImport::coordinate_system names
//! the system (the caller decides whether the project takes it).
//!
//! WHAT IS NOT READ, AND SAID: solids other than swept disks and triangulated
//! surfaces (Katana has no solid entity), IFC4X1-form alignments, cant,
//! complex instances, and the axes of an IfcGrid (what is placed on a grid is
//! placed where the grid puts it). Each is counted in `warnings`.
//!
//! The parse (step_reader) knows no schema; this gives the instances their
//! meaning, reading each entity by the positions of its attributes - which,
//! for everything read here, are the same in IFC2X3, IFC4 and IFC4X3. Where a
//! schema adds an attribute (a PredefinedType), it is looked for by kind
//! rather than assumed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use katana_commands::command::CommandPtr;
use katana_commands::command_stack::Transaction;
use katana_commands::{create_alignment, create_entities, create_layer};
use katana_core::error::{Error, ErrorCode, Result, make_error};
use katana_core::text::format_exact_real;
use katana_entity::entity::{
    Color, Entity, Geometry, PointGeometry, PropertyMap, PropertyValue, TextGeometry, TextJustify,
    property_to_string, set_heights,
};
use katana_entity::entity_geometry::{bounding_box, validate};
use katana_entity::layer_path::{MAXIMUM_LAYER_DEPTH, MAXIMUM_LAYER_NAME_LENGTH, layer_ancestors};
use katana_entity::model::Model;
use katana_entity::tables::{Alignment, Layer};
use katana_geometry::alignment::{HorizontalAlignment, solve_alignment};
use katana_geometry::profile::{VerticalAlignment, solve_profile};
use katana_geometry::spiral2::Spiral2;
use katana_geometry::{Arc2, Box2, Circle2, Polyline2, Segment2, Spline2};
use katana_math::mat4::{transform_point, transform_vector};
use katana_math::numerics::{TWO_PI, tolerance};
use katana_math::{Mat4, Vec2, Vec3, cpp};
use katana_terrain::TinSurface;
use katana_terrain::tin_surface::TinTriangle;

use crate::import_alignment::{
    HorizontalSegment, VerticalSegment, check_horizontal, check_vertical, height_at,
    reconstruct_horizontal, reconstruct_vertical, sample_horizontal,
};
use crate::product_classes::{ProductClass, ProductKind, product_class};
use crate::step_reader::{StepInstance, StepKind, StepModel, StepValue, parse_step};
use crate::support::path_text;

/// How far a reconstructed alignment may stand from the geometry the file
/// states before the reconstruction is refused: the tolerance the .12da
/// import checks its reconstructions to (docs/interop.md, "Alignments"), for
/// the same reason.
pub const ALIGNMENT_TOLERANCE: f64 = 0.010;

#[derive(Clone, Debug, PartialEq)]
pub struct ImportOptions {
    /// recorded in each entity's metadata as "source"
    pub source_name: String,
    /// Curves Katana has no exact kind for - an arc inside an indexed
    /// polycurve, a transition of a kind other than the clothoid - are
    /// chorded to within this of the true curve (model units).
    pub curve_tolerance: f64,
    /// Subtracted from every plan coordinate, as the other importers' LOCAL
    /// does; None reads the file where it is.
    pub origin_shift: Option<Vec2>,
    pub import_alignments: bool,
    pub import_elements: bool,
    pub import_surfaces: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            source_name: String::new(),
            curve_tolerance: 0.001,
            origin_shift: None,
            import_alignments: true,
            import_elements: true,
            import_surfaces: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportedSurface {
    pub name: String,
    pub surface: TinSurface,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IfcImport {
    /// "IFC4X3_ADD2", as FILE_SCHEMA names it
    pub schema: String,
    /// "EPSG:7856", when an IfcProjectedCRS names one
    pub coordinate_system: String,
    pub entities: Vec<Entity>,
    /// every layer the entities are on
    pub layers: Vec<Layer>,
    /// named uniquely within the file
    pub alignments: Vec<Alignment>,
    pub surfaces: Vec<ImportedSurface>,
    /// Of everything imported: the entities, the alignments' PIs and the
    /// surfaces - what LOCAL moves to the origin.
    pub bounds: Box2,
    /// Products read, by class in the schema's spelling ("IfcPipeSegment").
    pub classes: BTreeMap<String, usize>,
    /// elements and annotations read
    pub products: usize,
    /// of which drawn with their shape
    pub products_imported: usize,
    /// of which only a point at their placement
    pub products_as_points: usize,
    pub alignments_as_polylines: usize,
    pub warnings: Vec<String>,
}

/// Nested mapped items and composite curves are followed at most this deep;
/// a file that nests further (or refers to itself) is refused the rest.
const MAXIMUM_NESTING: i32 = 32;

/// A segment of text as a layer path segment: no separator, no control
/// character, not "." or "..", not only blanks.
fn layer_segment(text: &str) -> String {
    let out: String = text
        .chars()
        .map(|c| {
            if c == '/' || (c as u32) < 0x20 {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = out.trim_matches(' ');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed == "." || trimmed == ".." {
        return String::from("_");
    }
    String::from(trimmed)
}

/// A presentation layer's name as a Katana layer path: its '/'-separated parts
/// kept as the tree they already are, empty ones dropped.
fn layer_path(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for piece in name.split('/') {
        if depth >= MAXIMUM_LAYER_DEPTH {
            break;
        }
        let part: String = piece
            .chars()
            .map(|c| if (c as u32) < 0x20 { '-' } else { c })
            .collect();
        let part = layer_segment(&part);
        if !part.is_empty() {
            if !out.is_empty() {
                out.push('/');
            }
            out.push_str(&part);
            depth += 1;
        }
    }
    if out.len() > MAXIMUM_LAYER_NAME_LENGTH {
        // The C++ cuts the bytes there; a cut inside a character would not be
        // text, so it is made at the character boundary before it.
        let mut cut = MAXIMUM_LAYER_NAME_LENGTH;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    out
}

fn si_prefix(prefix: &str) -> f64 {
    const PREFIXES: [(&str, f64); 16] = [
        ("EXA", 1e18),
        ("PETA", 1e15),
        ("TERA", 1e12),
        ("GIGA", 1e9),
        ("MEGA", 1e6),
        ("KILO", 1e3),
        ("HECTO", 1e2),
        ("DECA", 1e1),
        ("DECI", 1e-1),
        ("CENTI", 1e-2),
        ("MILLI", 1e-3),
        ("MICRO", 1e-6),
        ("NANO", 1e-9),
        ("PICO", 1e-12),
        ("FEMTO", 1e-15),
        ("ATTO", 1e-18),
    ];
    PREFIXES
        .iter()
        .find(|(name, _)| *name == prefix)
        .map_or(1.0, |(_, factor)| *factor)
}

/// IfcBoxAlignment's words, back to a justification.
fn justification(box_alignment: &str) -> TextJustify {
    match box_alignment {
        "bottom-left" => TextJustify::BottomLeft,
        "bottom-middle" => TextJustify::BottomCentre,
        "bottom-right" => TextJustify::BottomRight,
        "middle-left" => TextJustify::MiddleLeft,
        "center" => TextJustify::MiddleCentre,
        "middle-right" => TextJustify::MiddleRight,
        "top-left" => TextJustify::TopLeft,
        "top-middle" => TextJustify::TopCentre,
        "top-right" => TextJustify::TopRight,
        _ => TextJustify::BottomLeft,
    }
}

// ---- access ---------------------------------------------------------------------

fn arg(instance: &StepInstance, index: usize) -> Option<&StepValue> {
    instance.arguments.get(index)
}

fn at<'m>(m: &'m StepModel, instance: &StepInstance, index: usize) -> Option<&'m StepInstance> {
    arg(instance, index).and_then(|value| m.follow(value))
}

fn text(instance: &StepInstance, index: usize) -> String {
    arg(instance, index)
        .and_then(StepValue::string)
        .cloned()
        .unwrap_or_default()
}

fn number(instance: &StepInstance, index: usize) -> Option<f64> {
    arg(instance, index).and_then(StepValue::number)
}

fn ids(instance: &StepInstance, index: usize) -> Vec<u32> {
    let Some(value) = arg(instance, index) else {
        return Vec::new();
    };
    match value.kind {
        StepKind::Reference => vec![value.reference],
        StepKind::List => value
            .items
            .iter()
            .filter(|item| item.kind == StepKind::Reference)
            .map(|item| item.reference)
            .collect(),
        _ => Vec::new(),
    }
}

fn is(instance: Option<&StepInstance>, entity_type: &str) -> bool {
    instance.is_some_and(|instance| instance.entity_type == entity_type)
}

/// The instances in file order. A function of the model alone, so that what
/// it hands out borrows the model and not the reader walking it.
fn ordered(m: &StepModel) -> impl Iterator<Item = (u32, &StepInstance)> {
    m.order
        .iter()
        .filter_map(move |&id| m.find(id).map(|instance| (id, instance)))
}

/// The point's coordinates and how many the file gave (0 to 3).
fn cartesian_with_count(point: Option<&StepInstance>) -> (Vec3, usize) {
    let mut out = Vec3::default();
    let mut count = 0;
    if let Some(first) = point
        .and_then(|point| point.arguments.first())
        .filter(|first| first.kind == StepKind::List)
    {
        let items = &first.items;
        count = items.len();
        if let Some(x) = items.first() {
            out.x = x.number().unwrap_or(0.0);
        }
        if let Some(y) = items.get(1) {
            out.y = y.number().unwrap_or(0.0);
        }
        if let Some(z) = items.get(2) {
            out.z = z.number().unwrap_or(0.0);
        }
    }
    (out, count)
}

fn cartesian(point: Option<&StepInstance>) -> Vec3 {
    cartesian_with_count(point).0
}

fn direction_of(direction: Option<&StepInstance>) -> Option<Vec3> {
    let direction = direction?;
    let d = cartesian(Some(direction));
    (d.length() > 0.0).then(|| d.normalized())
}

/// A drawn shape in project coordinates, and the item it came from.
#[derive(Clone, Debug, Default)]
struct Shape {
    geometry: Geometry,
    heights: Vec<Option<f64>>,
    item: u32,
    /// the body this is the centre line of
    swept_radius: Option<f64>,
}

impl Shape {
    fn new(geometry: Geometry, item: u32) -> Self {
        Self {
            geometry,
            heights: Vec::new(),
            item,
            swept_radius: None,
        }
    }
}

fn joined(parts: &[String]) -> String {
    let mut out = String::new();
    for part in parts {
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(part);
    }
    out
}

/// A run of points as a Line or Polyline shape.
fn add_run(points: Vec<Vec3>, in_3d: bool, item: u32, out: &mut Vec<Shape>) {
    let plan = |p: &Vec3| Vec2::new(p.x, p.y);
    let mut kept: Vec<Vec3> = Vec::new();
    for p in points {
        if kept
            .last()
            .is_none_or(|last| (plan(&p) - plan(last)).length() > tolerance::GEOMETRIC)
        {
            kept.push(p);
        }
    }
    let mut closed = false;
    if kept.len() > 2
        && (plan(&kept[0]) - plan(&kept[kept.len() - 1])).length() <= tolerance::GEOMETRIC
    {
        kept.pop();
        closed = true;
    }
    if kept.len() < 2 {
        return;
    }
    let geometry = if kept.len() == 2 && !closed {
        Geometry::Line(Segment2::new(plan(&kept[0]), plan(&kept[1])))
    } else {
        Geometry::Polyline(Polyline2::new(kept.iter().map(plan).collect(), closed))
    };
    let mut shape = Shape::new(geometry, item);
    if in_3d {
        shape.heights = kept.iter().map(|p| Some(p.z)).collect();
    }
    out.push(shape);
}

/// A name for a warning: the object's own, quoted, or its instance number
/// when it has none.
fn shown_name(id: u32, name: &str) -> String {
    if name.is_empty() {
        format!("#{id} (no name)")
    } else {
        format!("\"{name}\"")
    }
}

struct Reader<'m, 'o> {
    m: &'m StepModel,
    options: &'m ImportOptions,
    out: &'o mut IfcImport,

    length: f64,
    angle: f64,
    area: f64,
    volume: f64,
    world: Mat4,

    // Looked up by instance number, never walked: their order is not seen.
    property_sets: HashMap<u32, Vec<u32>>,
    type_of: HashMap<u32, u32>,
    classifications: HashMap<u32, Vec<u32>>,
    groups: HashMap<u32, Vec<u32>>,
    container: HashMap<u32, u32>,
    parent: HashMap<u32, u32>,
    nested: HashMap<u32, Vec<u32>>,
    layer_of: HashMap<u32, String>,
    colour_of: HashMap<u32, Color>,

    layers: BTreeSet<String>,
    alignment_names: BTreeSet<String>,
    warned: BTreeSet<String>,
    spatial: usize,
    invalid: usize,
    unplaced: usize,
}

impl<'m, 'o> Reader<'m, 'o> {
    fn new(m: &'m StepModel, options: &'m ImportOptions, out: &'o mut IfcImport) -> Self {
        Self {
            m,
            options,
            out,
            length: 1.0,
            angle: 1.0,
            area: 1.0,
            volume: 1.0,
            world: Mat4::default(),
            property_sets: HashMap::new(),
            type_of: HashMap::new(),
            classifications: HashMap::new(),
            groups: HashMap::new(),
            container: HashMap::new(),
            parent: HashMap::new(),
            nested: HashMap::new(),
            layer_of: HashMap::new(),
            colour_of: HashMap::new(),
            layers: BTreeSet::new(),
            alignment_names: BTreeSet::new(),
            warned: BTreeSet::new(),
            spatial: 0,
            invalid: 0,
            unplaced: 0,
        }
    }

    fn run(&mut self) {
        self.read_units();
        self.read_georeference();
        self.index();
        if self.options.import_alignments {
            self.read_alignments();
        }
        self.read_products();
        self.finish();
    }

    fn warn_once(&mut self, message: &str) {
        if self.warned.insert(String::from(message)) {
            self.out.warnings.push(String::from(message));
        }
    }

    // ---- units and frame -------------------------------------------------------------

    fn unit_scale(&self, unit: Option<&StepInstance>, depth: i32) -> f64 {
        let Some(unit) = unit.filter(|_| depth <= 4) else {
            return 1.0;
        };
        if unit.entity_type == "IFCSIUNIT" {
            let prefix = si_prefix(&text(unit, 2));
            let name = text(unit, 3);
            if name == "SQUARE_METRE" {
                return prefix * prefix;
            }
            if name == "CUBIC_METRE" {
                return prefix * prefix * prefix;
            }
            return prefix;
        }
        if unit.entity_type == "IFCCONVERSIONBASEDUNIT"
            || unit.entity_type == "IFCCONVERSIONBASEDUNITWITHOFFSET"
        {
            // IfcMeasureWithUnit
            if let Some(factor) = at(self.m, unit, 3) {
                let value = number(factor, 0).unwrap_or(1.0);
                return value * self.unit_scale(at(self.m, factor, 1), depth + 1);
            }
        }
        1.0
    }

    fn read_units(&mut self) {
        let m = self.m;
        let project = ordered(self.m)
            .find(|(_, instance)| instance.entity_type == "IFCPROJECT")
            .map(|(_, instance)| instance);
        let Some(project) = project else {
            self.warn_once("the file has no IfcProject: its lengths are read as metres");
            return;
        };
        let Some(assignment) = at(m, project, 8) else {
            return;
        };
        for id in ids(assignment, 0) {
            let Some(unit) = m.find(id) else {
                continue;
            };
            let unit_type = text(unit, 1);
            if unit_type == "LENGTHUNIT" {
                self.length = self.unit_scale(Some(unit), 0);
            } else if unit_type == "PLANEANGLEUNIT" {
                self.angle = self.unit_scale(Some(unit), 0);
            } else if unit_type == "AREAUNIT" {
                self.area = self.unit_scale(Some(unit), 0);
            } else if unit_type == "VOLUMEUNIT" {
                self.volume = self.unit_scale(Some(unit), 0);
            }
        }
    }

    /// The map conversion, or the length unit alone: what takes a point in
    /// the file's world frame to project coordinates in metres.
    fn read_georeference(&mut self) {
        let m = self.m;
        let length = self.length;
        self.world = Mat4::scaling(Vec3::new(length, length, length));
        let conversion = ordered(self.m).find(|(_, instance)| {
            instance.entity_type == "IFCMAPCONVERSION"
                || instance.entity_type == "IFCMAPCONVERSIONSCALED"
        });
        if let Some((_, instance)) = conversion {
            let crs = at(m, instance, 1);
            let mut map_unit = 1.0;
            if let Some(crs) = crs {
                let name = text(crs, 0);
                if name.starts_with("EPSG:") {
                    self.out.coordinate_system = name;
                }
                if let Some(unit) = at(m, crs, 6) {
                    map_unit = self.unit_scale(Some(unit), 0);
                }
            }
            let east = number(instance, 2).unwrap_or(0.0);
            let north = number(instance, 3).unwrap_or(0.0);
            let height = number(instance, 4).unwrap_or(0.0);
            let abscissa = number(instance, 5).unwrap_or(1.0);
            let ordinate = number(instance, 6).unwrap_or(0.0);
            let mut scale = number(instance, 7);
            if scale.is_none() && (length / map_unit - 1.0).abs() > 1e-12 {
                // GRF005: the scale is what takes the file's length unit to
                // the map's; a file that leaves it out while its units differ
                // is read as though it had said so.
                scale = Some(length / map_unit);
                self.warn_once(
                    "the map conversion gives no scale although the file's length unit is not \
                     the map's; the ratio of the two is used",
                );
            }
            let s = scale.unwrap_or(1.0);
            let scaled = instance.entity_type == "IFCMAPCONVERSIONSCALED";
            let sx = if scaled {
                number(instance, 8).unwrap_or(1.0)
            } else {
                1.0
            };
            let sy = if scaled {
                number(instance, 9).unwrap_or(1.0)
            } else {
                1.0
            };
            let sz = if scaled {
                number(instance, 10).unwrap_or(1.0)
            } else {
                1.0
            };
            let rotation = ordinate.atan2(abscissa);
            self.world = Mat4::scaling(Vec3::new(map_unit, map_unit, map_unit))
                * Mat4::translation(Vec3::new(east, north, height))
                * Mat4::rotation(Vec3::new(0.0, 0.0, 1.0), rotation)
                * Mat4::scaling(Vec3::new(s * sx, s * sy, s * sz));
        }
        let rigid =
            ordered(self.m).find(|(_, instance)| instance.entity_type == "IFCRIGIDOPERATION");
        if let Some((_, instance)) = rigid {
            // IFC 4.3's translation to a target system. In lengths it is a
            // shift; in angles it places the file in a geographic system,
            // which only a projection can turn into grid coordinates - and
            // this layer has none (geodesy is above it), so the file is read
            // in its own engineering coordinates and that is said.
            let first = arg(instance, 2);
            let second = arg(instance, 3);
            let lengths = match (first, second) {
                (Some(first), Some(second)) => {
                    first.kind == StepKind::Typed
                        && second.kind == StepKind::Typed
                        && first.text == "IFCLENGTHMEASURE"
                        && second.text == "IFCLENGTHMEASURE"
                }
                _ => false,
            };
            if !lengths {
                self.warn_once(
                    "the file is placed in a geographic coordinate system (an IfcRigidOperation \
                     in degrees); it is read in its own engineering coordinates",
                );
            } else {
                let mut map_unit = 1.0;
                if let Some(crs) = at(m, instance, 1) {
                    let name = text(crs, 0);
                    if name.starts_with("EPSG:") {
                        self.out.coordinate_system = name;
                    }
                    if let Some(unit) = at(m, crs, 6) {
                        map_unit = self.unit_scale(Some(unit), 0);
                    }
                }
                let east = first.and_then(StepValue::number).unwrap_or(0.0);
                let north = second.and_then(StepValue::number).unwrap_or(0.0);
                let ratio = length / map_unit;
                self.world = Mat4::scaling(Vec3::new(map_unit, map_unit, map_unit))
                    * Mat4::translation(Vec3::new(east, north, number(instance, 4).unwrap_or(0.0)))
                    * Mat4::scaling(Vec3::new(ratio, ratio, ratio));
            }
        }
        if let Some(shift) = self.options.origin_shift {
            self.world = Mat4::translation(Vec3::new(-shift.x, -shift.y, 0.0)) * self.world;
        }
    }

    /// An IfcAxis2Placement2D or 3D as the matrix taking its frame to its
    /// parent's.
    fn axis_placement(&self, placement: Option<&StepInstance>) -> Mat4 {
        let Some(placement) = placement else {
            return Mat4::default();
        };
        let m = self.m;
        let location = cartesian(at(m, placement, 0));
        let mut z = Vec3::new(0.0, 0.0, 1.0);
        let mut x = Vec3::new(1.0, 0.0, 0.0);
        if placement.entity_type == "IFCAXIS2PLACEMENT3D" {
            z = direction_of(at(m, placement, 1)).unwrap_or(z);
            x = direction_of(at(m, placement, 2)).unwrap_or(x);
        } else if placement.entity_type == "IFCAXIS2PLACEMENT2D" {
            x = direction_of(at(m, placement, 1)).unwrap_or(x);
            x.z = 0.0;
        }
        // The reference direction made perpendicular to the axis, as the
        // schema's function IfcBuildAxes does.
        let mut x_axis = x - z * x.dot(z);
        if x_axis.length() < 1e-12 {
            x_axis = if z.x.abs() < 0.9 {
                Vec3::new(1.0, 0.0, 0.0)
            } else {
                Vec3::new(0.0, 1.0, 0.0)
            };
            x_axis = x_axis - z * x_axis.dot(z);
        }
        x_axis = x_axis.normalized();
        let y_axis = z.cross(x_axis);
        Mat4::new(
            x_axis.x, y_axis.x, z.x, location.x, x_axis.y, y_axis.y, z.y, location.y, x_axis.z,
            y_axis.z, z.z, location.z, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// An object placement, all the way up, in the file's world frame.
    fn placement(&mut self, object: Option<&StepInstance>, depth: i32) -> Mat4 {
        let Some(object) = object.filter(|_| depth <= MAXIMUM_NESTING) else {
            return Mat4::default();
        };
        let m = self.m;
        if object.entity_type == "IFCLOCALPLACEMENT" {
            let parent = self.placement(at(m, object, 0), depth + 1);
            return parent * self.axis_placement(at(m, object, 1));
        }
        if object.entity_type == "IFCLINEARPLACEMENT" {
            if let Some(position) = at(m, object, 2) {
                let parent = self.placement(at(m, object, 0), depth + 1);
                return parent * self.axis_placement(Some(position));
            }
            self.warn_once(
                "a linear placement without a CartesianPosition is read at its parent's origin",
            );
            return self.placement(at(m, object, 0), depth + 1);
        }
        if object.entity_type == "IFCGRIDPLACEMENT" {
            return self.grid_placement(object, depth);
        }
        Mat4::default()
    }

    /// Where two grid axes meet, each offset to its left by its offset
    /// distance (the third distance is a height), in the grid's frame - the
    /// definition of IfcVirtualGridIntersection. IFC 4.3 gave the placement a
    /// PlacementRelTo of its own ahead of the location; IFC4 has two
    /// attributes, so the location is found by position from the end.
    fn grid_placement(&mut self, object: &StepInstance, depth: i32) -> Mat4 {
        let m = self.m;
        let relative = object.arguments.len() >= 3;
        let intersection = at(m, object, if relative { 1 } else { 0 });
        let Some(intersection) = intersection
            .filter(|intersection| intersection.entity_type == "IFCVIRTUALGRIDINTERSECTION")
        else {
            self.warn_once("a grid placement without a grid intersection is read at the origin");
            return Mat4::default();
        };
        let line = |axis_id: u32| -> Option<(Vec3, Vec3)> {
            let axis = m.find(axis_id)?;
            let curve = at(m, axis, 1);
            let points = if is(curve, "IFCPOLYLINE") {
                curve.map(|curve| ids(curve, 0)).unwrap_or_default()
            } else {
                Vec::new()
            };
            if points.len() < 2 {
                return None;
            }
            let mut a = cartesian(m.find(points[0]));
            let mut b = cartesian(m.find(points[1]));
            if let Some(same) = arg(axis, 2)
                && same.kind == StepKind::Enumeration
                && same.text == "F"
            {
                std::mem::swap(&mut a, &mut b);
            }
            Some((a, b))
        };
        let axes = ids(intersection, 0);
        let mut offsets = Vec::new();
        if let Some(list) = arg(intersection, 1).filter(|list| list.kind == StepKind::List) {
            for value in &list.items {
                offsets.push(value.number().unwrap_or(0.0));
            }
        }
        let first = if axes.len() == 2 { line(axes[0]) } else { None };
        let second = if axes.len() == 2 { line(axes[1]) } else { None };
        let (Some(first), Some(second)) = (first, second) else {
            self.warn_once(
                "a grid placement whose axes are not straight lines is read at the origin",
            );
            return Mat4::default();
        };
        let offset = |l: (Vec3, Vec3), d: f64| {
            let along = Vec2::new(l.1.x - l.0.x, l.1.y - l.0.y).normalized();
            let left = Vec3::new(-along.y * d, along.x * d, 0.0);
            (l.0 + left, l.1 + left)
        };
        let a = offset(first, offsets.first().copied().unwrap_or(0.0));
        let b = offset(second, offsets.get(1).copied().unwrap_or(0.0));
        let u = Vec2::new(a.1.x - a.0.x, a.1.y - a.0.y);
        let v = Vec2::new(b.1.x - b.0.x, b.1.y - b.0.y);
        let denominator = u.x * v.y - u.y * v.x;
        if denominator.abs() < 1e-12 {
            self.warn_once("a grid placement on two parallel axes is read at the origin");
            return Mat4::default();
        }
        let w = Vec2::new(b.0.x - a.0.x, b.0.y - a.0.y);
        let t = (w.x * v.y - w.y * v.x) / denominator;
        let meet = Vec3::new(
            a.0.x + u.x * t,
            a.0.y + u.y * t,
            offsets.get(2).copied().unwrap_or(0.0),
        );
        // IFC 4.3 says what the intersection is relative to; before it, the
        // grid the first axis belongs to places it.
        let mut grid = Mat4::default();
        let relative_to = if relative { at(m, object, 0) } else { None };
        if let Some(relative_to) = relative_to {
            grid = self.placement(Some(relative_to), depth + 1);
        } else {
            let owner = ordered(self.m).find(|(_, candidate)| {
                candidate.entity_type == "IFCGRID"
                    && [7, 8, 9]
                        .iter()
                        .any(|&index| ids(candidate, index).contains(&axes[0]))
            });
            if let Some((_, candidate)) = owner {
                grid = self.placement(at(m, candidate, 5), depth + 1);
            }
        }
        // Oriented along the first axis, unless a direction is given.
        let x = u.normalized();
        let mut x_axis = Vec3::new(x.x, x.y, 0.0);
        if let Some(given) = direction_of(at(m, object, if relative { 2 } else { 1 })) {
            x_axis = given;
        }
        let y_axis = Vec3::new(0.0, 0.0, 1.0).cross(x_axis);
        grid * Mat4::new(
            x_axis.x, y_axis.x, 0.0, meet.x, x_axis.y, y_axis.y, 0.0, meet.y, x_axis.z, y_axis.z,
            1.0, meet.z, 0.0, 0.0, 0.0, 1.0,
        )
    }

    fn world_point(&self, local: &Mat4, point: Vec3) -> Vec3 {
        transform_point(self.world * *local, point)
    }

    // ---- relationships ------------------------------------------------------------------

    fn index(&mut self) {
        for (_, r) in ordered(self.m) {
            let entity_type = r.entity_type.as_str();
            match entity_type {
                "IFCRELDEFINESBYPROPERTIES" => {
                    for object in ids(r, 4) {
                        for set in ids(r, 5) {
                            self.property_sets.entry(object).or_default().push(set);
                        }
                    }
                }
                "IFCRELDEFINESBYTYPE" => {
                    for object in ids(r, 4) {
                        for type_id in ids(r, 5) {
                            self.type_of.insert(object, type_id);
                        }
                    }
                }
                "IFCRELASSOCIATESCLASSIFICATION" => {
                    for object in ids(r, 4) {
                        for reference in ids(r, 5) {
                            self.classifications
                                .entry(object)
                                .or_default()
                                .push(reference);
                        }
                    }
                }
                "IFCRELASSIGNSTOGROUP" | "IFCRELASSIGNSTOGROUPBYFACTOR" => {
                    for object in ids(r, 4) {
                        for group in ids(r, 6) {
                            self.groups.entry(object).or_default().push(group);
                        }
                    }
                }
                "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                    for object in ids(r, 4) {
                        for structure in ids(r, 5) {
                            self.container.insert(object, structure);
                        }
                    }
                }
                "IFCRELAGGREGATES" => {
                    for part in ids(r, 5) {
                        for whole in ids(r, 4) {
                            self.parent.insert(part, whole);
                        }
                    }
                }
                "IFCRELNESTS" => {
                    for whole in ids(r, 4) {
                        let parts = ids(r, 5);
                        self.nested.entry(whole).or_default().extend(parts);
                    }
                }
                "IFCPRESENTATIONLAYERASSIGNMENT" | "IFCPRESENTATIONLAYERWITHSTYLE" => {
                    let name = layer_path(&text(r, 0));
                    if !name.is_empty() {
                        for item in ids(r, 2) {
                            self.layer_of.entry(item).or_insert_with(|| name.clone());
                        }
                    }
                }
                "IFCSTYLEDITEM" => {
                    if let Some(colour) = self.style_colour(r, 1, 0) {
                        for item in ids(r, 0) {
                            self.colour_of.entry(item).or_insert(colour);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn rgb(&self, colour: Option<&StepInstance>) -> Option<Color> {
        let colour = colour.filter(|colour| colour.entity_type == "IFCCOLOURRGB")?;
        let channel = |index: usize| {
            let value = cpp::clamp(number(colour, index).unwrap_or(0.0), 0.0, 1.0);
            (value * 255.0).round() as u8
        };
        Some(Color::new(channel(1), channel(2), channel(3), 255))
    }

    /// The colour a style (or a list of them) gives.
    fn style_colour(&self, owner: &StepInstance, index: usize, depth: i32) -> Option<Color> {
        if depth > 4 {
            return None;
        }
        let m = self.m;
        for id in ids(owner, index) {
            let Some(style) = m.find(id) else {
                continue;
            };
            if style.entity_type == "IFCCURVESTYLE" {
                if let Some(colour) = self.rgb(at(m, style, 3)) {
                    return Some(colour);
                }
            } else if style.entity_type == "IFCSURFACESTYLE" {
                for element in ids(style, 2) {
                    if let Some(shading) = m.find(element)
                        && (shading.entity_type == "IFCSURFACESTYLESHADING"
                            || shading.entity_type == "IFCSURFACESTYLERENDERING")
                        && let Some(colour) = self.rgb(at(m, shading, 0))
                    {
                        return Some(colour);
                    }
                }
            } else if style.entity_type == "IFCPRESENTATIONSTYLEASSIGNMENT"
                && let Some(colour) = self.style_colour(style, 0, depth + 1)
            {
                return Some(colour);
            }
        }
        None
    }

    // ---- properties -----------------------------------------------------------------------

    fn property_value(&self, value: &StepValue) -> Option<PropertyValue> {
        if value.kind == StepKind::Typed
            && let Some(inner) = value.items.first()
        {
            let value_type = value.text.as_str();
            if (value_type == "IFCBOOLEAN" || value_type == "IFCLOGICAL")
                && inner.kind == StepKind::Enumeration
            {
                if inner.text == "T" || inner.text == "TRUE" {
                    return Some(PropertyValue::Boolean(true));
                }
                if inner.text == "F" || inner.text == "FALSE" {
                    return Some(PropertyValue::Boolean(false));
                }
                return Some(PropertyValue::Text(String::from("UNKNOWN")));
            }
            if inner.kind == StepKind::Integer
                && (value_type == "IFCINTEGER"
                    || value_type == "IFCCOUNTMEASURE"
                    || value_type == "IFCPOSITIVEINTEGER"
                    || value_type == "IFCTIMESTAMP")
            {
                return Some(PropertyValue::Integer(inner.integer));
            }
            if let Some(n) = inner.number() {
                let scale = match value_type {
                    "IFCLENGTHMEASURE"
                    | "IFCPOSITIVELENGTHMEASURE"
                    | "IFCNONNEGATIVELENGTHMEASURE" => self.length,
                    "IFCAREAMEASURE" => self.area,
                    "IFCVOLUMEMEASURE" => self.volume,
                    "IFCPLANEANGLEMEASURE" | "IFCPOSITIVEPLANEANGLEMEASURE" => self.angle,
                    _ => 1.0,
                };
                return Some(PropertyValue::Real(n * scale));
            }
            return self.property_value(inner);
        }
        match value.kind {
            StepKind::String | StepKind::Enumeration => {
                Some(PropertyValue::Text(value.text.clone()))
            }
            StepKind::Integer => Some(PropertyValue::Integer(value.integer)),
            StepKind::Real => Some(PropertyValue::Real(value.real)),
            _ => None,
        }
    }

    fn add_property(
        &self,
        property: &StepInstance,
        prefix: &str,
        into: &mut PropertyMap,
        depth: i32,
    ) {
        let name = text(property, 0);
        if name.is_empty() || depth > 4 {
            return;
        }
        let key = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let property_type = property.entity_type.as_str();
        if property_type == "IFCPROPERTYSINGLEVALUE" {
            if let Some(converted) = arg(property, 2).and_then(|value| self.property_value(value)) {
                into.insert(key, converted);
            }
        } else if property_type == "IFCPROPERTYENUMERATEDVALUE"
            || property_type == "IFCPROPERTYLISTVALUE"
        {
            let mut values = Vec::new();
            if let Some(list) = arg(property, 2).filter(|list| list.kind == StepKind::List) {
                for item in &list.items {
                    if let Some(converted) = self.property_value(item) {
                        values.push(property_to_string(&converted));
                    }
                }
            }
            if values.len() == 1 {
                into.insert(key, PropertyValue::Text(values.remove(0)));
            } else if !values.is_empty() {
                into.insert(key, PropertyValue::Text(joined(&values)));
            }
        } else if property_type == "IFCPROPERTYBOUNDEDVALUE" {
            let lower = arg(property, 3)
                .and_then(|value| self.property_value(value))
                .map(|converted| property_to_string(&converted))
                .unwrap_or_default();
            let upper = arg(property, 2)
                .and_then(|value| self.property_value(value))
                .map(|converted| property_to_string(&converted))
                .unwrap_or_default();
            if !lower.is_empty() || !upper.is_empty() {
                into.insert(key, PropertyValue::Text(format!("{lower}..{upper}")));
            }
        } else if property_type == "IFCCOMPLEXPROPERTY" {
            for id in ids(property, 3) {
                if let Some(part) = self.m.find(id) {
                    self.add_property(part, &key, into, depth + 1);
                }
            }
        }
    }

    /// What the product's property sets and quantities say, and its
    /// provenance where Katana wrote it.
    fn add_property_set(&self, set: &StepInstance, entity: &mut Entity) {
        let m = self.m;
        let name = text(set, 2);
        if set.entity_type == "IFCPROPERTYSET" {
            if name == "Katana_Attributes" {
                for id in ids(set, 4) {
                    if let Some(property) = m.find(id) {
                        self.add_property(property, "", &mut entity.properties, 0);
                    }
                }
                return;
            }
            if name == "Katana_Provenance" {
                let mut provenance = PropertyMap::new();
                for id in ids(set, 4) {
                    if let Some(property) = m.find(id) {
                        self.add_property(property, "", &mut provenance, 0);
                    }
                }
                for (key, value) in provenance {
                    if key != "EntityId"
                        && key != "Layer"
                        && key != "Style"
                        && key != "ClassifiedBy"
                    {
                        entity.metadata.insert(key, value);
                    }
                }
                return;
            }
            for id in ids(set, 4) {
                if let Some(property) = m.find(id) {
                    self.add_property(property, &name, &mut entity.properties, 0);
                }
            }
        } else if set.entity_type == "IFCELEMENTQUANTITY" {
            for id in ids(set, 5) {
                let Some(quantity) = m.find(id) else {
                    continue;
                };
                let value = number(quantity, 3);
                let quantity_name = text(quantity, 0);
                let Some(value) = value.filter(|_| !quantity_name.is_empty()) else {
                    continue;
                };
                let scale = match quantity.entity_type.as_str() {
                    "IFCQUANTITYLENGTH" => self.length,
                    "IFCQUANTITYAREA" => self.area,
                    "IFCQUANTITYVOLUME" => self.volume,
                    _ => 1.0,
                };
                entity.properties.insert(
                    format!("{name}/{quantity_name}"),
                    PropertyValue::Real(value * scale),
                );
            }
        }
    }

    /// The classification system a reference belongs to, up its chain.
    fn classification_system(&self, reference: Option<&StepInstance>, depth: i32) -> String {
        let fallback = || String::from("Classification");
        let Some(reference) = reference.filter(|_| depth <= 8) else {
            return fallback();
        };
        let Some(source) = at(self.m, reference, 3) else {
            return fallback();
        };
        if source.entity_type == "IFCCLASSIFICATION" {
            let name = text(source, 3);
            return if name.is_empty() { fallback() } else { name };
        }
        self.classification_system(Some(source), depth + 1)
    }

    fn describe(
        &self,
        id: u32,
        product: &StepInstance,
        ifc_class: &ProductClass,
        entity: &mut Entity,
    ) {
        let m = self.m;
        let meta = |entity: &mut Entity, key: &str, value: String| {
            if !value.is_empty() {
                entity
                    .metadata
                    .insert(String::from(key), PropertyValue::Text(value));
            }
        };
        meta(entity, "ifc.class", String::from(ifc_class.name));
        meta(entity, "ifc.globalId", text(product, 0));
        meta(entity, "ifc.name", text(product, 2));
        meta(entity, "ifc.description", text(product, 3));
        meta(entity, "ifc.objectType", text(product, 4));
        if let Some(tag) = arg(product, 7).filter(|tag| tag.kind == StepKind::String) {
            meta(entity, "ifc.tag", tag.text.clone());
        }
        for index in [8, 7] {
            if let Some(value) =
                arg(product, index).filter(|value| value.kind == StepKind::Enumeration)
            {
                meta(entity, "ifc.predefinedType", value.text.clone());
                break;
            }
        }
        meta(entity, "source", self.options.source_name.clone());
        if let Some(container) = self.container_of(id) {
            // The C++ reads the container's name unchecked; a container that
            // is not an instance names nothing here.
            let name = m
                .find(container)
                .map(|container| text(container, 2))
                .unwrap_or_default();
            meta(entity, "ifc.container", name);
        }

        // The type's sets first, the occurrence's over them.
        if let Some(type_object) = self.type_of.get(&id).and_then(|&type_id| m.find(type_id)) {
            for set in ids(type_object, 5) {
                if let Some(definition) = m.find(set) {
                    self.add_property_set(definition, entity);
                }
            }
        }
        if let Some(sets) = self.property_sets.get(&id) {
            for &set in sets {
                if let Some(definition) = m.find(set) {
                    self.add_property_set(definition, entity);
                }
            }
        }
        if let Some(references) = self.classifications.get(&id) {
            let mut by_system: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for &reference in references {
                let Some(instance) = m.find(reference) else {
                    continue;
                };
                let mut identification = text(instance, 1);
                if identification.is_empty() {
                    identification = text(instance, 2);
                }
                if !identification.is_empty() {
                    by_system
                        .entry(self.classification_system(Some(instance), 0))
                        .or_default()
                        .push(identification);
                }
            }
            for (system, values) in &by_system {
                entity.properties.insert(
                    format!("Classification/{system}"),
                    PropertyValue::Text(joined(values)),
                );
            }
        }
        if let Some(groups) = self.groups.get(&id) {
            let mut names = Vec::new();
            for &group in groups {
                if let Some(instance) = m.find(group) {
                    let name = text(instance, 2);
                    if !name.is_empty() {
                        names.push(name);
                    }
                }
            }
            if !names.is_empty() {
                entity
                    .properties
                    .insert(String::from("System"), PropertyValue::Text(joined(&names)));
            }
        }
    }

    /// The spatial element holding `id`, directly or through what it is part
    /// of.
    fn container_of(&self, mut id: u32) -> Option<u32> {
        for _ in 0..MAXIMUM_NESTING {
            if let Some(&found) = self.container.get(&id) {
                return Some(found);
            }
            id = *self.parent.get(&id)?;
        }
        None
    }

    // ---- geometry ----------------------------------------------------------------------------

    /// Where a trimmed circle starts and ends, in radians about its own
    /// placement, `to` beyond `from` in the direction of the sense, so that
    /// `to - from` is the signed sweep. A trim is a parameter value - an
    /// angle, in the file's angle unit - or a point on the circle, whichever
    /// the file gives; with both, the one MasterRepresentation prefers, the
    /// parameter when it says neither. None when either end is neither.
    /// `stretch` is an ellipse's SemiAxis1 / SemiAxis2: a trimming point in
    /// its placement's frame is at eccentric anomaly atan2(y a / b, x), which
    /// for a circle (stretch 1) is its polar angle.
    fn circle_trim(
        &self,
        item: &StepInstance,
        circle: &StepInstance,
        stretch: f64,
    ) -> Option<(f64, f64)> {
        let m = self.m;
        let inverse = self.axis_placement(at(m, circle, 0)).inverse();
        let prefer_point = arg(item, 4).is_some_and(|master| {
            master.kind == StepKind::Enumeration && master.text == "CARTESIAN"
        });
        let trim = |index: usize| -> Option<f64> {
            let list = arg(item, index).filter(|list| list.kind == StepKind::List)?;
            let mut parameter: Option<f64> = None;
            let mut point: Option<f64> = None;
            for value in &list.items {
                if value.kind == StepKind::Typed && value.text == "IFCPARAMETERVALUE" {
                    if let Some(number) = value.number() {
                        parameter = Some(number * self.angle);
                    }
                } else if value.kind == StepKind::Reference
                    && let Some(inverse) = inverse
                {
                    let on = m.find(value.reference);
                    if is(on, "IFCCARTESIANPOINT") {
                        let local = transform_point(inverse, cartesian(on));
                        point = Some((local.y * stretch).atan2(local.x));
                    }
                }
            }
            if prefer_point && point.is_some() {
                return point;
            }
            parameter.or(point)
        };
        let from = trim(1)?;
        let mut to = trim(2)?;
        let forward = !arg(item, 3)
            .is_some_and(|sense| sense.kind == StepKind::Enumeration && sense.text == "F");
        if forward && to < from {
            to += TWO_PI;
        } else if !forward && to > from {
            to -= TWO_PI;
        }
        Some((from, to))
    }

    /// Points of a circle's arc from `from` to `to` (radians, in its own
    /// frame), chorded to the tolerance.
    fn circle_points(&self, frame: &Mat4, radius: f64, from: f64, to: f64) -> Vec<Vec3> {
        let sweep = to - from;
        let scale = transform_vector(self.world * *frame, Vec3::new(1.0, 0.0, 0.0)).length();
        let r = radius * scale;
        let tolerance = self.options.curve_tolerance;
        let mut count: usize = 8;
        if r > tolerance {
            let step = 2.0 * cpp::max(-1.0, 1.0 - tolerance / r).acos();
            // The C++ casts the ceiling unchecked; Rust saturates.
            count = 2_usize.max((sweep.abs() / step).ceil() as usize);
        }
        count = count.min(20_000);
        let mut points = Vec::with_capacity(count + 1);
        for i in 0..=count {
            let a = from + sweep * i as f64 / count as f64;
            points
                .push(self.world_point(frame, Vec3::new(radius * a.cos(), radius * a.sin(), 0.0)));
        }
        points
    }

    /// The points of a curve item, for curves that are only runs of points.
    fn curve_points(
        &mut self,
        item: &StepInstance,
        frame: &Mat4,
        points: &mut Vec<Vec3>,
        in_3d: &mut bool,
        depth: i32,
    ) -> bool {
        if depth > MAXIMUM_NESTING {
            return false;
        }
        let m = self.m;
        let item_type = item.entity_type.as_str();
        if item_type == "IFCPOLYLINE" {
            for id in ids(item, 0) {
                let (p, dimensions) = cartesian_with_count(m.find(id));
                *in_3d = *in_3d && dimensions == 3;
                points.push(self.world_point(frame, p));
            }
            return true;
        }
        if item_type == "IFCINDEXEDPOLYCURVE" {
            let Some(list) = at(m, item, 0).filter(|list| {
                list.arguments
                    .first()
                    .is_some_and(|first| first.kind == StepKind::List)
            }) else {
                return false;
            };
            *in_3d = *in_3d && list.entity_type == "IFCCARTESIANPOINTLIST3D";
            let mut coordinates = Vec::new();
            for row in &list.arguments[0].items {
                let mut p = Vec3::default();
                if row.kind == StepKind::List {
                    if let Some(x) = row.items.first() {
                        p.x = x.number().unwrap_or(0.0);
                    }
                    if let Some(y) = row.items.get(1) {
                        p.y = y.number().unwrap_or(0.0);
                    }
                    if let Some(z) = row.items.get(2) {
                        p.z = z.number().unwrap_or(0.0);
                    }
                }
                coordinates.push(p);
            }
            let point = |index: &StepValue| -> Option<Vec3> {
                let n = index.number()?;
                // A number is finite (parse_finite_double), so this is the
                // C++'s n < 1 || n > size.
                if !(1.0..=coordinates.len() as f64).contains(&n) {
                    return None;
                }
                Some(coordinates[n as usize - 1])
            };
            let Some(segments) = arg(item, 1).filter(|segments| segments.kind == StepKind::List)
            else {
                for &p in &coordinates {
                    points.push(self.world_point(frame, p));
                }
                return true;
            };
            for segment in &segments.items {
                let Some(first) = segment.items.first().filter(|first| {
                    segment.kind == StepKind::Typed && first.kind == StepKind::List
                }) else {
                    continue;
                };
                let indices = &first.items;
                if segment.text == "IFCARCINDEX" && indices.len() == 3 {
                    let (Some(a), Some(b), Some(c)) =
                        (point(&indices[0]), point(&indices[1]), point(&indices[2]))
                    else {
                        continue;
                    };
                    let Some(arc) = Arc2::through_points(
                        Vec2::new(a.x, a.y),
                        Vec2::new(b.x, b.y),
                        Vec2::new(c.x, c.y),
                    ) else {
                        points.push(self.world_point(frame, a));
                        points.push(self.world_point(frame, c));
                        continue;
                    };
                    // In the arc's own plane, with the height moving evenly
                    // from its start to its end.
                    let scale =
                        transform_vector(self.world * *frame, Vec3::new(1.0, 0.0, 0.0)).length();
                    let r = arc.radius * scale;
                    let tolerance = self.options.curve_tolerance;
                    let mut count: usize = 8;
                    if r > tolerance {
                        let step = 2.0 * cpp::max(-1.0, 1.0 - tolerance / r).acos();
                        count = 2_usize.max((arc.sweep.abs() / step).ceil() as usize);
                    }
                    count = count.min(20_000);
                    for i in 0..=count {
                        let t = i as f64 / count as f64;
                        let p = arc.point_at(t);
                        points.push(
                            self.world_point(frame, Vec3::new(p.x, p.y, a.z + (c.z - a.z) * t)),
                        );
                    }
                } else {
                    for index in indices {
                        if let Some(p) = point(index) {
                            points.push(self.world_point(frame, p));
                        }
                    }
                }
            }
            return true;
        }
        if item_type == "IFCCIRCLE" || item_type == "IFCELLIPSE" {
            let position = at(m, item, 0);
            let local = *frame * self.axis_placement(position);
            let radius = number(item, 1).unwrap_or(0.0);
            if item_type == "IFCELLIPSE" {
                let semi2 = number(item, 2).unwrap_or(radius);
                let squash = local
                    * Mat4::scaling(Vec3::new(
                        1.0,
                        if radius > 0.0 { semi2 / radius } else { 1.0 },
                        1.0,
                    ));
                points.extend(self.circle_points(&squash, radius, 0.0, TWO_PI));
            } else {
                points.extend(self.circle_points(&local, radius, 0.0, TWO_PI));
            }
            *in_3d = *in_3d && is(position, "IFCAXIS2PLACEMENT3D");
            return true;
        }
        if item_type == "IFCTRIMMEDCURVE" {
            let Some(basis) = at(m, item, 0) else {
                return false;
            };
            let trim = |index: usize| -> Option<f64> {
                let list = arg(item, index).filter(|list| list.kind == StepKind::List)?;
                list.items
                    .iter()
                    .find(|value| {
                        value.kind == StepKind::Typed && value.text == "IFCPARAMETERVALUE"
                    })
                    .and_then(StepValue::number)
            };
            if basis.entity_type == "IFCCIRCLE" {
                let Some((from, to)) = self.circle_trim(item, basis, 1.0) else {
                    return false;
                };
                let position = at(m, basis, 0);
                let local = *frame * self.axis_placement(position);
                points.extend(self.circle_points(
                    &local,
                    number(basis, 1).unwrap_or(0.0),
                    from,
                    to,
                ));
                *in_3d = *in_3d && is(position, "IFCAXIS2PLACEMENT3D");
                return true;
            }
            if basis.entity_type == "IFCELLIPSE" {
                // As a circle's arc, in the ellipse's frame squashed along its
                // second axis: IFC's conic parameter is the eccentric anomaly.
                // Read as the whole ellipse, an elliptical arc came back closed.
                let semi1 = number(basis, 1).unwrap_or(0.0);
                let semi2 = number(basis, 2).unwrap_or(semi1);
                if !(semi1 > 0.0) || !(semi2 > 0.0) {
                    return false;
                }
                let Some((from, to)) = self.circle_trim(item, basis, semi1 / semi2) else {
                    return false;
                };
                let position = at(m, basis, 0);
                let squash = *frame
                    * self.axis_placement(position)
                    * Mat4::scaling(Vec3::new(1.0, semi2 / semi1, 1.0));
                points.extend(self.circle_points(&squash, semi1, from, to));
                *in_3d = *in_3d && is(position, "IFCAXIS2PLACEMENT3D");
                return true;
            }
            if basis.entity_type == "IFCLINE"
                && let (Some(t1), Some(t2)) = (trim(1), trim(2))
            {
                let (origin, dimensions) = cartesian_with_count(at(m, basis, 0));
                let Some(vector) = at(m, basis, 1) else {
                    return false;
                };
                let direction = direction_of(at(m, vector, 0)).unwrap_or(Vec3::new(1.0, 0.0, 0.0))
                    * number(vector, 1).unwrap_or(1.0);
                points.push(self.world_point(frame, origin + direction * t1));
                points.push(self.world_point(frame, origin + direction * t2));
                *in_3d = *in_3d && dimensions == 3;
                return true;
            }
            return self.curve_points(basis, frame, points, in_3d, depth + 1);
        }
        if item_type == "IFCBSPLINECURVEWITHKNOTS"
            || item_type == "IFCRATIONALBSPLINECURVEWITHKNOTS"
        {
            // Degree, control points, the knots as distinct values with their
            // multiplicities, and a rational one's weights: a Spline2, chorded
            // within the import's curve tolerance, in plan (Katana's spline is
            // two-dimensional). Unread, it once came in as a point at 0,0.
            let mut spline = Spline2 {
                // The C++ casts the degree unchecked; Rust saturates.
                degree: number(item, 0).unwrap_or(0.0) as i32,
                ..Spline2::default()
            };
            for id in ids(item, 1) {
                let p = self.world_point(frame, cartesian(m.find(id)));
                spline.control_points.push(Vec2::new(p.x, p.y));
            }
            let (Some(multiplicities), Some(knots)) = (arg(item, 5), arg(item, 6)) else {
                return false;
            };
            if multiplicities.kind != StepKind::List
                || knots.kind != StepKind::List
                || multiplicities.items.len() != knots.items.len()
            {
                return false;
            }
            for (count, knot) in multiplicities.items.iter().zip(&knots.items) {
                let (Some(count), Some(knot)) = (count.number(), knot.number()) else {
                    return false;
                };
                if !(1.0..=64.0).contains(&count) {
                    return false;
                }
                spline
                    .knots
                    .extend(std::iter::repeat_n(knot, count as usize));
            }
            if item_type == "IFCRATIONALBSPLINECURVEWITHKNOTS"
                && let Some(weights) = arg(item, 8).filter(|weights| weights.kind == StepKind::List)
            {
                for weight in &weights.items {
                    spline.weights.push(weight.number().unwrap_or(0.0));
                }
            }
            if spline.check_structure().is_err() {
                return false;
            }
            // The control points are in the world already (a B-spline is
            // the same curve of its points moved by any affine map), so the
            // tolerance is the world's, as an arc's is above.
            for p in &spline.to_polyline(self.options.curve_tolerance).vertices {
                points.push(Vec3::new(p.x, p.y, 0.0));
            }
            *in_3d = false;
            return true;
        }
        if item_type == "IFCCOMPOSITECURVE" || item_type == "IFCCOMPOSITECURVEONSURFACE" {
            for id in ids(item, 0) {
                let Some(segment) = m.find(id) else {
                    continue;
                };
                if segment.entity_type == "IFCCOMPOSITECURVESEGMENT"
                    || segment.entity_type == "IFCREPARAMETRISEDCOMPOSITECURVESEGMENT"
                {
                    let mut part = Vec::new();
                    if let Some(parent) = at(m, segment, 2)
                        && self.curve_points(parent, frame, &mut part, in_3d, depth + 1)
                    {
                        if arg(segment, 1).is_some_and(|same| {
                            same.kind == StepKind::Enumeration && same.text == "F"
                        }) {
                            part.reverse();
                        }
                        points.extend(part);
                    }
                } else if segment.entity_type == "IFCCURVESEGMENT" {
                    self.curve_segment_points(segment, frame, points);
                    *in_3d = false;
                }
            }
            return true;
        }
        if item_type == "IFCGRADIENTCURVE" || item_type == "IFCSEGMENTEDREFERENCECURVE" {
            self.warn_once(
                "a gradient curve outside an alignment is read in plan, from its base curve",
            );
            if let Some(base) = at(m, item, 2) {
                return self.curve_points(base, frame, points, in_3d, depth + 1);
            }
            return false;
        }
        false
    }

    /// An IFC 4.3 IfcCurveSegment in plan: its parent curve from SegmentStart
    /// for SegmentLength, moved so that it starts at its placement in the
    /// placement's direction (the reading IfcOpenShell's kernel makes).
    fn curve_segment_points(
        &mut self,
        segment: &StepInstance,
        frame: &Mat4,
        points: &mut Vec<Vec3>,
    ) {
        let m = self.m;
        let parent = at(m, segment, 4);
        let start = number(segment, 2).unwrap_or(0.0);
        let length = number(segment, 3).unwrap_or(0.0);
        let Some(parent) = parent.filter(|_| length != 0.0) else {
            return;
        };
        let mut k0 = 0.0;
        let mut k1 = 0.0;
        if parent.entity_type == "IFCCIRCLE" {
            let radius = number(parent, 1).unwrap_or(0.0);
            k0 = if radius > 0.0 {
                (if length > 0.0 { 1.0 } else { -1.0 }) / radius
            } else {
                0.0
            };
            k1 = k0;
        } else if parent.entity_type == "IFCCLOTHOID" {
            let a = number(parent, 1).unwrap_or(0.0);
            if a != 0.0 {
                k0 = start / (a * a.abs());
                k1 = (start + length) / (a * a.abs());
            }
        } else if parent.entity_type != "IFCLINE" {
            let message = format!(
                "a curve segment on an {} is read as a straight",
                parent.entity_type
            );
            self.warn_once(&message);
        }
        let shape = Spiral2 {
            start: Vec2::new(0.0, 0.0),
            start_direction: 0.0,
            start_curvature: k0,
            end_curvature: k1,
            length: length.abs(),
        };
        let local = *frame * self.axis_placement(at(m, segment, 1));
        let count = if k0 == 0.0 && k1 == 0.0 {
            1
        } else {
            shape
                .chord_count_for(self.options.curve_tolerance)
                .min(20_000)
        };
        for i in 0..=count {
            let p = shape.point_at(shape.length * i as f64 / count as f64);
            points.push(self.world_point(&local, Vec3::new(p.x, p.y, 0.0)));
        }
    }

    fn shapes(
        &mut self,
        item: &StepInstance,
        id: u32,
        frame: &Mat4,
        out: &mut Vec<Shape>,
        depth: i32,
    ) {
        if depth > MAXIMUM_NESTING {
            let message = format!("geometry nested more than {MAXIMUM_NESTING} deep is not read");
            self.warn_once(&message);
            return;
        }
        let m = self.m;
        let item_type = item.entity_type.as_str();
        if item_type == "IFCCARTESIANPOINT" {
            let (local, dimensions) = cartesian_with_count(Some(item));
            let p = self.world_point(frame, local);
            let mut shape = Shape::new(
                Geometry::Point(PointGeometry {
                    position: Vec2::new(p.x, p.y),
                }),
                id,
            );
            if dimensions == 3 {
                shape.heights.push(Some(p.z));
            }
            out.push(shape);
            return;
        }
        let trimmed_circle = item_type == "IFCTRIMMEDCURVE" && is(at(m, item, 0), "IFCCIRCLE");
        if item_type == "IFCCIRCLE" || trimmed_circle {
            // A horizontal circle or arc keeps its exact form.
            let circle = if item_type == "IFCCIRCLE" {
                Some(item)
            } else {
                at(m, item, 0)
            };
            if let Some(circle) = circle {
                let position = at(m, circle, 0);
                let local = self.world * *frame * self.axis_placement(position);
                let axis = transform_vector(local, Vec3::new(0.0, 0.0, 1.0));
                let x_axis = transform_vector(local, Vec3::new(1.0, 0.0, 0.0));
                let flat = axis.length() > 0.0 && axis.normalized().z.abs() > 1.0 - 1e-9;
                let three_d = is(position, "IFCAXIS2PLACEMENT3D");
                if flat && item_type == "IFCCIRCLE" {
                    let centre = transform_point(local, Vec3::default());
                    let mut shape = Shape::new(
                        Geometry::Circle(Circle2::new(
                            Vec2::new(centre.x, centre.y),
                            number(circle, 1).unwrap_or(0.0) * x_axis.length(),
                        )),
                        id,
                    );
                    if three_d {
                        shape.heights.push(Some(centre.z));
                    }
                    out.push(shape);
                    return;
                }
                if flat && let Some((from, to)) = self.circle_trim(item, circle, 1.0) {
                    // Seen from above, a placement whose z axis points down
                    // turns the other way.
                    let hand = if axis.z > 0.0 { 1.0 } else { -1.0 };
                    let centre = transform_point(local, Vec3::default());
                    let mut shape = Shape::new(
                        Geometry::Arc(Arc2::new(
                            Vec2::new(centre.x, centre.y),
                            number(circle, 1).unwrap_or(0.0) * x_axis.length(),
                            x_axis.y.atan2(x_axis.x) + hand * from,
                            hand * (to - from),
                        )),
                        id,
                    );
                    if three_d {
                        shape.heights.push(Some(centre.z));
                    }
                    out.push(shape);
                    return;
                }
            }
        }
        if item_type == "IFCTEXTLITERAL" || item_type == "IFCTEXTLITERALWITHEXTENT" {
            let local = self.world * *frame * self.axis_placement(at(m, item, 1));
            let origin = transform_point(local, Vec3::default());
            let x_axis = transform_vector(local, Vec3::new(1.0, 0.0, 0.0));
            let mut literal = TextGeometry {
                text: text(item, 0),
                position: Vec2::new(origin.x, origin.y),
                rotation: x_axis.y.atan2(x_axis.x),
                ..TextGeometry::default()
            };
            if item_type == "IFCTEXTLITERALWITHEXTENT" {
                let lines = literal.text.bytes().filter(|&c| c == b'\n').count() + 1;
                if let Some(extent) = at(m, item, 3) {
                    let height = number(extent, 1).unwrap_or(0.0) * x_axis.length() / lines as f64;
                    if height > 0.0 {
                        literal.height = height;
                    }
                }
                literal.justify = justification(&text(item, 4));
            }
            if literal.text.is_empty() {
                return;
            }
            out.push(Shape::new(Geometry::Text(literal), id));
            return;
        }
        if item_type == "IFCGEOMETRICCURVESET" || item_type == "IFCGEOMETRICSET" {
            for element in ids(item, 0) {
                if let Some(child) = m.find(element) {
                    self.shapes(child, element, frame, out, depth + 1);
                }
            }
            return;
        }
        if item_type == "IFCANNOTATIONFILLAREA" {
            for index in [0, 1] {
                for boundary in ids(item, index) {
                    if let Some(child) = m.find(boundary) {
                        self.shapes(child, boundary, frame, out, depth + 1);
                    }
                }
            }
            return;
        }
        if item_type == "IFCMAPPEDITEM" {
            let map = at(m, item, 0);
            let target = at(m, item, 1);
            let Some(map) = map else {
                return;
            };
            let origin = self.axis_placement(at(m, map, 0)).inverse();
            let mapped = *frame * self.transformation_operator(target) * origin.unwrap_or_default();
            if let Some(representation) = at(m, map, 1) {
                for child in ids(representation, 3) {
                    if let Some(instance) = m.find(child) {
                        self.shapes(instance, child, &mapped, out, depth + 1);
                    }
                }
            }
            return;
        }
        if item_type == "IFCSWEPTDISKSOLID" || item_type == "IFCSWEPTDISKSOLIDPOLYGONAL" {
            let before = out.len();
            if let Some(directrix) = at(m, item, 0) {
                let directrix_id = ids(item, 0).first().copied().unwrap_or(id);
                self.shapes(directrix, directrix_id, frame, out, depth + 1);
            }
            let scale = transform_vector(self.world * *frame, Vec3::new(1.0, 0.0, 0.0)).length();
            for shape in &mut out[before..] {
                shape.swept_radius = Some(number(item, 1).unwrap_or(0.0) * scale);
                shape.item = id;
            }
            return;
        }
        let mut points = Vec::new();
        let mut in_3d = true;
        if self.curve_points(item, frame, &mut points, &mut in_3d, depth + 1) {
            add_run(points, in_3d, id, out);
        }
    }

    fn transformation_operator(&self, op: Option<&StepInstance>) -> Mat4 {
        let Some(op) = op else {
            return Mat4::default();
        };
        let m = self.m;
        let three_d = op
            .entity_type
            .starts_with("IFCCARTESIANTRANSFORMATIONOPERATOR3D");
        let mut x = direction_of(at(m, op, 0)).unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        let mut y = direction_of(at(m, op, 1)).unwrap_or(Vec3::new(0.0, 1.0, 0.0));
        let origin = cartesian(at(m, op, 2));
        let scale = number(op, 3).unwrap_or(1.0);
        let mut z = if three_d {
            direction_of(at(m, op, 4)).unwrap_or(x.cross(y))
        } else {
            Vec3::new(0.0, 0.0, 1.0)
        };
        let mut sy = scale;
        let mut sz = scale;
        if op.entity_type.ends_with("NONUNIFORM") {
            sy = number(op, if three_d { 5 } else { 4 }).unwrap_or(scale);
            sz = if three_d {
                number(op, 6).unwrap_or(scale)
            } else {
                scale
            };
        }
        x *= scale;
        y *= sy;
        z *= sz;
        Mat4::new(
            x.x, y.x, z.x, origin.x, x.y, y.y, z.y, origin.y, x.z, y.z, z.z, origin.z, 0.0, 0.0,
            0.0, 1.0,
        )
    }

    // ---- products ----------------------------------------------------------------------------

    fn fallback_layer(&self, id: u32, ifc_class: &ProductClass) -> String {
        let mut layer = String::from("IFC");
        if let Some(container) = self.container_of(id) {
            let name = self
                .m
                .find(container)
                .map(|container| layer_segment(&text(container, 2)))
                .unwrap_or_default();
            if !name.is_empty() {
                layer.push('/');
                layer.push_str(&name);
            }
        }
        let name = ifc_class.name.strip_prefix("Ifc").unwrap_or(ifc_class.name);
        format!("{layer}/{name}")
    }

    fn layer_for(
        &self,
        representation: u32,
        drawn: &[Shape],
        id: u32,
        ifc_class: &ProductClass,
    ) -> String {
        if let Some(found) = self.layer_of.get(&representation) {
            return found.clone();
        }
        for shape in drawn {
            if let Some(found) = self.layer_of.get(&shape.item) {
                return found.clone();
            }
        }
        self.fallback_layer(id, ifc_class)
    }

    fn read_products(&mut self) {
        let products: Vec<(u32, &'m StepInstance)> = ordered(self.m).collect();
        for (id, product) in products {
            let Some(ifc_class) = product_class(&product.entity_type) else {
                continue;
            };
            if ifc_class.kind == ProductKind::Spatial {
                // A site or storey is what holds the elements (ifc.container);
                // only a shape of its own - a site boundary, a space's body -
                // is something not drawn.
                if at(self.m, product, 6).is_some() {
                    self.spatial += 1;
                }
                continue;
            }
            if ifc_class.kind != ProductKind::Drawn {
                continue;
            }
            self.out.products += 1;
            *self
                .out
                .classes
                .entry(String::from(ifc_class.name))
                .or_insert(0) += 1;
            if self.options.import_surfaces && self.read_surface(product, ifc_class) {
                continue;
            }
            if self.options.import_elements {
                self.read_product(id, product, ifc_class);
            }
        }
        if self.spatial > 0 {
            self.out.warnings.push(format!(
                "{} spatial elements (sites, buildings, storeys, spaces) have a shape of their \
                 own, which is not drawn: they are read as the containers of what they hold",
                self.spatial
            ));
        }
    }

    /// The representations of a product, most drawable first.
    fn representations(&self, product: &StepInstance) -> Vec<u32> {
        let m = self.m;
        let Some(shape) = at(m, product, 6) else {
            return Vec::new();
        };
        let mut ranked: Vec<(i32, u32)> = Vec::new();
        for id in ids(shape, 2) {
            let Some(representation) = m.find(id) else {
                continue;
            };
            let identifier = text(representation, 1);
            let rank = match identifier.as_str() {
                "Axis" => 0,
                "FootPrint" => 1,
                "Annotation" => 2,
                "Body" | "Body-Fallback" => 3,
                _ => 4,
            };
            ranked.push((rank, id));
        }
        // std::stable_sort by rank alone.
        ranked.sort_by_key(|&(rank, _)| rank);
        ranked.into_iter().map(|(_, id)| id).collect()
    }

    fn read_product(&mut self, id: u32, product: &StepInstance, ifc_class: &ProductClass) {
        let m = self.m;
        let frame = self.placement(at(m, product, 5), 0);
        let mut drawn: Vec<Shape> = Vec::new();
        let mut used: u32 = 0;
        for representation in self.representations(product) {
            let Some(instance) = m.find(representation) else {
                continue;
            };
            for item in ids(instance, 3) {
                if let Some(child) = m.find(item) {
                    self.shapes(child, item, &frame, &mut drawn, 0);
                }
            }
            if !drawn.is_empty() {
                used = representation;
                break;
            }
        }
        let mut base = Entity::default();
        self.describe(id, product, ifc_class, &mut base);
        if drawn.is_empty() {
            if at(m, product, 5).is_none() {
                self.unplaced += 1;
                return;
            }
            // Nothing Katana can draw: the object at its placement, so that
            // it and what it says are not lost.
            let place = self.world_point(&frame, Vec3::default());
            let mut shape = Shape::new(
                Geometry::Point(PointGeometry {
                    position: Vec2::new(place.x, place.y),
                }),
                0,
            );
            shape.heights.push(Some(place.z));
            drawn.push(shape);
            base.metadata.insert(
                String::from("ifc.geometry"),
                PropertyValue::Text(String::from("placement only")),
            );
            self.out.products_as_points += 1;
        } else {
            self.out.products_imported += 1;
        }
        base.layer = self.layer_for(used, &drawn, id, ifc_class);
        for shape in drawn {
            let mut entity = base.clone();
            entity.geometry = shape.geometry;
            if let Some(&colour) = self.colour_of.get(&shape.item) {
                entity.color = Some(colour);
            }
            if let Some(radius) = shape.swept_radius.filter(|radius| *radius > 0.0) {
                entity.properties.insert(
                    String::from("SweptDisk/Radius"),
                    PropertyValue::Real(radius),
                );
            }
            let count = match &entity.geometry {
                Geometry::Polyline(polyline) => polyline.vertices.len(),
                Geometry::Line(_) => 2,
                _ => 1,
            };
            if shape.heights.len() == count {
                set_heights(&mut entity.properties, &shape.heights);
            }
            self.add(entity);
        }
    }

    fn add(&mut self, entity: Entity) {
        if validate(&entity.geometry).is_err() {
            self.invalid += 1;
            return;
        }
        self.out.bounds.expand_box(bounding_box(&entity.geometry));
        self.layers.insert(entity.layer.clone());
        self.out.entities.push(entity);
    }

    // ---- surfaces --------------------------------------------------------------------------

    fn read_surface(&mut self, product: &StepInstance, ifc_class: &ProductClass) -> bool {
        let m = self.m;
        let geographic = ifc_class.name == "IfcGeographicElement";
        let frame = self.placement(at(m, product, 5), 0);
        for representation in self.representations(product) {
            let Some(instance) = m.find(representation) else {
                continue;
            };
            for item in ids(instance, 3) {
                let Some(faces) = m.find(item) else {
                    continue;
                };
                let tin = faces.entity_type == "IFCTRIANGULATEDIRREGULARNETWORK";
                if !(tin || (geographic && faces.entity_type == "IFCTRIANGULATEDFACESET")) {
                    continue;
                }
                let mut name = text(product, 2);
                if name.is_empty() {
                    name = format!("Surface {}", self.out.surfaces.len() + 1);
                }
                if let Some(surface) = self.triangles(faces, &frame) {
                    self.out.surfaces.push(ImportedSurface { name, surface });
                    self.out.products_imported += 1;
                    return true;
                }
                return false;
            }
        }
        false
    }

    fn triangles(&mut self, faces: &StepInstance, frame: &Mat4) -> Option<TinSurface> {
        let list = at(self.m, faces, 0)?;
        let rows = list
            .arguments
            .first()
            .filter(|first| first.kind == StepKind::List)?;
        let mut vertices = Vec::with_capacity(rows.items.len());
        for row in &rows.items {
            let mut p = Vec3::default();
            if row.kind == StepKind::List && row.items.len() >= 3 {
                p = Vec3::new(
                    row.items[0].number().unwrap_or(0.0),
                    row.items[1].number().unwrap_or(0.0),
                    row.items[2].number().unwrap_or(0.0),
                );
            }
            vertices.push(self.world_point(frame, p));
        }
        // PnIndex, when given, says which point each index means. The C++
        // casts each number to an unsigned 32-bit integer unchecked; Rust
        // saturates one out of range.
        let mut remap: Vec<u32> = Vec::new();
        if let Some(pn) = arg(faces, 4).filter(|pn| pn.kind == StepKind::List) {
            for value in &pn.items {
                remap.push(value.number().unwrap_or(0.0) as u32);
            }
        }
        let indices = arg(faces, 3).filter(|indices| indices.kind == StepKind::List)?;
        let mut triangles: Vec<TinTriangle> = Vec::new();
        for row in &indices.items {
            if row.kind != StepKind::List || row.items.len() != 3 {
                continue;
            }
            let mut t: TinTriangle = [0; 3];
            let mut valid = true;
            for (slot, value) in t.iter_mut().zip(&row.items) {
                let mut n = value.number().unwrap_or(0.0) as u32;
                if !remap.is_empty() {
                    n = if n >= 1 && n as usize <= remap.len() {
                        remap[n as usize - 1]
                    } else {
                        0
                    };
                }
                valid = valid && n >= 1 && n as usize <= vertices.len();
                *slot = n.wrapping_sub(1);
            }
            if !valid {
                continue;
            }
            // Either way round, as the .12da import takes a triangle.
            let a = vertices[t[0] as usize];
            let b = vertices[t[1] as usize];
            let c = vertices[t[2] as usize];
            if (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x) < 0.0 {
                t.swap(1, 2);
            }
            triangles.push(t);
        }
        match TinSurface::create(vertices, triangles, Vec::new()) {
            Ok(surface) => Some(surface),
            Err(error) => {
                self.out.warnings.push(format!(
                    "a surface could not be built: {}",
                    error.describe()
                ));
                None
            }
        }
    }

    // ---- alignments ------------------------------------------------------------------------

    fn unique_alignment_name(&mut self, name: &str) -> String {
        let name = if name.is_empty() {
            format!("Alignment {}", self.alignment_names.len() + 1)
        } else {
            String::from(name)
        };
        let mut candidate = name.clone();
        let mut n = 2;
        while self.alignment_names.contains(&candidate) {
            candidate = format!("{name} ({n})");
            n += 1;
        }
        self.alignment_names.insert(candidate.clone());
        candidate
    }

    fn layout(&self, alignment: u32, layout_type: &str) -> Option<u32> {
        self.nested
            .get(&alignment)?
            .iter()
            .copied()
            .find(|&child| is(self.m.find(child), layout_type))
    }

    fn design_parameters(&self, layout: u32) -> Vec<&'m StepInstance> {
        let m = self.m;
        let mut out = Vec::new();
        if let Some(children) = self.nested.get(&layout) {
            for &child in children {
                let segment = m.find(child);
                if is(segment, "IFCALIGNMENTSEGMENT")
                    && let Some(parameters) = segment.and_then(|segment| at(m, segment, 7))
                {
                    out.push(parameters);
                }
            }
        }
        out
    }

    fn start_station(&self, alignment: u32) -> f64 {
        let m = self.m;
        // distance, station
        let mut best: Option<(f64, f64)> = None;
        let Some(children) = self.nested.get(&alignment) else {
            return 0.0;
        };
        for &child in children {
            let referent = m.find(child);
            let Some(referent) = referent.filter(|referent| referent.entity_type == "IFCREFERENT")
            else {
                continue;
            };
            let mut station: Option<f64> = None;
            if let Some(sets) = self.property_sets.get(&child) {
                for &set in sets {
                    let Some(pset) = m
                        .find(set)
                        .filter(|pset| text(pset, 2) == "Pset_Stationing")
                    else {
                        continue;
                    };
                    for property in ids(pset, 4) {
                        if let Some(value) = m.find(property)
                            && text(value, 0) == "Station"
                        {
                            station = number(value, 2);
                        }
                    }
                }
            }
            let mut distance = 0.0;
            if let Some(linear) =
                at(m, referent, 5).filter(|linear| linear.entity_type == "IFCLINEARPLACEMENT")
                && let Some(relative) = at(m, linear, 1)
                && let Some(expression) = at(m, relative, 0)
            {
                distance = number(expression, 0).unwrap_or(0.0);
            }
            if let Some(station) = station
                && best.is_none_or(|(known, _)| distance < known)
            {
                best = Some((distance, station));
            }
        }
        best.map_or(0.0, |(distance, station)| {
            (station - distance) * self.length
        })
    }

    fn read_alignments(&mut self) {
        let m = self.m;
        let alignments: Vec<(u32, &'m StepInstance)> = ordered(self.m)
            .filter(|(_, instance)| instance.entity_type == "IFCALIGNMENT")
            .collect();
        for (id, alignment) in alignments {
            let mut horizontal = self.layout(id, "IFCALIGNMENTHORIZONTAL");
            let mut horizontal_owner = id;
            // A child alignment reuses its parent's horizontal (the IFC 4.3
            // concept template for a horizontal shared by several verticals).
            if horizontal.is_none()
                && let Some(&parent) = self.parent.get(&id)
                && is(m.find(parent), "IFCALIGNMENT")
            {
                horizontal = self.layout(parent, "IFCALIGNMENTHORIZONTAL");
                horizontal_owner = parent;
            }
            let name = text(alignment, 2);
            let Some(horizontal) = horizontal else {
                // No business logic: its representation is read as linework,
                // as any other product's is.
                if let Some(class) = product_class("IFCALIGNMENT") {
                    self.read_product(id, alignment, class);
                }
                self.out.warnings.push(format!(
                    "alignment {} has no horizontal layout and came in as its shape",
                    shown_name(id, &name)
                ));
                self.out.alignments_as_polylines += 1;
                continue;
            };
            let frame = self.world * self.placement(at(m, alignment, 5), 0);
            let linear = transform_vector(frame, Vec3::new(1.0, 0.0, 0.0)).length();
            let mut h_segments = Vec::new();
            for p in self.design_parameters(horizontal) {
                let start = transform_point(frame, cartesian(at(m, p, 2)));
                let a = number(p, 3).unwrap_or(0.0) * self.angle;
                let d = transform_vector(frame, Vec3::new(a.cos(), a.sin(), 0.0));
                let curvature = |index: usize| {
                    let r = number(p, index).unwrap_or(0.0) * linear;
                    if r == 0.0 { 0.0 } else { 1.0 / r }
                };
                h_segments.push(HorizontalSegment {
                    segment_type: text(p, 8),
                    start: Vec2::new(start.x, start.y),
                    direction: d.y.atan2(d.x),
                    start_curvature: curvature(4),
                    end_curvature: curvature(5),
                    length: number(p, 6).unwrap_or(0.0) * linear,
                });
            }
            let mut v_segments = Vec::new();
            if let Some(vertical) = self.layout(id, "IFCALIGNMENTVERTICAL") {
                for p in self.design_parameters(vertical) {
                    v_segments.push(VerticalSegment {
                        segment_type: text(p, 8),
                        start_distance: number(p, 2).unwrap_or(0.0) * linear,
                        length: number(p, 3).unwrap_or(0.0) * linear,
                        start_height: transform_point(
                            frame,
                            Vec3::new(0.0, 0.0, number(p, 4).unwrap_or(0.0)),
                        )
                        .z,
                        start_grade: number(p, 5).unwrap_or(0.0),
                        end_grade: number(p, 6).unwrap_or(0.0),
                    });
                }
            }
            if self.layout(id, "IFCALIGNMENTCANT").is_some() {
                self.warn_once("cant (superelevation) is not read: Katana's alignments have none");
            }
            let station = self.start_station(horizontal_owner);
            self.read_alignment(id, alignment, &name, station, &h_segments, &v_segments);
        }
    }

    fn read_alignment(
        &mut self,
        id: u32,
        alignment: &StepInstance,
        name: &str,
        station: f64,
        h_segments: &[HorizontalSegment],
        v_segments: &[VerticalSegment],
    ) {
        let mut why = String::new();
        let mut horizontal: Option<HorizontalAlignment> =
            reconstruct_horizontal(h_segments, station, &mut why);
        if let Some(definition) = &horizontal {
            match solve_alignment(definition) {
                Err(error) => {
                    why = format!("its reconstruction does not solve: {}", error.describe());
                    horizontal = None;
                }
                Ok(solved) => {
                    let off = check_horizontal(&solved, h_segments);
                    if off > ALIGNMENT_TOLERANCE {
                        why = format!(
                            "its reconstruction stands {} m from the geometry the file states",
                            format_exact_real(off)
                        );
                        horizontal = None;
                    }
                }
            }
        }
        let mut vertical: Option<VerticalAlignment> = None;
        let mut vertical_why = String::new();
        if !v_segments.is_empty() {
            vertical = reconstruct_vertical(v_segments, station, &mut vertical_why);
            if let Some(definition) = &vertical {
                match solve_profile(definition) {
                    Err(error) => {
                        vertical_why =
                            format!("its reconstruction does not solve: {}", error.describe());
                        vertical = None;
                    }
                    Ok(solved) => {
                        let off = check_vertical(&solved, v_segments, station);
                        if off > ALIGNMENT_TOLERANCE {
                            vertical_why = format!(
                                "its reconstruction stands {} m from the levels the file states",
                                format_exact_real(off)
                            );
                            vertical = None;
                        }
                    }
                }
            }
        }
        if let Some(horizontal) = horizontal {
            let result = Alignment {
                name: self.unique_alignment_name(name),
                description: text(alignment, 3),
                horizontal,
                vertical,
            };
            if !v_segments.is_empty() && result.vertical.is_none() {
                self.out.warnings.push(format!(
                    "the profile of alignment \"{}\" is not a Katana design profile \
                     ({vertical_why}); its levels are on a 3D polyline beside it",
                    result.name
                ));
                self.alignment_line(id, alignment, name, h_segments, v_segments);
            }
            if let Err(status) = validate(&result) {
                self.out.warnings.push(format!(
                    "alignment \"{}\" is not valid: {}",
                    result.name,
                    status.describe()
                ));
                self.alignment_line(id, alignment, name, h_segments, v_segments);
                self.out.alignments_as_polylines += 1;
                return;
            }
            self.out.alignments.push(result);
            return;
        }
        self.out.warnings.push(format!(
            "alignment {} came in as a polyline of its geometry: {why}",
            shown_name(id, name)
        ));
        self.alignment_line(id, alignment, name, h_segments, v_segments);
        self.out.alignments_as_polylines += 1;
    }

    /// The alignment's exact geometry as a 3D polyline, heights from its
    /// profile where it has one.
    fn alignment_line(
        &mut self,
        id: u32,
        alignment: &StepInstance,
        name: &str,
        h_segments: &[HorizontalSegment],
        v_segments: &[VerticalSegment],
    ) {
        let samples = sample_horizontal(h_segments, self.options.curve_tolerance);
        if samples.len() < 2 {
            return;
        }
        let mut line = Polyline2::default();
        let mut heights = Vec::with_capacity(samples.len());
        for &(distance, point) in &samples {
            line.vertices.push(point);
            heights.push(if v_segments.is_empty() {
                None
            } else {
                height_at(v_segments, distance)
            });
        }
        let mut entity = Entity::default();
        if let Some(class) = product_class("IFCALIGNMENT") {
            self.describe(id, alignment, class, &mut entity);
        }
        entity.metadata.insert(
            String::from("ifc.alignment"),
            PropertyValue::Text(String::from(name)),
        );
        entity.geometry = Geometry::Polyline(line);
        set_heights(&mut entity.properties, &heights);
        entity.layer = String::from("IFC/Alignments");
        if let Some(shape) = at(self.m, alignment, 6) {
            for representation in ids(shape, 2) {
                if let Some(found) = self.layer_of.get(&representation) {
                    entity.layer = found.clone();
                }
            }
        }
        self.add(entity);
    }

    fn finish(&mut self) {
        // The extent of everything the import brings, not only its entities:
        // what LOCAL shifts by and what the far-apart question weighs, so a
        // file of an alignment or a terrain alone is placed like any other.
        for alignment in &self.out.alignments {
            for pi in &alignment.horizontal.pis {
                self.out.bounds.expand(pi.point);
            }
        }
        for surface in &self.out.surfaces {
            if surface.surface.vertex_count() > 0 {
                self.out.bounds.expand_box(*surface.surface.bounds());
            }
        }
        for name in &self.layers {
            self.out.layers.push(Layer {
                name: name.clone(),
                ..Layer::default()
            });
        }
        if self.invalid > 0 {
            self.out.warnings.push(format!(
                "{} shapes had no extent Katana can hold (a zero-length line, an empty text) and \
                 were not taken",
                self.invalid
            ));
        }
        if self.unplaced > 0 {
            self.out.warnings.push(format!(
                "{} objects had neither a shape Katana can draw nor a placement",
                self.unplaced
            ));
        }
        if self.out.products_as_points > 0 {
            // Not only solids: an object with no representation at all (the
            // stand-in ground a road model's cut voids, when the surface is
            // not in the file) has no shape either, and "solids" said what it
            // is not.
            self.out.warnings.push(format!(
                "{} objects have no shape Katana can draw - a solid, or no shape in the file at \
                 all - and came in as a point at their placement, with their properties",
                self.out.products_as_points
            ));
        }
        if self.m.complex_instances > 0 {
            self.out.warnings.push(format!(
                "{} complex instances were read past",
                self.m.complex_instances
            ));
        }
    }
}

/// ParseFailure, naming the line, for text that is not an ISO 10303-21 file
/// or that breaks its syntax; Unsupported for a schema that is not IFC.
pub fn read_ifc(text: impl AsRef<[u8]>, options: &ImportOptions) -> Result<IfcImport> {
    let parsed = parse_step(text.as_ref())?;
    if !parsed.schema.starts_with("IFC") {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "the file's schema is \"{}\", not an IFC schema",
                parsed.schema
            ),
        ));
    }
    let mut out = IfcImport {
        schema: parsed.schema.clone(),
        ..IfcImport::default()
    };
    Reader::new(&parsed, options, &mut out).run();
    Ok(out)
}

/// NotFound when the file cannot be read.
pub fn read_ifc_file(path: &Path, options: &ImportOptions) -> Result<IfcImport> {
    let Ok(text) = std::fs::read(path) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "the file cannot be read",
            path_text(path),
        ));
    };
    let mut named = options.clone();
    if named.source_name.is_empty() {
        named.source_name = crate::support::file_name_text(path);
    }
    read_ifc(&text, &named).map_err(|mut error| {
        error.context = if error.context.is_empty() {
            path_text(path)
        } else {
            format!("{}: {}", path_text(path), error.context)
        };
        error
    })
}

/// The import as ONE undoable step: the layers the model lacks (parents
/// first), the alignments - renamed "<name> (2)" where the model already has
/// one of the name, and said so in `imported.warnings` - and the entities,
/// moved out of `imported`. None when there is nothing to add.
pub fn import_command(imported: &mut IfcImport, model: &Model) -> Option<CommandPtr> {
    let mut transaction = Transaction::new("IMPORT");
    // By name, so that "a" comes before "a/b".
    let mut layers = imported.layers.clone();
    layers.sort_by(|a, b| Ord::cmp(&a.name, &b.name));
    let mut created: BTreeSet<String> = BTreeSet::new();
    for layer in layers {
        // Parents first: a layer "a/b" needs "a", which add() makes for it,
        // so only the layers the model lacks are asked for, once.
        if model.layers.contains(&layer.name) || created.contains(&layer.name) {
            continue;
        }
        for ancestor in layer_ancestors(&layer.name) {
            created.insert(ancestor);
        }
        created.insert(layer.name.clone());
        transaction.add(create_layer(layer));
    }
    for alignment in &mut imported.alignments {
        let mut name = alignment.name.clone();
        let mut n = 2;
        while model.alignments.contains(&name) {
            name = format!("{} ({n})", alignment.name);
            n += 1;
        }
        if name != alignment.name {
            imported.warnings.push(format!(
                "alignment \"{}\" is already in the drawing; the import's is \"{name}\"",
                alignment.name
            ));
            alignment.name = name;
        }
        transaction.add(create_alignment(alignment.clone()));
    }
    if !imported.entities.is_empty() {
        transaction.add(create_entities(std::mem::take(&mut imported.entities)));
    }
    if transaction.size() == 0 {
        return None;
    }
    let command: CommandPtr = Box::new(transaction);
    Some(command)
}
