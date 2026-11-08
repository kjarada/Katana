//! The road model's 3D objects - horizontal BIM: every pavement course, kerb,
//! footpath, verge, batter, barrier, sign, light pole, culvert and wall of a
//! road (include/katana/cad/bim/bim_object.hpp, src/katana_cad/bim/
//! bim_object.cpp, src/katana_cad/road/corridor_objects.cpp and the C++
//! road_objects.cpp's placedObjects; docs/road_design.md,
//! docs/road_objects.md).
//!
//! An object is MADE, every time, from the drawing's definitions: a corridor
//! and the road template it sweeps along an alignment and its profile
//! (road::corridor_model, road::road_template), or an object placed along an
//! alignment or a line (road::road_objects). The definitions are the data -
//! saved with the project, edited by verbs as one undo step - and these
//! objects are what the 3D view, the plan view, the quantities and the IFC
//! export read. None of them is stored, so none can disagree with the
//! definition it came from.
//!
//! A definition that cannot be built (an alignment with no profile, a station
//! off the alignment, a template with no component) builds what it can and
//! says what it could not in `problems`, a list an agent reads - the views
//! still draw the rest.
//!
//! How the views show the model (the C++ bim_view.hpp, whose three functions
//! the C++ keeps in bim_object.cpp) is bim::bim_view's.

use std::sync::{Arc, Weak};

use katana_entity::Color;
use katana_geometry::{Polyline2, TriangleMesh};
use katana_terrain::TinSurface;

use crate::document::{CORRIDORS_KEY, Document, ROAD_OBJECTS_KEY, ROAD_TEMPLATES_KEY};
use crate::road::road_objects::{build_road_object, placement_context};
use crate::road::road_section::{build_corridor_in, corridor_solids};

