//! The road model -> IFC 4.3 (docs/ifc.md, "Road model").
//!
//! Horizontal BIM: the objects a corridor sweeps and the objects placed along
//! a road (RoadModelInput, made by katana_app from cad's BIM model). The
//! sections cited are those of the IFC 4.3.2.0 documentation (IFC4X3_ADD2).
//!
//! THE SPATIAL STRUCTURE. An IfcRoad (7.9.3.3) per facility - a corridor -
//! aggregated into the site, and an IfcRoadPart (7.9.3.4) per part of it,
//! aggregated into its road; each element contained in its part, in its road
//! when it names no part, in the site when it names no facility. The road is
//! an ELEMENT and its parts PARTIAL, the steps IfcRoad's page asks of one
//! facility over another (an ELEMENT parent over PARTIAL children). A part's
//! UsageType (IfcFacilityPart, mandatory; IfcFacilityUsageEnum 5.4.2.11) is
//! LATERAL for the parts across the road's section - most a "lateral part" in
//! IfcRoadPartTypeEnum's own definitions (7.9.2.2: CARRIAGEWAY, TRAFFICLANE,
//! SHOULDER, ROADSIDE, CENTRALRESERVE ...), the rest beside one (SIDEWALK, a
//! footpath along the side of the road; ROADSIDEPART) - LONGITUDINAL for a
//! ROADSEGMENT ("Longitudinal, linear segment of a road"), REGION for a
//! bounded area (an INTERSECTION, a ROUNDABOUT, a LAYBY ...). Every placement
//! is relative to its container's, as IfcLocalPlacement (8.7.3.14) has it: the
//! road to the site, a part to its road, an element to its part, a feature to
//! its host.
//!
//! THE BODY is an IfcTriangulatedFaceSet (8.8.3.47), Closed when it is: every
//! edge used by exactly two faces, each once in each direction, which is what
//! the schema asks of a closed face set ("each edge shall be used by exactly
//! two boundaries of faces", normals pointing away from the material). A
//! closed mesh wound inwards is turned outwards; vertices at exactly the same
//! place are written once and a face naming one vertex twice is dropped,
//! neither of which moves anything. Rejected: IfcPolygonalFaceSet. The road
//! model's meshes are triangles already, so merging coplanar ones into
//! polygons would only save bytes, and the triangulated set is the one the
//! surfaces already write, which is in scope of the Alignment-based view this
//! file declares (surfaces.rs). Rejected too: an extruded profile (a course's
//! section swept along the alignment) - truer to the design, but a sweep along
//! an IfcGradientCurve is beyond what every viewer draws, and the mesh is
//! already what the 3D view shows and the quantities were measured on.
//!
//! FEATURES. An IfcEarthworksCut (6.6.3.4) is "the resulting void from
//! modification of existing terrain", an IfcFeatureElementSubtraction whose
//! IfcRelVoidsElement is a mandatory inverse and which "shall have no
//! independent containment relationship to the spatial structure" (5.4.3.30);
//! an IfcSurfaceFeature (7.11.3.16) - a line marking - likewise adheres to an
//! element through IfcRelAdheresToElement (5.4.3.45). So each is written on
//! its host and contained nowhere: a cut voids the element it names, else the
//! surface it was taken from when the file holds it, else an
//! IfcGeographicElement TERRAIN that stands for that ground with no shape of
//! its own; a marking adheres to the element it names, else to the course,
//! pavement or slab of its road that lies under it - the highest there - and
//! with none, is not written, said, since a marking on nothing would be an
//! invalid file.
//!
//! MATERIAL is an IfcMaterial of the element's material's name, written once
//! and associated (IfcRelAssociatesMaterial) with every element of it - but
//! not with a cut, which is a void: that relationship's NoVoidElement rule
//! refuses one. Every element names its material in Katana_RoadElement too.
//!
//! QUANTITIES are an IfcElementQuantity (5.4.3.21) of IfcQuantityLength, Area,
//! Volume, Weight and Count. Those a class's base quantity set names - with
//! the quantity type it gives them - go in that set (Qto_CourseBaseQuantities
//! 6.6.5.2, Qto_EarthworksCutBaseQuantities 6.6.5.3,
//! Qto_EarthworksFillBaseQuantities 6.6.5.4, Qto_KerbBaseQuantities 6.6.5.6,
//! Qto_PavementBaseQuantities 6.6.5.8, Qto_SignBaseQuantities 6.6.5.11,
//! Qto_RailingBaseQuantities 6.1.5.10), whose MethodOfMeasurement is
//! 'BaseQuantities'; the rest in Katana_Quantities, so a quantity-take-off
//! tool finds a course's volume where it looks and nothing is claimed to be a
//! base quantity that the template does not define.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use katana_core::text::equals_ignoring_case;
use katana_geometry::mesh::{TriangleMesh, validate};
use katana_math::numerics::{nearly_zero, tolerance};
use katana_math::{Vec2, Vec3, cpp};

use crate::builder::{Builder, Container, PropertyList};
use crate::classification::IfcClass;
use crate::export::{RoadElement, RoadModelInput, RoadQuantity};
use crate::product_layout::{find_product_layout, lists_word, predefined_types_of};
use crate::step::{character_count, step_real};
use crate::step_file::{Args, Id, list_of};
use crate::support::{lower, trimmed, upper};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuantityKind {
    Length,
    Area,
    Volume,
    Weight,
    Count,
}

fn entity_of(kind: QuantityKind) -> &'static str {
    match kind {
        QuantityKind::Length => "IfcQuantityLength",
        QuantityKind::Area => "IfcQuantityArea",
        QuantityKind::Volume => "IfcQuantityVolume",
        QuantityKind::Weight => "IfcQuantityWeight",
        QuantityKind::Count => "IfcQuantityCount",
    }
}

/// A quantity's kind and its value in the units the file is written in
/// (metres, square and cubic metres, kilograms); None for a unit that is
/// none of them.
#[derive(Clone, Copy, Debug)]
struct Measured {
    kind: QuantityKind,
    value: f64,
}

