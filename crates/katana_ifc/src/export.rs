//! A drawing, its alignments, its surfaces and a subsurface utility
//! investigation -> an IFC 4.3 file (IFC4X3_ADD2), with no third-party library.
//!
//! WHAT GOES WHERE (classification.rs decides the classes; docs/ifc.md has
//! the full tables):
//!
//! ```text
//! IfcProject                the project, its units (metre, radian) and one
//!                           3D model context with Axis, Body, FootPrint and
//!                           Annotation subcontexts
//! IfcProjectedCRS +         the project's coordinate system, when it has an
//! IfcMapConversion          EPSG code. Coordinates are written relative to a
//!                           local origin near the work, and the conversion
//!                           gives that origin's easting, northing and height,
//!                           so the file is both georeferenced and drawable:
//!                           a viewer holding MGA coordinates in single
//!                           precision would have a metre of resolution.
//! IfcSite                   holds every element and annotation
//!
//! an alignment              IfcAlignment aggregated into the project, with
//!                           its business logic - IfcAlignmentHorizontal and
//!                           IfcAlignmentVertical nesting IfcAlignmentSegment
//!                           design parameters (LINE, CIRCULARARC, CLOTHOID;
//!                           CONSTANTGRADIENT, PARABOLICARC), each layout
//!                           closed by a zero-length segment - and its
//!                           geometry: an IfcCompositeCurve of IfcCurveSegment
//!                           (IfcLine, IfcCircle, IfcClothoid) and, with a
//!                           profile, an IfcGradientCurve over it
//!                           (IfcLine, IfcPolynomialCurve). Stations are
//!                           IfcReferent STATION with Pset_Stationing at the
//!                           start, the end and every key station.
//!
//! a service (AS 5488)       one IfcDistributionSystem per service, grouping
//!                           one segment element per graded segment - the
//!                           standard grades segments, not services - of the
//!                           class classify_utility_run gives, and an
//!                           IfcAnnotation SURVEY for every located point.
//!                           Each carries its AS 5488 grade (the
//!                           AS5488_QualityLevel property set, Pset_Uncertainty
//!                           and an IfcClassificationReference to its quality
//!                           level) and the delivery schema's attributes
//!                           (a set named by the schema's title) with the
//!                           values the schedule wrote.
//!
//! a drawing entity          the class classify_entity gives: an IfcKerb, an
//!                           IfcRailing, a pipe or a pit, or an IfcAnnotation.
//!                           A .12da archive's drainage strings become pipes
//!                           pit to pit and the pits chambers, in one system
//!                           per string.
//! a surface                 IfcGeographicElement TERRAIN, an
//!                           IfcTriangulatedFaceSet
//!
//! the road model            an IfcRoad per facility, aggregated into the
//! (RoadModelInput)          site, and an IfcRoadPart per part of it
//!                           (CARRIAGEWAY, SHOULDER, SIDEWALK ...); each
//!                           element - IfcCourse, IfcKerb, IfcEarthworksFill,
//!                           IfcRailing, IfcSign ... - contained in its part,
//!                           its body a closed IfcTriangulatedFaceSet, with
//!                           its material, its quantities (IfcElementQuantity)
//!                           and its properties. A cut (IfcEarthworksCut)
//!                           voids the ground it is taken from and a line
//!                           marking (IfcSurfaceFeature) adheres to the course
//!                           it is painted on, as the schema has features.
//! ```
//!
//! NOTHING IS INVENTED. A service segment is drawn in 3D - its centre line as
//! the Axis and, with a size, a swept disk as its Body - only where a level is
//! known at both ends and can be brought to the centre of the service. Where
//! it cannot, the segment has a 2D FootPrint and no Body: a line drawn at a
//! guessed depth would be believed. Absent heights are absent in every
//! representation, never zero.
//!
//! WHY A WRITER OF OUR OWN. IFC is text (ISO 10303-21), and what the export
//! needs is the mapping, not a model library: the open toolkit that writes 4.3
//! alignments, and that buildingSMART's validation service is built on, is
//! IfcOpenShell, whose C++ core is a large dependency to take for the writing
//! of a text file, and which would be a second model of the drawing beside
//! Katana's own. So, like the DXF and .12da writers, this module needs nothing
//! above the domain model. IfcOpenShell is used instead where it is worth most,
//! as the independent judge of what is written (tools/check_ifc.py, the
//! cli.ifc_* tests).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use katana_core::error::{ErrorCode, Result, make_error};
use katana_core::text::equals_ignoring_case;
use katana_entity::entity::{Color, EntityId};
use katana_entity::leader_values::LeaderContext;
use katana_entity::model::Model;
use katana_geometry::mesh::TriangleMesh;
use katana_geometry::{Polyline2, Vec2};
use katana_math::Vec3;
use katana_survey::subsurface::{DeliverySchema, GradingSettings, UtilityLine};
use katana_terrain::TinSurface;

