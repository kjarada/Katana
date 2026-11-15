//! Which IFC 4.3 class each thing Katana exports becomes - the decision the
//! export exists to get right. An IFC file whose every object is an
//! IfcBuildingElementProxy says nothing a DXF did not: a consumer cannot ask it
//! for the water mains, the kerbs or the pits. So nothing here falls back to a
//! proxy. What is known to be an element becomes the element class the schema
//! has for it; what is only known to be a line someone surveyed or drew
//! becomes an IfcAnnotation (SURVEY, CONTOURLINE, TEXT, DIMENSION, LEADER),
//! which is what IFC 4.3 made that class for - "annotations are also used to
//! represent survey elements" - and which claims no more than is known.
//!
//! Two sources decide it:
//!
//! * A SERVICE from an AS 5488 schedule (survey/subsurface): its type - the
//!   AS 5488.2 Table A.4 asset type a delivery schema codes as AssetTypeCode -
//!   gives the distribution system and the default segment class, and the
//!   words of its AssetFeature, AssetSubtype, configuration, material and
//!   description refine the class: a "Conduit (with Cable)" is an
//!   IfcCableCarrierSegment, a "Culvert" an IfcPipeSegment CULVERT. A service
//!   recorded at one point is a pit, a valve, a hydrant or a marker when its
//!   feature says which.
//! * A DRAWING ENTITY: an ordered list of rules over the words of its layer
//!   path, its survey code and its string name (from a .12da archive) -
//!   "KERB" is an IfcKerb, "FENCE" an IfcRailing FENCE, a point on a "SEWER
//!   PITS" layer an IfcDistributionChamberElement in the SEWAGE system. The
//!   first rule that matches wins; the rules are data (ClassificationRule), so
//!   a project whose layers are named otherwise passes its own.
//!
//! docs/ifc.md has the tables, and why each mapping was chosen.

use std::sync::OnceLock;

use katana_core::error::{Error, ErrorCode, Result, make_error};
use katana_core::text::{equals_ignoring_case, lowered};
use katana_entity::entity::{
    ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, Entity, EntityType, Geometry, PropertyMap,
    property_to_string, to_string as entity_name,
};
use katana_survey::subsurface::{self as sub, UtilityAttributes, UtilityType};

use crate::product_layout::{PRODUCT_LAYOUTS, lists_word, predefined_types_of};
use crate::support::{find_from, text_of, trimmed, upper};

/// An IFC class and its predefined type, spelt as the schema spells them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IfcClass {
    /// "IfcPipeSegment"
    pub entity: String,
    /// "RIGIDSEGMENT"; empty when the class has none or it is not known
    pub predefined_type: String,
    /// what a USERDEFINED one is ("HEADWALL"); empty otherwise
    pub object_type: String,
}

impl IfcClass {
    pub fn new(
        entity: impl Into<String>,
        predefined_type: impl Into<String>,
        object_type: impl Into<String>,
    ) -> Self {
        Self {
            entity: entity.into(),
            predefined_type: predefined_type.into(),
            object_type: object_type.into(),
        }
    }
}

// ---- services (AS 5488) -----------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UtilityClass {
    /// a run's segments, or a point feature
    pub element: IfcClass,
    /// the IfcDistributionSystemEnum value: "WATERSUPPLY"
    pub system: String,
    /// what a USERDEFINED system is: "RECYCLEDWATER"
    pub system_object_type: String,
    /// What decided the element class, for the export's report and for a
    /// person asking why: "AssetFeature \"Conduit (with Cable)\"", "type water".
    pub reason: String,
}

/// Upper case, every run of characters that are not letters or digits one
/// blank, no blank at either end: "Survey/Kerb_line" is "SURVEY KERB LINE".
/// A byte past ASCII counts as a letter, as the C++'s byte test has it, so a
/// character beyond ASCII is kept whole.
fn normalised(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut gap = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || !c.is_ascii() {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.push(c.to_ascii_uppercase());
        } else {
            gap = true;
        }
    }
    out
}

fn property_text(map: &PropertyMap, key: &str) -> String {
    map.get(key).map(property_to_string).unwrap_or_default()
}

/// The distribution systems, by the service words that name them. Longer and
/// more specific words first: "RECYCLED WATER" before "WATER", and
/// "STORMWATER" is one word, so it never reads as WATER.
struct ServiceWords {
    system: &'static str,
    /// for a USERDEFINED system
    object_type: &'static str,
    words: &'static [&'static str],
}