fn measured(quantity: &RoadQuantity) -> Option<Measured> {
    let unit = lower(trimmed(&quantity.unit));
    let kind_value = |kind: QuantityKind, value: f64| Some(Measured { kind, value });
    match unit.as_str() {
        "m" => kind_value(QuantityKind::Length, quantity.value),
        "m2" | "m\u{b2}" => kind_value(QuantityKind::Area, quantity.value),
        "m3" | "m\u{b3}" => kind_value(QuantityKind::Volume, quantity.value),
        "kg" => kind_value(QuantityKind::Weight, quantity.value),
        // a tonne is 1000 kg
        "t" => kind_value(QuantityKind::Weight, quantity.value * 1000.0),
        "" => kind_value(QuantityKind::Count, quantity.value),
        _ => None,
    }
}

/// IfcQuantityCount's CountValue is an IfcCountMeasure, which is an INTEGER in
/// IFC4X3_ADD2 (the type is `TYPE IfcCountMeasure = INTEGER`; in IFC4 it was a
/// NUMBER, so a REAL there was valid and is not here: IfcOpenShell 0.8.5 and
/// 0.9.0 both refuse `28.`). A count is written as that integer; one that is
/// not a whole number, or that is beyond what a 32-bit reader holds, has no
/// valid spelling and is left out, said. The bound is the largest 32-bit signed
/// integer because that is what IfcOpenShell 0.8.5 reads a larger STEP INTEGER
/// as (3000000000 came back as 2147483647), silently, which is worse than
/// absent. The tolerance is math::tolerance::RELATIVE from the nearest whole
/// number: a count is dimensionless, its double spacing is 1.2e-10 up to
/// a million, and a few operations on it stay inside 1e-9.
const LARGEST_COUNT: f64 = 2_147_483_647.0;

fn whole_count(value: f64) -> Option<i64> {
    let whole = value.round();
    if !(whole <= LARGEST_COUNT) || !nearly_zero(value - whole, tolerance::RELATIVE) {
        return None;
    }
    Some(whole as i64)
}

/// A class's base quantity set: its name, and each quantity it defines with
/// its type, "Name=K" separated by blanks (L length, A area, V volume, W
/// weight). Each row is the IFC 4.3.2.0 template cited in the header.
struct BaseQuantities {
    entity: &'static str,
    set: &'static str,
    quantities: &'static str,
}

const BASE_QUANTITIES: [BaseQuantities; 7] = [
    BaseQuantities {
        entity: "IfcCourse",
        set: "Qto_CourseBaseQuantities",
        quantities: "Length=L Width=L Thickness=L Volume=V GrossVolume=V Weight=W",
    },
    BaseQuantities {
        entity: "IfcEarthworksCut",
        set: "Qto_EarthworksCutBaseQuantities",
        quantities: "Length=L Width=L Depth=L UndisturbedVolume=V LooseVolume=V Weight=W",
    },
    BaseQuantities {
        entity: "IfcEarthworksFill",
        set: "Qto_EarthworksFillBaseQuantities",
        quantities: "Length=L Width=L Depth=L CompactedVolume=V LooseVolume=V",
    },
    BaseQuantities {
        entity: "IfcKerb",
        set: "Qto_KerbBaseQuantities",
        quantities: "Length=L Width=L Height=L Depth=L Volume=V Weight=W",
    },
    BaseQuantities {
        entity: "IfcPavement",
        set: "Qto_PavementBaseQuantities",
        quantities: "Length=L Width=L Depth=L GrossArea=A NetArea=A GrossVolume=V NetVolume=V",
    },
    BaseQuantities {
        entity: "IfcRailing",
        set: "Qto_RailingBaseQuantities",
        quantities: "Length=L",
    },
    BaseQuantities {
        entity: "IfcSign",
        set: "Qto_SignBaseQuantities",
        quantities: "Height=L Width=L Thickness=L Weight=W",
    },
];

/// The road model names its quantities by one scheme for every object -
/// GrossVolume for any solid's volume, Depth for a course's thickness (cad's
/// road sections) - and the classes' base quantity sets name theirs. Where the
/// model's name is not the set's, and the set gives the name its type, the
/// quantity is written under the set's: an earthworks cut's GrossVolume is
/// its UndisturbedVolume, a fill's its CompactedVolume (the in-place volume
/// the end areas measure), a kerb's its Volume, a course's Depth its
/// Thickness. Only when the element has no quantity of that name already; the
/// model's own names stay in its own reports.
struct QuantityAlias {
    entity: &'static str,
    from: &'static str,
    kind: QuantityKind,
    to: &'static str,
}

const QUANTITY_ALIASES: [QuantityAlias; 4] = [
    QuantityAlias {
        entity: "IfcCourse",
        from: "Depth",
        kind: QuantityKind::Length,
        to: "Thickness",
    },
    QuantityAlias {
        entity: "IfcKerb",
        from: "GrossVolume",
        kind: QuantityKind::Volume,
        to: "Volume",
    },
    QuantityAlias {
        entity: "IfcEarthworksCut",
        from: "GrossVolume",
        kind: QuantityKind::Volume,
        to: "UndisturbedVolume",
    },
    QuantityAlias {
        entity: "IfcEarthworksFill",
        from: "GrossVolume",
        kind: QuantityKind::Volume,
        to: "CompactedVolume",
    },
];

fn base_quantities_of(entity: &str) -> Option<&'static BaseQuantities> {
    BASE_QUANTITIES.iter().find(|row| row.entity == entity)
}

/// True when `row` defines `name` as a quantity of `kind`.
fn template_defines(row: &BaseQuantities, name: &str, kind: QuantityKind) -> bool {
    let letter = match kind {
        QuantityKind::Length => 'L',
        QuantityKind::Area => 'A',
        QuantityKind::Volume => 'V',
        QuantityKind::Weight => 'W',
        QuantityKind::Count => return false, // no template here defines a count
    };
    lists_word(row.quantities, &format!("{name}={letter}"))
}

/// The parts across a road's section, written LATERAL: most a "lateral part"
/// in IfcRoadPartTypeEnum's own definitions (7.9.2.2), the rest beside one.
const LATERAL_PARTS: [&str; 10] = [
    "CARRIAGEWAY",
    "TRAFFICLANE",
    "SHOULDER",
    "HARDSHOULDER",
    "SOFTSHOULDER",
    "SIDEWALK",
    "ROADSIDE",
    "ROADSIDEPART",
    "CENTRALRESERVE",
    "ROADWAYPLATEAU",
];