/// A measured quantity of an object, named as IFC's base quantities are
/// (Length, Width, Depth, Height, GrossArea, NetArea, GrossVolume, NetVolume,
/// Count, Weight) so the IFC export writes them as they are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quantity {
    pub name: String,
    pub value: f64,
    /// "m", "m2", "m3", "" for a count.
    pub unit: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BimObject {
    /// Stable: the same definition gives the same id on every run and every
    /// machine ("corridor/Main/course/AC14/left/3"), since the IFC export
    /// derives the GlobalId from it and a model handed over twice must name
    /// the same things the same way.
    pub id: String,
    pub name: String,
    /// What it is, in Katana's words: "pavement", "kerb", "footpath",
    /// "verge", "batter", "earthworks", "barrier", "sign", "light", "culvert",
    /// "wall", "marking", "guidepost", "fence".
    pub kind: String,
    /// The facility it belongs to - a corridor's name, which the IFC export
    /// writes as an IfcRoad - and the part of it, as IfcRoadPart's predefined
    /// type: CARRIAGEWAY, TRAFFICLANE, SHOULDER, SIDEWALK, ROADSIDE,
    /// CENTRALRESERVE, ROADSEGMENT. Empty: an object of the site.
    pub facility: String,
    pub part: String,
    /// How IFC 4.3 names it: the class (IfcCourse, IfcPavement, IfcKerb,
    /// IfcRailing, IfcSign, IfcEarthworksFill, IfcEarthworksCut, ...) and its
    /// predefined type, both as the schema spells them.
    pub ifc_class: String,
    pub predefined_type: String,
    pub material: String,
    pub colour: Color,
    /// In world coordinates. Closed where the object is a solid.
    pub mesh: TriangleMesh,
    /// What the plan view draws of it: its outline, or its line for a thin
    /// object (a barrier, a line marking). Empty: the plan view draws the
    /// mesh's plan hull.
    pub plan: Vec<Polyline2>,
    pub quantities: Vec<Quantity>,
    /// Further attributes, in order: the template component, the station
    /// range, the course thickness, the sign code...
    pub properties: Vec<(String, String)>,
    /// The drawing layer it is shown on; its visibility follows the layer's.
    pub layer: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BimProblem {
    /// The definition: "corridor Main", "object sign-3".
    pub source: String,
    /// What could not be built, and why.
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BimModel {
    pub objects: Vec<BimObject>,
    pub problems: Vec<BimProblem>,
}

/// How many times bim_model has made each half of a document's model - the
/// corridors', the placed objects' - rather than found it as it was. What the
/// tests ask to show that an edit of nothing a half is made from makes it
/// again no more: the model it hands out is the same object either way when
/// the result is equal, so only the work differs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BimBuildCounts {
    pub corridors: usize,
    pub placed: usize,
}

/// The objects of every corridor of the drawing, corridor by corridor in the
/// order they were defined (src/katana_cad/road/corridor_objects.cpp): made
/// from the corridors and the templates each time (road::road_section).
pub fn corridor_objects(document: &Document) -> BimModel {
    let mut model = BimModel::default();
    // Definitions that cannot be read build nothing, and say so: a project a
    // newer Katana wrote, or one damaged by hand.
    if let Err(error) = document.road_templates_status() {
        model.problems.push(BimProblem {
            source: String::from("road templates"),
            message: error.describe(),
        });
    }
    if let Err(error) = document.corridors_status() {
        model.problems.push(BimProblem {
            source: String::from("corridors"),
            message: error.describe(),
        });
    }
    let corridors = document.corridors();
    for corridor in &corridors.corridors {
        let made = corridor_solids(&build_corridor_in(document, corridor));
        model.objects.extend(made.objects);
        model.problems.extend(made.problems);
    }
    model
}

/// The objects placed along an alignment or a line, in the order they were
/// placed (the C++ road_objects.cpp).
pub fn placed_objects(document: &Document) -> BimModel {
    let mut model = BimModel::default();
    if let Err(error) = document.road_objects_status() {
        model.problems.push(BimProblem {
            source: String::from("road objects"),
            message: error.describe(),
        });
    }
    let context = placement_context(document);
    let objects = document.road_objects();
    for object in &objects.objects {
        let one = build_road_object(&context, object);
        model.objects.extend(one.objects);
        model.problems.extend(one.problems);
    }
    model
}

// What the road model was last made from, and what was made. Two halves, each
// kept on what it is made from, because they are made from different things
// (docs/road_design.md, "The cache"): the corridors - the templates and the
// corridors (their text), the alignments, and the surfaces the corridors
// TARGET (a corridor's `surface`, its batters' daylight and its earthworks'
// ground; it reads no other); the placed objects - the `road_objects` key
// (their text), the alignments and the surfaces they NAME
// (RoadObjectPlacement::surface; a line they follow was copied in when they
// were placed). Every other command - a line drawn, a layer, a surface no one
// names - leaves both halves as they were.
//
// A surface is told apart by identity, not by its name or the store's
// generation: an edit of a surface replaces the object (SurfaceStore::replace),
// so a corridor's ground that is the same object is the same ground, and one
// that is not is not, whatever else in the store moved. An undo that puts back
// the old object is a change too. The cache holds each as a Weak, which keeps
// the allocation - and so the identity - alive after the surface itself is
// gone, so a new surface cannot be taken for the one it replaced by being made
// at the same address (the C++'s weak_ptr and its control block).
//
// A half made again that equals what it replaced keeps the old one, so the
// model a view holds stays the same object until something in it changes.
type Grounds = Vec<Option<Weak<TinSurface>>>;

// The C++ `Document::BimCache`, kept in the document's bim_cache slot.
#[derive(Default)]
struct BimCache {
    // road_templates, corridors
    corridor_texts: [Option<String>; 2],
    alignments: u64,
    corridor_grounds: Grounds,
    corridors: Option<Arc<BimModel>>,

    // road_objects
    objects_text: Option<String>,
    placed_alignments: u64,
    placed_grounds: Grounds,
    placed: Option<Arc<BimModel>>,

    combined: Option<Arc<BimModel>>,

    // Times each half was made (bim_build_counts).
    built: BimBuildCounts,
}

fn key_text<'a>(document: &'a Document, key: &str) -> Option<&'a String> {
    document.metadata().unknown_keys.get(key)
}

// Whether `seen` is what the key holds now; set to it when not.
fn same(seen: &mut Option<String>, now: Option<&String>) -> bool {
    let Some(now) = now else {
        let was = seen.is_none();
        *seen = None;
        return was;
    };
    if seen.as_ref() == Some(now) {
        return true;
    }
    *seen = Some(now.clone());
    false
}

// The surface the drawing has under `name` (found as the verbs find it,
// ignoring case), or none for an empty name and one the drawing lacks.
fn add_ground(document: &Document, name: &str, grounds: &mut Grounds) {
    let found = if name.is_empty() {
        None
    } else {
        document.surfaces().find(name)
    };
    grounds.push(
        found
            .and_then(|named| named.surface.as_ref())
            .map(Arc::downgrade),
    );
}