use crate::builder::{Builder, Frame};
use crate::classification::{ClassificationRule, IfcClass, default_classification_rules};
use crate::parts;
use crate::product_layout::find_product_layout;
use crate::step::step_string;
use crate::support::{error_message, file_name_text, path_text};

/// The project's coordinate system, as IfcProjectedCRS takes it. Written only
/// when `name` is an EPSG code ("EPSG:7856"): IFC4X3_ADD2 names a system by
/// its EPSG code and has no attribute for a definition in WKT, and a name no
/// validator can resolve is georeferencing in appearance only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Georeference {
    /// "EPSG:7856"
    pub name: String,
    /// "GDA2020 / MGA zone 56"
    pub description: String,
    /// "GDA2020"; empty to leave it to the code
    pub geodetic_datum: String,
    /// "EPSG:5711" (AHD height); empty when not known
    pub vertical_datum: String,
    /// "UTM"; empty to leave it to the code
    pub map_projection: String,
    /// "56"
    pub map_zone: String,
}

/// A surface to export, by name. The drawing's surfaces are kept beside the
/// entity model, not in it (Document::surfaces), so the caller passes the ones
/// it has.
#[derive(Clone, Debug, Default)]
pub struct SurfaceInput<'a> {
    pub name: String,
    pub surface: Option<&'a TinSurface>,
}

/// A subsurface utility investigation: the services as survey/subsurface read
/// them, graded here with `grading`.
#[derive(Clone, Debug, Default)]
pub struct UtilityInput {
    pub lines: Vec<UtilityLine>,
    pub grading: GradingSettings,
    /// The delivery schema the schedule was written to (delivery_schema.rs),
    /// when the caller has it. With it, the delivery property set lists the
    /// schema's attributes in its order, each described by the schema's label,
    /// and a value from a listed domain is an IfcPropertyEnumeratedValue
    /// referring to that domain; without it, the property set holds what the
    /// schedule has, by its column names. Shared rather than borrowed, so that
    /// the files an export names (ExportFiles) can own it beside the input
    /// that refers to it - the C++'s pointer into an owner beside it.
    pub schema: Option<Arc<DeliverySchema>>,
    /// the schedule's file name, for provenance
    pub source_name: String,
}

/// A measured quantity of a road element, named as IFC's base quantities are
/// ("Length", "Width", "Thickness", "GrossArea", "GrossVolume", "Weight",
/// "Count") and written as the IfcPhysicalSimpleQuantity its unit says:
/// "m" IfcQuantityLength, "m2" IfcQuantityArea, "m3" IfcQuantityVolume, "kg"
/// or "t" IfcQuantityWeight (in kilograms), "" IfcQuantityCount - written as an
/// INTEGER, which IFC4X3_ADD2 asks of it; a count that is not a whole number
/// is left out, said.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadQuantity {
    pub name: String,
    pub value: f64,
    pub unit: String,
}

impl RoadQuantity {
    pub fn new(name: impl Into<String>, value: f64, unit: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value,
            unit: unit.into(),
        }
    }
}