/// IfcRoadPart's PredefinedType, ObjectType and UsageType for a part named
/// `part`: the enumeration's own value where it lists it, else USERDEFINED of
/// that name.
struct PartType {
    predefined_type: String,
    object_type: String,
    usage: &'static str,
}

fn part_type_of(part: &str) -> PartType {
    let upper = upper(part);
    let values = predefined_types_of("IfcRoadPart");
    if !values.is_some_and(|values| lists_word(values, &upper)) || upper == "USERDEFINED" {
        return PartType {
            predefined_type: String::from("USERDEFINED"),
            object_type: String::from(part),
            usage: "NOTDEFINED",
        };
    }
    let usage = if upper == "NOTDEFINED" {
        "NOTDEFINED"
    } else if upper == "ROADSEGMENT" {
        "LONGITUDINAL"
    } else if LATERAL_PARTS.contains(&upper.as_str()) {
        "LATERAL"
    } else {
        "REGION"
    };
    PartType {
        predefined_type: upper,
        object_type: String::new(),
        usage,
    }
}

/// A mesh as a face set writes it. A mesh closed by its own indices is written
/// as it is (closed_by_index). Any other is face soup: each place once, in the
/// order the faces first name it, no face naming a place twice, and closed
/// only when, so merged, every edge is used once in each direction. Closed
/// face sets are wound outwards.
#[derive(Clone, Debug, Default)]
struct Tessellation {
    /// world
    points: Vec<Vec3>,
    faces: Vec<[u32; 3]>,
    closed: bool,
}

/// A closed face set is wound outwards: the signed volume (the divergence
/// theorem over the faces, about the first point to keep the products small)
/// is negative when the faces are wound inwards, and the winding is turned.
fn wind_outwards(out: &mut Tessellation) {
    let origin = out.points[0];
    let mut volume = 0.0;
    for face in &out.faces {
        let p = out.points[face[0] as usize] - origin;
        let q = out.points[face[1] as usize] - origin;
        let r = out.points[face[2] as usize] - origin;
        volume += p.dot(q.cross(r));
    }
    if volume < 0.0 {
        for face in &mut out.faces {
            face.swap(1, 2);
        }
    }
}

/// Every edge used once in each direction.
fn closed_edges(edges: &BTreeMap<(u32, u32), i32>) -> bool {
    edges.iter().all(|(&(from, to), &count)| {
        count == 1 && edges.get(&(to, from)).is_some_and(|&reverse| reverse == 1)
    })
}

/// The mesh as it is, when its own indices already close it: every edge used
/// once in each direction, no face naming a vertex twice. That is how the road
/// objects and the corridor's solids are built, and it is the only closure the
/// face set can claim - a consumer reads the topology from the indices. Places
/// at one position are NOT merged: several closed shells that meet (the cells
/// of a box culvert side by side, with no gap between their walls) share
/// positions without sharing an edge, and merging them would make four faces
/// of an edge and an open face set of a closed solid. None for a mesh that is
/// not closed by its indices - face soup from another source - which
/// tessellate then merges by position.
fn closed_by_index(mesh: &TriangleMesh) -> Option<Tessellation> {
    if mesh.faces.is_empty() {
        return None;
    }
    let mut edges: BTreeMap<(u32, u32), i32> = BTreeMap::new();
    for face in &mesh.faces {
        if face[0] == face[1] || face[1] == face[2] || face[2] == face[0] {
            return None;
        }
        for i in 0..3 {
            if face[i] as usize >= mesh.vertices.len() {
                return None;
            }
            *edges.entry((face[i], face[(i + 1) % 3])).or_insert(0) += 1;
        }
    }
    if !closed_edges(&edges) {
        return None;
    }
    let mut out = Tessellation {
        closed: true,
        ..Tessellation::default()
    };
    // A vertex no face names is not written.
    let mut renumbered = vec![0_u32; mesh.vertices.len()];
    let mut seen = vec![false; mesh.vertices.len()];
    for face in &mesh.faces {
        let mut written = [0_u32; 3];
        for (slot, &index) in written.iter_mut().zip(face) {
            let at = index as usize;
            if !seen[at] {
                seen[at] = true;
                renumbered[at] = out.points.len() as u32;
                out.points.push(mesh.vertices[at]);
            }
            *slot = renumbered[at];
        }
        out.faces.push(written);
    }
    wind_outwards(&mut out);
    Some(out)
}

/// The key of a place, as the C++'s map of `std::array<double, 3>` compares
/// them: -0 and +0 are one place. (The mesh is checked finite before.)
fn place_key(p: Vec3) -> [u64; 3] {
    let bits = |value: f64| {
        let unsigned = if value == 0.0 { 0.0_f64 } else { value };
        unsigned.to_bits()
    };
    [bits(p.x), bits(p.y), bits(p.z)]
}

fn tessellate(mesh: &TriangleMesh) -> Option<Tessellation> {
    if let Some(shared) = closed_by_index(mesh) {
        return Some(shared);
    }
    let mut out = Tessellation::default();
    let mut placed: HashMap<[u64; 3], u32> = HashMap::new();
    let mut index_of = |out: &mut Tessellation, p: Vec3| -> u32 {
        let key = place_key(p);
        if let Some(&found) = placed.get(&key) {
            return found;
        }
        let index = out.points.len() as u32;
        placed.insert(key, index);
        out.points.push(p);
        index
    };
    for face in &mesh.faces {
        let a = index_of(&mut out, mesh.vertices[face[0] as usize]);
        let b = index_of(&mut out, mesh.vertices[face[1] as usize]);
        let c = index_of(&mut out, mesh.vertices[face[2] as usize]);
        if a == b || b == c || c == a {
            continue;
        }
        out.faces.push([a, b, c]);
    }
    if out.faces.is_empty() {
        return None;
    }
    // A point only a dropped face named is not written.
    let mut renumbered = vec![0_u32; out.points.len()];
    let mut used: Vec<Vec3> = Vec::new();
    let mut seen = vec![false; out.points.len()];
    for face in &mut out.faces {
        for index in face.iter_mut() {
            let at = *index as usize;
            if !seen[at] {
                seen[at] = true;
                renumbered[at] = used.len() as u32;
                used.push(out.points[at]);
            }
            *index = renumbered[at];
        }
    }
    out.points = used;

    let mut edges: BTreeMap<(u32, u32), i32> = BTreeMap::new();
    for face in &out.faces {
        for i in 0..3 {
            *edges.entry((face[i], face[(i + 1) % 3])).or_insert(0) += 1;
        }
    }
    out.closed = closed_edges(&edges);
    if out.closed {
        wind_outwards(&mut out);
    }
    Some(out)
}