const SERVICES: [ServiceWords; 9] = [
    ServiceWords {
        system: "USERDEFINED",
        object_type: "RECYCLEDWATER",
        words: &["RECYCLED WATER", "RECYCLED", "REUSE", "PURPLE PIPE"],
    },
    ServiceWords {
        system: "FIREPROTECTION",
        object_type: "",
        words: &["FIRE SERVICE", "FIRE MAIN", "HYDRANT", "HYDRANTS"],
    },
    ServiceWords {
        system: "STORMWATER",
        object_type: "",
        words: &[
            "STORMWATER",
            "STORM",
            "DRAINAGE",
            "DRAIN",
            "DRAINS",
            "SW",
            "CULVERT",
            "CULVERTS",
        ],
    },
    ServiceWords {
        system: "SEWAGE",
        object_type: "",
        words: &["SEWER", "SEWERAGE", "SEWAGE", "SEWERS", "WASTEWATER"],
    },
    ServiceWords {
        system: "WATERSUPPLY",
        object_type: "",
        words: &["WATER", "POTABLE", "WATERMAIN", "WATERMAINS"],
    },
    ServiceWords {
        system: "GAS",
        object_type: "",
        words: &["GAS"],
    },
    ServiceWords {
        system: "FUEL",
        object_type: "",
        words: &["FUEL", "PETROLEUM"],
    },
    ServiceWords {
        system: "ELECTRICAL",
        object_type: "",
        words: &["ELEC*", "POWER", "HV", "LV", "STREETLIGHT*"],
    },
    ServiceWords {
        system: "COMMUNICATION",
        object_type: "",
        words: &[
            "COMMS",
            "TELECOM*",
            "TELSTRA",
            "NBN",
            "OPTIC*",
            "FIBRE",
            "COMMUNICATION*",
            "TELCO",
        ],
    },
];

/// Whole-word search in a normalised text for a normalised word or phrase.
fn has_word(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let prefix = word.ends_with('*');
    let needle = normalised(if prefix {
        &word[..word.len() - 1]
    } else {
        word
    });
    if needle.is_empty() {
        return false;
    }
    let text = text.as_bytes();
    let needle = needle.as_bytes();
    let mut at = 0;
    while let Some(found) = find_from(text, needle, at) {
        let starts_word = found == 0 || text[found - 1] == b' ';
        let end = found + needle.len();
        let ends_word = end == text.len() || text[end] == b' ';
        if starts_word && (ends_word || prefix) {
            return true;
        }
        at = found + 1;
    }
    false
}

fn has_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| has_word(text, word))
}

fn field_of<'a>(service: &'a UtilityAttributes, name: &str) -> Option<&'a String> {
    service.fields.get(name)
}

const FEATURE_FIELDS: [&str; 4] = [
    "AssetFeature",
    "AssetFeatureDescription",
    "AssetSubtype",
    "AssetSubtypeDescription",
];

/// Everything a service says about what it is, normalised, AssetFeature first.
fn feature_text(service: &UtilityAttributes) -> String {
    let mut text = String::new();
    let mut append = |part: &str| {
        if !part.is_empty() {
            text.push(' ');
            text.push_str(part);
        }
    };
    for name in FEATURE_FIELDS {
        if let Some(value) = field_of(service, name) {
            append(value);
        }
    }
    append(&service.configuration);
    append(&service.material);
    append(&service.description);
    normalised(&text)
}

/// Which of the feature's fields holds `word`, for the reason given: the
/// schema's own attribute name where it came from one.
fn reason_for(service: &UtilityAttributes, word: &str) -> String {
    for name in FEATURE_FIELDS {
        if let Some(value) = field_of(service, name)
            && has_word(&normalised(value), word)
        {
            return format!("{name} \"{value}\"");
        }
    }
    for (name, value) in [
        ("configuration", &service.configuration),
        ("material", &service.material),
        ("description", &service.description),
    ] {
        if has_word(&normalised(value), word) {
            return format!("{name} \"{value}\"");
        }
    }
    String::from(word)
}

fn carries_cable(utility: UtilityType) -> bool {
    matches!(
        utility,
        UtilityType::Electricity
            | UtilityType::Telecommunications
            | UtilityType::IntelligentTransport
    )
}

fn carries_fluid(utility: UtilityType) -> bool {
    matches!(
        utility,
        UtilityType::Water
            | UtilityType::RecycledWater
            | UtilityType::FireService
            | UtilityType::Sewer
            | UtilityType::Stormwater
            | UtilityType::Gas
            | UtilityType::Fuel
    )
}

fn system_of(utility: UtilityType) -> (&'static str, &'static str) {
    match utility {
        UtilityType::Water => ("WATERSUPPLY", ""),
        UtilityType::RecycledWater => ("USERDEFINED", "RECYCLEDWATER"),
        UtilityType::FireService => ("FIREPROTECTION", ""),
        UtilityType::Sewer => ("SEWAGE", ""),
        UtilityType::Stormwater => ("STORMWATER", ""),
        UtilityType::Gas => ("GAS", ""),
        UtilityType::Fuel => ("FUEL", ""),
        UtilityType::Electricity => ("ELECTRICAL", ""),
        UtilityType::Telecommunications => ("COMMUNICATION", ""),
        // Traffic signals, their detector loops and cameras: the control of
        // traffic. IFC's SIGNAL is railway signalling.
        UtilityType::IntelligentTransport => ("CONTROL", ""),
        UtilityType::Unknown | UtilityType::Other => ("NOTDEFINED", ""),
    }
}