/// One 3D object of the road model - a pavement course, a kerb, a batter, a
/// barrier, a sign - as the export takes it. It mirrors cad::bim::BimObject
/// field for field without seeing it, since this layer may not see cad;
/// katana_app converts one into the other (ifc_verbs, road_model_input).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadElement {
    /// Stable: the same definition gives the same id on every run, and the
    /// GlobalId is made from it ("road/element/<id>"), so a model handed over
    /// twice names the same things the same way.
    pub id: String,
    pub name: String,
    /// What it is in Katana's words - "pavement", "kerb", "batter", "sign" -
    /// written as the element's Description and in Katana_RoadElement.
    pub kind: String,
    /// The class, its predefined type and, for USERDEFINED, what it is. A
    /// class this export does not write, or a predefined type its enumeration
    /// does not have, leaves the element out, said.
    pub ifc_class: IfcClass,
    /// The IfcRoad it belongs to (a corridor's name), and its part as
    /// IfcRoadPart's predefined type ("CARRIAGEWAY", "SHOULDER", "SIDEWALK",
    /// "ROADSIDE", "CENTRALRESERVE", "ROADSEGMENT" ...); a part the
    /// enumeration does not list is a USERDEFINED part of that name. No
    /// facility: an element of the site; no part: of the road itself.
    pub facility: String,
    pub part: String,
    /// an IfcMaterial of this name; empty for none
    pub material: String,
    /// entity::Color{} by default: white, opaque.
    pub colour: Color,
    /// In world coordinates, as the drawing's. Written with Closed true when
    /// every edge is shared by exactly two faces, oriented outwards.
    pub mesh: TriangleMesh,
    /// Its outline in plan, written as its FootPrint; empty: the mesh's plan
    /// hull is.
    pub plan: Vec<Polyline2>,
    pub quantities: Vec<RoadQuantity>,
    /// Further attributes, in order (Katana_RoadProperties).
    pub properties: Vec<(String, String)>,
    /// its representations' presentation layer
    pub layer: String,
    /// For a feature only. An IfcSurfaceFeature (a line marking) adheres to
    /// the element with the id `host`; given none, to the course or pavement
    /// of its facility that lies under it, the highest where several do. An
    /// IfcEarthworksCut voids the element `host`, else the surface named
    /// `ground` when the file holds it (SurfaceInput), else an
    /// IfcGeographicElement TERRAIN standing for that ground, with no shape.
    pub host: String,
    pub ground: String,
}

/// The road model as the export takes it: its objects, corridors first, and
/// what it could not build ("corridor Main: the alignment has no profile"),
/// which the export's warnings repeat, since a file missing a corridor's kerbs
/// must say why.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadModelInput {
    pub elements: Vec<RoadElement>,
    pub problems: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ExportInput<'a> {
    /// may be None: utilities or surfaces alone
    pub model: Option<&'a Model>,
    pub surfaces: Vec<SurfaceInput<'a>>,
    pub utilities: Option<UtilityInput>,
    /// The road model's objects; empty for none (NOROADMODEL).
    pub road: RoadModelInput,
}

