//! Buried services as surveyed lines, graded by AS 5488.1 quality level.
//!
//! A UtilityLine is one service run (a water main, a duct bank, a gas service)
//! as the vertices the survey located, in order along the run. Each vertex
//! carries the evidence it was located with and is classified on its own
//! (quality_level). What this module adds is the grade of the path BETWEEN
//! vertices, which is where most over-claiming happens: a line drawn through
//! two potholes is not QL-A between them, because nobody saw the service there.
//!
//! Conventions follow the survey data model: metres, (northing, easting), and
//! "absent is not zero" - a vertex with no level is a vertex whose level nobody
//! measured, and a surface level of 0.0 is a real surface at the datum.

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};

use super::quality_level::{
    Classification, Named, PositionEvidence, QualityLevel, QualityLevelTolerances, classify,
    to_string,
};
use super::table::key;
use crate::coordinate::Coordinate2;

/// The kinds of service the standard's attribute schedule distinguishes (AS
/// 5488.2 Table A.4, which delivery schemas such as TfNSW's code by a letter).
/// Other is a service whose kind is stated but not listed here; Unknown is one
/// whose kind nobody established, which is a finding in its own right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UtilityType {
    #[default]
    Unknown,
    Electricity,
    Telecommunications,
    Gas,
    Water,
    RecycledWater,
    FireService,
    Sewer,
    Stormwater,
    Fuel,
    /// ITS: traffic signals, cameras, loops, signalling.
    IntelligentTransport,
    Other,
}

impl Named for UtilityType {
    fn name(self) -> &'static str {
        match self {
            UtilityType::Unknown => "unknown",
            UtilityType::Electricity => "electricity",
            UtilityType::Telecommunications => "telecommunications",
            UtilityType::Gas => "gas",
            UtilityType::Water => "water",
            UtilityType::RecycledWater => "recycled water",
            UtilityType::FireService => "fire service",
            UtilityType::Sewer => "sewer",
            UtilityType::Stormwater => "stormwater",
            UtilityType::Fuel => "fuel",
            UtilityType::IntelligentTransport => "ITS",
            UtilityType::Other => "other",
        }
    }
}

/// The first of `names` that `text` is, compared by `key`.
fn look_up<Enum: Copy>(names: &[(&str, Enum)], text: &str) -> Option<Enum> {
    let wanted = key(text);
    names
        .iter()
        .find(|(name, _)| *name == wanted)
        .map(|(_, value)| *value)
}