fn with_system(service: &UtilityAttributes) -> UtilityClass {
    let (system, object_type) = system_of(service.r#type);
    UtilityClass {
        system: String::from(system),
        system_object_type: String::from(object_type),
        ..UtilityClass::default()
    }
}

/// The class of every segment of a service run:
///
/// ```text
/// water, sewer, stormwater, gas, fuel, fire service, recycled water
///                             IfcPipeSegment RIGIDSEGMENT
/// electricity, communications, ITS
///                             IfcCableSegment CABLESEGMENT
/// a culvert                   IfcPipeSegment CULVERT
/// a conduit or duct           IfcCableCarrierSegment CONDUITSEGMENT
/// a trough or trunking        IfcCableCarrierSegment CABLETRUNKINGSEGMENT
/// optical fibre               IfcCableSegment OPTICALCABLESEGMENT
/// a service of unknown kind   IfcDistributionFlowElement - the schema's
///                             element of a distribution flow system whose
///                             kind is not said, which is exactly what
///                             AS 5488 calls a service of unknown type
///                             (IfcFlowSegment would say it too, but IFC 4.3
///                             deprecates instantiating it)
/// ```
///
/// and the system: WATERSUPPLY, SEWAGE, STORMWATER, GAS, FUEL, FIREPROTECTION,
/// ELECTRICAL, COMMUNICATION, CONTROL (ITS: traffic signals and their
/// detection), USERDEFINED RECYCLEDWATER (IFC has no recycled water system),
/// NOTDEFINED for an unknown service.
pub fn classify_utility_run(service: &UtilityAttributes) -> UtilityClass {
    let mut result = with_system(service);
    let text = feature_text(service);
    let set = |result: &mut UtilityClass, entity: &str, predefined: &str, word: &str| {
        result.element = IfcClass::new(entity, predefined, "");
        result.reason = reason_for(service, word);
    };

    // What the service is laid in, or as, decides the class before its
    // type does: a culvert is a culvert whatever it drains, and a conduit is
    // a cable carrier whatever it carries.
    if has_word(&text, "CULVERT*") {
        set(&mut result, "IfcPipeSegment", "CULVERT", "CULVERT*");
        return result;
    }
    if !carries_fluid(service.r#type) {
        if has_any(&text, &["CONDUIT*", "DUCT*"]) {
            let word = if has_word(&text, "CONDUIT*") {
                "CONDUIT*"
            } else {
                "DUCT*"
            };
            set(
                &mut result,
                "IfcCableCarrierSegment",
                "CONDUITSEGMENT",
                word,
            );
            return result;
        }
        if has_any(&text, &["TROUGH*", "TRUNKING"]) {
            let word = if has_word(&text, "TROUGH*") {
                "TROUGH*"
            } else {
                "TRUNKING"
            };
            set(
                &mut result,
                "IfcCableCarrierSegment",
                "CABLETRUNKINGSEGMENT",
                word,
            );
            return result;
        }
        if has_any(&text, &["OPTIC*", "FIBRE*", "FIBER*"]) {
            let word = if has_word(&text, "OPTIC*") {
                "OPTIC*"
            } else if has_word(&text, "FIBRE*") {
                "FIBRE*"
            } else {
                "FIBER*"
            };
            set(&mut result, "IfcCableSegment", "OPTICALCABLESEGMENT", word);
            return result;
        }
        if has_word(&text, "CABLE*") {
            set(&mut result, "IfcCableSegment", "CABLESEGMENT", "CABLE*");
            return result;
        }
    }
    if carries_cable(service.r#type) {
        result.element = IfcClass::new("IfcCableSegment", "CABLESEGMENT", "");
        result.reason = format!("type {}", sub::to_string(service.r#type));
        return result;
    }
    if carries_fluid(service.r#type) {
        result.element = IfcClass::new("IfcPipeSegment", "RIGIDSEGMENT", "");
        result.reason = format!("type {}", sub::to_string(service.r#type));
        return result;
    }
    if has_any(&text, &["PIPE*", "MAIN", "MAINS"]) {
        let word = if has_word(&text, "PIPE*") {
            "PIPE*"
        } else {
            "MAIN*"
        };
        set(&mut result, "IfcPipeSegment", "RIGIDSEGMENT", word);
        return result;
    }
    // A service whose kind nobody established: the schema's element of a
    // distribution flow system, which says exactly that and no more. (Not
    // IfcFlowSegment, whose instantiation IFC 4.3 deprecates - IFC102.)
    result.element = IfcClass::new("IfcDistributionFlowElement", "", "UNKNOWN SERVICE");
    result.reason = format!("type {}", sub::to_string(service.r#type));
    result
}

/// A service recorded at a single point: a manhole, pit, chamber or sump
/// (IfcDistributionChamberElement), a hydrant (IfcFireSuppressionTerminal
/// FIREHYDRANT), a valve (IfcValve), a marker post (IfcSign MARKER), a pillar
/// or cabinet (IfcJunctionBox POWER or DATA). None when its words name none
/// of them: the point is then exported as the survey record it is and nothing
/// more.
pub fn classify_utility_point(service: &UtilityAttributes) -> Option<UtilityClass> {
    let mut result = with_system(service);
    let text = feature_text(service);
    let mut make = |entity: &str, predefined: &str, word: &str| {
        result.element = IfcClass::new(entity, predefined, "");
        result.reason = reason_for(service, word);
        Some(result.clone())
    };
    // Most specific first: a "valve pit" is a chamber, not a valve.
    if has_word(&text, "MANHOLE*") {
        return make("IfcDistributionChamberElement", "MANHOLE", "MANHOLE*");
    }
    if has_word(&text, "VALVE PIT") || has_word(&text, "VALVE CHAMBER") {
        let word = if has_word(&text, "VALVE PIT") {
            "VALVE PIT"
        } else {
            "VALVE CHAMBER"
        };
        return make("IfcDistributionChamberElement", "VALVECHAMBER", word);
    }
    if has_word(&text, "METER PIT") || has_word(&text, "METER CHAMBER") {
        let word = if has_word(&text, "METER PIT") {
            "METER PIT"
        } else {
            "METER CHAMBER"
        };
        return make("IfcDistributionChamberElement", "METERCHAMBER", word);
    }
    if has_word(&text, "SUMP*") {
        return make("IfcDistributionChamberElement", "SUMP", "SUMP*");
    }
    if has_word(&text, "CHAMBER*") {
        return make(
            "IfcDistributionChamberElement",
            "INSPECTIONCHAMBER",
            "CHAMBER*",
        );
    }
    if has_word(&text, "PIT") || has_word(&text, "PITS") {
        let word = if has_word(&text, "PIT") {
            "PIT"
        } else {
            "PITS"
        };
        return make("IfcDistributionChamberElement", "INSPECTIONPIT", word);
    }
    if has_word(&text, "HYDRANT*") {
        return make("IfcFireSuppressionTerminal", "FIREHYDRANT", "HYDRANT*");
    }
    if has_word(&text, "STOP VALVE") || has_word(&text, "STOPCOCK") {
        let word = if has_word(&text, "STOPCOCK") {
            "STOPCOCK"
        } else {
            "STOP VALVE"
        };
        return make("IfcValve", "STOPCOCK", word);
    }
    if has_word(&text, "AIR VALVE") {
        return make("IfcValve", "AIRRELEASE", "AIR VALVE");
    }
    if has_word(&text, "SCOUR*") {
        return make("IfcValve", "FLUSHING", "SCOUR*");
    }
    if has_word(&text, "VALVE*") {
        return make("IfcValve", "ISOLATING", "VALVE*");
    }
    if has_word(&text, "MARKER*") {
        return make("IfcSign", "MARKER", "MARKER*");
    }
    if has_any(&text, &["PILLAR*", "CABINET*"]) {
        let predefined = if service.r#type == UtilityType::Electricity {
            "POWER"
        } else {
            "DATA"
        };
        let word = if has_word(&text, "PILLAR*") {
            "PILLAR*"
        } else {
            "CABINET*"
        };
        return make("IfcJunctionBox", predefined, word);
    }
    None
}

/// The one-letter asset type code of AS 5488.2 Table A.4 that a delivery
/// schema's AssetTypeCode takes (C D E F G I P S W N), for a type the
/// table lists; empty for recycled water and "other", which it does not.
pub fn asset_type_code(utility: UtilityType) -> &'static str {
    match utility {
        UtilityType::Telecommunications => "C",
        UtilityType::Stormwater => "D",
        UtilityType::Electricity => "E",
        UtilityType::FireService => "F",
        UtilityType::Gas => "G",
        UtilityType::IntelligentTransport => "I",
        UtilityType::Fuel => "P",
        UtilityType::Sewer => "S",
        UtilityType::Water => "W",
        UtilityType::Unknown => "N",
        UtilityType::RecycledWater | UtilityType::Other => "",
    }
}

// ---- drawing entities --------------------------------------------------------

/// Words are matched whole and without regard to case against the entity's
/// classification_text: "KERB" matches the layer "Survey/Kerb" and the code
/// "KERB" but not "KERBSIDE"; a word ending in '*' matches as a prefix, so
/// "ELEC*" matches "ELECTRICAL" and "ELEC". A word of several words ("EDGE OF
/// SEAL") matches them in sequence.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassificationRule {
    /// shown in the report: "kerb"
    pub name: String,
    /// any one of them
    pub words: Vec<String>,
    /// The entity kinds the rule applies to; empty for any. A point on a
    /// "PITS" layer is a pit; a line on it is not.
    pub kinds: Vec<EntityType>,
    pub target: IfcClass,
    /// The distribution system a service element belongs to, when the rule
    /// itself says ("SEWER" lines are in SEWAGE). Empty: taken from the
    /// service words of the same text (service_system_for), so a pit on a
    /// "STORMWATER PITS" layer is in STORMWATER without a rule per service.
    pub system: String,
}

/// A run of something - a pipe, a kerb, a fence - is a line, an arc or a
/// polyline; a circle on a pits layer is a pit's outline, not a pipe. A curve
/// polyline and a spline are runs as a polyline is: a kerb or a pipe drawn
/// with arcs, or imported from DXF with bulges, is still that kerb.
const RUNS: [EntityType; 5] = [
    EntityType::Line,
    EntityType::Arc,
    EntityType::Polyline,
    EntityType::CurvePolyline,
    EntityType::Spline,
];
const POINTS: [EntityType; 1] = [EntityType::Point];
const POINTS_AND_OUTLINES: [EntityType; 2] = [EntityType::Point, EntityType::Circle];

fn rule(
    name: &str,
    words: &[&str],
    kinds: &[EntityType],
    target: IfcClass,
    system: &str,
) -> ClassificationRule {
    ClassificationRule {
        name: String::from(name),
        words: words.iter().map(|word| String::from(*word)).collect(),
        kinds: kinds.to_vec(),
        target,
        system: String::from(system),
    }
}

fn class(entity: &str, predefined: &str) -> IfcClass {
    IfcClass::new(entity, predefined, "")
}

/// Services first (a "SEWER" polyline is a pipe, whatever else its layer
/// says), then point features, then the road furniture and contours; in that
/// order, first match wins. The reason for each entry is with it, and in
/// docs/ifc.md.
pub fn default_classification_rules() -> &'static [ClassificationRule] {
    static RULES: OnceLock<Vec<ClassificationRule>> = OnceLock::new();
    RULES.get_or_init(|| {
        vec![
            // Services first: linework on a service's layer is the service.
            // Its system comes from the same words (service_system_for), so
            // the rules name only the class. A culvert before a pipe, conduits
            // before cables.
            rule(
                "culvert",
                &["CULVERT", "CULVERTS", "BOX CULVERT"],
                &RUNS,
                class("IfcPipeSegment", "CULVERT"),
                "",
            ),
            rule(
                "conduit",
                &["CONDUIT*", "DUCT", "DUCTS"],
                &RUNS,
                class("IfcCableCarrierSegment", "CONDUITSEGMENT"),
                "",
            ),
            rule(
                "fluid service",
                &[
                    "STORMWATER",
                    "STORM",
                    "DRAINAGE",
                    "DRAIN",
                    "DRAINS",
                    "SW",
                    "SEWER",
                    "SEWERAGE",
                    "SEWAGE",
                    "WASTEWATER",
                    "WATER",
                    "WATERMAIN",
                    "WATERMAINS",
                    "POTABLE",
                    "RECYCLED",
                    "GAS",
                    "FUEL",
                    "FIRE MAIN",
                ],
                &RUNS,
                class("IfcPipeSegment", "RIGIDSEGMENT"),
                "",
            ),
            rule(
                "optical cable",
                &["OPTIC*", "FIBRE", "FIBRE OPTIC"],
                &RUNS,
                class("IfcCableSegment", "OPTICALCABLESEGMENT"),
                "",
            ),
            rule(
                "cable",
                &[
                    "ELEC*",
                    "POWER",
                    "HV",
                    "LV",
                    "COMMS",
                    "TELECOM*",
                    "TELSTRA",
                    "NBN",
                    "TELCO",
                    "COMMUNICATION*",
                    "CABLE*",
                ],
                &RUNS,
                class("IfcCableSegment", "CABLESEGMENT"),
                "",
            ),
            // Features of a service, surveyed as points.
            rule(
                "manhole",
                &["MANHOLE*", "MH"],
                &POINTS_AND_OUTLINES,
                class("IfcDistributionChamberElement", "MANHOLE"),
                "",
            ),
            rule(
                "sump",
                &["SUMP*"],
                &POINTS_AND_OUTLINES,
                class("IfcDistributionChamberElement", "SUMP"),
                "",
            ),
            rule(
                "chamber",
                &["CHAMBER*", "ACCESS CHAMBER"],
                &POINTS_AND_OUTLINES,
                class("IfcDistributionChamberElement", "INSPECTIONCHAMBER"),
                "",
            ),
            rule(
                "pit",
                &["PIT", "PITS", "SIP", "GULLY", "GULLIES", "KIP"],
                &POINTS_AND_OUTLINES,
                class("IfcDistributionChamberElement", "INSPECTIONPIT"),
                "",
            ),
            rule(
                "hydrant",
                &["HYDRANT*", "FH"],
                &POINTS,
                class("IfcFireSuppressionTerminal", "FIREHYDRANT"),
                "FIREPROTECTION",
            ),
            rule(
                "valve",
                &["VALVE*", "SV", "WV", "GV"],
                &POINTS,
                class("IfcValve", "ISOLATING"),
                "",
            ),
            rule(
                "marker",
                &["MARKER*"],
                &POINTS,
                class("IfcSign", "MARKER"),
                "",
            ),
            // Road furniture and the ground.
            rule(
                "kerb",
                &[
                    "KERB*",
                    "KB",
                    "EDGE OF KERB",
                    "LIP OF KERB",
                    "BACK OF KERB",
                    "KERB AND GUTTER",
                ],
                &RUNS,
                class("IfcKerb", "NOTDEFINED"),
                "",
            ),
            rule(
                "guardrail",
                &[
                    "GUARDRAIL*",
                    "GUARD RAIL",
                    "BARRIER*",
                    "W BEAM",
                    "WBEAM",
                    "THRIE BEAM",
                ],
                &RUNS,
                class("IfcRailing", "GUARDRAIL"),
                "",
            ),
            rule(
                "handrail",
                &["HANDRAIL*", "BALUSTRADE*"],
                &RUNS,
                class("IfcRailing", "HANDRAIL"),
                "",
            ),
            rule(
                "fence",
                &["FENCE*", "FENCING"],
                &RUNS,
                class("IfcRailing", "FENCE"),
                "",
            ),
            rule(
                "retaining wall",
                &["RETAINING WALL", "RETAINING"],
                &RUNS,
                class("IfcWall", "RETAININGWALL"),
                "",
            ),
            rule(
                "wall",
                &["WALL", "WALLS"],
                &RUNS,
                class("IfcWall", "NOTDEFINED"),
                "",
            ),
            rule(
                "vegetation",
                &["TREE", "TREES", "SHRUB*", "VEGETATION", "HEDGE*"],
                &[],
                class("IfcGeographicElement", "VEGETATION"),
                "",
            ),
            // IfcBorehole, IFC 4.3's geotechnical borehole: the value
            // SOIL_BORING_POINT of IfcGeographicElement is deprecated (IFC102).
            rule(
                "borehole",
                &["BOREHOLE*", "BH", "TEST PIT"],
                &POINTS,
                class("IfcBorehole", ""),
                "",
            ),
            rule(
                "sign",
                &["SIGN", "SIGNS"],
                &POINTS,
                class("IfcSign", "NOTDEFINED"),
                "",
            ),
            rule(
                "contour",
                &["CONTOUR*"],
                &RUNS,
                class("IfcAnnotation", "CONTOURLINE"),
                "",
            ),
        ]
    })
}

/// What an entity is classified by, upper case, words separated by single
/// blanks: its layer path, its survey code (the "code" property), and the
/// name the archive gave the string ("12d.name"). "Survey/Kerb" with code
/// "KB01" is "SURVEY KERB KB01".
pub fn classification_text(entity: &Entity) -> String {
    let mut text = entity.layer.clone();
    for part in [
        property_text(&entity.properties, "code"),
        property_text(&entity.metadata, "12d.name"),
    ] {
        if !part.is_empty() {
            text.push(' ');
            text.push_str(&part);
        }
    }
    normalised(&text)
}

/// True when `text` (as classification_text makes it) holds `word` under the
/// matching rules above.
pub fn contains_word(text: &str, word: &str) -> bool {
    has_word(text, word)
}

/// The distribution system (IfcDistributionSystemEnum) a text's service words
/// name - STORMWATER for "STORM", "DRAINAGE" or "SW"; SEWAGE, WATERSUPPLY, GAS,
/// FUEL, ELECTRICAL, COMMUNICATION - or empty for none.
pub fn service_system_for(text: &str) -> String {
    for service in &SERVICES {
        for word in service.words {
            if has_word(text, word) {
                // A system IFC has no value for is named by what it is,
                // and written USERDEFINED with that as its object type.
                return String::from(if service.system == "USERDEFINED" {
                    service.object_type
                } else {
                    service.system
                });
            }
        }
    }
    String::new()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntityClass {
    pub ifc_class: IfcClass,
    /// for a distribution element; empty otherwise
    pub system: String,
    /// the rule that matched; empty for the fallback
    pub rule: String,
}

/// The first rule that matches, else by the entity's kind: a point or a line
/// someone surveyed (it has heights or a survey code) is an IfcAnnotation
/// SURVEY; other drawn linework an IfcAnnotation NOTDEFINED; text TEXT; a
/// dimension DIMENSION; a leader LEADER. A contour-line rule makes
/// CONTOURLINE.
pub fn classify_entity(entity: &Entity, rules: &[ClassificationRule]) -> EntityClass {
    let kind = entity.r#type();
    let text = classification_text(entity);
    for candidate in rules {
        if !candidate.kinds.is_empty() && !candidate.kinds.contains(&kind) {
            continue;
        }
        let matches = candidate.words.iter().any(|word| has_word(&text, word));
        if !matches {
            continue;
        }
        let mut result = EntityClass {
            ifc_class: candidate.target.clone(),
            system: candidate.system.clone(),
            rule: candidate.name.clone(),
        };
        let entity_class = result.ifc_class.entity.as_str();
        if result.system.is_empty()
            && entity_class != "IfcAnnotation"
            && entity_class != "IfcGeographicElement"
            && entity_class != "IfcKerb"
            && entity_class != "IfcRailing"
            && entity_class != "IfcWall"
            && entity_class != "IfcSign"
        {
            result.system = service_system_for(&text);
        }
        return result;
    }

    // The fallback, by kind: never a proxy.
    let predefined = match kind {
        EntityType::Text | EntityType::Label => "TEXT",
        EntityType::Dimension => "DIMENSION",
        EntityType::Leader => "LEADER",
        EntityType::Point => "SURVEY",
        EntityType::Line
        | EntityType::Arc
        | EntityType::Polyline
        | EntityType::Circle
        | EntityType::CurvePolyline
        | EntityType::Ellipse
        | EntityType::Spline => {
            // A curve polyline carries its heights on its vertices, not in
            // the elevation properties.
            let curve_heights = match &entity.geometry {
                Geometry::CurvePolyline(curve) => {
                    curve.vertices.iter().any(|vertex| vertex.height.is_some())
                }
                _ => false,
            };
            let surveyed = entity.properties.contains_key(ELEVATION_PROPERTY)
                || entity.properties.contains_key(ELEVATIONS_PROPERTY)
                || entity.properties.contains_key("code")
                || curve_heights;
            if surveyed { "SURVEY" } else { "NOTDEFINED" }
        }
    };
    EntityClass {
        ifc_class: IfcClass::new("IfcAnnotation", predefined, ""),
        system: String::new(),
        rule: String::new(),
    }
}

// ---- a project's rules, as a file ------------------------------------------------

/// The rows of a small CSV file: an optional BOM, '#' comment lines and blank
/// lines skipped, fields separated by commas, a field double-quoted with ""
/// for a quote. Each row with its 1-based line number.
struct CsvRow {
    line: usize,
    fields: Vec<String>,
}

/// `field` as the reader keeps it: whole when it was quoted, its ASCII blanks
/// at either end dropped when it was not.
fn finished_field(field: &[u8], was_quoted: bool) -> String {
    if was_quoted {
        text_of(field.to_vec())
    } else {
        text_of(katana_core::text::trimmed(field).to_vec())
    }
}

fn read_csv(text: &str) -> Result<Vec<CsvRow>> {
    let text = text.as_bytes();
    let text = text.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(text);
    let past_line_break = |at: &mut usize| {
        if *at < text.len() && text[*at] == b'\r' {
            *at += 1;
        }
        if *at < text.len() && text[*at] == b'\n' {
            *at += 1;
        }
    };
    let fail_at = |message: &str, line: usize| -> Error {
        make_error(ErrorCode::ParseFailure, message, format!("line {line}"))
    };
    let mut rows = Vec::new();
    let mut line = 1;
    let mut at = 0;
    while at < text.len() {
        // A blank line or a comment is passed over whole, before any of it
        // is read as fields: a quote in a comment must not open a field
        // that swallows the rules after it. A quoted field starting '#' is
        // data, not a comment.
        let mut first = at;
        while first < text.len() && (text[first] == b' ' || text[first] == b'\t') {
            first += 1;
        }
        if first == text.len()
            || text[first] == b'\n'
            || text[first] == b'\r'
            || text[first] == b'#'
        {
            let end = text[first..]
                .iter()
                .position(|&c| c == b'\r' || c == b'\n')
                .map(|offset| first + offset);
            at = end.unwrap_or(text.len());
            past_line_break(&mut at);
            line += 1;
            continue;
        }
        // One record, which a quoted field may carry over line breaks.
        let mut row = CsvRow {
            line,
            fields: Vec::new(),
        };
        let mut field: Vec<u8> = Vec::new();
        let mut quoted = false; // inside a quoted field
        let mut was_quoted = false; // the field being read was quoted
        let mut closed = false; // its closing quote has been read
        while at < text.len() {
            let c = text[at];
            if quoted {
                if c == b'"' && at + 1 < text.len() && text[at + 1] == b'"' {
                    field.push(b'"');
                    at += 1;
                } else if c == b'"' {
                    quoted = false;
                    closed = true;
                } else {
                    if c == b'\n' {
                        line += 1;
                    }
                    field.push(c);
                }
                at += 1;
                continue;
            }
            if c == b'\n' || c == b'\r' {
                break;
            }
            if closed {
                // After a closing quote: blanks, then the comma or the end.
                if c == b' ' || c == b'\t' {
                    at += 1;
                    continue;
                }
                if c != b',' {
                    return Err(fail_at("text after a quoted field's closing quote", line));
                }
            }
            if c == b'"' && !was_quoted && katana_core::text::trimmed(&field).is_empty() {
                field.clear();
                quoted = true;
                was_quoted = true;
            } else if c == b',' {
                row.fields.push(finished_field(&field, was_quoted));
                field.clear();
                was_quoted = false;
                closed = false;
            } else {
                field.push(c);
            }
            at += 1;
        }
        if quoted {
            return Err(fail_at("a quoted field is never closed", row.line));
        }
        row.fields.push(finished_field(&field, was_quoted));
        past_line_break(&mut at);
        line += 1;
        rows.push(row);
    }
    Ok(rows)
}

fn split_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut at = 0;
    while at <= bytes.len() {
        let end = find_from(bytes, b";", at).unwrap_or(bytes.len());
        let part = trimmed(&text[at..end]);
        if !part.is_empty() {
            out.push(String::from(part));
        }
        at = end + 1;
    }
    out
}

const KINDS: [EntityType; 12] = [
    EntityType::Point,
    EntityType::Line,
    EntityType::Arc,
    EntityType::Polyline,
    EntityType::Circle,
    EntityType::Text,
    EntityType::Dimension,
    EntityType::Label,
    EntityType::Leader,
    EntityType::CurvePolyline,
    EntityType::Ellipse,
    EntityType::Spline,
];

fn csv_field(text: &str) -> String {
    if !text.contains([',', '"', '\r', '\n', '#']) && trimmed(text) == text {
        return String::from(text);
    }
    let mut out = String::from("\"");
    for c in text.chars() {
        if c == '"' {
            out.push_str("\"\"");
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

/// A project's rules as a file, so that layers named otherwise than the
/// defaults expect are classified as the project means them - the way a
/// wrong class shown in the window's preview is put right. CSV, one rule a
/// row, the columns found by name in a header row, in any order and letter
/// case; '#' starts a comment line:
///
/// ```text
/// rule        the rule's name, shown in reports                   required
/// words       its words, separated by ';' ("POLE*;LP")             required
/// kinds       Point;Line;Arc;Polyline;Circle;Text;Dimension;
///             Label;Leader;CurvePolyline;Ellipse;Spline - empty for
///             any kind
/// class       the IFC class, as the schema spells it ("IfcColumn")  required
/// predefined_type   "COLUMN"; empty when the class has none
/// object_type what a USERDEFINED one is; required with USERDEFINED
/// system      the distribution system its elements serve ("SEWAGE")
/// ```
///
/// ParseFailure naming the line for a class this export does not write, a
/// predefined type on a class without one, a kind that is not one, a rule
/// with no words, and USERDEFINED without an object type - the same things
/// write_ifc refuses, found when the file is read rather than when it is used.
pub fn parse_classification_rules(text: &str) -> Result<Vec<ClassificationRule>> {
    let rows = read_csv(text)?;
    let Some(header) = rows.first() else {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "no header row: rule,words,kinds,class,predefined_type,object_type,system",
        ));
    };
    const COLUMNS: [&str; 7] = [
        "rule",
        "words",
        "kinds",
        "class",
        "predefined_type",
        "object_type",
        "system",
    ];
    let mut column: [Option<usize>; 7] = [None; 7];
    for (i, name) in header.fields.iter().enumerate() {
        let lower = lowered(name);
        let Some(known) = COLUMNS
            .iter()
            .position(|candidate| candidate.as_bytes() == lower)
        else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!(
                    "unknown column \"{name}\"; the columns are rule, words, kinds, class, \
                     predefined_type, object_type and system"
                ),
                format!("line {}", header.line),
            ));
        };
        column[known] = Some(i);
    }
    for required in [0, 1, 3] {
        if column[required].is_none() {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("\"{}\" is missing", COLUMNS[required]),
                format!("line {}", header.line),
            ));
        }
    }
    let mut rules = Vec::new();
    for row in &rows[1..] {
        let place = format!("line {}", row.line);
        if row.fields.len() != header.fields.len() {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!(
                    "{} fields where the header has {}",
                    row.fields.len(),
                    header.fields.len()
                ),
                place,
            ));
        }
        let cell = |index: usize| column[index].map_or("", |at| row.fields[at].as_str());
        let fail = |message: String| -> Error {
            make_error(ErrorCode::ParseFailure, message, place.clone())
        };
        let mut rule = ClassificationRule {
            name: String::from(cell(0)),
            ..ClassificationRule::default()
        };
        if rule.name.is_empty() {
            return Err(fail(String::from("a rule has no name")));
        }
        rule.words = split_list(cell(1));
        if rule.words.is_empty() {
            return Err(fail(format!("rule \"{}\" has no words", rule.name)));
        }
        for kind in split_list(cell(2)) {
            let Some(found) = KINDS
                .iter()
                .find(|entity_type| {
                    equals_ignoring_case(entity_name(**entity_type).as_bytes(), kind.as_bytes())
                })
                .copied()
            else {
                return Err(fail(format!(
                    "\"{kind}\" is not a kind: Point, Line, Arc, Polyline, Circle, Text, \
                     Dimension, Label, Leader, CurvePolyline, Ellipse or Spline"
                )));
            };
            rule.kinds.push(found);
        }
        let named = cell(3);
        let Some(layout) = PRODUCT_LAYOUTS
            .iter()
            .find(|candidate| equals_ignoring_case(candidate.entity.as_bytes(), named.as_bytes()))
        else {
            let mut known = String::new();
            for candidate in &PRODUCT_LAYOUTS {
                if !candidate.feature {
                    // the road model's alone: refused below
                    if !known.is_empty() {
                        known.push_str(", ");
                    }
                    known.push_str(candidate.entity);
                }
            }
            return Err(fail(format!(
                "\"{named}\" is not a class this export writes; it writes {known}"
            )));
        };
        if layout.feature {
            return Err(fail(format!(
                "{} is a feature of another element, which a drawing entity does not name: it \
                 is written only for the road model",
                layout.entity
            )));
        }
        rule.target.entity = String::from(layout.entity);
        rule.target.predefined_type = upper(cell(4));
        rule.target.object_type = String::from(cell(5));
        if !rule.target.predefined_type.is_empty() {
            let values = predefined_types_of(&rule.target.entity);
            let Some(values) = values.filter(|_| layout.has_predefined_type) else {
                return Err(fail(format!(
                    "{} has no predefined type",
                    rule.target.entity
                )));
            };
            if !lists_word(values, &rule.target.predefined_type) {
                return Err(fail(format!(
                    "\"{}\" is not a predefined type of {}: {values}",
                    rule.target.predefined_type, rule.target.entity
                )));
            }
        }
        if rule.target.predefined_type == "USERDEFINED" && rule.target.object_type.is_empty() {
            return Err(fail(format!(
                "rule \"{}\" is USERDEFINED and says nothing of what it is (object_type)",
                rule.name
            )));
        }
        rule.system = upper(cell(6));
        rules.push(rule);
    }
    Ok(rules)
}