/// The level of `mesh` at plan point `at`: the highest of its faces over it,
/// None where none is.
fn level_at(mesh: &Tessellation, at: Vec2) -> Option<f64> {
    // Inside to within a nanometre of an edge, so a point on the edge
    // between two faces is on both rather than on neither.
    const ON_EDGE: f64 = 1e-9;
    let mut best: Option<f64> = None;
    for face in &mesh.faces {
        let a = mesh.points[face[0] as usize];
        let b = mesh.points[face[1] as usize];
        let c = mesh.points[face[2] as usize];
        let det = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
        if det.abs() < 1e-12 {
            continue; // vertical in plan: nothing lies on it from above
        }
        let u = ((b.y - c.y) * (at.x - c.x) + (c.x - b.x) * (at.y - c.y)) / det;
        let v = ((c.y - a.y) * (at.x - c.x) + (a.x - c.x) * (at.y - c.y)) / det;
        let w = 1.0 - u - v;
        if u < -ON_EDGE || v < -ON_EDGE || w < -ON_EDGE {
            continue;
        }
        let z = u * a.z + v * b.z + w * c.z;
        best = Some(match best {
            Some(best) => cpp::max(best, z),
            None => z,
        });
    }
    best
}

fn is_surface_class(entity: &str) -> bool {
    entity == "IfcCourse" || entity == "IfcPavement" || entity == "IfcSlab"
}

/// An element that will be written: its tessellation, and once written its
/// product and the placement it hangs off.
struct Accepted<'r> {
    element: &'r RoadElement,
    body: Tessellation,
    feature: bool,
    /// a feature that voids its host (ProductLayout::voids)
    voids: bool,
    product: Id,
    placement: Id,
}

#[derive(Clone, Debug, Default)]
struct Facility {
    road: Id,
    /// the road itself, for elements of no part
    container: Container,
    parts: BTreeMap<String, Container>,
}

#[derive(Clone, Debug, Default)]
struct Adhering {
    key: String,
    features: Vec<Id>,
}

/// Which container an element is placed in: the site, its road, or a part of
/// its road.
enum Place {
    Site,
    Road(String),
    Part(String, String),
}

struct RoadWriter<'b, 'a, 'r> {
    b: &'b mut Builder<'a>,
    road: &'r RoadModelInput,
    terrain: &'r BTreeMap<String, Id>,
    /// folded -> the first spelling
    facility_names: BTreeMap<String, String>,
    part_names: BTreeMap<(String, String), String>,
    facilities: BTreeMap<String, Facility>,
    part_order: BTreeMap<String, Vec<String>>,
    site: Container,
    accepted: Vec<Accepted<'r>>,
    /// an element's id -> its place in `accepted`
    written: BTreeMap<String, usize>,
    /// a host's placement -> its features'
    feature_placements: BTreeMap<Id, Id>,
    adherences: BTreeMap<Id, Adhering>,
    grounds: BTreeMap<String, Id>,
    materials: BTreeMap<String, Vec<Id>>,
    material_order: Vec<String>,
    units: BTreeSet<String>,
    open: usize,
    invalid_quantities: usize,
    fractional_counts: usize,
    repeated_quantities: usize,
    repeated: usize,
    unnamed: usize,
}

impl<'b, 'a, 'r> RoadWriter<'b, 'a, 'r> {
    fn new(
        builder: &'b mut Builder<'a>,
        road: &'r RoadModelInput,
        terrain: &'r BTreeMap<String, Id>,
    ) -> Self {
        Self {
            b: builder,
            road,
            terrain,
            facility_names: BTreeMap::new(),
            part_names: BTreeMap::new(),
            facilities: BTreeMap::new(),
            part_order: BTreeMap::new(),
            site: Container::default(),
            accepted: Vec::new(),
            written: BTreeMap::new(),
            feature_placements: BTreeMap::new(),
            adherences: BTreeMap::new(),
            grounds: BTreeMap::new(),
            materials: BTreeMap::new(),
            material_order: Vec::new(),
            units: BTreeSet::new(),
            open: 0,
            invalid_quantities: 0,
            fractional_counts: 0,
            repeated_quantities: 0,
            repeated: 0,
            unnamed: 0,
        }
    }

    fn write(&mut self) {
        let road = self.road;
        for problem in &road.problems {
            self.b.warn(format!("the road model: {problem}"));
        }
        for element in &road.elements {
            self.name_first(element);
        }
        let mut ids = BTreeSet::new();
        for element in &road.elements {
            if let Some(ready) = self.accept(element, &mut ids) {
                self.accepted.push(ready);
            }
        }
        self.write_facilities();
        for index in 0..self.accepted.len() {
            if !self.accepted[index].feature {
                self.write_element(index);
            }
        }
        for index in 0..self.accepted.len() {
            if self.accepted[index].feature {
                self.write_feature(index);
            }
        }
        self.write_adherences();
        self.write_materials();
        self.say_once();
    }

    /// A facility is a corridor's name, and Katana finds a corridor ignoring
    /// case (CorridorSet::find, "found ignoring case" in docs/road_design.md),
    /// so a road object that says facility=main beside the corridor Main is on
    /// Main. Matching the spelling exactly made it a second IfcRoad with a part
    /// of its own, and a line marking of it found no course "of its road"
    /// under it and was left out. A facility, and a part within it, are written
    /// under the spelling of the first element of the model that names them,
    /// in any letter case: the IfcRoad's and IfcRoadPart's names and GlobalIds
    /// come from that one. (Rejected: folding the case where the app converts
    /// the model - it would leave every other caller of the writer to repeat
    /// it.)
    fn name_first(&mut self, element: &RoadElement) {
        if element.facility.is_empty() {
            return;
        }
        let facility = lower(&element.facility);
        self.facility_names
            .entry(facility.clone())
            .or_insert_with(|| element.facility.clone());
        if !element.part.is_empty() {
            self.part_names
                .entry((facility, lower(&element.part)))
                .or_insert_with(|| element.part.clone());
        }
    }