/// Enumerator names and common abbreviations ("elec", "power", "comms", "telco",
/// "nbn", "sw", "storm", "ww", "sewerage", "reuse", "oil" ...), the asset type
/// names of AS 5488.2 ("Communication", "Drainage", "Fire Service", "ITS",
/// "Petroleum", "Not Specified") and their one-letter codes (C D E F G I P S W
/// N), any letter case. None for text that names nothing, so that a typo is
/// an error and not an Unknown service.
pub fn parse_utility_type(text: &str) -> Option<UtilityType> {
    const NAMES: [(&str, UtilityType); 45] = [
        ("unknown", UtilityType::Unknown),
        ("electricity", UtilityType::Electricity),
        ("electrical", UtilityType::Electricity),
        ("elec", UtilityType::Electricity),
        ("power", UtilityType::Electricity),
        ("hv", UtilityType::Electricity),
        ("lv", UtilityType::Electricity),
        ("telecommunications", UtilityType::Telecommunications),
        ("telecoms", UtilityType::Telecommunications),
        ("telco", UtilityType::Telecommunications),
        ("comms", UtilityType::Telecommunications),
        ("communications", UtilityType::Telecommunications),
        ("nbn", UtilityType::Telecommunications),
        ("gas", UtilityType::Gas),
        ("water", UtilityType::Water),
        ("potable", UtilityType::Water),
        ("recycledwater", UtilityType::RecycledWater),
        ("recycled", UtilityType::RecycledWater),
        ("reuse", UtilityType::RecycledWater),
        ("sewer", UtilityType::Sewer),
        ("sewerage", UtilityType::Sewer),
        ("ww", UtilityType::Sewer),
        ("stormwater", UtilityType::Stormwater),
        ("storm", UtilityType::Stormwater),
        ("sw", UtilityType::Stormwater),
        ("drainage", UtilityType::Stormwater),
        ("fuel", UtilityType::Fuel),
        ("oil", UtilityType::Fuel),
        ("other", UtilityType::Other),
        // The asset type names of AS 5488.2 Table A.4, and the one-letter
        // codes delivery schemas key them by.
        ("communication", UtilityType::Telecommunications),
        ("fireservice", UtilityType::FireService),
        ("its", UtilityType::IntelligentTransport),
        ("intelligenttransport", UtilityType::IntelligentTransport),
        ("petroleum", UtilityType::Fuel),
        ("notspecified", UtilityType::Unknown),
        ("c", UtilityType::Telecommunications),
        ("d", UtilityType::Stormwater),
        ("e", UtilityType::Electricity),
        ("f", UtilityType::FireService),
        ("g", UtilityType::Gas),
        ("i", UtilityType::IntelligentTransport),
        ("p", UtilityType::Fuel),
        ("s", UtilityType::Sewer),
        ("w", UtilityType::Water),
        ("n", UtilityType::Unknown),
    ];
    look_up(&NAMES, text)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UtilityStatus {
    #[default]
    Unknown,
    InService,
    /// Out of use, still the owner's and possibly still charged.
    Disused,
    /// Given up by its owner.
    Abandoned,
    Proposed,
}

impl Named for UtilityStatus {
    fn name(self) -> &'static str {
        match self {
            UtilityStatus::Unknown => "unknown",
            UtilityStatus::InService => "in service",
            UtilityStatus::Disused => "disused",
            UtilityStatus::Abandoned => "abandoned",
            UtilityStatus::Proposed => "proposed",
        }
    }
}

pub fn parse_utility_status(text: &str) -> Option<UtilityStatus> {
    const NAMES: [(&str, UtilityStatus); 8] = [
        ("unknown", UtilityStatus::Unknown),
        ("inservice", UtilityStatus::InService),
        ("live", UtilityStatus::InService),
        ("active", UtilityStatus::InService),
        ("abandoned", UtilityStatus::Abandoned),
        ("disused", UtilityStatus::Disused),
        ("redundant", UtilityStatus::Abandoned),
        ("proposed", UtilityStatus::Proposed),
    ];
    look_up(&NAMES, text)
}

/// Cells as one file spelt them, by that file's header: provenance for a
/// deliverable that must repeat a schedule's own words (UtilityAttributes::
/// written), not part of what a line IS. Two lines that say the same thing -
/// one read from a schedule, one from what was drawn, or the same schedule
/// written back in Katana's own spelling - are the same line, so this map
/// takes no part in the equality of what holds it: it is always equal. Its
/// entries are compared, where that matters, as the map it is (`.0`, or
/// through `Deref`).
#[derive(Clone, Debug, Default)]
pub struct WrittenCells(pub BTreeMap<String, String>);