// One for each corridor, in order: the surface it targets.
fn corridor_grounds(document: &Document) -> Grounds {
    let mut grounds = Grounds::new();
    for corridor in &document.corridors().corridors {
        add_ground(document, &corridor.surface, &mut grounds);
    }
    grounds
}

// One for each placed object, in order: the surface it names.
fn placed_grounds(document: &Document) -> Grounds {
    let mut grounds = Grounds::new();
    for object in &document.road_objects().objects {
        add_ground(document, &object.placement.surface, &mut grounds);
    }
    grounds
}

// The same surfaces: the same objects, or none in the same places.
fn same_grounds(a: &[Option<Weak<TinSurface>>], b: &[Option<Weak<TinSurface>>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|pair| match pair {
            (None, None) => true,
            (Some(x), Some(y)) => Weak::ptr_eq(x, y),
            _ => false,
        })
}

// The C++ friend of the Document that reaches its BIM cache; here the cache is
// the document's bim_cache slot (document.rs, DocumentState).
struct BimCacheAccess;

impl BimCacheAccess {
    fn model(document: &Document) -> Arc<BimModel> {
        document.state.bim_cache.with(|cache: &mut BimCache| {
            // Every comparison is made, so each key is brought up to date.
            let mut corridors_fresh = cache.corridors.is_some();
            corridors_fresh = same(
                &mut cache.corridor_texts[0],
                key_text(document, ROAD_TEMPLATES_KEY),
            ) && corridors_fresh;
            corridors_fresh = same(
                &mut cache.corridor_texts[1],
                key_text(document, CORRIDORS_KEY),
            ) && corridors_fresh;
            let alignments = document.model().alignments.revision();
            let targets = corridor_grounds(document);
            corridors_fresh = corridors_fresh
                && cache.alignments == alignments
                && same_grounds(&cache.corridor_grounds, &targets);
            let mut changed = false;
            if !corridors_fresh {
                // Made again, but kept as the old one when it came out the
                // same: an alignment no corridor uses moves the revision, and a
                // view that is handed a different object for an unchanged
                // model makes every 3D view build its terrain again
                // (RoadWorkbench).
                let made = corridor_objects(document);
                cache.built.corridors += 1;
                if cache.corridors.as_deref() != Some(&made) {
                    cache.corridors = Some(Arc::new(made));
                    changed = true;
                }
                cache.alignments = alignments;
                cache.corridor_grounds = targets;
            }

            let mut placed_fresh = cache.placed.is_some();
            placed_fresh = same(
                &mut cache.objects_text,
                key_text(document, ROAD_OBJECTS_KEY),
            ) && placed_fresh;
            let named = placed_grounds(document);
            placed_fresh = placed_fresh
                && cache.placed_alignments == alignments
                && same_grounds(&cache.placed_grounds, &named);
            if !placed_fresh {
                let made = placed_objects(document);
                cache.built.placed += 1;
                if cache.placed.as_deref() != Some(&made) {
                    cache.placed = Some(Arc::new(made));
                    changed = true;
                }
                cache.placed_alignments = alignments;
                cache.placed_grounds = named;
            }

            if changed || cache.combined.is_none() {
                // Both halves are there: each was found fresh, which needs it,
                // or was made just now.
                let mut both = cache.corridors.as_deref().cloned().unwrap_or_default();
                if let Some(placed) = cache.placed.as_deref() {
                    both.objects.extend(placed.objects.iter().cloned());
                    both.problems.extend(placed.problems.iter().cloned());
                }
                cache.combined = Some(Arc::new(both));
            }
            cache.combined.clone().unwrap_or_default()
        })
    }

    fn counts(document: &Document) -> BimBuildCounts {
        document
            .state
            .bim_cache
            .with(|cache: &mut BimCache| cache.built)
    }
}

/// Both, corridors first, made once per change of what they are made from -
/// the definitions, the alignments, and the surfaces the corridors target and
/// the placed objects name (not the others) - and shared, so a view can call
/// it every frame.
pub fn bim_model(document: &Document) -> Arc<BimModel> {
    BimCacheAccess::model(document)
}

/// How many times bim_model has made each half of `document`'s model
/// (BimBuildCounts).
pub fn bim_build_counts(document: &Document) -> BimBuildCounts {
    BimCacheAccess::counts(document)
}