    /// The facility and part an element is written under (name_first); empty
    /// for none.
    fn facility_of(&self, element: &RoadElement) -> String {
        if element.facility.is_empty() {
            return String::new();
        }
        self.facility_names
            .get(&lower(&element.facility))
            .cloned()
            .unwrap_or_default()
    }

    fn part_of(&self, element: &RoadElement) -> String {
        // A part with no facility is no part: the element is the site's.
        if element.part.is_empty() || element.facility.is_empty() {
            return element.part.clone();
        }
        self.part_names
            .get(&(lower(&element.facility), lower(&element.part)))
            .cloned()
            .unwrap_or_default()
    }

    fn source_of(&self, element: &RoadElement) -> String {
        if element.facility.is_empty() {
            String::from("road model")
        } else {
            format!("road {}", self.facility_of(element))
        }
    }

    fn why_of(element: &RoadElement) -> String {
        format!(
            "the road model's {}",
            if element.kind.is_empty() {
                "object"
            } else {
                element.kind.as_str()
            }
        )
    }

    fn skip(&mut self, element: &RoadElement, reason: &str) {
        self.b.report().road_elements_skipped += 1;
        self.b.warn(format!(
            "road element \"{}\" is not written: {reason}",
            element.id
        ));
        let source = self.source_of(element);
        self.b.tally(
            &source,
            &IfcClass::default(),
            "",
            &format!("not written: {reason}"),
        );
    }