impl PartialEq for WrittenCells {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Deref for WrittenCells {
    type Target = BTreeMap<String, String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for WrittenCells {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The descriptive attributes the standard asks to be recorded with a service.
/// Empty strings are "not recorded"; a zero diameter is "not recorded" too,
/// because no service has none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UtilityAttributes {
    pub r#type: UtilityType,
    pub status: UtilityStatus,
    /// Asset owner / operator.
    pub owner: String,
    /// "PVC", "DICL", "copper" ...
    pub material: String,
    /// Size of the service across, metres; see `diameter_is_inside`.
    pub diameter: f64,
    /// "4 x 100 conduits", "2 x 2", "Single" ...
    pub configuration: String,
    pub description: String,
    /// True when `diameter` is an INSIDE dimension - a delivery schema's pipe
    /// size usually is (TfNSW's Size is the inside diameter, a culvert's inside
    /// width by height). The top of the service found from an invert or a centre
    /// is then the inside top, below the outside by the wall, and a cover
    /// computed from it is larger than the real one by that wall. Reports say so
    /// (depth_of_cover's note); false means an outside dimension.
    pub diameter_is_inside: bool,
    /// Attributes a delivery schema carries that nothing here interprets -
    /// subtype, feature, capacity, limitation, condition, clash, treatment ... -
    /// by the schema's own attribute name. Kept so that they can be checked
    /// (delivery_schema) and reported, never dropped.
    pub fields: BTreeMap<String, String>,
    /// The attributes that ARE interpreted above - the service's id, type,
    /// owner, material, size, status, configuration, description - exactly as
    /// the schedule wrote them, by the header they were written under:
    /// "AssetTypeCode" -> "W", "AssetStatus" -> "In service". Reading turns
    /// "In service" into UtilityStatus::InService; a deliverable written back
    /// out (the IFC export's delivery property set, docs/ifc.md) must carry
    /// the schedule's own value, misspelling and all, or it would quietly
    /// correct what `UTILITY CHECK` reports as wrong. Empty for a line built
    /// in code.
    pub written: WrittenCells,
    /// Cells the schedule reader reads as "not recorded", or reads only in
    /// part, kept as the schedule wrote them by the reader's column name: a
    /// "type" that names no kind ("Not Specified", "N"), a "status" of
    /// "Unknown", and any "size" that a diameter written back would not say
    /// again - "Not Applicable", "Unknown", "1200 x 900", or a size given
    /// beside diameter_mm. So a schedule written back (write_utility_csv) says
    /// what this one said and meets a delivery schema this one met. Nothing
    /// grades from them; the writer takes one back only while it still reads
    /// as the value held above, so an edit made since wins.
    pub recorded: BTreeMap<String, String>,
}

/// Which part of the service a recorded level is ON. A level without this is
/// ambiguous by up to a pipe diameter, which on a 600 mm main is more than
/// QL-B's whole vertical tolerance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LevelReference {
    /// Top of the service: crown, top of pipe or cable, or the first thing
    /// above it that is met - an encasement, a cover slab.
    #[default]
    Top,
    /// Centre line.
    Centre,
    /// Bottom of the bore.
    Invert,
    /// Stated as unknown or "other": the level cannot be placed on the
    /// service, so no top and no cover are computed from it.
    Unknown,
}

impl Named for LevelReference {
    fn name(self) -> &'static str {
        match self {
            LevelReference::Top => "top",
            LevelReference::Centre => "centre",
            LevelReference::Invert => "invert",
            LevelReference::Unknown => "unknown",
        }
    }
}