#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub project_name: String,
    pub project_description: String,
    pub site_name: String,
    /// FILE_NAME in the header: the file's name, when it was written (ISO
    /// 8601, the caller's clock - never read here, so that an export is a
    /// function of its inputs), by whom, and the program that wrote it.
    pub file_name: String,
    pub timestamp: String,
    pub author: String,
    pub organisation: String,
    pub application_version: String,

    pub georeference: Georeference,
    /// Where the file's (0, 0, 0) is in the project's coordinates. None:
    /// chosen from the data - the south-west corner of everything exported,
    /// rounded down to 100 m in plan, at height 0 - when there is a
    /// coordinate system to record it in, and (0, 0, 0) when there is not,
    /// since a shift nothing records would move the survey.
    pub local_origin: Option<Vec3>,
    /// Seeds every GlobalId (guid_for): the same project exported twice gives
    /// each object the same GlobalId, and two projects different ones. The
    /// application passes something that identifies the project; empty uses
    /// the project name.
    pub guid_namespace: String,

    /// Only these entities, and only these layers; empty for all.
    pub entities: Vec<EntityId>,
    pub layers: Vec<String>,
    /// The rules drawing entities are classified by; empty uses
    /// default_classification_rules().
    pub rules: Vec<ClassificationRule>,
    pub export_entities: bool,
    pub export_alignments: bool,

    /// What a smart leader's note is read from. A leader is written as the
    /// note it SAYS (entity::leader_note), never as its template: a template
    /// is "RL {rl:.3f}", the note "RL 12.000". The note reads the entity the
    /// tip is on and, for a value of a surface or an alignment
    /// ({dsurface.ground}, {station.MC01}), the drawing's own, which this
    /// layer cannot see (katana_ifc sees entity and terrain, not cad), so the
    /// front end hands them over - cad's leader context and code property
    /// candidates, as every other consumer of a leader does. Left empty, a
    /// leader's note is read off its target alone, and a line that names a
    /// surface or an alignment is dropped from it, as the DXF writer's own
    /// fallback does (nothing made up).
    pub leader_context: LeaderContext,
    pub code_properties: Vec<String>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            project_name: String::from("Untitled"),
            project_description: String::new(),
            site_name: String::from("Site"),
            file_name: String::new(),
            timestamp: String::from("1970-01-01T00:00:00"),
            author: String::new(),
            organisation: String::new(),
            application_version: String::new(),
            georeference: Georeference::default(),
            local_origin: None,
            guid_namespace: String::new(),
            entities: Vec::new(),
            layers: Vec::new(),
            rules: Vec::new(),
            export_entities: true,
            export_alignments: true,
            leader_context: LeaderContext::default(),
            code_properties: Vec::new(),
        }
    }
}

/// One line of the account of what an export wrote: so many objects of one
/// source that became one class, and why. The export's report, and what the
/// window's export dialog previews, so that what a person is shown is what
/// the writer did rather than a second guess at it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassTally {
    /// Where they came from: "layer Survey/Kerb", "service W1",
    /// "alignment MC01", "surface Ground".
    pub source: String,
    /// The class written, as the schema spells it; EMPTY for objects that
    /// were not written (labels, entities with nothing to draw), `why` saying
    /// so.
    pub entity: String,
    pub predefined_type: String,
    /// what a USERDEFINED one is
    pub object_type: String,
    /// the IfcDistributionSystemEnum value, for a service element
    pub system: String,
    /// What decided it: "rule kerb", "no rule: a surveyed point",
    /// "AssetFeature \"Conduit (with Cable)\"", "drainage pipe".
    pub why: String,
    pub count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IfcExport {
    /// the file (write_ifc); empty from write_ifc_file
    pub text: String,
    pub bytes_written: usize,
    /// STEP entity instances
    pub instances: usize,
    /// How many of each rooted class were written, by the schema's name:
    /// "IfcPipeSegment" -> 12. What the report prints, and what a test
    /// asserts "no proxies" with.
    pub classes: BTreeMap<String, usize>,
    pub alignments: usize,
    /// distribution systems from the schedule
    pub services: usize,
    /// graded segments written as elements
    pub service_segments: usize,
    /// of which drawn with an Axis in 3D
    pub segments_in_3d: usize,
    /// IfcAnnotation SURVEY of the schedule
    pub located_points: usize,
    /// drawing entities written
    pub entities_written: usize,
    pub entities_skipped: usize,
    pub surfaces: usize,
    /// the road model's elements written
    pub road_elements: usize,
    /// ... and left out, each said in the warnings
    pub road_elements_skipped: usize,
    /// IfcRoad
    pub roads: usize,
    /// IfcRoadPart
    pub road_parts: usize,
    /// What each source became, ordered by source, then class, then why.
    pub tally: Vec<ClassTally>,
    pub warnings: Vec<String>,
}