    /// Checks what would make an invalid file, and tessellates; None, said,
    /// for an element that is not written.
    fn accept(
        &mut self,
        element: &'r RoadElement,
        ids: &mut BTreeSet<String>,
    ) -> Option<Accepted<'r>> {
        if element.id.is_empty() {
            self.skip(
                element,
                "it has no id, from which its GlobalId would be made",
            );
            return None;
        }
        if !ids.insert(element.id.clone()) {
            self.skip(element, "an earlier element has the same id");
            return None;
        }
        let ifc_class = &element.ifc_class;
        if ifc_class.entity.is_empty() {
            self.skip(element, "it names no IFC class");
            return None;
        }
        let layout = find_product_layout(&ifc_class.entity);
        let Some(layout) = layout.filter(|_| ifc_class.entity != "IfcAnnotation") else {
            self.skip(
                element,
                &format!(
                    "{} is not a class this export writes for an element",
                    ifc_class.entity
                ),
            );
            return None;
        };
        if !ifc_class.predefined_type.is_empty() {
            let values = predefined_types_of(&ifc_class.entity);
            let listed = layout.has_predefined_type
                && values.is_some_and(|values| lists_word(values, &ifc_class.predefined_type));
            if !listed {
                self.skip(
                    element,
                    &format!(
                        "\"{}\" is not a predefined type of {}",
                        ifc_class.predefined_type, ifc_class.entity
                    ),
                );
                return None;
            }
        }
        if ifc_class.predefined_type == "USERDEFINED" && ifc_class.object_type.is_empty() {
            self.skip(
                element,
                "it is USERDEFINED and says nothing of what it is (its object type)",
            );
            return None;
        }
        if let Err(error) = validate(&element.mesh) {
            self.skip(
                element,
                &format!("its mesh is not valid: {}", error.describe()),
            );
            return None;
        }
        for line in &element.plan {
            for vertex in &line.vertices {
                if !vertex.x.is_finite() || !vertex.y.is_finite() {
                    self.skip(element, "its plan outline has a vertex that is not finite");
                    return None;
                }
            }
        }
        let Some(body) = tessellate(&element.mesh) else {
            self.skip(element, "its mesh has no face");
            return None;
        };
        Some(Accepted {
            element,
            body,
            feature: layout.feature,
            voids: layout.voids,
            product: 0,
            placement: 0,
        })
    }

    fn write_facilities(&mut self) {
        let mut roads = Vec::new();
        for index in 0..self.accepted.len() {
            let element = self.accepted[index].element;
            if element.facility.is_empty() || self.accepted[index].feature {
                continue; // a feature is contained nowhere
            }
            let facility = self.facility_of(element);
            if !self.facilities.contains_key(&facility) {
                let written = self.write_road(&facility);
                roads.push(written.road);
                self.facilities.insert(facility.clone(), written);
            }
            let part = self.part_of(element);
            let known = self
                .facilities
                .get(&facility)
                .is_some_and(|written| written.parts.contains_key(&part));
            if !part.is_empty() && !known {
                let road_placement = self
                    .facilities
                    .get(&facility)
                    .map_or(0, |written| written.container.placement);
                let container = self.write_part(&facility, road_placement, &part);
                if let Some(written) = self.facilities.get_mut(&facility) {
                    written.parts.insert(part.clone(), container);
                }
                self.part_order.entry(facility).or_default().push(part);
            }
        }
        let site = self.b.site();
        self.b.aggregate(site, "site", &roads);
        let names: Vec<String> = self.facilities.keys().cloned().collect();
        for name in names {
            let order = self.part_order.get(&name).cloned().unwrap_or_default();
            let Some(facility) = self.facilities.get(&name) else {
                continue;
            };
            let road = facility.road;
            let parts: Vec<Id> = order
                .iter()
                .filter_map(|part| facility.parts.get(part).map(|container| container.element))
                .collect();
            self.b.aggregate(road, &format!("road/{name}"), &parts);
        }
    }

    fn write_road(&mut self, name: &str) -> Facility {
        let key = format!("road/facility/{name}");
        let site_placement = self.b.site_placement();
        let placement = self.b.placement_in(site_placement);
        let guid = self.b.guid(&key);
        let label = self.b.label(name, &format!("IfcRoad {name}"));
        let road = self.b.file().add(
            "IfcRoad",
            Args::new()
                .string(&guid)
                .null()
                .string_or_null(&label)
                .null()
                .null()
                .reference(placement)
                .null()
                .null()
                .enumeration("ELEMENT")
                .enumeration("NOTDEFINED"),
        );
        *self
            .b
            .report()
            .classes
            .entry(String::from("IfcRoad"))
            .or_insert(0) += 1;
        self.b.report().roads += 1;
        // In the account too, as every product the report counts is
        // (IfcExportTally): the preview's table is filled from it, and its
        // count of what would be written is a sum of it.
        self.b.tally(
            &format!("road {name}"),
            &IfcClass::new("IfcRoad", "NOTDEFINED", ""),
            "",
            "a corridor, the road its parts and elements are in",
        );
        Facility {
            road,
            container: Container {
                element: road,
                placement,
                element_placement: 0,
                key: format!("road/{name}"),
            },
            parts: BTreeMap::new(),
        }
    }

    fn write_part(&mut self, facility: &str, road_placement: Id, part: &str) -> Container {
        let part_type = part_type_of(part);
        let key = format!("road/facility/{facility}/part/{part}");
        let placement = self.b.placement_in(road_placement);
        let name = format!("{facility} {part}");
        let guid = self.b.guid(&key);
        let label = self.b.label(&name, &format!("IfcRoadPart {name}"));
        let object_type = self
            .b
            .label(&part_type.object_type, &format!("IfcRoadPart {name}"));
        let id = self.b.file().add(
            "IfcRoadPart",
            Args::new()
                .string(&guid)
                .null()
                .string_or_null(&label)
                .null()
                .string_or_null(&object_type)
                .reference(placement)
                .null()
                .null()
                .enumeration("PARTIAL")
                .enumeration(part_type.usage)
                .enumeration(&part_type.predefined_type),
        );
        *self
            .b
            .report()
            .classes
            .entry(String::from("IfcRoadPart"))
            .or_insert(0) += 1;
        self.b.report().road_parts += 1;
        self.b.tally(
            &format!("road {facility}"),
            &IfcClass::new(
                "IfcRoadPart",
                part_type.predefined_type.clone(),
                part_type.object_type.clone(),
            ),
            "",
            &format!("a part of road {facility}"),
        );
        Container {
            element: id,
            placement,
            element_placement: 0,
            key: format!("road/{facility}/{part}"),
        }
    }

    fn place_of(&self, element: &RoadElement) -> Place {
        if element.facility.is_empty() {
            Place::Site
        } else if element.part.is_empty() {
            Place::Road(self.facility_of(element))
        } else {
            Place::Part(self.facility_of(element), self.part_of(element))
        }
    }

    /// The container an element is contained in, its element placement made
    /// the first time one is placed in it.
    fn container_of(&mut self, element: &RoadElement) -> Container {
        let place = self.place_of(element);
        let container = match &place {
            Place::Site => {
                self.site = self.b.site_container();
                return self.site.clone();
            }
            Place::Road(facility) => self
                .facilities
                .get(facility)
                .map(|written| written.container.clone()),
            Place::Part(facility, part) => self
                .facilities
                .get(facility)
                .and_then(|written| written.parts.get(part).cloned()),
        };
        let mut container = container.unwrap_or_default();
        if container.element_placement == 0 {
            container.element_placement = self.b.placement_in(container.placement);
            // Remembered, so the next element of the container hangs off the
            // same placement.
            match &place {
                Place::Road(facility) => {
                    if let Some(written) = self.facilities.get_mut(facility) {
                        written.container.element_placement = container.element_placement;
                    }
                }
                Place::Part(facility, part) => {
                    if let Some(stored) = self
                        .facilities
                        .get_mut(facility)
                        .and_then(|written| written.parts.get_mut(part))
                    {
                        stored.element_placement = container.element_placement;
                    }
                }
                Place::Site => {}
            }
        }
        container
    }

    /// The body and the footprint, styled and on the element's layer.
    fn representation_of(&mut self, index: usize) -> Id {
        let element = self.accepted[index].element;
        let mut coordinates = Vec::with_capacity(self.accepted[index].body.points.len());
        for &point in &self.accepted[index].body.points {
            let at = self.b.local_3d(point);
            coordinates.push(list_of(&[
                step_real(at.x),
                step_real(at.y),
                step_real(at.z),
            ]));
        }
        let mut triangles = Vec::with_capacity(self.accepted[index].body.faces.len());
        for face in &self.accepted[index].body.faces {
            triangles.push(list_of(&[
                format!("{}", face[0] + 1),
                format!("{}", face[1] + 1),
                format!("{}", face[2] + 1),
            ]));
        }
        let closed = self.accepted[index].body.closed;
        let points = self.b.file().add(
            "IfcCartesianPointList3D",
            Args::new().raw(&list_of(&coordinates)).null(),
        );
        let faces = self.b.file().add(
            "IfcTriangulatedFaceSet",
            Args::new()
                .reference(points)
                .null()
                .boolean(closed)
                .raw(&list_of(&triangles))
                .null(),
        );
        if !closed {
            self.open += 1;
        }
        self.b.colour_surface(faces, &element.colour);
        let context = self.b.body_context();
        let body = self.b.shape(context, "Body", "Tessellation", &[faces]);
        self.b.layer(&element.layer, body);
        let mut representations = vec![body];

        let mut outline = Vec::new();
        let mut add_outline = |b: &mut Builder<'a>, vertices: &[Vec2], closed: bool| {
            if vertices.len() >= 2 {
                let curve = b.polyline(vertices, closed);
                b.colour_curve(curve, &element.colour);
                outline.push(curve);
            }
        };
        if !element.plan.is_empty() {
            for line in &element.plan {
                add_outline(self.b, &line.vertices, line.closed);
            }
        } else {
            let hull = element.mesh.plan_hull();
            add_outline(self.b, &hull, hull.len() >= 3);
        }
        if !outline.is_empty() {
            let context = self.b.foot_print_context();
            let foot_print = self.b.shape(context, "FootPrint", "Curve2D", &outline);
            self.b.layer(&element.layer, foot_print);
            representations.push(foot_print);
        }
        self.b.product_shape(&representations)
    }

    fn write_element(&mut self, index: usize) {
        let element = self.accepted[index].element;
        let shape = self.representation_of(index);
        let container = self.container_of(element);
        let product = self.b.product_in(
            &element.ifc_class,
            &key_of(element),
            &element.name,
            &element.kind,
            shape,
            &element.id,
            &container,
        );
        self.accepted[index].product = product;
        self.accepted[index].placement = container.element_placement;
        self.written.entry(element.id.clone()).or_insert(index);
        self.finish_element(index);
    }

    fn write_feature(&mut self, index: usize) {
        let element = self.accepted[index].element;
        let voids = self.accepted[index].voids;
        let host: Id;
        let host_placement: Id;
        let host_key: String;
        if !element.host.is_empty() {
            let Some(&found) = self.written.get(&element.host) else {
                self.skip(
                    element,
                    &format!("the element it is on, \"{}\", is not written", element.host),
                );
                return;
            };
            host = self.accepted[found].product;
            host_placement = self.accepted[found].placement;
            host_key = format!("road/element/{}", element.host);
        } else if voids {
            let (ground, ground_key) = self.ground_of(element);
            host = ground;
            host_key = ground_key;
            host_placement = self.b.element_placement();
        } else {
            let Some(under) = self.surface_under(index) else {
                self.skip(
                    element,
                    if element.facility.is_empty() {
                        "a surface feature adheres to an element, and it names no road \
                         (facility=) and no course, pavement or slab of any road lies under it"
                    } else {
                        "a surface feature adheres to an element, and no course, pavement or \
                         slab of its road lies under it"
                    },
                );
                return;
            };
            host = self.accepted[under].product;
            host_placement = self.accepted[under].placement;
            host_key = format!("road/element/{}", self.accepted[under].element.id);
        }
        let placement = match self.feature_placements.get(&host_placement) {
            Some(&placement) => placement,
            None => {
                let placement = self.b.placement_in(host_placement);
                self.feature_placements.insert(host_placement, placement);
                placement
            }
        };
        let shape = self.representation_of(index);
        let product = self.b.feature(
            &element.ifc_class,
            &key_of(element),
            &element.name,
            &element.kind,
            shape,
            &element.id,
            placement,
        );
        self.accepted[index].product = product;
        self.accepted[index].placement = placement;
        if voids {
            let guid = self.b.guid(&format!("voids/{}", key_of(element)));
            self.b.file().add(
                "IfcRelVoidsElement",
                Args::new()
                    .string(&guid)
                    .null()
                    .null()
                    .null()
                    .reference(host)
                    .reference(product),
            );
        } else {
            let features = self.adherences.entry(host).or_default();
            features.key = host_key;
            features.features.push(product);
        }
        self.finish_element(index);
    }

    /// The element a cut with no host voids: the surface it was taken from,
    /// where the file holds it, else a TERRAIN standing for it - one a ground.
    fn ground_of(&mut self, element: &RoadElement) -> (Id, String) {
        if let Some(&surface) = self.terrain.get(&element.ground) {
            return (surface, format!("surface/{}", element.ground));
        }
        let name = if element.ground.is_empty() {
            String::from("Existing ground")
        } else {
            element.ground.clone()
        };
        let key = format!("road/ground/{name}");
        let id = match self.grounds.get(&name) {
            Some(&id) => id,
            None => {
                let terrain = IfcClass::new("IfcGeographicElement", "TERRAIN", "");
                let id = self.b.product(
                    &terrain,
                    &key,
                    &name,
                    "the ground the road's earthworks are cut from; its surface is not in this \
                     file",
                    0,
                    "",
                );
                self.b.tally(
                    "road model",
                    &terrain,
                    "",
                    "the ground a cut is taken from, not in the file",
                );
                self.grounds.insert(name, id);
                id
            }
        };
        (id, key)
    }

    /// The course, pavement or slab of the marking's road under its first
    /// face's centre, the highest there; None when none is. A marking that
    /// names no road is the site's, and lies on the surface of whichever road
    /// is under it: a road object's facility is empty unless it is given, and
    /// dropping every default marking from the file for want of a road it was
    /// never told would lose them all.
    fn surface_under(&self, index: usize) -> Option<usize> {
        let item = &self.accepted[index];
        let face = item.body.faces[0];
        let centre = (item.body.points[face[0] as usize]
            + item.body.points[face[1] as usize]
            + item.body.points[face[2] as usize])
            * (1.0 / 3.0);
        let at = Vec2::new(centre.x, centre.y);
        let mut best: Option<usize> = None;
        let mut best_level = 0.0;
        for candidate in self.written_in_order() {
            let element = self.accepted[candidate].element;
            if (!item.element.facility.is_empty()
                && !equals_ignoring_case(
                    element.facility.as_bytes(),
                    item.element.facility.as_bytes(),
                ))
                || !is_surface_class(&element.ifc_class.entity)
            {
                continue;
            }
            let level = level_at(&self.accepted[candidate].body, at);
            if let Some(level) = level
                && (best.is_none() || level > best_level)
            {
                best = Some(candidate);
                best_level = level;
            }
        }
        best
    }

    /// The written elements in the order they were written, so that of two at
    /// one level the first is chosen whatever their ids.
    fn written_in_order(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.written.values().copied().collect();
        out.sort_by_key(|&index| self.accepted[index].product);
        out
    }

    /// What every written element carries: its account, its properties, its
    /// quantities and its material.
    fn finish_element(&mut self, index: usize) {
        let element = self.accepted[index].element;
        let product = self.accepted[index].product;
        let closed = self.accepted[index].body.closed;
        let voids = self.accepted[index].voids;
        let key = key_of(element);
        self.b.report().road_elements += 1;
        let source = self.source_of(element);
        self.b
            .tally(&source, &element.ifc_class, "", &Self::why_of(element));

        let mut own = PropertyList::new();
        own.identifier("Id", &element.id);
        own.label("Kind", &element.kind);
        own.label("Facility", &element.facility);
        own.label("Part", &element.part);
        own.label("Layer", &element.layer);
        own.label("Material", &element.material);
        own.boolean("ClosedSolid", Some(closed));
        let set = self
            .b
            .property_set(&format!("{key}/element"), "Katana_RoadElement", &own);
        self.b.defines(&format!("{key}/element"), set, &[product]);

        let mut properties = PropertyList::new();
        let mut named = BTreeSet::new();
        for (name, value) in &element.properties {
            if name.is_empty() || character_count(name) > 255 {
                self.unnamed += 1;
                continue;
            }
            if !named.insert(name.clone()) {
                self.repeated += 1;
                continue;
            }
            properties.label(name, value);
        }
        let set = self.b.property_set(
            &format!("{key}/properties"),
            "Katana_RoadProperties",
            &properties,
        );
        self.b
            .defines(&format!("{key}/properties"), set, &[product]);

        self.write_quantities(element, product, &key);
        // A void takes no material (IfcRelAssociatesMaterial's NoVoidElement):
        // a cut's - the ground it removes - is its Katana_RoadElement Material.
        if !element.material.is_empty() && !voids {
            let objects = self.materials.entry(element.material.clone()).or_default();
            if objects.is_empty() {
                self.material_order.push(element.material.clone());
            }
            objects.push(product);
        }
    }

    fn write_quantities(&mut self, element: &RoadElement, product: Id, key: &str) {
        let base = base_quantities_of(&element.ifc_class.entity);
        let mut standard = Vec::new();
        let mut own = Vec::new();
        let mut named = BTreeSet::new();
        for quantity in &element.quantities {
            let Some(value) = measured(quantity) else {
                self.units.insert(quantity.unit.clone());
                continue;
            };
            if quantity.name.is_empty() || !value.value.is_finite() || value.value < 0.0 {
                self.invalid_quantities += 1; // the schema's WR: a quantity's value is not negative
                continue;
            }
            let mut count: Option<i64> = None;
            if value.kind == QuantityKind::Count {
                count = whole_count(value.value);
                if count.is_none() {
                    self.fractional_counts += 1; // before the name is taken: nothing was written
                    continue;
                }
            }
            // The name the class's base quantity set gives it, where the
            // model's own is not one of the set's.
            let mut name = quantity.name.clone();
            if let Some(base) = base
                && !template_defines(base, &name, value.kind)
            {
                for alias in &QUANTITY_ALIASES {
                    if alias.entity == element.ifc_class.entity
                        && alias.from == name
                        && alias.kind == value.kind
                        && !element.quantities.iter().any(|each| each.name == alias.to)
                    {
                        name = String::from(alias.to);
                        break;
                    }
                }
            }
            if !named.insert(name.clone()) {
                self.repeated_quantities += 1; // UniqueQuantityNames, within the element
                continue;
            }
            let label = self
                .b
                .label(&name, &format!("a quantity of {}", element.id));
            let mut args = Args::new();
            args.string(&label).null();
            if value.kind == QuantityKind::Weight {
                let kilogram = self.b.kilogram();
                args.reference(kilogram);
            } else {
                args.null(); // the project's unit (write_project_frame)
            }
            match count {
                Some(count) => args.integer(count).null(), // INTEGER, not REAL (whole_count)
                None => args.real(value.value).null(),
            };
            let id = self.b.file().add(entity_of(value.kind), &args);
            if base.is_some_and(|base| template_defines(base, &name, value.kind)) {
                standard.push(id);
            } else {
                own.push(id);
            }
        }
        let set = |b: &mut Builder<'_>, name: &str, method: &str, quantities: &[Id]| {
            if quantities.is_empty() {
                return;
            }
            let set_key = format!("{key}/{name}");
            let guid = b.guid(&format!("qto/{set_key}"));
            let id = b.file().add(
                "IfcElementQuantity",
                Args::new()
                    .string(&guid)
                    .null()
                    .string(name)
                    .null()
                    .string_or_null(method)
                    .refs(quantities),
            );
            b.defines(&set_key, id, &[product]);
        };
        if let Some(base) = base {
            set(self.b, base.set, "BaseQuantities", &standard);
        }
        set(self.b, "Katana_Quantities", "", &own);
    }

    fn write_adherences(&mut self) {
        let adherences = std::mem::take(&mut self.adherences);
        for (host, adhering) in &adherences {
            let guid = self.b.guid(&format!("adheres/{}", adhering.key));
            self.b.file().add(
                "IfcRelAdheresToElement",
                Args::new()
                    .string(&guid)
                    .null()
                    .null()
                    .null()
                    .reference(*host)
                    .refs(&adhering.features),
            );
        }
        self.adherences = adherences;
    }

    fn write_materials(&mut self) {
        let order = self.material_order.clone();
        for name in &order {
            let label = self.b.label(name, &format!("the material {name}"));
            let material = self
                .b
                .file()
                .add("IfcMaterial", Args::new().string(&label).null().null());
            let guid = self.b.guid(&format!("material/{name}"));
            let objects = self.materials.get(name).cloned().unwrap_or_default();
            self.b.file().add(
                "IfcRelAssociatesMaterial",
                Args::new()
                    .string(&guid)
                    .null()
                    .null()
                    .null()
                    .refs(&objects)
                    .reference(material),
            );
        }
    }

    /// What was passed over in many elements, said once rather than once an
    /// element.
    fn say_once(&mut self) {
        if self.open > 0 {
            self.b.warn(format!(
                "{} road elements are not closed solids - an edge is not shared by exactly two \
                 faces - and are written as open face sets",
                self.open
            ));
        }
        if !self.units.is_empty() {
            let mut listed = String::new();
            for unit in &self.units {
                listed.push_str(if listed.is_empty() { "\"" } else { ", \"" });
                listed.push_str(unit);
                listed.push('"');
            }
            self.b.warn(format!(
                "road element quantities in {listed} are left out: a quantity is in m, m2, m3, \
                 kg or t, or is a count"
            ));
        }
        if self.invalid_quantities > 0 {
            self.b.warn(format!(
                "{} road element quantities are negative, not finite or unnamed and are left out",
                self.invalid_quantities
            ));
        }
        if self.fractional_counts > 0 {
            self.b.warn(format!(
                "{} road element counts are not whole numbers, or are beyond 2147483647, and are \
                 left out: IfcQuantityCount takes an INTEGER in IFC4X3_ADD2",
                self.fractional_counts
            ));
        }
        if self.repeated_quantities > 0 || self.repeated > 0 {
            self.b.warn(format!(
                "{} road element quantities or properties repeat a name on their element; the \
                 first of each is written",
                self.repeated_quantities + self.repeated
            ));
        }
        if self.unnamed > 0 {
            self.b.warn(format!(
                "{} road element properties have no name, or one longer than IFC's 255 \
                 characters, and are left out",
                self.unnamed
            ));
        }
    }
}

fn key_of(element: &RoadElement) -> String {
    format!("road/element/{}", element.id)
}

pub(crate) fn export_road(
    builder: &mut Builder<'_>,
    road: &RoadModelInput,
    terrain: &BTreeMap<String, Id>,
) {
    RoadWriter::new(builder, road, terrain).write();
}