/// "top", "crown", "obvert", "centre", "invert", "unknown" and the Depth
/// Location names of AS 5488.2 as delivery schemas spell them: "Top of Pipe",
/// "Obvert", "Top of Concrete Encasement", "Plastic Cover Protection
/// Encountered" and "Ground Level" are Top, "Top Row Invert" is Invert,
/// "Other" is Unknown.
pub fn parse_level_reference(text: &str) -> Option<LevelReference> {
    const NAMES: [(&str, LevelReference); 15] = [
        ("top", LevelReference::Top),
        ("crown", LevelReference::Top),
        ("obvert", LevelReference::Top),
        ("centre", LevelReference::Centre),
        ("center", LevelReference::Centre),
        ("cl", LevelReference::Centre),
        ("invert", LevelReference::Invert),
        ("il", LevelReference::Invert),
        ("unknown", LevelReference::Unknown),
        // AS 5488.2's Depth Location names, as delivery schemas spell them.
        // What is met first above the service counts as its top: cover is
        // measured to it.
        ("topofpipe", LevelReference::Top),
        ("topofconcreteencasement", LevelReference::Top),
        ("plasticcoverprotectionencountered", LevelReference::Top),
        ("groundlevel", LevelReference::Top),
        ("toprowinvert", LevelReference::Invert),
        ("other", LevelReference::Unknown),
    ];
    look_up(&NAMES, text)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UtilityVertex {
    pub id: String,
    pub position: Coordinate2,
    /// Of the service, on `level_reference`.
    pub level: Option<f64>,
    pub level_reference: LevelReference,
    /// Finished surface directly above.
    pub surface_level: Option<f64>,
    /// Depth of the `level_reference` point below the surface, metres, as a
    /// delivery schema records it instead of (or beside) a level. With a
    /// surface level it gives a level (service_level); alone it still gives a
    /// cover. Where a level and a depth are both given the level is used.
    pub depth: Option<f64>,
    pub evidence: PositionEvidence,
    /// The level the deliverable CLAIMS for this vertex, when it states one.
    /// Checked against what the evidence supports; None means "grade it".
    pub claimed: Option<QualityLevel>,
    /// For an exposure made to check a detection: the id of the detected vertex
    /// it checks (verification). Empty for any other vertex.
    pub verifies: String,
    /// Per-vertex attributes of a delivery schema that nothing here interprets
    /// (a depth description, the date obtained, a pothole report), by the
    /// schema's attribute name.
    pub fields: BTreeMap<String, String>,
    /// The vertex's interpreted cells - point, coordinates, method, level,
    /// level reference, depth, surface, uncertainties, claimed level, path,
    /// verifies - as written, by header: "LocateMethod" -> "Electronic
    /// Detection". See UtilityAttributes::written for why.
    pub written: WrittenCells,
    /// As UtilityAttributes::recorded, for a vertex's cells: a "ql" of
    /// "Unknown" (no claim), and a "level_ref" given with no level and no
    /// depth that reads as the top - which an empty cell reads as too.
    pub recorded: BTreeMap<String, String>,
}

/// The level of the recorded point on the service: `level`, or else surface
/// level minus depth. None when neither can be had.
pub fn service_level(vertex: &UtilityVertex) -> Option<f64> {
    if vertex.level.is_some() {
        return vertex.level;
    }
    if let (Some(surface), Some(depth)) = (vertex.surface_level, vertex.depth) {
        return Some(surface - depth);
    }
    None
}

/// True when anything vertical was measured at the vertex - a level, or a depth.
pub fn has_vertical_measurement(vertex: &UtilityVertex) -> bool {
    vertex.level.is_some() || vertex.depth.is_some()
}

/// What is known about the service between one vertex and the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathEvidence {
    /// Traced continuously, or detected at intervals no longer than the
    /// project's maximum spacing.
    Detected,
    /// Seen along its whole length: an open trench.
    Exposed,
    /// Joined up from the ends and the records; nothing observed between.
    Assumed,
}

impl Named for PathEvidence {
    fn name(self) -> &'static str {
        match self {
            PathEvidence::Detected => "detected",
            PathEvidence::Exposed => "exposed",
            PathEvidence::Assumed => "assumed",
        }
    }
}

/// "detected", "exposed", "assumed" and the field words "traced", "trench" and
/// "inferred", compared as a schedule's header is (any case; blanks, '-' and
/// '_' ignored): how a schedule's path column reads, and so how the same word
/// reads wherever else it is typed. None for anything else.
pub fn parse_path_evidence(text: &str) -> Option<PathEvidence> {
    let name = key(text);
    if name == "detected" || name == "traced" {
        return Some(PathEvidence::Detected);
    }
    if name == "exposed" || name == "trench" {
        return Some(PathEvidence::Exposed);
    }
    if name == "assumed" || name == "inferred" {
        return Some(PathEvidence::Assumed);
    }
    None
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UtilityLine {
    pub id: String,
    pub attributes: UtilityAttributes,
    /// In order along the run.
    pub vertices: Vec<UtilityVertex>,
    /// One per segment, i.e. vertices.len() - 1 entries; empty means every
    /// segment is Detected. Any other size is an InvalidArgument.
    pub path_evidence: Vec<PathEvidence>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradingSettings {
    pub tolerances: QualityLevelTolerances,
    /// The longest segment a Detected path may span and keep QL-B. Set by the
    /// project specification, not by the standard: a line interpolated across a
    /// gap longer than this was not traced, it was assumed. Infinity switches the
    /// rule off.
    pub maximum_detected_spacing: f64,
}

impl Default for GradingSettings {
    fn default() -> Self {
        Self {
            tolerances: QualityLevelTolerances::default(),
            maximum_detected_spacing: 10.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GradedVertex {
    pub classification: Classification,
    /// Set when `claimed` is better than the evidence supports: the deliverable
    /// over-states this vertex. Empty otherwise.
    pub over_claim: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradedSegment {
    /// Vertex index; the segment runs to from + 1.
    pub from: usize,
    /// Plan length, metres.
    pub length: f64,
    pub level: QualityLevel,
    /// Why the segment is below the better of its ends.
    pub limited_by: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradedLine {
    pub vertices: Vec<GradedVertex>,
    pub segments: Vec<GradedSegment>,
    /// Plan length at each level, indexed by `QualityLevel as usize`.
    pub length_at: [f64; 4],
}

impl GradedLine {
    pub fn length(&self) -> f64 {
        self.length_at[0] + self.length_at[1] + self.length_at[2] + self.length_at[3]
    }
}

fn plan_distance(a: &Coordinate2, b: &Coordinate2) -> f64 {
    (b.northing - a.northing).hypot(b.easting - a.easting)
}

fn check_line(line: &UtilityLine) -> Status {
    if line.vertices.len() < 2 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a utility line needs at least two vertices",
            line.id.clone(),
        ));
    }
    for vertex in &line.vertices {
        if !vertex.position.northing.is_finite() || !vertex.position.easting.is_finite() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "vertex has a non-finite coordinate",
                format!("{} / {}", line.id, vertex.id),
            ));
        }
    }
    if !line.path_evidence.is_empty() && line.path_evidence.len() != line.vertices.len() - 1 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "path evidence must name one entry per segment ({}), not {}",
                line.vertices.len() - 1,
                line.path_evidence.len()
            ),
            line.id.clone(),
        ));
    }
    Ok(())
}