/// The local origin is rounded down to this in plan, so that it reads as a
/// round number in the map conversion and moves by whole steps as a job
/// grows rather than with every point added.
const ORIGIN_STEP: f64 = 100.0;

/// The south-west corner of everything the export writes, in plan.
fn extent_corner(input: &ExportInput<'_>, options: &ExportOptions) -> Option<Vec2> {
    let mut corner: Option<Vec2> = None;
    let mut include = |x: f64, y: f64| {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        corner = Some(match corner {
            Some(at) => Vec2::new(at.x.min(x), at.y.min(y)),
            None => Vec2::new(x, y),
        });
    };
    if let Some(model) = input.model {
        if options.export_entities && !model.entities.empty() {
            let bounds = model.entities.bounds();
            if !bounds.empty() {
                include(bounds.min.x, bounds.min.y);
            }
        }
        if options.export_alignments {
            model.alignments.for_each(|alignment| {
                for pi in &alignment.horizontal.pis {
                    include(pi.point.x, pi.point.y);
                }
            });
        }
    }
    if let Some(utilities) = &input.utilities {
        for line in &utilities.lines {
            for vertex in &line.vertices {
                include(vertex.position.easting, vertex.position.northing);
            }
        }
    }
    for surface in &input.surfaces {
        if let Some(surface) = surface.surface
            && surface.vertex_count() > 0
        {
            let bounds = surface.bounds();
            include(bounds.min.x, bounds.min.y);
        }
    }
    for element in &input.road.elements {
        for vertex in &element.mesh.vertices {
            include(vertex.x, vertex.y);
        }
    }
    corner
}

fn header_list(value: &str) -> String {
    format!("({})", step_string(value))
}

/// True when `name` is what the export georeferences by: "EPSG:" and a code
/// of digits alone ("EPSG:7856"). A compound "EPSG:7856+5711", WKT or a PROJ
/// string is not, and a file written under one is not georeferenced (the
/// report says so), since IFC4X3_ADD2 names a system by one code.
pub fn is_epsg_code(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() < 6 || !bytes.starts_with(b"EPSG:") {
        return false;
    }
    bytes[5..].iter().all(u8::is_ascii_digit)
}

/// True for a path ending ".ifc", any letter case.
pub fn is_ifc_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| equals_ignoring_case(extension.as_encoded_bytes(), b"ifc"))
}