/// `rules` as parse_classification_rules reads them: what the window writes
/// as a starting point for a project's own file (the defaults, commented).
pub fn format_classification_rules(rules: &[ClassificationRule]) -> String {
    let mut out = String::from(
        "# Classification rules for EXPORT <file.ifc> RULES <this file> (docs/ifc.md).\n\
         # Tried in order before the defaults; the first rule whose words the entity's\n\
         # layer, survey code or string name holds wins. Words are whole words, any case;\n\
         # a trailing * matches a prefix. Kinds and system may be left empty.\n\
         rule,words,kinds,class,predefined_type,object_type,system\n",
    );
    for rule in rules {
        // A ';' before each but the first that is not empty, as the C++ joins
        // them.
        let mut words = String::new();
        for word in &rule.words {
            if !words.is_empty() {
                words.push(';');
            }
            words.push_str(word);
        }
        let mut kinds = String::new();
        for kind in &rule.kinds {
            if !kinds.is_empty() {
                kinds.push(';');
            }
            kinds.push_str(entity_name(*kind));
        }
        out.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            csv_field(&rule.name),
            csv_field(&words),
            csv_field(&kinds),
            csv_field(&rule.target.entity),
            csv_field(&rule.target.predefined_type),
            csv_field(&rule.target.object_type),
            csv_field(&rule.system)
        ));
    }
    out
}