/// Each cap is applied only when it lowers the level, and then names itself,
/// so that the reason given is the rule that decided.
fn cap(level: &mut QualityLevel, limited_by: &mut String, limit: QualityLevel, why: &str) {
    if *level > limit {
        *level = limit;
        *limited_by = String::from(why);
    }
}

/// Grades every vertex and every segment of `line`. A segment is never better
/// than the worse of its two ends, and further:
/// - Exposed keeps that level;
/// - Detected is capped at QL-B, and at QL-C when longer than
///   settings.maximum_detected_spacing;
/// - Assumed is capped at QL-C.
///
/// InvalidArgument for a line of fewer than two vertices, a non-finite
/// coordinate, or a path_evidence list of the wrong size.
pub fn grade_line(line: &UtilityLine, settings: &GradingSettings) -> Result<GradedLine> {
    check_line(line)?;

    let mut graded = GradedLine::default();
    graded.vertices.reserve(line.vertices.len());
    for vertex in &line.vertices {
        let mut evidence = vertex.evidence;
        evidence.has_level = has_vertical_measurement(vertex);
        let classification = classify(&evidence, &settings.tolerances);
        let mut over_claim = String::new();
        if let Some(claimed) = vertex.claimed
            && claimed > classification.level
        {
            over_claim = format!(
                "claimed {}, the {} evidence supports {}",
                to_string(claimed),
                to_string(evidence.method),
                to_string(classification.level)
            );
        }
        graded.vertices.push(GradedVertex {
            classification,
            over_claim,
        });
    }

    for i in 0..line.vertices.len() - 1 {
        let length = plan_distance(&line.vertices[i].position, &line.vertices[i + 1].position);
        let start = graded.vertices[i].classification.level;
        let end = graded.vertices[i + 1].classification.level;
        let mut level = start.min(end);
        let mut limited_by = String::new();
        if start != end {
            limited_by = format!(
                "the {} vertex {}",
                to_string(level),
                line.vertices[if start < end { i } else { i + 1 }].id
            );
        }

        let path = if line.path_evidence.is_empty() {
            PathEvidence::Detected
        } else {
            line.path_evidence[i]
        };
        match path {
            PathEvidence::Exposed => {}
            PathEvidence::Detected => {
                cap(
                    &mut level,
                    &mut limited_by,
                    QualityLevel::B,
                    "the path between the vertices was detected, not exposed",
                );
                if length > settings.maximum_detected_spacing {
                    cap(
                        &mut level,
                        &mut limited_by,
                        QualityLevel::C,
                        "the segment is longer than the maximum detected spacing and was \
                         interpolated, not traced",
                    );
                }
            }
            PathEvidence::Assumed => cap(
                &mut level,
                &mut limited_by,
                QualityLevel::C,
                "the path between the vertices was assumed, not observed",
            ),
        }
        graded.length_at[level as usize] += length;
        graded.segments.push(GradedSegment {
            from: i,
            length,
            level,
            limited_by,
        });
    }
    Ok(graded)
}