/// InvalidArgument for a model entity or alignment that cannot be written
/// (a non-finite coordinate reaching the writer), with the object named.
pub fn write_ifc(input: &ExportInput<'_>, options: &ExportOptions) -> Result<IfcExport> {
    let rules: &[ClassificationRule] = if options.rules.is_empty() {
        default_classification_rules()
    } else {
        &options.rules
    };
    for rule in rules {
        let Some(layout) = find_product_layout(&rule.target.entity) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the classification rule \"{}\" names {}, which is not a class this export \
                     writes",
                    rule.name, rule.target.entity
                ),
                rule.target.entity.clone(),
            ));
        };
        if layout.feature {
            // A feature exists only on the element it voids or adheres to,
            // which a drawing entity does not name (product_layout.rs).
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the classification rule \"{}\" names {}, a feature of another element, \
                     which only the road model writes",
                    rule.name, rule.target.entity
                ),
                rule.target.entity.clone(),
            ));
        }
        if rule.target.predefined_type == "USERDEFINED" && rule.target.object_type.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the classification rule \"{}\" is USERDEFINED and says nothing of what it \
                     is (objectType)",
                    rule.name
                ),
                rule.name.clone(),
            ));
        }
    }

    let mut report = IfcExport::default();
    let mut frame = Frame::default();
    let crs = &options.georeference;
    frame.georeferenced = is_epsg_code(&crs.name);
    if !crs.name.is_empty() && !frame.georeferenced {
        report.warnings.push(format!(
            "the coordinate system \"{}\" has no EPSG code, which IFC4X3_ADD2 names a system by; \
             the file is not georeferenced",
            crs.name
        ));
    } else if crs.name.is_empty() {
        report.warnings.push(String::from(
            "the project has no coordinate system: the file is not georeferenced, and its \
             coordinates are the project's own",
        ));
    }
    if let Some(origin) = options.local_origin {
        frame.origin = origin;
        if !frame.georeferenced
            && (frame.origin.x != 0.0 || frame.origin.y != 0.0 || frame.origin.z != 0.0)
        {
            // A shift that nothing in the file records would move the job.
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a local origin needs a coordinate system (an EPSG code) to be recorded in",
                "",
            ));
        }
    } else if frame.georeferenced
        && let Some(corner) = extent_corner(input, options)
    {
        frame.origin = Vec3::new(
            (corner.x / ORIGIN_STEP).floor() * ORIGIN_STEP,
            (corner.y / ORIGIN_STEP).floor() * ORIGIN_STEP,
            0.0,
        );
    }

    let file = {
        let mut builder = Builder::new(options, frame, &mut report);
        if let Some(model) = input.model
            && options.export_alignments
        {
            parts::export_alignments(&mut builder, model);
        }
        if let Some(utilities) = &input.utilities {
            parts::export_utilities(&mut builder, utilities);
        }
        if let Some(model) = input.model
            && options.export_entities
        {
            parts::export_entities(&mut builder, model, rules);
        }
        let terrain = parts::export_surfaces(&mut builder, &input.surfaces);
        // After the surfaces, so that a cut can void the ground it is taken
        // from (road.rs).
        parts::export_road(&mut builder, &input.road, &terrain);
        builder.finish();
        builder.into_file()
    };

    if file.non_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a number that is not finite reached the IFC writer; nothing was written",
            "",
        ));
    }

    let mut text = String::with_capacity(file.data().len() + 1024);
    text.push_str("ISO-10303-21;\nHEADER;\n");
    // The Alignment-based view: IFC 4.3's view for alignments and what is
    // positioned along them, whose scope the entities written stay within
    // (ReferenceView leaves out the alignment business logic and the curves
    // IfcClothoid and IfcCircle).
    text.push_str("FILE_DESCRIPTION(('ViewDefinition [Alignment-basedView]'),'2;1');\n");
    let application = if options.application_version.is_empty() {
        String::from("Katana")
    } else {
        format!("Katana {}", options.application_version)
    };
    text.push_str(&format!(
        "FILE_NAME({},{},{},{},{},{},'');\n",
        step_string(&options.file_name),
        step_string(&options.timestamp),
        header_list(&options.author),
        header_list(&options.organisation),
        step_string(&application),
        step_string(&application)
    ));
    text.push_str("FILE_SCHEMA(('IFC4X3_ADD2'));\nENDSEC;\nDATA;\n");
    text.push_str(file.data());
    text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");

    report.instances = file.size();
    report.bytes_written = text.len();
    report.text = text;
    Ok(report)
}

/// Writes to `path`, replacing it; the file is written under a temporary
/// name and renamed into place, so a failed write leaves nothing behind.
/// FileExportFailure when it cannot be written.
pub fn write_ifc_file(
    input: &ExportInput<'_>,
    path: &Path,
    options: &ExportOptions,
) -> Result<IfcExport> {
    let mut named = options.clone();
    if named.file_name.is_empty() {
        named.file_name = file_name_text(path);
    }
    let mut written = write_ifc(input, &named)?;
    // Beside the target first, then over it: a write that fails part way
    // leaves the previous file, not half of a new one.
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".partial");
    let temporary = PathBuf::from(temporary);
    if std::fs::write(&temporary, written.text.as_bytes()).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file could not be written",
            path_text(path),
        ));
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            format!("the file could not be replaced: {}", error_message(&error)),
            path_text(path),
        ));
    }
    written.text = String::new();
    Ok(written)
}