/// How far the top of the service is above the point a level or depth was
/// recorded on: 0 for Top, half the diameter for Centre, the diameter for
/// Invert. None for Unknown, or for Centre and Invert without a diameter.
pub fn top_offset(reference: LevelReference, diameter: f64) -> Option<f64> {
    let have_diameter = diameter.is_finite() && diameter > 0.0;
    match reference {
        LevelReference::Top => Some(0.0),
        LevelReference::Centre => have_diameter.then_some(diameter / 2.0),
        LevelReference::Invert => have_diameter.then_some(diameter),
        LevelReference::Unknown => None,
    }
}

/// The level of the TOP of the service at a vertex (service_level plus
/// top_offset). With an outside diameter, from an invert the diameter is added
/// whole, which ignores the wall and so places the top slightly high - the
/// direction that under-states cover. With an inside diameter it places the
/// top at the inside top, below the outside by the wall
/// (UtilityAttributes::diameter_is_inside). None when either is unknown.
pub fn top_level(vertex: &UtilityVertex, diameter: f64) -> Option<f64> {
    let level = service_level(vertex)?;
    let offset = top_offset(vertex.level_reference, diameter)?;
    Some(level + offset)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoverResult {
    pub vertex_id: String,
    /// Surface level - top of service; None if not computable.
    pub cover: Option<f64>,
    /// Why cover is missing, or why it is not to be relied on.
    pub note: String,
    /// Cover < the minimum asked for.
    pub below_minimum: bool,
}

/// Depth of cover at every vertex of `line`: surface level minus top of
/// service, or, where only a depth was recorded, that depth less top_offset.
/// A cover computed from a level that is not qualified (Classification::
/// level_qualified) is reported with a note, because the number is only as good
/// as the level under it, and so is one found from an inside diameter below
/// the top. `minimum_cover`, when given, flags each vertex whose cover is below
/// it.
pub fn depth_of_cover(
    line: &UtilityLine,
    minimum_cover: Option<f64>,
    settings: &GradingSettings,
) -> Result<Vec<CoverResult>> {
    if minimum_cover.is_some_and(|minimum| !minimum.is_finite()) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the minimum cover is not a finite number",
            line.id.clone(),
        ));
    }
    let graded = grade_line(line, settings)?;

    let mut results = Vec::with_capacity(line.vertices.len());
    for (i, vertex) in line.vertices.iter().enumerate() {
        let mut result = CoverResult {
            vertex_id: vertex.id.clone(),
            ..Default::default()
        };
        let offset = top_offset(vertex.level_reference, line.attributes.diameter);
        let level = service_level(vertex);
        if !has_vertical_measurement(vertex) {
            result.note = String::from("no level or depth of the service");
        } else if vertex.level_reference == LevelReference::Unknown {
            result.note = String::from("the level's place on the service is unknown");
        } else if let Some(offset) = offset {
            if let (Some(level), Some(surface)) = (level, vertex.surface_level) {
                result.cover = Some(surface - (level + offset));
            } else if let (Some(depth), None) = (vertex.depth, vertex.level) {
                result.cover = Some(depth - offset);
            } else {
                result.note = String::from("no surface level");
            }
        } else {
            result.note = format!(
                "a level on the {} needs the service's diameter",
                to_string(vertex.level_reference)
            );
        }
        if let Some(cover) = result.cover {
            let classification = &graded.vertices[i].classification;
            if !classification.level_qualified {
                result.note = format!(
                    "the level is not qualified at {}; do not rely on this cover",
                    to_string(classification.level)
                );
            } else if line.attributes.diameter_is_inside && offset.is_some_and(|top| top > 0.0) {
                result.note = String::from(
                    "the size is an inside dimension: this cover is to the inside top, and \
                     the service's outside is higher by its wall",
                );
            }
            result.below_minimum = minimum_cover.is_some_and(|minimum| cover < minimum);
        }
        results.push(result);
    }
    Ok(results)
}
